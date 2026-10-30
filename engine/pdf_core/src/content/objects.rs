//! Objets éditables d'une page : blocs de texte, images, tracés, formulaires et
//! dégradés, dans l'ordre d'empilement.
//!
//! Les glyphes sont réunis en lignes (même ligne de base, sans grand écart ni
//! changement de style à distance), puis les lignes en blocs (même style,
//! interligne régulier, lignes alignées) : un paragraphe forme un bloc, que
//! l'on peut modifier d'un seul tenant. Les tracés qui se touchent et se suivent dans le
//! flux forment un seul objet (un logo, la grille d'un tableau).
//!
//! Les tableaux sont détectés d'abord (crate `layout`), d'après les filets des
//! tracés et la position des glyphes : une ligne de texte ne passe pas d'une
//! cellule à l'autre et un bloc reste dans sa cellule. Chaque tableau forme un
//! objet de plus, qui réunit les objets de ses cellules.

use std::ops::Range;

use super::geometry::{Point, Rect};
use super::interp::{Glyph, Interpretation, ShowItem, Subpath, UnitKind};
use super::lexer::Operation;

/// Nature d'un objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// Bloc de texte.
    Text,
    /// Image.
    Image,
    /// Tracés vectoriels.
    Path,
    /// Formulaire (contenu réutilisable, déplacé d'un bloc).
    Form,
    /// Dégradé.
    Shading,
    /// Tableau : réunit le texte de ses cellules, ses filets et ses fonds.
    Table,
}

/// Alignement des lignes d'un bloc de texte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Aligné à gauche (premier alinéa éventuel conservé).
    Left,
    /// Aligné à droite.
    Right,
    /// Centré.
    Center,
    /// Justifié : lignes pleines, sauf la dernière.
    Justified,
}

/// Une ligne de texte.
#[derive(Clone, Debug)]
pub struct TextLine {
    /// Glyphes de la ligne, dans l'ordre.
    pub glyphs: Vec<usize>,
    /// Début de la ligne de base.
    pub start: Point,
    /// Fin du dernier glyphe.
    pub end: Point,
    /// Début du premier glyphe visible qui n'est pas une espace (`start` si la
    /// ligne n'a que des espaces) : les espaces de tête ne comptent pas pour
    /// l'alignement.
    pub left: Point,
    /// Fin du dernier glyphe qui n'est pas une espace (`end` à défaut).
    pub right: Point,
    /// Glyphe qui porte le style dominant de la ligne (police, corps et
    /// couleur du plus grand nombre de glyphes autres que des espaces) ; le
    /// premier glyphe si la ligne n'a que des espaces.
    pub style: usize,
    /// Corps du texte dans l'espace de la page.
    pub size: f64,
    /// Direction de la ligne de base (vecteur unitaire).
    pub direction: (f64, f64),
    /// Texte de la ligne.
    pub text: String,
    /// Emprise.
    pub bbox: Rect,
}

impl TextLine {
    fn along(&self, point: Point) -> f64 {
        point.x * self.direction.0 + point.y * self.direction.1
    }

    fn across(&self, point: Point) -> f64 {
        -point.x * self.direction.1 + point.y * self.direction.0
    }
}

/// Bloc de texte : un paragraphe ou une ligne isolée.
#[derive(Clone, Debug)]
pub struct TextBlock {
    /// Lignes, de haut en bas.
    pub lines: Vec<TextLine>,
    /// Alignement détecté.
    pub alignment: Alignment,
    /// Distance entre deux lignes de base, dans l'espace de la page (0 pour une
    /// ligne isolée).
    pub leading: f64,
    /// Corps de la première ligne, dans l'espace de la page.
    pub size: f64,
    /// Texte du bloc : retours à la ligne voulus conservés, coupures de mots
    /// en fin de ligne recollées.
    pub text: String,
    /// Emprise de la cellule de tableau qui contient le bloc, dans l'espace
    /// de la page.
    pub cell: Option<Rect>,
}

/// Objet éditable d'une page.
#[derive(Clone, Debug)]
pub struct PageObject {
    /// Numéro dans l'ordre d'empilement, à partir de 0.
    pub id: u32,
    /// Nature.
    pub kind: ObjectKind,
    /// Emprise dans l'espace de la page PDF.
    pub bbox: Rect,
    /// Première opération qui le dessine (ordre d'empilement).
    pub first_op: usize,
    /// Texte, pour un bloc de texte.
    pub text: Option<TextBlock>,
    /// Éléments dessinés (tracés, images…), pour un objet autre que du texte.
    pub units: Vec<usize>,
    /// Plages d'opérations à déplacer ou supprimer d'un bloc (autre que texte).
    pub segments: Vec<Range<usize>>,
    /// L'objet peut être déplacé et supprimé.
    pub movable: bool,
    /// Le texte peut être remplacé.
    pub editable: bool,
    /// Fond qui couvre presque toute la page : pas sélectionné par un clic.
    pub background: bool,
    /// Numéro de l'objet tableau dont l'objet occupe une cellule.
    pub table: Option<u32>,
    /// Structure du tableau, pour un objet tableau.
    pub structure: Option<layout::Table>,
}

/// Part de la page au-delà de laquelle un objet est un fond.
const BACKGROUND_SHARE: f64 = 0.9;

/// Cellule de chaque glyphe : (tableau, cellule), par indice de glyphe.
type GlyphCells = Vec<Option<(usize, usize)>>;

