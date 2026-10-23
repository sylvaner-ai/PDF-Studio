//! Interprétation du contenu d'une page : état graphique, position de chaque
//! glyphe, emprise des tracés, des images et des formulaires.
//!
//! L'interpréteur suit les règles de la norme PDF (§ 8 et 9) sans rien
//! dessiner. Il retient pour chaque élément dessiné les opérations dont il
//! provient et l'état graphique en vigueur, de quoi le déplacer, le supprimer
//! ou le redessiner ailleurs sans toucher au reste de la page.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use super::font::FontMetrics;
use super::geometry::{Matrix, Point, Rect};
use super::lexer::{Operand, Operation};

/// Ressources de la page dont l'interpréteur a besoin.
pub trait Resources {
    /// Métriques de la police désignée par son nom de ressource.
    fn font(&mut self, name: &[u8]) -> Option<Arc<FontMetrics>>;
    /// Nature de l'objet externe (`Do`) désigné par son nom de ressource.
    fn xobject(&mut self, name: &[u8]) -> XObject;
}

/// Nature d'un objet externe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum XObject {
    /// Image.
    Image,
    /// Formulaire (contenu réutilisable), avec son cadre et sa matrice.
    Form {
        /// Cadre `/BBox`, dans l'espace du formulaire.
        bbox: Rect,
        /// Matrice `/Matrix`.
        matrix: Matrix,
    },
    /// Objet inconnu ou absent : rien n'est dessiné.
    Other,
}

/// Opérations qui rétablissent une couleur : espace de couleur, puis couleur.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorOps {
    /// Dernière opération `cs` ou `CS`.
    pub space: Option<Vec<u8>>,
    /// Dernière opération de couleur (`g`, `rg`, `k`, `sc`, `scn` ou leurs
    /// majuscules).
    pub color: Option<Vec<u8>>,
}

/// État graphique en vigueur.
#[derive(Clone, Debug)]
pub struct GraphicsState {
    /// Matrice de transformation courante (espace utilisateur → page).
    pub ctm: Matrix,
    /// Nom de ressource de la police.
    pub font_name: Option<Vec<u8>>,
    /// Métriques de la police.
    pub font: Option<Arc<FontMetrics>>,
    /// Corps de la police (`Tf`).
    pub font_size: f64,
    /// Espacement des caractères (`Tc`).
    pub char_spacing: f64,
    /// Espacement des mots (`Tw`).
    pub word_spacing: f64,
    /// Échelle horizontale (`Tz`), 1 pour 100 %.
    pub h_scale: f64,
    /// Interligne (`TL`).
    pub leading: f64,
    /// Élévation (`Ts`).
    pub rise: f64,
    /// Mode de rendu du texte (`Tr`).
    pub render_mode: u8,
    /// Couleur de remplissage.
    pub fill: ColorOps,
    /// Couleur de contour.
    pub stroke: ColorOps,
    /// Épaisseur du trait.
    pub line_width: f64,
    /// Dernières opérations de style de trait (`w`, `J`, `j`, `M`, `d`), par opérateur.
    pub line_ops: Vec<(Vec<u8>, Vec<u8>)>,
    /// Opérations `gs` appliquées, dans l'ordre (les plus récentes, sans doublon).
    pub ext_gstates: Vec<Vec<u8>>,
    /// Emprise de la zone de détourage, dans l'espace de la page.
    pub clip: Rect,
}

impl GraphicsState {
    fn new(clip: Rect) -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            font_name: None,
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
            fill: ColorOps::default(),
            stroke: ColorOps::default(),
            line_width: 1.0,
            line_ops: Vec::new(),
            ext_gstates: Vec::new(),
            clip,
        }
    }

    /// Vrai si le mode de rendu dessine le contour des glyphes.
    pub fn strokes_text(&self) -> bool {
        matches!(self.render_mode, 1 | 2 | 5 | 6)
    }
}