/// Établit les objets d'une page interprétée.
pub fn build(ops: &[Operation], run: &Interpretation, page_box: Rect) -> Vec<PageObject> {
    let tables = tables(run, page_box);
    let cells = glyph_cells(run, &tables);
    let mut objects = Vec::new();
    for block in blocks_with(run, &cells, &tables) {
        let glyphs: Vec<usize> = block
            .lines
            .iter()
            .flat_map(|l| l.glyphs.iter().copied())
            .collect();
        let first = glyphs.iter().map(|&g| run.glyphs[g].op).min().unwrap_or(0);
        let bbox = block
            .lines
            .iter()
            .fold(Rect::EMPTY, |rect, line| rect.union(&line.bbox));
        let movable = glyphs.iter().all(|&g| glyph_movable(run, g));
        let editable = movable && glyphs.iter().all(|&g| !run.glyphs[g].text.is_empty());
        objects.push(PageObject {
            id: 0,
            kind: ObjectKind::Text,
            bbox,
            first_op: first,
            text: Some(block),
            units: Vec::new(),
            segments: Vec::new(),
            movable,
            editable,
            background: false,
            table: None,
            structure: None,
        });
    }
    for units in unit_groups(run, page_box) {
        let first = &run.units[units[0]];
        let kind = match first.kind {
            UnitKind::Path => ObjectKind::Path,
            UnitKind::Image | UnitKind::InlineImage => ObjectKind::Image,
            UnitKind::Form => ObjectKind::Form,
            UnitKind::Shading => ObjectKind::Shading,
        };
        let bbox = units
            .iter()
            .fold(Rect::EMPTY, |rect, &u| rect.union(&run.units[u].bbox));
        objects.push(PageObject {
            id: 0,
            kind,
            bbox,
            first_op: first.ops.start,
            text: None,
            units,
            segments: Vec::new(),
            movable: true,
            editable: false,
            background: false,
            table: None,
            structure: None,
        });
    }
    objects.sort_by_key(|object| {
        let first_glyph = object
            .text
            .as_ref()
            .and_then(|t| t.lines.first())
            .and_then(|l| l.glyphs.first().copied())
            .unwrap_or(0);
        (object.first_op, first_glyph)
    });
    let page_area = page_box.area();
    for (id, object) in objects.iter_mut().enumerate() {
        object.id = id as u32;
        object.background = page_area > 0.0
            && object.kind != ObjectKind::Text
            && object.bbox.area() >= BACKGROUND_SHARE * page_area;
    }
    assign_segments(ops, run, &mut objects);
    add_tables(&cells, tables, &mut objects);
    objects
}

/// Ajoute un objet par tableau, après les objets dessinés, et lui rattache
/// les objets de ses cellules : les blocs de texte de ses cellules et les
/// autres objets compris dans son emprise.
fn add_tables(cells: &GlyphCells, tables: Vec<layout::Table>, objects: &mut Vec<PageObject>) {
    for (index, table) in tables.into_iter().enumerate() {
        let id = objects.len() as u32;
        let bbox = Rect::from(table.bbox());
        let area = bbox.inflate(1.5);
        let mut members = Vec::new();
        for object in objects.iter_mut() {
            let inside = match &object.text {
                Some(block) => block_cell(cells, block).is_some_and(|(t, _)| t == index),
                None => {
                    object.kind != ObjectKind::Table
                        && !object.background
                        && !object.bbox.is_empty()
                        && object.bbox.x0 >= area.x0
                        && object.bbox.x1 <= area.x1
                        && object.bbox.y0 >= area.y0
                        && object.bbox.y1 <= area.y1
                }
            };
            if inside && object.table.is_none() {
                object.table = Some(id);
                members.push(object.id as usize);
            }
        }
        let movable = members.iter().all(|&m| objects[m].movable);
        let first_op = members
            .iter()
            .map(|&m| objects[m].first_op)
            .min()
            .unwrap_or(0);
        objects.push(PageObject {
            id,
            kind: ObjectKind::Table,
            bbox,
            first_op,
            text: None,
            units: Vec::new(),
            segments: Vec::new(),
            movable,
            editable: false,
            background: false,
            table: None,
            structure: Some(table),
        });
    }
}

// MARK: - Tableaux

/// Écart maximal d'un rectangle plein à un filet, en points : au-delà, ses
/// quatre bords comptent (fond d'une cellule).
const THIN_RULE: f64 = 2.5;

/// Tableaux de la page, d'après les filets de ses tracés et ses glyphes.
pub fn tables(run: &Interpretation, page_box: Rect) -> Vec<layout::Table> {
    let page_area = page_box.area();
    let mut segments: Vec<layout::Segment> = Vec::new();
    let point = |p: Point| (p.x, p.y);
    for unit in &run.units {
        if unit.kind != UnitKind::Path
            || unit.bbox.is_empty()
            || (page_area > 0.0 && unit.bbox.area() >= BACKGROUND_SHARE * page_area)
        {
            continue;
        }
        for subpath in unit.subpaths.iter().filter(|s| s.straight) {
            let points: Vec<Point> = subpath.points().collect();
            if points.len() < 2 {
                continue;
            }
            if unit.stroked {
                if subpath.rect {
                    segments.extend(layout::Segment::edges(&subpath.bbox().into()));
                    continue;
                }
                for pair in points.windows(2) {
                    segments.push(layout::Segment::new(point(pair[0]), point(pair[1])));
                }
                if subpath.closed {
                    segments.push(layout::Segment::new(
                        point(points[points.len() - 1]),
                        point(points[0]),
                    ));
                }
            } else if unit.filled && is_box(subpath, &points) {
                let rect = subpath.bbox();
                if rect.width().min(rect.height()) <= THIN_RULE {
                    let (cx, cy) = ((rect.x0 + rect.x1) / 2.0, (rect.y0 + rect.y1) / 2.0);
                    segments.push(if rect.width() >= rect.height() {
                        layout::Segment::new((rect.x0, cy), (rect.x1, cy))
                    } else {
                        layout::Segment::new((cx, rect.y0), (cx, rect.y1))
                    });
                } else {
                    segments.extend(layout::Segment::edges(&rect.into()));
                }
            }
        }
    }
    let boxes: Vec<layout::TextBox> = run
        .glyphs
        .iter()
        .filter(|glyph| glyph.visible && is_ink(glyph) && !glyph.bbox.is_empty())
        .filter(|glyph| glyph.trm.b.abs() <= 0.035 * glyph.trm.a.abs() && glyph.trm.a > 0.0)
        .map(|glyph| layout::TextBox {
            bbox: glyph.bbox.into(),
            baseline: glyph.baseline.y,
            size: glyph.trm.vertical_scale(),
        })
        .collect();
    if segments.is_empty() {
        return Vec::new();
    }
    layout::detect(&segments, &boxes)
}

/// Vrai si le sous-chemin est un rectangle aux côtés parallèles aux axes.
fn is_box(subpath: &Subpath, points: &[Point]) -> bool {
    if subpath.rect {
        return true;
    }
    let rect = subpath.bbox();
    let on_edge = |p: &Point| {
        let near = |a: f64, b: f64| (a - b).abs() <= 0.5;
        (near(p.x, rect.x0) || near(p.x, rect.x1)) && (near(p.y, rect.y0) || near(p.y, rect.y1))
    };
    (4..=5).contains(&points.len()) && points.iter().all(on_edge)
}

/// Cellule de chaque glyphe visible, d'après son centre.
fn glyph_cells(run: &Interpretation, tables: &[layout::Table]) -> GlyphCells {
    if tables.is_empty() {
        return Vec::new();
    }
    let boxes: Vec<layout::Rect> = tables.iter().map(layout::Table::bbox).collect();
    run.glyphs
        .iter()
        .map(|glyph| {
            if !glyph.visible || glyph.bbox.is_empty() {
                return None;
            }
            let (x, y) = (
                (glyph.bbox.x0 + glyph.bbox.x1) / 2.0,
                (glyph.bbox.y0 + glyph.bbox.y1) / 2.0,
            );
            tables.iter().enumerate().find_map(|(index, table)| {
                if !boxes[index].contains(x, y) {
                    return None;
                }
                table.cell_at(x, y).map(|cell| (index, cell))
            })
        })
        .collect()
}

/// Cellule d'un bloc : celle du glyphe de son style.
fn block_cell(cells: &GlyphCells, block: &TextBlock) -> Option<(usize, usize)> {
    block
        .lines
        .first()
        .and_then(|line| cells.get(line.style).copied().flatten())
}

/// Vrai si le glyphe peut être déplacé ou effacé sans fausser le reste :
/// métriques connues, écriture horizontale, pas de détourage par le texte.
fn glyph_movable(run: &Interpretation, glyph: usize) -> bool {
    let glyph = &run.glyphs[glyph];
    let state = &run.states[glyph.state];
    let font_ok = state
        .font
        .as_ref()
        .is_some_and(|font| font.supported && !font.vertical);
    font_ok && state.font_size != 0.0 && state.h_scale != 0.0 && state.render_mode < 4
}

// MARK: - Texte

fn unit_vector(x: f64, y: f64) -> (f64, f64) {
    let length = x.hypot(y);
    if length < 1e-9 {
        (1.0, 0.0)
    } else {
        (x / length, y / length)
    }
}

/// Lignes de texte, dans l'ordre du flux.
pub fn lines(run: &Interpretation) -> Vec<TextLine> {
    lines_with(run, &Vec::new())
}

/// Lignes de texte, dans l'ordre du flux ; une ligne ne passe pas d'une
/// cellule de tableau à une autre.
fn lines_with(run: &Interpretation, cells: &GlyphCells) -> Vec<TextLine> {
    let cell = |g: usize| cells.get(g).copied().flatten();
    let mut lines: Vec<TextLine> = Vec::new();
    let mut current: Option<TextLine> = None;
    for (index, glyph) in run.glyphs.iter().enumerate() {
        if !glyph.visible {
            continue;
        }
        let size = glyph.trm.vertical_scale();
        let direction = unit_vector(glyph.trm.a, glyph.trm.b);
        if let Some(line) = &mut current {
            let previous = &run.glyphs[*line.glyphs.last().unwrap_or(&index)];
            let same_direction =
                (direction.0 * line.direction.1 - direction.1 * line.direction.0).abs() < 0.035
                    && direction.0 * line.direction.0 + direction.1 * line.direction.1 > 0.0;
            let reference = line.size.max(size).max(1e-6);
            let gap = line.along(glyph.origin) - line.along(previous.end);
            let shift = (line.across(glyph.baseline) - line.across(previous.baseline)).abs();
            // Un changement de style après un écart plus large qu'une espace
            // (une étiquette suivie de sa valeur) commence une autre ligne ;
            // un mot en gras dans une phrase reste dans sa ligne. Un
            // changement de police et de corps à la fois, séparé au moins par
            // une espace (« imh » en gras, gros, puis « FACTURE V 102-26 »),
            // commence aussi une autre ligne.
            let restyled = is_ink(glyph)
                && line
                    .glyphs
                    .iter()
                    .rev()
                    .find(|&&g| is_ink(&run.glyphs[g]))
                    .is_some_and(|&ink| {
                        let distance = line.along(glyph.origin) - line.along(run.glyphs[ink].end);
                        !same_style(run, ink, index)
                            && (distance > reference
                                || (distance > 0.15 * reference
                                    && font_and_size_differ(run, ink, index)))
                    });
            // Le texte d'une autre cellule commence une autre ligne.
            let crosses = is_ink(glyph)
                && line
                    .glyphs
                    .iter()
                    .rev()
                    .find(|&&g| is_ink(&run.glyphs[g]))
                    .is_some_and(|&ink| cell(ink) != cell(index));
            if same_direction
                && !restyled
                && !crosses
                && shift <= 0.3 * reference
                && gap <= 1.5 * reference
                && gap >= -0.5 * reference
            {
                if gap > 0.18 * reference
                    && !line.text.ends_with(char::is_whitespace)
                    && !glyph.text.starts_with(char::is_whitespace)
                {
                    line.text.push(' ');
                }
                line.text.push_str(&glyph.text);
                line.glyphs.push(index);
                line.end = glyph.end;
                line.bbox = line.bbox.union(&glyph.bbox);
                line.size = line.size.max(size);
                continue;
            }
            lines.extend(current.take());
        }
        current = Some(TextLine {
            glyphs: vec![index],
            start: glyph.baseline,
            end: glyph.end,
            left: glyph.baseline,
            right: glyph.end,
            style: index,
            size,
            direction,
            text: glyph.text.clone(),
            bbox: glyph.bbox,
        });
    }
    lines.extend(current);
    for line in &mut lines {
        let ink: Vec<usize> = line
            .glyphs
            .iter()
            .copied()
            .filter(|&g| is_ink(&run.glyphs[g]))
            .collect();
        if let (Some(&first), Some(&last)) = (ink.first(), ink.last()) {
            line.left = run.glyphs[first].baseline;
            line.right = run.glyphs[last].end;
            // Le premier glyphe du style le plus représenté.
            let count = |g: usize| ink.iter().filter(|&&o| same_style(run, g, o)).count();
            let mut best = (first, 0);
            for &g in &ink {
                if best.1 > 0 && same_style(run, best.0, g) {
                    continue;
                }
                let n = count(g);
                if n > best.1 {
                    best = (g, n);
                }
            }
            line.style = best.0;
        }
    }
    lines
}