/// Un glyphe dessiné.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// Opération d'affichage de texte dont il provient.
    pub op: usize,
    /// Code du glyphe dans la police.
    pub code: u32,
    /// Nombre d'octets du code.
    pub code_len: usize,
    /// Texte du glyphe ; vide s'il est inconnu.
    pub text: String,
    /// Matrice de texte au début du glyphe (`Tm`).
    pub tm: Matrix,
    /// Chasse déclarée, en millièmes de corps.
    pub width: f64,
    /// Avance dans l'espace du texte (espacements et échelle compris).
    pub advance: f64,
    /// Origine sur la ligne de base, élévation comprise, dans l'espace de la page.
    pub origin: Point,
    /// Origine sur la ligne de base, sans élévation.
    pub baseline: Point,
    /// Fin du glyphe (origine du suivant s'il le suit sans décalage).
    pub end: Point,
    /// Matrice de rendu (corps, échelle, élévation, `Tm` et CTM).
    pub trm: Matrix,
    /// Emprise approximative, dans l'espace de la page.
    pub bbox: Rect,
    /// État graphique (indice dans [`Interpretation::states`]).
    pub state: usize,
    /// Objet texte `BT … ET` (indice dans [`Interpretation::text_objects`]).
    pub text_object: usize,
    /// Vrai si le glyphe est visible (mode de rendu autre que 3 et 7).
    pub visible: bool,
}

/// Élément d'une opération d'affichage de texte, dans l'ordre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShowItem {
    /// Glyphe (indice dans [`Interpretation::glyphs`]).
    Glyph(usize),
    /// Décalage d'un tableau `TJ`, en millièmes de corps.
    Kern(f64),
}

/// Objet texte `BT … ET`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextObject {
    /// Opération `BT`.
    pub begin: usize,
    /// Opération `ET` ; `None` si le flux se termine avant.
    pub end: Option<usize>,
}

/// Nature d'un élément dessiné autre que du texte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// Tracé rempli ou dessiné.
    Path,
    /// Image (`Do`).
    Image,
    /// Image en ligne (`BI … EI`).
    InlineImage,
    /// Formulaire (`Do`).
    Form,
    /// Dégradé (`sh`).
    Shading,
}

/// Opération de construction d'un tracé et ses points.
#[derive(Clone, Debug, PartialEq)]
pub struct PathOp {
    /// Indice de l'opération (`m`, `l`, `c`, `v`, `y` ou `re`).
    pub op: usize,
    /// Points de l'opération dans l'espace de la page : extrémité (et points
    /// de contrôle) ; pour `re`, les quatre coins, depuis l'origine du
    /// rectangle.
    pub points: Vec<Point>,
}

/// Sous-chemin d'un tracé : un `re`, ou un `m` suivi de segments.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    /// Opérations de construction, `h` compris.
    pub ops: Range<usize>,
    /// Opérations qui portent des points, dans l'ordre.
    pub items: Vec<PathOp>,
    /// Rectangle `re`.
    pub rect: bool,
    /// Fermé (`re` ou `h`).
    pub closed: bool,
    /// Fait seulement de segments de droite (ni courbe).
    pub straight: bool,
}

impl Subpath {
    /// Points du sous-chemin, dans l'espace de la page.
    pub fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.items
            .iter()
            .flat_map(|item| item.points.iter().copied())
    }

    /// Emprise des points.
    pub fn bbox(&self) -> Rect {
        Rect::bounding(self.points())
    }
}

/// Élément dessiné autre que du texte.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    /// Nature.
    pub kind: UnitKind,
    /// Opérations : construction du tracé et peinture, ou opération unique.
    pub ops: Range<usize>,
    /// Emprise dans l'espace de la page, détourage compris.
    pub bbox: Rect,
    /// Le tracé sert aussi de détourage (`W` ou `W*` avant la peinture).
    pub clips: bool,
    /// Sous-chemins d'un tracé.
    pub subpaths: Vec<Subpath>,
    /// Le tracé est rempli.
    pub filled: bool,
    /// Le tracé est dessiné au trait.
    pub stroked: bool,
}

/// Résultat de l'interprétation d'une page.
#[derive(Debug, Default)]
pub struct Interpretation {
    /// Glyphes dessinés, dans l'ordre.
    pub glyphs: Vec<Glyph>,
    /// États graphiques des glyphes.
    pub states: Vec<Arc<GraphicsState>>,
    /// Éléments de chaque opération d'affichage de texte, par opération.
    pub shows: HashMap<usize, Vec<ShowItem>>,
    /// Objets texte.
    pub text_objects: Vec<TextObject>,
    /// Éléments dessinés autres que le texte, dans l'ordre.
    pub units: Vec<Unit>,
    /// Paires `q … Q` équilibrées : indices des deux opérations.
    pub groups: Vec<(usize, usize)>,
    /// Matrice de transformation en vigueur avant chaque opération.
    pub ctm_before: Vec<Matrix>,
}

/// Profondeur maximale de la pile d'états (`q`).
const MAX_STACK: usize = 256;

/// Interprète les opérations d'une page dont la zone visible est `page_box`
/// (espace de la page PDF).
pub fn interpret(
    ops: &[Operation],
    resources: &mut dyn Resources,
    page_box: Rect,
) -> Interpretation {
    let mut run = Run {
        out: Interpretation::default(),
        state: GraphicsState::new(page_box),
        stack: Vec::new(),
        q_stack: Vec::new(),
        state_index: None,
        text: None,
        path: None,
        pending_clip: false,
        resources,
    };
    run.out.ctm_before.reserve(ops.len());
    for (index, op) in ops.iter().enumerate() {
        run.out.ctm_before.push(run.state.ctm);
        run.execute(index, op);
    }
    run.out
}

/// Objet texte en cours.
struct TextState {
    tm: Matrix,
    tlm: Matrix,
    object: usize,
}

/// Tracé en cours de construction (points dans l'espace utilisateur).
struct PathState {
    start: usize,
    points: Vec<Point>,
    current: Point,
    subpath_start: Point,
    subpaths: Vec<Subpath>,
    /// Le dernier sous-chemin peut être prolongé (ni `re` ni fermé).
    open: bool,
}

impl PathState {
    fn new(start: usize, current: Point) -> Self {
        PathState {
            start,
            points: Vec::new(),
            current,
            subpath_start: current,
            subpaths: Vec::new(),
            open: false,
        }
    }

    /// Ajoute l'opération `op` et ses points au sous-chemin en cours, ou à un
    /// nouveau sous-chemin qui part du point courant.
    fn record(&mut self, op: usize, points: &[Point], curve: bool) {
        if !self.open {
            self.subpaths.push(Subpath {
                ops: op..op,
                items: vec![PathOp {
                    op,
                    points: vec![self.current],
                }],
                rect: false,
                closed: false,
                straight: true,
            });
            self.open = true;
        }
        if let Some(subpath) = self.subpaths.last_mut() {
            subpath.ops.end = op + 1;
            subpath.items.push(PathOp {
                op,
                points: points.to_vec(),
            });
            subpath.straight &= !curve;
        }
    }
}

struct Run<'a> {
    out: Interpretation,
    state: GraphicsState,
    stack: Vec<GraphicsState>,
    q_stack: Vec<usize>,
    /// État déjà retenu pour les glyphes, tant qu'il ne change pas.
    state_index: Option<usize>,
    text: Option<TextState>,
    path: Option<PathState>,
    pending_clip: bool,
    resources: &'a mut dyn Resources,
}