/// Vrai si le glyphe se voit : autre chose qu'une espace (un glyphe sans
/// texte connu compte comme visible).
fn is_ink(glyph: &Glyph) -> bool {
    glyph.text.is_empty() || !glyph.text.chars().all(char::is_whitespace)
}

/// Vrai si deux glyphes ont la même police, le même corps et la même couleur
/// de remplissage.
fn same_style(run: &Interpretation, a: usize, b: usize) -> bool {
    let (ga, gb) = (&run.glyphs[a], &run.glyphs[b]);
    let (sa, sb) = (&run.states[ga.state], &run.states[gb.state]);
    let font = match (&sa.font, &sb.font) {
        (Some(x), Some(y)) => x.base_name == y.base_name,
        (None, None) => sa.font_name == sb.font_name,
        _ => false,
    };
    let (za, zb) = (ga.trm.vertical_scale(), gb.trm.vertical_scale());
    font && (za - zb).abs() <= 0.05 * za.max(zb) && sa.fill == sb.fill
}

/// Vrai si deux glyphes n'ont ni la même police ni le même corps.
fn font_and_size_differ(run: &Interpretation, a: usize, b: usize) -> bool {
    let (ga, gb) = (&run.glyphs[a], &run.glyphs[b]);
    let (sa, sb) = (&run.states[ga.state], &run.states[gb.state]);
    let font = match (&sa.font, &sb.font) {
        (Some(x), Some(y)) => x.base_name != y.base_name,
        (None, None) => sa.font_name != sb.font_name,
        _ => true,
    };
    let (za, zb) = (ga.trm.vertical_scale(), gb.trm.vertical_scale());
    font && (za - zb).abs() > 0.05 * za.max(zb)
}

/// Nombre de blocs récents auxquels une ligne peut encore se joindre (texte
/// en colonnes entrelacées dans le flux).
const OPEN_BLOCKS: usize = 8;

/// Blocs de texte, dans l'ordre du flux de leur première ligne.
///
/// Une ligne rejoint, parmi les blocs encore ouverts qu'elle peut continuer,
/// celui dont elle est le mieux alignée ; à égalité, le plus récent.
pub fn blocks(run: &Interpretation) -> Vec<TextBlock> {
    blocks_with(run, &Vec::new(), &[])
}

/// Blocs de texte ; un bloc ne réunit que des lignes d'une même cellule.
fn blocks_with(
    run: &Interpretation,
    cells: &GlyphCells,
    tables: &[layout::Table],
) -> Vec<TextBlock> {
    let cell = |line: &TextLine| cells.get(line.style).copied().flatten();
    let mut groups: Vec<Vec<TextLine>> = Vec::new();
    // Une ligne faite d'espaces ne dessine rien : elle ne forme pas de bloc.
    let visible = lines_with(run, cells)
        .into_iter()
        .filter(|line| line.glyphs.iter().any(|&g| is_ink(&run.glyphs[g])));
    for line in visible {
        let open = groups.len().saturating_sub(OPEN_BLOCKS);
        let mut best: Option<(usize, f64)> = None;
        for index in (open..groups.len()).rev() {
            if cell(&groups[index][0]) != cell(&line) {
                continue;
            }
            if let Some(score) = joins(run, &groups[index], &line) {
                if best.is_none_or(|(_, top)| score < top) {
                    best = Some((index, score));
                }
            }
        }
        match best {
            Some((index, _)) => groups[index].push(line),
            None => groups.push(vec![line]),
        }
    }
    groups
        .into_iter()
        .map(|lines| {
            let rect = cell(&lines[0]).map(|(table, index)| {
                let table = &tables[table];
                Rect::from(table.cell_rect(&table.cells[index]))
            });
            finish_block(run, lines, rect)
        })
        .collect()
}