impl Run<'_> {
    fn changed(&mut self) {
        self.state_index = None;
    }

    fn execute(&mut self, index: usize, op: &Operation) {
        let source_op = || op_bytes(op);
        match op.operator.as_slice() {
            // État graphique.
            b"q" => {
                if self.stack.len() < MAX_STACK {
                    self.stack.push(self.state.clone());
                    self.q_stack.push(index);
                }
            }
            b"Q" => {
                if let Some(state) = self.stack.pop() {
                    self.state = state;
                    self.changed();
                    if let Some(start) = self.q_stack.pop() {
                        self.out.groups.push((start, index));
                    }
                }
            }
            b"cm" => {
                if let Some(values) = op.numbers(6) {
                    self.state.ctm = Matrix::from_slice(&values).then(&self.state.ctm);
                    self.changed();
                }
            }
            b"w" => {
                self.state.line_width = op.number(0);
                self.set_line_op(op);
            }
            b"J" | b"j" | b"M" | b"d" => self.set_line_op(op),
            b"gs" => {
                let bytes = source_op();
                self.state.ext_gstates.retain(|known| known != &bytes);
                self.state.ext_gstates.push(bytes);
                if self.state.ext_gstates.len() > 8 {
                    self.state.ext_gstates.remove(0);
                }
                self.changed();
            }
            // Couleurs.
            b"cs" => {
                self.state.fill = ColorOps {
                    space: Some(source_op()),
                    color: None,
                };
                self.changed();
            }
            b"CS" => {
                self.state.stroke = ColorOps {
                    space: Some(source_op()),
                    color: None,
                };
                self.changed();
            }
            b"sc" | b"scn" => {
                self.state.fill.color = Some(source_op());
                self.changed();
            }
            b"SC" | b"SCN" => {
                self.state.stroke.color = Some(source_op());
                self.changed();
            }
            b"g" | b"rg" | b"k" => {
                self.state.fill = ColorOps {
                    space: None,
                    color: Some(source_op()),
                };
                self.changed();
            }
            b"G" | b"RG" | b"K" => {
                self.state.stroke = ColorOps {
                    space: None,
                    color: Some(source_op()),
                };
                self.changed();
            }
            // Tracés.
            b"m" => {
                let p = self.point(op, 0);
                let path = self.path.get_or_insert_with(|| PathState::new(index, p));
                path.points.push(p);
                path.current = p;
                path.subpath_start = p;
                path.subpaths.push(Subpath {
                    ops: index..index + 1,
                    items: vec![PathOp {
                        op: index,
                        points: vec![p],
                    }],
                    rect: false,
                    closed: false,
                    straight: true,
                });
                path.open = true;
            }
            b"l" => {
                let p = self.point(op, 0);
                self.extend_path(index, &[p]);
                self.record(index, &[p], false);
            }
            b"c" => {
                let points = [self.point(op, 0), self.point(op, 2), self.point(op, 4)];
                self.extend_path(index, &points);
                self.record(index, &points, true);
            }
            b"v" | b"y" => {
                let points = [self.point(op, 0), self.point(op, 2)];
                self.extend_path(index, &points);
                self.record(index, &points, true);
            }
            b"re" => {
                let (x, y, w, h) = (op.number(0), op.number(1), op.number(2), op.number(3));
                let corners = [
                    Point::new(x, y),
                    Point::new(x + w, y),
                    Point::new(x + w, y + h),
                    Point::new(x, y + h),
                ];
                self.extend_path(index, &corners);
                if let Some(path) = &mut self.path {
                    path.current = corners[0];
                    path.subpath_start = corners[0];
                    path.subpaths.push(Subpath {
                        ops: index..index + 1,
                        items: vec![PathOp {
                            op: index,
                            points: corners.to_vec(),
                        }],
                        rect: true,
                        closed: true,
                        straight: true,
                    });
                    path.open = false;
                }
            }
            b"h" => {
                if let Some(path) = &mut self.path {
                    path.current = path.subpath_start;
                    if path.open {
                        if let Some(subpath) = path.subpaths.last_mut() {
                            subpath.ops.end = index + 1;
                            subpath.closed = true;
                        }
                        path.open = false;
                    }
                }
            }
            b"W" | b"W*" => {
                self.pending_clip = true;
            }
            b"S" | b"s" => self.paint_path(index, false, true),
            b"f" | b"F" | b"f*" => self.paint_path(index, true, false),
            b"B" | b"B*" | b"b" | b"b*" => self.paint_path(index, true, true),
            b"n" => self.paint_path(index, false, false),
            // Images, formulaires, dégradés.
            b"Do" => {
                let kind = op
                    .operands
                    .last()
                    .and_then(Operand::name)
                    .map(|name| self.resources.xobject(name))
                    .unwrap_or(XObject::Other);
                match kind {
                    XObject::Image => {
                        let bbox = self.unit_square();
                        self.push_unit(UnitKind::Image, index..index + 1, bbox, false);
                    }
                    XObject::Form { bbox, matrix } => {
                        let bbox = matrix.then(&self.state.ctm).apply_rect(bbox);
                        let bbox = bbox.intersect(&self.state.clip);
                        self.push_unit(UnitKind::Form, index..index + 1, bbox, false);
                    }
                    XObject::Other => {}
                }
            }
            b"BI" => {
                let bbox = self.unit_square();
                self.push_unit(UnitKind::InlineImage, index..index + 1, bbox, false);
            }
            b"sh" => {
                let bbox = self.state.clip;
                self.push_unit(UnitKind::Shading, index..index + 1, bbox, false);
            }
            // Texte.
            b"BT" => {
                self.out.text_objects.push(TextObject {
                    begin: index,
                    end: None,
                });
                self.text = Some(TextState {
                    tm: Matrix::IDENTITY,
                    tlm: Matrix::IDENTITY,
                    object: self.out.text_objects.len() - 1,
                });
            }
            b"ET" => {
                if let Some(text) = self.text.take() {
                    self.out.text_objects[text.object].end = Some(index);
                }
            }
            b"Tf" => {
                let name = op
                    .operands
                    .first()
                    .and_then(Operand::name)
                    .map(<[u8]>::to_vec);
                self.state.font = name.as_deref().and_then(|name| self.resources.font(name));
                self.state.font_name = name;
                self.state.font_size = op.number(1);
                self.changed();
            }
            b"Tc" => {
                self.state.char_spacing = op.number(0);
                self.changed();
            }
            b"Tw" => {
                self.state.word_spacing = op.number(0);
                self.changed();
            }
            b"Tz" => {
                self.state.h_scale = op.number(0) / 100.0;
                self.changed();
            }
            b"TL" => {
                self.state.leading = op.number(0);
                self.changed();
            }
            b"Ts" => {
                self.state.rise = op.number(0);
                self.changed();
            }
            b"Tr" => {
                self.state.render_mode = op.number(0).clamp(0.0, 7.0) as u8;
                self.changed();
            }
            b"Td" => self.move_line(op.number(0), op.number(1)),
            b"TD" => {
                self.state.leading = -op.number(1);
                self.changed();
                self.move_line(op.number(0), op.number(1));
            }
            b"Tm" => {
                if let (Some(values), Some(text)) = (op.numbers(6), &mut self.text) {
                    text.tm = Matrix::from_slice(&values);
                    text.tlm = text.tm;
                }
            }
            b"T*" => self.next_line(),
            b"Tj" => {
                if let Some(Operand::String(bytes)) = op.operands.last() {
                    let mut items = Vec::new();
                    self.show(index, bytes, &mut items);
                    self.out.shows.insert(index, items);
                }
            }
            b"'" => {
                self.next_line();
                if let Some(Operand::String(bytes)) = op.operands.last() {
                    let mut items = Vec::new();
                    self.show(index, bytes, &mut items);
                    self.out.shows.insert(index, items);
                }
            }
            b"\"" => {
                if op.operands.len() >= 3 {
                    self.state.word_spacing = op.number(0);
                    self.state.char_spacing = op.number(1);
                    self.changed();
                }
                self.next_line();
                if let Some(Operand::String(bytes)) = op.operands.last() {
                    let mut items = Vec::new();
                    self.show(index, bytes, &mut items);
                    self.out.shows.insert(index, items);
                }
            }
            b"TJ" => {
                if let Some(Operand::Array(elements)) = op.operands.last() {
                    let mut items = Vec::new();
                    for element in elements {
                        match element {
                            Operand::String(bytes) => self.show(index, bytes, &mut items),
                            Operand::Number(kern) => {
                                items.push(ShowItem::Kern(*kern));
                                self.kern(*kern);
                            }
                            _ => {}
                        }
                    }
                    self.out.shows.insert(index, items);
                }
            }
            _ => {}
        }
    }

    fn set_line_op(&mut self, op: &Operation) {
        let bytes = op_bytes(op);
        let operator = op.operator.clone();
        self.state.line_ops.retain(|(known, _)| known != &operator);
        self.state.line_ops.push((operator, bytes));
        self.changed();
    }

    fn point(&self, op: &Operation, at: usize) -> Point {
        Point::new(op.number(at), op.number(at + 1))
    }

    fn extend_path(&mut self, index: usize, points: &[Point]) {
        let path = self
            .path
            .get_or_insert_with(|| PathState::new(index, Point::default()));
        path.points.extend_from_slice(points);
    }

    /// Retient l'opération dans le sous-chemin en cours, puis avance le point
    /// courant.
    fn record(&mut self, index: usize, points: &[Point], curve: bool) {
        if let Some(path) = &mut self.path {
            path.record(index, points, curve);
            if let Some(&last) = points.last() {
                path.current = last;
            }
        }
    }

    fn paint_path(&mut self, index: usize, fill: bool, stroke: bool) {
        let clips = std::mem::take(&mut self.pending_clip);
        let Some(path) = self.path.take() else {
            return;
        };
        let ctm = self.state.ctm;
        let mut bbox = Rect::bounding(path.points.iter().map(|&p| ctm.apply(p)));
        if stroke {
            let scale = ctm.horizontal_scale().max(ctm.vertical_scale());
            bbox = bbox.inflate(self.state.line_width.max(0.0) * scale / 2.0);
        }
        if fill || stroke {
            let shown = bbox.intersect(&self.state.clip);
            let mut subpaths = path.subpaths;
            for subpath in &mut subpaths {
                for item in &mut subpath.items {
                    for point in &mut item.points {
                        *point = ctm.apply(*point);
                    }
                }
            }
            self.out.units.push(Unit {
                kind: UnitKind::Path,
                ops: path.start..index + 1,
                bbox: shown,
                clips,
                subpaths,
                filled: fill,
                stroked: stroke,
            });
        }
        if clips {
            self.state.clip = self.state.clip.intersect(&bbox);
            self.changed();
        }
    }

    fn unit_square(&self) -> Rect {
        self.state
            .ctm
            .apply_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
            .intersect(&self.state.clip)
    }

    fn push_unit(&mut self, kind: UnitKind, ops: Range<usize>, bbox: Rect, clips: bool) {
        self.out.units.push(Unit {
            kind,
            ops,
            bbox,
            clips,
            subpaths: Vec::new(),
            filled: false,
            stroked: false,
        });
    }

    fn move_line(&mut self, tx: f64, ty: f64) {
        if let Some(text) = &mut self.text {
            text.tlm = Matrix::translate(tx, ty).then(&text.tlm);
            text.tm = text.tlm;
        }
    }

    fn next_line(&mut self) {
        let leading = self.state.leading;
        self.move_line(0.0, -leading);
    }

    fn kern(&mut self, kern: f64) {
        let state = &self.state;
        let Some(text) = &mut self.text else { return };
        let vertical = state.font.as_ref().is_some_and(|font| font.vertical);
        let shift = -kern / 1000.0 * state.font_size;
        text.tm = if vertical {
            Matrix::translate(0.0, shift).then(&text.tm)
        } else {
            Matrix::translate(shift * state.h_scale, 0.0).then(&text.tm)
        };
    }

    /// Affiche une chaîne : un glyphe par code.
    fn show(&mut self, index: usize, bytes: &[u8], items: &mut Vec<ShowItem>) {
        let Some(object) = self.text.as_ref().map(|text| text.object) else {
            return;
        };
        let Some(font) = self.state.font.clone() else {
            return;
        };
        let state_index = match self.state_index {
            Some(known) => known,
            None => {
                self.out.states.push(Arc::new(self.state.clone()));
                let known = self.out.states.len() - 1;
                self.state_index = Some(known);
                known
            }
        };
        let state = &self.state;
        let size = state.font_size;
        let ctm = state.ctm;
        let visible = !matches!(state.render_mode, 3 | 7);
        for (code, len) in font.codes(bytes) {
            let Some(text) = &mut self.text else { return };
            let width = font.width(code);
            let word = if len == 1 && code == 32 {
                state.word_spacing
            } else {
                0.0
            };
            let tm = text.tm;
            let user = tm.then(&ctm);
            let trm =
                Matrix::new(size * state.h_scale, 0.0, 0.0, size, 0.0, state.rise).then(&user);
            let advance;
            let end;
            if font.vertical {
                advance = width / 1000.0 * size + state.char_spacing + word;
                end = user.apply(Point::new(0.0, state.rise - advance));
                text.tm = Matrix::translate(0.0, -advance).then(&text.tm);
            } else {
                advance = (width / 1000.0 * size + state.char_spacing + word) * state.h_scale;
                end = user.apply(Point::new(advance, state.rise));
                text.tm = Matrix::translate(advance, 0.0).then(&text.tm);
            }
            let (ascent, descent) = (font.ascent / 1000.0, font.descent / 1000.0);
            let bbox = trm.apply_rect(Rect::new(0.0, descent, (width / 1000.0).max(0.0), ascent));
            self.out.glyphs.push(Glyph {
                op: index,
                code,
                code_len: len,
                text: font.text(code).unwrap_or_default().to_owned(),
                tm,
                width,
                advance,
                origin: user.apply(Point::new(0.0, state.rise)),
                baseline: user.apply(Point::new(0.0, 0.0)),
                end,
                trm,
                bbox,
                state: state_index,
                text_object: object,
                visible,
            });
            items.push(ShowItem::Glyph(self.out.glyphs.len() - 1));
        }
    }
}

/// Texte source d'une opération, reconstitué : opérandes puis opérateur.
fn op_bytes(op: &Operation) -> Vec<u8> {
    let mut out = Vec::new();
    for operand in &op.operands {
        super::writer::write_operand(&mut out, operand);
        out.push(b' ');
    }
    out.extend_from_slice(&op.operator);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content::lexer::parse;

    pub(crate) struct TestResources {
        pub font: Arc<FontMetrics>,
    }

    impl Resources for TestResources {
        fn font(&mut self, _name: &[u8]) -> Option<Arc<FontMetrics>> {
            Some(Arc::clone(&self.font))
        }
        fn xobject(&mut self, name: &[u8]) -> XObject {
            match name {
                b"Im1" => XObject::Image,
                b"Fm1" => XObject::Form {
                    bbox: Rect::new(0.0, 0.0, 100.0, 50.0),
                    matrix: Matrix::IDENTITY,
                },
                _ => XObject::Other,
            }
        }
    }

    /// Police simple : chaque caractère ASCII a une chasse de 500.
    pub(crate) fn test_font() -> Arc<FontMetrics> {
        let widths = (32..127).map(|code| (code, 500.0)).collect();
        let unicode = (32..127u32)
            .map(|code| (code, char::from_u32(code).unwrap().to_string()))
            .collect();
        Arc::new(FontMetrics::simple("Test", widths, unicode))
    }

    fn run(source: &[u8]) -> Interpretation {
        let mut resources = TestResources { font: test_font() };
        interpret(
            &parse(source),
            &mut resources,
            Rect::new(0.0, 0.0, 600.0, 800.0),
        )
    }

    #[test]
    fn position_des_glyphes() {
        let out = run(b"BT /F1 10 Tf 100 700 Td 2 Tc (AB) Tj [(C) -1000 (D)] TJ ET");
        let x: Vec<f64> = out.glyphs.iter().map(|g| g.origin.x).collect();
        // A en 100 ; B après 5 (chasse) + 2 (Tc) ; C ; D après un décalage de 10.
        assert_eq!(x, [100.0, 107.0, 114.0, 131.0]);
        assert!(out.glyphs.iter().all(|g| g.origin.y == 700.0));
        assert_eq!(out.glyphs[0].text, "A");
        assert_eq!(
            out.shows[&5],
            [
                ShowItem::Glyph(2),
                ShowItem::Kern(-1000.0),
                ShowItem::Glyph(3)
            ]
        );
        assert_eq!(
            out.text_objects,
            [TextObject {
                begin: 0,
                end: Some(6)
            }]
        );
        let bbox = out.glyphs[0].bbox;
        assert_eq!(
            (bbox.x0, bbox.y0, bbox.x1, bbox.y1),
            (100.0, 698.0, 105.0, 708.0)
        );
    }

    #[test]
    fn espacement_des_mots_et_lignes() {
        let out = run(b"BT /F1 10 Tf 14 TL 50 500 Td 3 Tw (A B) Tj T* (C) Tj 2 1 (D) \" ET");
        let g = &out.glyphs;
        assert_eq!(g[2].origin.x, 50.0 + 5.0 + 5.0 + 3.0);
        assert_eq!((g[3].origin.x, g[3].origin.y), (50.0, 486.0));
        // « " » fixe Tw et Tc puis passe à la ligne.
        assert_eq!((g[4].origin.x, g[4].origin.y), (50.0, 472.0));
        assert_eq!(out.states[g[4].state].char_spacing, 1.0);
    }

    #[test]
    fn transformation_et_echelle() {
        let out = run(b"q 2 0 0 2 10 10 cm BT /F1 10 Tf 50 Tz 1 0 0 1 5 5 Tm (AA) Tj ET Q");
        let g = &out.glyphs;
        assert_eq!((g[0].origin.x, g[0].origin.y), (20.0, 20.0));
        // 5 points de chasse à 50 %, puis ×2 par la CTM.
        assert_eq!(g[1].origin.x, 25.0);
        assert_eq!(out.groups, [(0, 8)]);
    }

    #[test]
    fn traces_images_et_detourage() {
        let out = run(b"q 0 0 100 100 re W n 1 0 0 RG 10 10 m 300 10 l S Q \
              q 50 0 0 40 20 30 cm /Im1 Do Q /Fm1 Do 0 0 10 10 re f");
        assert_eq!(out.units.len(), 4);
        let line = &out.units[0];
        assert_eq!(line.kind, UnitKind::Path);
        // Le trait est coupé par le détourage à 100.
        assert_eq!(line.bbox.x1, 100.0);
        assert_eq!(line.ops, 5..8);
        let image = &out.units[1];
        assert_eq!(image.kind, UnitKind::Image);
        assert_eq!(
            (image.bbox.x0, image.bbox.y0, image.bbox.x1, image.bbox.y1),
            (20.0, 30.0, 70.0, 70.0)
        );
        assert_eq!(out.units[2].kind, UnitKind::Form);
        assert_eq!(out.units[2].bbox.x1, 100.0);
        assert_eq!(out.units[3].ops, 14..16);
    }

    #[test]
    fn sous_chemins_d_un_trace() {
        let out =
            run(b"q 2 0 0 2 0 0 cm 0 0 m 10 0 l 10 5 l h 20 20 5 5 re 30 30 l 1 1 2 2 3 3 c S Q");
        let unit = &out.units[0];
        assert!(unit.stroked && !unit.filled);
        assert_eq!(unit.subpaths.len(), 3);
        let triangle = &unit.subpaths[0];
        assert_eq!(triangle.ops, 2..6);
        assert!(triangle.closed && triangle.straight && !triangle.rect);
        assert_eq!(triangle.items[1].points, [Point::new(20.0, 0.0)]);
        let rect = &unit.subpaths[1];
        assert!(rect.rect);
        assert_eq!(rect.bbox(), Rect::new(40.0, 40.0, 50.0, 50.0));
        // Après un rectangle, un segment part de son origine.
        let tail = &unit.subpaths[2];
        assert_eq!(tail.ops, 7..9);
        assert_eq!(tail.items[0].points, [Point::new(40.0, 40.0)]);
        assert!(!tail.straight);
    }

    #[test]
    fn texte_invisible() {
        let out = run(b"BT /F1 10 Tf 3 Tr (A) Tj 0 Tr (B) Tj ET");
        assert!(!out.glyphs[0].visible);
        assert!(out.glyphs[1].visible);
    }
}