/// Si `line` peut continuer le bloc formé par `group`, écart entre leurs bords
/// alignés (plus il est faible, mieux la ligne s'y rattache).
///
/// La ligne doit être de même direction et de même style dominant (police,
/// corps, couleur), sous la dernière ligne à un interligne cohérent, et
/// alignée sur elle : même bord gauche,
/// même bord droit ou même centre. Une ligne isolée accepte aussi une suivante
/// qui commence un peu plus à gauche (alinéa) ou un peu plus à droite (retrait
/// d'une liste). Deux colonnes voisines, une étiquette et sa valeur par
/// exemple, restent ainsi des blocs distincts.
fn joins(run: &Interpretation, group: &[TextLine], line: &TextLine) -> Option<f64> {
    let first = &group[0];
    let last = &group[group.len() - 1];
    let (u, v) = (first.direction, line.direction);
    if (u.0 * v.1 - u.1 * v.0).abs() > 0.035 || u.0 * v.0 + u.1 * v.1 <= 0.0 {
        return None;
    }
    let size = first.size.max(1e-6);
    if !same_style(run, last.style, line.style) {
        return None;
    }
    // Distance entre lignes de base : la nouvelle ligne est sous la précédente.
    let step = first.across(last.start) - first.across(line.start);
    if !(0.8 * size..=2.0 * size).contains(&step) {
        return None;
    }
    if group.len() >= 2 {
        let leading = first.across(group[0].start) - first.across(group[1].start);
        if (step - leading).abs() > 0.15 * size {
            return None;
        }
    }
    let (b0, b1) = (first.along(last.left), first.along(last.right));
    let (l0, l1) = (first.along(line.left), first.along(line.right));
    let overlap = b1.min(l1) - b0.max(l0);
    if overlap <= 0.0 {
        return None;
    }
    let tolerance = 0.5 + 0.15 * size;
    let edges = [
        (b0 - l0).abs(),
        (b1 - l1).abs(),
        ((b0 + b1) - (l0 + l1)).abs() / 2.0,
    ];
    let aligned = edges.into_iter().fold(f64::INFINITY, f64::min);
    if aligned <= tolerance {
        return Some(aligned);
    }
    let indent = b0 - l0;
    let indented = group.len() == 1
        && (-2.0 * size..=4.0 * size).contains(&indent)
        && overlap >= 0.3 * (b1 - b0).min(l1 - l0);
    indented.then_some(tolerance + indent.abs())
}

fn finish_block(run: &Interpretation, lines: Vec<TextLine>, cell: Option<Rect>) -> TextBlock {
    let first = &lines[0];
    let size = first.size;
    let leading = if lines.len() >= 2 {
        let steps: Vec<f64> = lines
            .windows(2)
            .map(|pair| first.across(pair[0].start) - first.across(pair[1].start))
            .collect();
        median(steps)
    } else {
        0.0
    };
    let alignment = match cell {
        Some(rect) if lines.len() == 1 => alignment_in_cell(first, rect),
        _ => alignment(&lines),
    };
    let text = block_text(run, &lines, alignment);
    TextBlock {
        lines,
        alignment,
        leading,
        size,
        text,
        cell,
    }
}

/// Alignement d'une ligne seule dans sa cellule, d'après les marges qu'elle
/// y laisse à gauche et à droite.
fn alignment_in_cell(line: &TextLine, cell: Rect) -> Alignment {
    let left = line.left.x - cell.x0;
    let right = cell.x1 - line.right.x;
    let tolerance = (0.1 * (left + right)).max(1.5);
    if (left - right).abs() <= tolerance && left > 1.0 && right > 1.0 {
        Alignment::Center
    } else if right < left {
        Alignment::Right
    } else {
        Alignment::Left
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// Largeur minimale d'un paragraphe, en cadratins : en deçà, des lignes
/// pleines sont une colonne de valeurs (un tableau, un encadré), pas un texte
/// coupé par manque de place.
const PARAGRAPH_EMS: f64 = 12.0;

fn alignment(lines: &[TextLine]) -> Alignment {
    if lines.len() < 2 {
        return Alignment::Left;
    }
    let first = &lines[0];
    let tolerance = 0.5 + 0.1 * first.size;
    let extents: Vec<(f64, f64)> = lines
        .iter()
        .map(|l| (first.along(l.left), first.along(l.right)))
        .collect();
    let close = |values: &mut dyn Iterator<Item = f64>| {
        let values: Vec<f64> = values.collect();
        let (lo, hi) = values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        values.is_empty() || hi - lo <= tolerance
    };
    // Bord gauche : à partir de la deuxième ligne (alinéa de la première).
    let left =
        close(&mut extents.iter().skip(1).map(|e| e.0)) && extents[0].0 >= extents[1].0 - tolerance;
    let full_rights = close(&mut extents[..extents.len() - 1].iter().map(|e| e.1));
    let all_rights = close(&mut extents.iter().map(|e| e.1));
    let centers = close(&mut extents.iter().map(|e| (e.0 + e.1) / 2.0));
    let all_lefts = close(&mut extents.iter().map(|e| e.0));
    // Une colonne de valeurs de même largeur n'est pas un paragraphe justifié.
    let wide = extents
        .iter()
        .any(|e| e.1 - e.0 >= PARAGRAPH_EMS * first.size);
    if wide && left && full_rights && lines.len() >= 3 {
        Alignment::Justified
    } else if wide && lines.len() == 2 && left && extents[0].1 > extents[1].1 + tolerance {
        // Deux lignes, la première pleine et la seconde plus courte.
        Alignment::Justified
    } else if all_lefts || left {
        Alignment::Left
    } else if all_rights {
        Alignment::Right
    } else if centers {
        Alignment::Center
    } else {
        Alignment::Left
    }
}

/// Texte du bloc : les lignes coupées par manque de place sont jointes par
/// une espace (un mot coupé en fin de ligne est recollé) ; les autres se
/// terminent par un retour à la ligne.
///
/// Une ligne est coupée par manque de place si elle est pleine (au moins 80 %
/// de la plus longue, et une autre ligne l'est aussi : la largeur du bloc est
/// alors celle d'une colonne et non celle d'une ligne isolée), que le premier
/// mot de la suivante n'y tenait pas et que le bloc est un paragraphe (voir
/// plus bas). Les espaces en début et en fin de ligne sont omises. Dans le doute, le retour à la ligne est
/// gardé : une adresse ne doit pas devenir une seule ligne.
fn block_text(run: &Interpretation, lines: &[TextLine], alignment: Alignment) -> String {
    let first = &lines[0];
    let widths: Vec<f64> = lines
        .iter()
        .map(|l| first.along(l.right) - first.along(l.start))
        .collect();
    let max_width = widths.iter().copied().fold(0.0, f64::max);
    let full: Vec<bool> = widths.iter().map(|&w| w >= 0.8 * max_width).collect();
    // Un paragraphe est assez large et a au moins la moitié de ses lignes
    // pleines, la dernière exceptée ; sinon c'est une liste (colonne
    // d'étiquettes ou de valeurs, adresse), dont chaque ligne est voulue.
    let wrapped = full[..lines.len() - 1].iter().filter(|&&f| f).count();
    let paragraph = 2 * wrapped >= lines.len() - 1 && max_width >= PARAGRAPH_EMS * first.size;
    let mut text = String::new();
    for (index, line) in lines.iter().enumerate() {
        let line_text = line.text.trim();
        if index == 0 {
            text.push_str(line_text);
            continue;
        }
        let previous = index - 1;
        let hard = match alignment {
            _ if !paragraph => true,
            Alignment::Justified => false,
            _ => {
                let other_full = (0..lines.len() - 1).any(|j| j != previous && full[j]);
                let free = max_width - widths[previous];
                let fits = free >= first_word_width(run, line) + 0.25 * first.size;
                !full[previous] || !other_full || fits
            }
        };
        if hard {
            text.push('\n');
            text.push_str(line_text);
            continue;
        }
        let rejoin = text.ends_with('-')
            && text.chars().rev().nth(1).is_some_and(char::is_alphabetic)
            && line_text.starts_with(char::is_lowercase);
        if rejoin || text.ends_with('\u{00AD}') {
            text.pop();
        } else if !text.ends_with(' ') {
            text.push(' ');
        }
        text.push_str(line_text);
    }
    text
}

/// Largeur du premier mot d'une ligne (espaces de tête exclues), le long de
/// sa ligne de base.
fn first_word_width(run: &Interpretation, line: &TextLine) -> f64 {
    let begin = line.along(line.left);
    let mut end = begin;
    let mut previous_end: Option<f64> = None;
    for &g in &line.glyphs {
        let glyph = &run.glyphs[g];
        if glyph.text.chars().all(char::is_whitespace) && !glyph.text.is_empty() {
            if previous_end.is_some() {
                break;
            }
            continue;
        }
        let start = line.along(glyph.origin);
        if previous_end.is_some_and(|previous| start - previous > 0.18 * line.size) {
            break;
        }
        end = line.along(glyph.end);
        previous_end = Some(end);
    }
    end - begin
}

// MARK: - Tracés, images, formulaires

/// Éléments dessinés réunis par objet : les tracés consécutifs qui se touchent
/// forment un objet, chaque autre élément un objet à lui seul.
fn unit_groups(run: &Interpretation, page_box: Rect) -> Vec<Vec<usize>> {
    let page_area = page_box.area();
    let is_large = |rect: &Rect| page_area > 0.0 && rect.area() >= BACKGROUND_SHARE * page_area;
    // Opérations de texte, pour savoir si du texte s'intercale entre deux tracés.
    let mut text_ops: Vec<usize> = run.shows.keys().copied().collect();
    text_ops.sort_unstable();
    let text_between = |from: usize, to: usize| {
        let at = text_ops.partition_point(|&op| op < from);
        text_ops.get(at).is_some_and(|&op| op < to)
    };

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_bbox = Rect::EMPTY;
    for (index, unit) in run.units.iter().enumerate() {
        if unit.bbox.is_empty() {
            continue;
        }
        if let Some(group) = groups.last_mut() {
            let previous = &run.units[*group.last().unwrap_or(&index)];
            let joinable = unit.kind == UnitKind::Path
                && previous.kind == UnitKind::Path
                && !is_large(&unit.bbox)
                && !is_large(&group_bbox)
                && !text_between(previous.ops.end, unit.ops.start)
                && !group_bbox.inflate(1.0).intersect(&unit.bbox).is_empty();
            if joinable {
                group.push(index);
                group_bbox = group_bbox.union(&unit.bbox);
                continue;
            }
        }
        groups.push(vec![index]);
        group_bbox = unit.bbox;
    }
    groups
}

/// Qui dessine une opération.
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    /// Rien n'est dessiné.
    Nobody,
    /// Un seul objet.
    Object(u32),
    /// Plusieurs objets, du texte invisible ou un dessin inconnu.
    Mixed,
}

impl Owner {
    fn merge(self, other: Owner) -> Owner {
        match (self, other) {
            (Owner::Nobody, other) | (other, Owner::Nobody) => other,
            (Owner::Object(a), Owner::Object(b)) if a == b => self,
            _ => Owner::Mixed,
        }
    }
}

/// Calcule les plages d'opérations des objets autres que du texte : chaque
/// élément est étendu au groupe `q … Q` le plus large qui ne dessine que lui
/// (ou d'autres éléments du même objet) ; déplacer ou supprimer un tel groupe
/// ne change rien à l'état graphique du reste de la page.
fn assign_segments(ops: &[Operation], run: &Interpretation, objects: &mut [PageObject]) {
    let mut owners = vec![Owner::Nobody; ops.len()];
    for object in objects.iter() {
        for &u in &object.units {
            for op in run.units[u].ops.clone() {
                owners[op] = owners[op].merge(Owner::Object(object.id));
            }
        }
    }
    let mut glyph_owner = vec![Owner::Mixed; run.glyphs.len()];
    for object in objects.iter() {
        if let Some(block) = &object.text {
            for line in &block.lines {
                for &g in &line.glyphs {
                    glyph_owner[g] = Owner::Object(object.id);
                }
            }
        }
    }
    for (&op, items) in &run.shows {
        for item in items {
            if let ShowItem::Glyph(g) = item {
                owners[op] = owners[op].merge(glyph_owner[*g]);
            }
        }
    }
    for (index, op) in ops.iter().enumerate() {
        // Un formulaire ou une image inconnus dessinent peut-être quelque chose.
        if op.is("Do") && owners[index] == Owner::Nobody {
            owners[index] = Owner::Mixed;
        }
    }

    // Groupes : parent de chacun et résumé de ce qu'il contient.
    let groups = &run.groups;
    let mut enclosing: Vec<Option<usize>> = vec![None; ops.len()];
    let mut opens = vec![None; ops.len()];
    let mut closes = vec![false; ops.len()];
    for (g, &(start, end)) in groups.iter().enumerate() {
        opens[start] = Some(g);
        closes[end] = true;
    }
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..ops.len() {
        if closes[index] {
            stack.pop();
        }
        enclosing[index] = stack.last().copied();
        if let Some(g) = opens[index] {
            stack.push(g);
        }
    }
    let summaries: Vec<(Owner, bool)> = groups
        .iter()
        .map(|&(start, end)| summarize(ops, &owners, start, end))
        .collect();

    for object in objects.iter_mut() {
        if object.text.is_some() {
            continue;
        }
        let mut segments: Vec<Range<usize>> = Vec::new();
        for &u in &object.units {
            let unit = &run.units[u];
            let (mut start, mut end) = (unit.ops.start, unit.ops.end - 1);
            let mut group = enclosing[start];
            while let Some(g) = group {
                let (q, close) = groups[g];
                if close < end {
                    // Groupe fermé avant la fin de l'élément : il ne le contient pas.
                    group = enclosing[q];
                    continue;
                }
                let (owner, balanced) = summaries[g];
                if !balanced || !matches!(owner, Owner::Object(id) if id == object.id) {
                    break;
                }
                start = q;
                end = close;
                group = enclosing[q];
            }
            let range = start..end + 1;
            if segments
                .iter()
                .any(|s| s.start <= range.start && range.end <= s.end)
            {
                continue;
            }
            segments.retain(|s| !(range.start <= s.start && s.end <= range.end));
            segments.push(range);
        }
        segments.sort_by_key(|s| s.start);
        object.segments = segments;
    }
}

/// Ce que dessine un groupe `q … Q`, et s'il est équilibré (objets texte et
/// contenu marqué ouverts et fermés à l'intérieur).
fn summarize(ops: &[Operation], owners: &[Owner], start: usize, end: usize) -> (Owner, bool) {
    let mut owner = Owner::Nobody;
    let (mut text, mut marked) = (0i32, 0i32);
    let mut balanced = true;
    for index in start..=end {
        owner = owner.merge(owners[index]);
        match ops[index].operator.as_slice() {
            b"BT" => text += 1,
            b"ET" => text -= 1,
            b"BMC" | b"BDC" => marked += 1,
            b"EMC" => marked -= 1,
            _ => {}
        }
        if text < 0 || marked < 0 {
            balanced = false;
        }
        if owner == Owner::Mixed {
            break;
        }
    }
    (owner, balanced && text == 0 && marked == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::interp::{interpret, tests::test_font, tests::TestResources};
    use crate::content::lexer::parse;

    fn objects(source: &[u8]) -> (Vec<Operation>, Vec<PageObject>) {
        let ops = parse(source);
        let mut resources = TestResources { font: test_font() };
        let page = Rect::new(0.0, 0.0, 600.0, 800.0);
        let run = interpret(&ops, &mut resources, page);
        let objects = build(&ops, &run, page);
        (ops, objects)
    }

    #[test]
    fn un_paragraphe_justifie_forme_un_bloc() {
        // Trois lignes de même largeur (justifiées), puis une ligne courte, puis
        // un titre plus gros, ailleurs.
        let (_, objects) = objects(
            b"BT /F1 10 Tf 12 TL 100 700 Td (aaaa bbbb cc dddd eeee ff) Tj T* (gggg hhhh ii jjjj kkkk ll) Tj \
              T* (mmmm nnnn oo pppp qqqq rr) Tj T* (ss tt) Tj ET BT /F1 20 Tf 100 750 Td (Titre) Tj ET",
        );
        assert_eq!(objects.len(), 2);
        let block = objects[0].text.as_ref().unwrap();
        assert_eq!(block.lines.len(), 4);
        assert_eq!(block.alignment, Alignment::Justified);
        assert_eq!(block.leading, 12.0);
        assert_eq!(
            block.text,
            "aaaa bbbb cc dddd eeee ff gggg hhhh ii jjjj kkkk ll mmmm nnnn oo pppp qqqq rr ss tt"
        );
        assert_eq!(objects[1].text.as_ref().unwrap().text, "Titre");
        assert!(objects[0].editable && objects[0].movable);
    }

    #[test]
    fn retours_a_la_ligne_voulus() {
        // Adresse : lignes courtes alignées à gauche, le mot suivant tenait.
        let (_, objects) = objects(
            b"BT /F1 10 Tf 12 TL 50 500 Td (Jean Dupont) Tj T* (12 rue) Tj T* (Paris) Tj ET",
        );
        let block = objects[0].text.as_ref().unwrap();
        assert_eq!(block.alignment, Alignment::Left);
        assert_eq!(block.text, "Jean Dupont\n12 rue\nParis");
    }

    #[test]
    fn mots_coupes_et_espaces_par_decalage() {
        let (_, objects) = objects(
            b"BT /F1 10 Tf 12 TL 50 500 Td (le pont de la ville effica-) Tj T* (cite des ponts et des rues) Tj \
              T* (de la ville) Tj ET \
              BT /F1 10 Tf 50 300 Td [(un) -400 (deux)] TJ ET",
        );
        assert_eq!(
            objects[0].text.as_ref().unwrap().text,
            "le pont de la ville efficacite des ponts et des rues de la ville"
        );
        assert_eq!(objects[1].text.as_ref().unwrap().text, "un deux");
    }

    #[test]
    fn colonnes_et_lignes_eloignees() {
        // Deux colonnes entrelacées dans le flux, ligne par ligne.
        let (_, objects) = objects(
            b"BT /F1 10 Tf 50 700 Td (gauche un) Tj 300 0 Td (droite un) Tj -300 -12 Td (gauche deux) Tj \
              300 0 Td (droite deux) Tj ET",
        );
        let texts: Vec<&str> = objects
            .iter()
            .map(|o| o.text.as_ref().unwrap().text.as_str())
            .collect();
        // Lignes courtes : les retours à la ligne sont gardés.
        assert_eq!(texts, ["gauche un\ngauche deux", "droite un\ndroite deux"]);
    }

    #[test]
    fn blocs_homogenes_par_style() {
        let (_, objects) = objects(
            b"BT /F1 10 Tf 50 500 Td (Nom) Tj 1 0 0 rg 30 0 Td (Dupont) Tj ET \
              BT 0 g /F1 10 Tf 50 300 Td (un mot ) Tj 1 0 0 rg (rouge) Tj 0 g ( ici) Tj ET \
              BT /F1 10 Tf 12 TL 50 200 Td (aaa) Tj T* 1 0 0 rg (bbb) Tj ET \
              BT 0 g /F1 10 Tf 50 100 Td (    ) Tj ET",
        );
        let texts: Vec<&str> = objects
            .iter()
            .map(|o| o.text.as_ref().unwrap().text.as_str())
            .collect();
        // Étiquette et valeur d'un autre style, séparées par un écart : deux
        // blocs ; un mot coloré dans une phrase y reste ; deux lignes de
        // couleurs différentes ne se joignent pas ; une ligne d'espaces ne
        // forme pas de bloc.
        assert_eq!(texts, ["Nom", "Dupont", "un mot rouge ici", "aaa", "bbb"]);
    }

    #[test]
    fn mot_de_police_et_de_corps_differents_en_tete_de_ligne() {
        // « imh » en gras et plus gros, puis le titre : deux blocs. Un mot en
        // gras du même corps dans une phrase reste dans son bloc.
        let (_, objects) = objects(
            b"BT /F1 13 Tf 50 700 Td (imh) Tj /F2 12 Tf ( FACTURE V 102-26) Tj ET \
              BT /F1 12 Tf 50 600 Td (une phrase avec ) Tj /F2 12 Tf (un mot) Tj /F1 12 Tf ( en gras) Tj ET",
        );
        let texts: Vec<&str> = objects
            .iter()
            .map(|o| o.text.as_ref().unwrap().text.as_str())
            .collect();
        assert_eq!(
            texts,
            ["imh", "FACTURE V 102-26", "une phrase avec un mot en gras"]
        );
    }

    #[test]
    fn traces_regroupes_et_plages() {
        let (ops, objects) = objects(
            b"q 1 0 0 RG 10 10 m 100 10 l S Q q 100 10 m 100 100 l S Q \
              q 0 0 1 rg 300 300 50 50 re f Q \
              q 50 0 0 40 20 400 cm /Im1 Do Q \
              0 0 600 800 re f",
        );
        assert_eq!(objects.len(), 4);
        // Les deux traits se touchent : un seul objet, deux groupes q … Q.
        assert_eq!(objects[0].kind, ObjectKind::Path);
        assert_eq!(objects[0].units.len(), 2);
        assert_eq!(objects[0].segments, [0..6, 6..11]);
        assert!(ops[objects[0].segments[0].start].is("q"));
        assert_eq!(objects[1].segments.len(), 1);
        assert_eq!(objects[2].kind, ObjectKind::Image);
        assert!(ops[objects[2].segments[0].clone()].first().unwrap().is("q"));
        // Le fond couvre la page.
        assert!(objects[3].background);
        assert!(!objects[2].background);
    }

    #[test]
    fn un_groupe_partage_n_est_pas_etendu() {
        let (_, objects) = objects(b"q 1 0 0 rg 0 0 10 10 re f BT /F1 10 Tf 50 50 Td (A) Tj ET Q");
        let path = objects.iter().find(|o| o.kind == ObjectKind::Path).unwrap();
        // Le groupe contient aussi du texte : seul le tracé est concerné.
        assert_eq!(path.segments, vec![2..4]);
    }
}
