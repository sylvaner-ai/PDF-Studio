//! Édition du contenu des pages avec MuPDF : objets d'une page, choix de la
//! police d'un texte remplacé et application des modifications à une page,
//! pour l'aperçu comme pour l'enregistrement.
//!
//! Les modifications ([`PageEdits`]) ne touchent jamais au document source :
//! elles sont appliquées à une copie du contenu de la page, recalculée à
//! chaque fois à partir du contenu d'origine. Le même code produit l'aperçu et
//! le fichier enregistré, qui ne peuvent donc pas différer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use edit::{FontChoice, PageEdits, ScanText, TableEdit, TextReplacement};
use layout::{Cell, Restructure};
use mupdf::path::PathWalker;
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::text::Text;
use mupdf::{Buffer, ColorParams, Colorspace, Device, Font, NativeDevice};

use crate::content::encodings::BaseEncoding;
use crate::content::font::{code_bytes, FontKind, FontMetrics};
use crate::content::geometry::{Matrix, Rect};
use crate::content::interp::{interpret, only_invisible_text, GraphicsState, Interpretation};
use crate::content::layout::{layout_in, Layout, Shaper};
use crate::content::lexer::{self, Operation};
use crate::content::objects::{self, Alignment, ObjectKind, PageObject};
use crate::content::resources::PageResources;
use crate::content::rewrite::{
    rewrite, without_text, Change, Insertion, NewText, Reshape, ReshapeStep,
};
use crate::content::unfold::{self, Alias, Unfolded};
use crate::error::Result;
use crate::text::PageRect;

/// Taille maximale d'un fichier de police du Mac intégrée au document.
const MAX_FONT_FILE: u64 = 16 * 1024 * 1024;

/// Objets d'une page, établis à partir de son contenu d'origine.
pub struct PageModel {
    /// Contenu de la page, formulaires à texte dépliés.
    source: Vec<u8>,
    /// Ressources des formulaires dépliés (cf. [`unfold`]).
    aliases: Vec<Alias>,
    ops: Vec<Operation>,
    run: Interpretation,
    objects: Vec<PageObject>,
    /// Espace de la page PDF → espace d'affichage (origine en haut à gauche,
    /// rotation `/Rotate` comprise).
    to_display: Matrix,
}

/// Un objet de la page, tel que l'affiche l'application.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectInfo {
    /// Numéro de l'objet (ordre d'empilement).
    pub id: u32,
    /// Nature.
    pub kind: ObjectKind,
    /// Emprise, modifications comprises, dans l'espace d'affichage de la page.
    pub bounds: PageRect,
    /// L'objet peut être déplacé et supprimé.
    pub movable: bool,
    /// Le texte peut être remplacé.
    pub editable: bool,
    /// Fond de page : il n'est pas sélectionné par un clic.
    pub background: bool,
    /// Texte du bloc (le nouveau texte s'il a été remplacé).
    pub text: Option<String>,
    /// Police du texte (celle de substitution si le texte a été remplacé avec
    /// une autre police).
    pub font_name: Option<String>,
    /// Corps du texte, en points.
    pub font_size: f32,
    /// Le nouveau texte est dessiné avec une autre police que l'originale.
    pub substituted: bool,
    /// Le nouveau texte ne tient pas dans le bloc d'origine.
    pub overflow: bool,
    /// Caractères du nouveau texte qu'aucune police disponible ne contient.
    pub missing: String,
    /// Empreinte de l'objet d'origine (nature et emprise), qui permet de
    /// vérifier qu'une modification retrouvée vise toujours le même objet.
    pub fingerprint: String,
    /// Tableau dont l'objet fait partie.
    pub table: Option<u32>,
    /// Grille d'un tableau, modifications comprises.
    pub grid: Option<TableInfo>,
    /// Objet d'une page scannée (pixels de l'image du scan).
    pub scanned: bool,
    /// Texte retouché d'une ligne scannée, avec ses réglages.
    pub scan: Option<ScanText>,
    /// Objet ajouté à la page (texte ou image), avec son style.
    pub added: Option<crate::added::AddedInfo>,
}

/// Grille d'un tableau, telle que l'affiche l'application.
#[derive(Clone, Debug, PartialEq)]
pub struct TableInfo {
    /// Nombre de lignes.
    pub rows: u32,
    /// Nombre de colonnes.
    pub columns: u32,
    /// Lignes d'origine, dans l'ordre actuel.
    pub row_order: Vec<u32>,
    /// Colonnes d'origine, dans l'ordre actuel.
    pub column_order: Vec<u32>,
    /// Nombre de lignes du tableau d'origine.
    pub source_rows: u32,
    /// Nombre de colonnes du tableau d'origine.
    pub source_columns: u32,
    /// Cellules, ligne par ligne.
    pub cells: Vec<CellInfo>,
}

/// Une cellule d'un tableau.
#[derive(Clone, Debug, PartialEq)]
pub struct CellInfo {
    /// Ligne de son coin supérieur gauche.
    pub row: u32,
    /// Colonne de son coin supérieur gauche.
    pub column: u32,
    /// Nombre de lignes couvertes.
    pub row_span: u32,
    /// Nombre de colonnes couvertes.
    pub column_span: u32,
    /// Ligne d'origine de son coin supérieur gauche.
    pub source_row: u32,
    /// Colonne d'origine de son coin supérieur gauche.
    pub source_column: u32,
    /// Emprise, dans l'espace d'affichage de la page.
    pub bounds: PageRect,
    /// Blocs de texte de la cellule.
    pub objects: Vec<u32>,
    /// Texte saisi dans la cellule, vide à l'origine.
    pub text: Option<String>,
    /// Bloc dont reprend le style un texte saisi dans la cellule ; `None` si
    /// la cellule avait un texte, ou si le tableau n'en a aucun.
    pub reference: Option<u32>,
    /// Le texte saisi ne tient pas dans la cellule.
    pub overflow: bool,
}

/// Marge de l'emprise d'un tableau pour ce qui lui appartient, en points.
const TABLE_MARGIN: f64 = 1.5;

/// Tableau de la page et ses modifications.
struct TableState<'a> {
    object: &'a PageObject,
    table: &'a layout::Table,
    edit: Option<&'a TableEdit>,
    /// Supprimé, ou vidé de toutes ses lignes ou colonnes.
    deleted: bool,
    offset: (f64, f64),
    restructure: Option<Restructure>,
}

impl TableState<'_> {
    fn area(&self) -> Rect {
        self.object.bbox.inflate(TABLE_MARGIN)
    }

    fn changed(&self) -> bool {
        self.deleted || self.offset != (0.0, 0.0) || self.restructure.is_some()
    }

    /// Déplacement de ce qui occupe `rect` dans le tableau (une cellule, une
    /// image) ; `None` s'il disparaît.
    fn follow(&self, rect: Rect) -> Option<(f64, f64)> {
        if self.deleted {
            return None;
        }
        let mut offset = self.offset;
        if let Some(restructure) = &self.restructure {
            let rect = layout::Rect::from(rect);
            if restructure.removes(&rect) {
                return None;
            }
            let (dx, dy) = restructure.offset(&rect);
            offset = (offset.0 + dx, offset.1 + dy);
        }
        Some(offset)
    }

    /// Effet du tableau sur les tracés compris dans son emprise.
    fn step(&self) -> ReshapeStep {
        ReshapeStep {
            area: self.area(),
            deleted: self.deleted,
            restructure: self.restructure.clone(),
            offset: self.offset,
        }
    }
}

/// Effet des modifications sur un objet : les siennes et celles de son
/// tableau, ou des tableaux que ses tracés traversent.
#[derive(Clone, Debug, Default)]
struct Effect<'a> {
    deleted: bool,
    offset: (f64, f64),
    text: Option<&'a TextReplacement>,
    reshape: Option<Reshape>,
}

/// Texte saisi dans une cellule vide.
struct CellText<'a> {
    /// Tableau.
    table: u32,
    /// Indice de la cellule dans le tableau d'origine.
    cell: usize,
    /// Bloc dont le texte reprend le style.
    reference: u32,
    /// Cellule où mettre le texte en page, ramenée à la place du bloc de
    /// référence.
    frame: Rect,
    /// Déplacement de la mise en page jusqu'à sa place sur la page.
    offset: (f64, f64),
    replacement: &'a TextReplacement,
}

/// Modifications d'une page traduites objet par objet.
struct Derived<'a> {
    effects: BTreeMap<u32, Effect<'a>>,
    tables: BTreeMap<u32, TableState<'a>>,
    cells: Vec<CellText<'a>>,
}

impl PageModel {
    /// Lit et interprète le contenu de la page `page`, formulaires à texte
    /// dépliés (cf. [`unfold`]).
    pub fn load(page: &PdfObject) -> Result<Self> {
        let page_resources = page.get_dict_inheritable("Resources")?;
        let Unfolded { source, aliases } =
            unfold::unfold(page_contents(page)?, page_resources.as_ref());
        let ops = lexer::parse(&source);
        let page_box = page_box(page)?;
        let mut resources = PageResources::with_aliases(page_resources, &aliases);
        let run = interpret(&ops, &mut resources, page_box);
        let objects = objects::build(&ops, &run, page_box);
        let ctm = page.page_ctm()?;
        let to_display = Matrix::new(
            f64::from(ctm.a),
            f64::from(ctm.b),
            f64::from(ctm.c),
            f64::from(ctm.d),
            f64::from(ctm.e),
            f64::from(ctm.f),
        );
        Ok(Self {
            source,
            aliases,
            ops,
            run,
            objects,
            to_display,
        })
    }

    /// Objets de la page, dans l'ordre d'empilement.
    pub fn objects(&self) -> &[PageObject] {
        &self.objects
    }

    /// Transformation de l'espace de la page PDF vers l'espace d'affichage
    /// (origine en haut à gauche, rotation `/Rotate` comprise).
    pub fn display_matrix(&self) -> Matrix {
        self.to_display
    }

    /// Vrai si le seul texte de la page est invisible, comme la couche de
    /// texte d'un OCR (cf. [`only_invisible_text`]).
    pub fn has_only_invisible_text(&self) -> bool {
        only_invisible_text(&self.ops, &self.run)
    }

    /// Ressources qui accompagnent le contenu du modèle : celles de la page
    /// `page`, augmentées de celles des formulaires dépliés ; `None` si aucun
    /// formulaire n'a été déplié (les ressources de la page suffisent).
    pub(crate) fn resources(
        &self,
        doc: &mut PdfDocument,
        page: &PdfObject,
    ) -> Result<Option<PdfObject>> {
        if self.aliases.is_empty() {
            return Ok(None);
        }
        let base = page.get_dict_inheritable("Resources")?;
        let mut merged = match &base {
            Some(resources) if resources.is_dict()? => crate::writer::detached_copy(resources)?,
            _ => doc.new_dict()?,
        };
        let mut categories: Vec<&'static str> = Vec::new();
        for alias in &self.aliases {
            if !categories.contains(&alias.category) {
                categories.push(alias.category);
            }
        }
        for category in categories {
            let mut dict = match merged.get_dict(category)? {
                Some(dict) if dict.is_dict()? => crate::writer::detached_copy(&dict)?,
                _ => doc.new_dict()?,
            };
            for alias in self.aliases.iter().filter(|a| a.category == category) {
                let Some(object) = unfold::resolve(base.as_ref(), alias) else {
                    continue;
                };
                let object = if object.is_indirect()? {
                    object
                } else {
                    object.try_clone()?
                };
                dict.dict_put(String::from_utf8_lossy(&alias.name).into_owned(), object)?;
            }
            merged.dict_put(category, dict)?;
        }
        Ok(Some(merged))
    }

    /// Contenu de la page sans son texte (cf. [`without_text`]).
    pub(crate) fn without_text(&self) -> Vec<u8> {
        without_text(&self.source, &self.ops)
    }

    /// Convertit un déplacement de l'espace d'affichage vers l'espace de la
    /// page PDF.
    pub fn display_to_page_offset(&self, dx: f64, dy: f64) -> (f64, f64) {
        match self.to_display.linear().inverse() {
            Some(inverse) => inverse.apply_vector(dx, dy),
            None => (dx, -dy),
        }
    }

    /// Structure du texte de la page, pour diagnostiquer le découpage en blocs
    /// sur un document privé sans en dévoiler le contenu : lettres remplacées
    /// par « x » ou « X », chiffres par « 9 », positions et styles en clair.
    pub fn text_structure(&self) -> String {
        use std::fmt::Write;
        let mask = |text: &str| -> String {
            text.chars()
                .map(|c| match c {
                    c if c.is_uppercase() => 'X',
                    c if c.is_alphabetic() => 'x',
                    c if c.is_numeric() => '9',
                    c => c,
                })
                .collect()
        };
        let mut out = String::new();
        for object in &self.objects {
            if let Some(table) = &object.structure {
                let b = object.bbox;
                let merged = table
                    .cells
                    .iter()
                    .filter(|c| c.row_span > 1 || c.column_span > 1)
                    .count();
                let members = self
                    .objects
                    .iter()
                    .filter(|o| o.table == Some(object.id))
                    .count();
                let _ = writeln!(
                    out,
                    "tableau {} [{:.1} {:.1} {:.1} {:.1}] {:?} {} lignes × {} colonnes, {} cellules \
                     dont {} fusionnées, {} objets, limites sans filet {}",
                    object.id,
                    b.x0,
                    b.y0,
                    b.x1,
                    b.y1,
                    table.kind,
                    table.row_count(),
                    table.column_count(),
                    table.cells.len(),
                    merged,
                    members,
                    table.ruled_rows.iter().filter(|r| !**r).count()
                );
                continue;
            }
            let Some(block) = &object.text else {
                continue;
            };
            let b = object.bbox;
            let cell = if object.table.is_some() {
                format!(" cellule du tableau {}", object.table.unwrap_or(0))
            } else {
                String::new()
            };
            let _ =
                writeln!(
                out,
                "bloc {} [{:.1} {:.1} {:.1} {:.1}] {:?} interligne {:.2} corps {:.2} modifiable {}{}",
                object.id, b.x0, b.y0, b.x1, b.y1, block.alignment, block.leading, block.size,
                object.editable, cell
            );
            for line in &block.lines {
                let first = &self.run.glyphs[line.style];
                let state = &self.run.states[first.state];
                let font = state.font.as_ref().map_or("?", |f| f.base_name.as_str());
                let color = state
                    .fill
                    .color
                    .as_ref()
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .unwrap_or_default();
                let objects: HashSet<usize> = line
                    .glyphs
                    .iter()
                    .map(|&g| self.run.glyphs[g].text_object)
                    .collect();
                let styles: HashSet<usize> = line
                    .glyphs
                    .iter()
                    .map(|&g| self.run.glyphs[g].state)
                    .collect();
                let _ = writeln!(
                    out,
                    "  ligne ({:.1}, {:.1}) → ({:.1}, {:.1}) corps {:.2} police {} couleur [{}] \
                     objets BT {:?} états {} « {} »",
                    line.start.x,
                    line.start.y,
                    line.end.x,
                    line.end.y,
                    line.size,
                    font,
                    color.trim(),
                    objects,
                    styles.len(),
                    mask(&line.text)
                );
            }
            let _ = writeln!(
                out,
                "  texte « {} »",
                mask(&block.text).replace('\n', " ⏎ ")
            );
        }
        out
    }

    /// État graphique du style dominant de la première ligne d'un bloc.
    fn first_state(&self, object: &PageObject) -> Option<&GraphicsState> {
        let block = object.text.as_ref()?;
        let glyph = block.lines.first()?.style;
        Some(&self.run.states[self.run.glyphs[glyph].state])
    }

    /// Mise en page du nouveau texte d'un bloc.
    fn plan(
        &self,
        object: &PageObject,
        replacement: &TextReplacement,
        fonts: &mut FontProvider,
    ) -> Option<Plan> {
        let cell = object.text.as_ref()?.cell;
        self.plan_in(object, replacement, fonts, cell)
    }

    /// Mise en page d'un texte dans le style du bloc `object`, dans la
    /// cellule `cell` s'il y en a une.
    fn plan_in(
        &self,
        object: &PageObject,
        replacement: &TextReplacement,
        fonts: &mut FontProvider,
        cell: Option<Rect>,
    ) -> Option<Plan> {
        let block = object.text.as_ref()?;
        let state = self.first_state(object)?;
        let metrics = state.font.as_ref()?;
        let (choice, font) = fonts.resolve(&replacement.font, metrics);
        let text = &replacement.text;
        let laid = match (&choice, &font) {
            (FontChoice::Original, _) | (_, None) => {
                layout_in(&self.run, block, text, &OriginalShaper(metrics), cell)
            }
            (FontChoice::Standard { .. }, Some(font)) => {
                layout_in(&self.run, block, text, &WinAnsiShaper(font), cell)
            }
            (FontChoice::Installed { .. }, Some(font)) => {
                layout_in(&self.run, block, text, &GlyphIdShaper(font), cell)
            }
        };
        let choice = if font.is_some() {
            choice
        } else {
            FontChoice::Original
        };
        Some(Plan {
            layout: laid,
            choice,
            font,
        })
    }

    /// Traduit les modifications `edits` objet par objet : celles d'un
    /// tableau s'appliquent aux blocs de ses cellules, à ses filets et à ses
    /// fonds.
    fn derive<'a>(&'a self, edits: Option<&'a PageEdits>) -> Derived<'a> {
        let edit_of = |id: u32| edits.and_then(|edits| edits.get(id));
        let mut tables = BTreeMap::new();
        for object in &self.objects {
            let Some(table) = &object.structure else {
                continue;
            };
            let edit = edit_of(object.id);
            let table_edit = edit.and_then(|edit| edit.table.as_ref());
            let restructure = table_edit
                .filter(|t| t.rows.is_some() || t.columns.is_some())
                .map(|t| {
                    Restructure::new(
                        table,
                        &order(t.rows.as_deref(), table.row_count()),
                        &order(t.columns.as_deref(), table.column_count()),
                    )
                })
                .filter(|restructure| !restructure.is_identity());
            let emptied = restructure
                .as_ref()
                .is_some_and(|restructure| restructure.table().is_none());
            tables.insert(
                object.id,
                TableState {
                    object,
                    table,
                    edit: table_edit,
                    deleted: edit.is_some_and(|edit| edit.deleted) || emptied,
                    offset: edit
                        .map_or((0.0, 0.0), |edit| (f64::from(edit.dx), f64::from(edit.dy))),
                    restructure,
                },
            );
        }

        let mut effects = BTreeMap::new();
        for object in &self.objects {
            if object.structure.is_some() {
                continue;
            }
            let edit = edit_of(object.id);
            let mut effect = Effect {
                deleted: edit.is_some_and(|edit| edit.deleted),
                offset: edit.map_or((0.0, 0.0), |edit| (f64::from(edit.dx), f64::from(edit.dy))),
                text: edit
                    .and_then(|edit| edit.text.as_ref())
                    .filter(|_| object.editable),
                reshape: None,
            };
            let member = object
                .table
                .and_then(|table| tables.get(&table))
                .filter(|state| state.changed());
            if let Some(state) = member {
                if object.kind == ObjectKind::Path && state.restructure.is_some() && !state.deleted
                {
                    effect.reshape = Some(Reshape {
                        steps: vec![state.step()],
                    });
                } else {
                    let anchor = object
                        .text
                        .as_ref()
                        .and_then(|block| block.cell)
                        .unwrap_or(object.bbox);
                    match state.follow(anchor) {
                        Some((dx, dy)) => {
                            effect.offset = (effect.offset.0 + dx, effect.offset.1 + dy);
                        }
                        None => effect.deleted = true,
                    }
                }
            } else if object.kind == ObjectKind::Path && object.table.is_none() {
                // Tracés qui touchent un tableau sans en faire partie (cadre
                // de la page relié à la grille) : seuls leurs sous-chemins
                // compris dans le tableau suivent ses modifications.
                let steps: Vec<ReshapeStep> = tables
                    .values()
                    .filter(|state| {
                        state.changed() && !state.area().intersect(&object.bbox).is_empty()
                    })
                    .map(TableState::step)
                    .collect();
                if !steps.is_empty() {
                    effect.reshape = Some(Reshape { steps });
                }
            }
            let unchanged = !effect.deleted
                && effect.offset == (0.0, 0.0)
                && effect.text.is_none()
                && effect.reshape.is_none();
            if !unchanged {
                effects.insert(object.id, effect);
            }
        }

        let mut cells = Vec::new();
        for state in tables.values() {
            let Some(edit) = state.edit.filter(|_| !state.deleted) else {
                continue;
            };
            let blocks = self.cell_blocks(state.object);
            for (&(row, column), replacement) in &edit.cells {
                let Some(cell) = state
                    .table
                    .cells
                    .iter()
                    .position(|c| c.row == row as usize && c.column == column as usize)
                else {
                    continue;
                };
                if !blocks[cell].is_empty() {
                    continue;
                }
                let Some(reference) = self.reference_block(state.object, &blocks, cell) else {
                    continue;
                };
                let target = Rect::from(state.table.cell_rect(&state.table.cells[cell]));
                let Some(follow) = state.follow(target) else {
                    continue;
                };
                let delta = self.reference_delta(reference, target);
                cells.push(CellText {
                    table: state.object.id,
                    cell,
                    reference,
                    frame: target.offset(-delta.0, -delta.1),
                    offset: (delta.0 + follow.0, delta.1 + follow.1),
                    replacement,
                });
            }
        }
        Derived {
            effects,
            tables,
            cells,
        }
    }

    /// Blocs de texte de chaque cellule du tableau `table` (indices du
    /// tableau d'origine).
    fn cell_blocks(&self, table: &PageObject) -> Vec<Vec<u32>> {
        let Some(structure) = &table.structure else {
            return Vec::new();
        };
        let mut blocks = vec![Vec::new(); structure.cells.len()];
        for object in &self.objects {
            if object.table != Some(table.id) {
                continue;
            }
            let Some(cell) = object.text.as_ref().and_then(|block| block.cell) else {
                continue;
            };
            let (x, y) = ((cell.x0 + cell.x1) / 2.0, (cell.y0 + cell.y1) / 2.0);
            if let Some(index) = structure.cell_at(x, y) {
                blocks[index].push(object.id);
            }
        }
        blocks
    }

    /// Bloc dont reprend le style un texte saisi dans la cellule vide
    /// `target` : le plus proche de la même colonne (hors en-tête), sinon de
    /// la même ligne, sinon du tableau.
    fn reference_block(
        &self,
        table: &PageObject,
        blocks: &[Vec<u32>],
        target: usize,
    ) -> Option<u32> {
        let structure = table.structure.as_ref()?;
        let goal = structure.cells.get(target)?;
        let mut best: Option<((u8, usize), u32)> = None;
        for (index, ids) in blocks.iter().enumerate() {
            let cell = &structure.cells[index];
            for &id in ids {
                if !self.objects[id as usize].editable {
                    continue;
                }
                let rank = if cell.column == goal.column
                    && cell.column_span == goal.column_span
                    && (cell.row != 0 || goal.row == 0)
                {
                    (0, cell.row.abs_diff(goal.row))
                } else if cell.row == goal.row {
                    (1, cell.column.abs_diff(goal.column))
                } else {
                    (
                        2,
                        cell.row.abs_diff(goal.row) + cell.column.abs_diff(goal.column),
                    )
                };
                if best.is_none_or(|(r, _)| rank < r) {
                    best = Some((rank, id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// Déplacement du bloc de référence `reference` jusqu'à la cellule
    /// `target` : même place par rapport au bord d'alignement, même distance
    /// au haut de la cellule (ou centré verticalement, comme lui).
    fn reference_delta(&self, reference: u32, target: Rect) -> (f64, f64) {
        let object = &self.objects[reference as usize];
        let Some(block) = &object.text else {
            return (0.0, 0.0);
        };
        let source = block.cell.unwrap_or(object.bbox);
        let center = |r: &Rect| ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let dx = match block.alignment {
            Alignment::Right => target.x1 - source.x1,
            Alignment::Center => center(&target).0 - center(&source).0,
            Alignment::Left | Alignment::Justified => target.x0 - source.x0,
        };
        let (above, below) = (source.y1 - object.bbox.y1, object.bbox.y0 - source.y0);
        let dy = if (above - below).abs() <= 1.0 {
            center(&target).1 - center(&source).1
        } else {
            target.y1 - source.y1
        };
        (dx, dy)
    }

    /// Objets de la page, modifications `edits` comprises : les objets
    /// supprimés n'y figurent pas.
    pub(crate) fn infos(
        &self,
        edits: Option<&PageEdits>,
        fonts: &mut FontProvider,
    ) -> Vec<ObjectInfo> {
        let derived = self.derive(edits);
        let mut infos = Vec::with_capacity(self.objects.len());
        for object in &self.objects {
            if let Some(state) = derived.tables.get(&object.id) {
                infos.extend(self.table_info(state, &derived, fonts));
                continue;
            }
            let effect = derived.effects.get(&object.id);
            if effect.is_some_and(|effect| effect.deleted) {
                continue;
            }
            let state = self.first_state(object);
            let mut text = object.text.as_ref().map(|block| block.text.clone());
            let mut font_name = state
                .and_then(|state| state.font.as_ref())
                .map(|font| font.base_name.clone());
            let font_size = object.text.as_ref().map_or(0.0, |block| block.size as f32);
            let (mut bbox, mut substituted, mut overflow, mut missing) =
                (object.bbox, false, false, String::new());
            if let Some(replacement) = effect.and_then(|effect| effect.text) {
                if let Some(plan) = self.plan(object, replacement, fonts) {
                    if !plan.layout.bbox.is_empty() {
                        bbox = plan.layout.bbox;
                    }
                    text = Some(replacement.text.clone());
                    overflow = plan.layout.overflow;
                    missing = plan.layout.missing.iter().collect();
                    match &plan.choice {
                        FontChoice::Original => {}
                        FontChoice::Installed { postscript_name } => {
                            substituted = true;
                            font_name = Some(postscript_name.clone());
                        }
                        FontChoice::Standard { name } => {
                            substituted = true;
                            font_name = Some(name.clone());
                        }
                    }
                }
            }
            if let Some(reshape) = effect.and_then(|effect| effect.reshape.as_ref()) {
                bbox = reshape.bbox(&self.run, &object.units);
                // Tous ses tracés ont disparu avec des lignes ou des colonnes.
                if bbox.is_empty() {
                    continue;
                }
            }
            if let Some((dx, dy)) = effect.map(|effect| effect.offset) {
                bbox = bbox.offset(dx, dy);
            }
            infos.push(ObjectInfo {
                id: object.id,
                kind: object.kind,
                bounds: self.display_rect(bbox),
                movable: object.movable,
                editable: object.editable,
                background: object.background,
                text,
                font_name,
                font_size,
                substituted,
                overflow,
                missing,
                fingerprint: fingerprint(object),
                table: object.table,
                grid: None,
                scanned: false,
                scan: None,
                added: None,
            });
        }
        infos
    }

    /// Le tableau `state`, modifications comprises ; `None` s'il est supprimé.
    fn table_info(
        &self,
        state: &TableState,
        derived: &Derived,
        fonts: &mut FontProvider,
    ) -> Option<ObjectInfo> {
        if state.deleted {
            return None;
        }
        let restructured = state.restructure.as_ref().and_then(Restructure::table);
        let table = restructured.as_ref().unwrap_or(state.table);
        let cells: Vec<(usize, Cell)> = match &state.restructure {
            Some(restructure) => restructure.cells(),
            None => state.table.cells.iter().copied().enumerate().collect(),
        };
        let blocks = self.cell_blocks(state.object);
        let (dx, dy) = state.offset;
        let mut missing = String::new();
        let mut infos = Vec::with_capacity(cells.len());
        for (source, cell) in cells {
            let origin = &state.table.cells[source];
            let rect = Rect::from(table.cell_rect(&cell)).offset(dx, dy);
            let inserted = derived
                .cells
                .iter()
                .find(|c| c.table == state.object.id && c.cell == source);
            let mut overflow = false;
            if let Some(inserted) = inserted {
                let reference = &self.objects[inserted.reference as usize];
                if let Some(plan) =
                    self.plan_in(reference, inserted.replacement, fonts, Some(inserted.frame))
                {
                    overflow = plan.layout.overflow;
                    for c in plan.layout.missing {
                        if !missing.contains(c) {
                            missing.push(c);
                        }
                    }
                }
            }
            let reference = if blocks[source].is_empty() {
                self.reference_block(state.object, &blocks, source)
            } else {
                None
            };
            infos.push(CellInfo {
                row: cell.row as u32,
                column: cell.column as u32,
                row_span: cell.row_span as u32,
                column_span: cell.column_span as u32,
                source_row: origin.row as u32,
                source_column: origin.column as u32,
                bounds: self.display_rect(rect),
                objects: blocks[source]
                    .iter()
                    .copied()
                    .filter(|id| !derived.effects.get(id).is_some_and(|e| e.deleted))
                    .collect(),
                text: inserted.map(|inserted| inserted.replacement.text.clone()),
                reference,
                overflow,
            });
        }
        let orders = |given: Option<&[u32]>, count: usize| -> Vec<u32> {
            order(given, count).into_iter().map(|b| b as u32).collect()
        };
        let edit = state.edit;
        let grid = TableInfo {
            rows: table.row_count() as u32,
            columns: table.column_count() as u32,
            row_order: orders(
                edit.and_then(|e| e.rows.as_deref()),
                state.table.row_count(),
            ),
            column_order: orders(
                edit.and_then(|e| e.columns.as_deref()),
                state.table.column_count(),
            ),
            source_rows: state.table.row_count() as u32,
            source_columns: state.table.column_count() as u32,
            cells: infos,
        };
        let object = state.object;
        Some(ObjectInfo {
            id: object.id,
            kind: object.kind,
            bounds: self.display_rect(Rect::from(table.bbox()).offset(dx, dy)),
            movable: object.movable,
            editable: false,
            background: false,
            text: None,
            font_name: None,
            font_size: 0.0,
            substituted: false,
            overflow: grid.cells.iter().any(|cell| cell.overflow),
            missing,
            fingerprint: fingerprint(object),
            table: None,
            grid: Some(grid),
            scanned: false,
            scan: None,
            added: None,
        })
    }

    fn display_rect(&self, rect: Rect) -> PageRect {
        let rect = self.to_display.apply_rect(rect);
        PageRect {
            x: rect.x0 as f32,
            y: rect.y0 as f32,
            width: rect.width() as f32,
            height: rect.height() as f32,
        }
    }
}

/// Empreinte d'un objet : nature et emprise arrondie au point ; pour un
/// tableau, le nombre de ses lignes et de ses colonnes.
fn fingerprint(object: &PageObject) -> String {
    let b = object.bbox;
    let grid = object.structure.as_ref().map_or(String::new(), |table| {
        format!(":{}x{}", table.row_count(), table.column_count())
    });
    format!(
        "{:?}:{}:{}:{}:{}{grid}",
        object.kind,
        b.x0.round(),
        b.y0.round(),
        b.x1.round(),
        b.y1.round()
    )
}

/// Bandes d'origine conservées, dans leur ordre : `given`, ou toutes.
fn order(given: Option<&[u32]>, count: usize) -> Vec<usize> {
    match given {
        Some(order) => order.iter().map(|&b| b as usize).collect(),
        None => (0..count).collect(),
    }
}

/// Contenu de la page : ses flux, mis bout à bout.
pub(crate) fn page_contents(page: &PdfObject) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let Some(contents) = page.get_dict("Contents")? else {
        return Ok(out);
    };
    if contents.is_array()? {
        for item in contents.array_iter()? {
            let item = item?;
            if item.is_stream()? {
                out.extend_from_slice(&item.read_stream()?);
                out.push(b'\n');
            }
        }
    } else if contents.is_stream()? {
        out = contents.read_stream()?;
    }
    Ok(out)
}

/// Zone visible de la page, dans l'espace de la page PDF.
pub(crate) fn page_box(page: &PdfObject) -> Result<Rect> {
    let read = |key: &str| -> Result<Option<Rect>> {
        let Some(array) = page.get_dict_inheritable(key)? else {
            return Ok(None);
        };
        if !array.is_array()? || array.len()? != 4 {
            return Ok(None);
        }
        let mut v = [0.0; 4];
        for (index, slot) in v.iter_mut().enumerate() {
            if let Some(item) = array.get_array(index as i32)? {
                *slot = f64::from(item.as_float().unwrap_or(0.0));
            }
        }
        Ok(Some(Rect::new(
            v[0].min(v[2]),
            v[1].min(v[3]),
            v[0].max(v[2]),
            v[1].max(v[3]),
        )))
    };
    let media = read("MediaBox")?.unwrap_or(Rect::new(0.0, 0.0, 612.0, 792.0));
    Ok(match read("CropBox")? {
        Some(crop) if !crop.intersect(&media).is_empty() => crop.intersect(&media),
        _ => media,
    })
}

// MARK: - Polices

/// Nouveau texte mis en page et police qui le dessine.
struct Plan {
    layout: Layout,
    /// Police effectivement employée.
    choice: FontChoice,
    /// Police chargée, sauf pour la police d'origine.
    font: Option<Rc<Font>>,
}

/// Polices de substitution chargées (polices du Mac, polices standard).
#[derive(Default)]
pub(crate) struct FontProvider {
    loaded: HashMap<FontChoice, Option<Rc<Font>>>,
}

impl FontProvider {
    pub(crate) fn load(&mut self, choice: &FontChoice) -> Option<Rc<Font>> {
        if let Some(font) = self.loaded.get(choice) {
            return font.clone();
        }
        let font = match choice {
            FontChoice::Original => None,
            FontChoice::Installed { postscript_name } => installed_font(postscript_name),
            FontChoice::Standard { name } => crate::fonts::standard_font(name),
        }
        .map(Rc::new);
        self.loaded.insert(choice.clone(), font.clone());
        font
    }

    /// Police à employer pour `choice` : une police du Mac devenue
    /// introuvable est remplacée par la police standard la plus proche.
    fn resolve(
        &mut self,
        choice: &FontChoice,
        metrics: &FontMetrics,
    ) -> (FontChoice, Option<Rc<Font>>) {
        if *choice == FontChoice::Original {
            return (FontChoice::Original, None);
        }
        if let Some(font) = self.load(choice) {
            return (choice.clone(), Some(font));
        }
        let standard = FontChoice::Standard {
            name: standard_substitute(metrics).to_owned(),
        };
        let font = self.load(&standard);
        (standard, font)
    }
}

/// Police d'origine : codes et chasses de la police de la page.
struct OriginalShaper<'a>(&'a FontMetrics);

impl Shaper for OriginalShaper<'_> {
    fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)> {
        let (code, len) = self.0.encode(c)?;
        Some((code_bytes(code, len), self.0.width(code)))
    }
    fn ascent(&self) -> f64 {
        self.0.ascent
    }
    fn descent(&self) -> f64 {
        self.0.descent
    }
}

/// Police standard non intégrée, encodage WinAnsi : un octet par caractère.
struct WinAnsiShaper<'a>(&'a Font);

impl Shaper for WinAnsiShaper<'_> {
    fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)> {
        let code = win_ansi_code(c)?;
        Some((vec![code], glyph_advance(self.0, c)?))
    }
    fn ascent(&self) -> f64 {
        f64::from(self.0.ascender()) * 1000.0
    }
    fn descent(&self) -> f64 {
        f64::from(self.0.descender()) * 1000.0
    }
}

/// Police intégrée par MuPDF (`Identity-H`) : le code est l'identifiant du
/// glyphe, sur deux octets.
struct GlyphIdShaper<'a>(&'a Font);

impl Shaper for GlyphIdShaper<'_> {
    fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)> {
        let gid = self
            .0
            .encode_character(c as i32)
            .ok()
            .filter(|&gid| gid > 0)?;
        let advance = f64::from(self.0.advance_glyph(gid).ok()?) * 1000.0;
        Some((vec![(gid >> 8) as u8, gid as u8], advance))
    }
    fn ascent(&self) -> f64 {
        f64::from(self.0.ascender()) * 1000.0
    }
    fn descent(&self) -> f64 {
        f64::from(self.0.descender()) * 1000.0
    }
}

pub(crate) fn glyph_advance(font: &Font, c: char) -> Option<f64> {
    let gid = font
        .encode_character(c as i32)
        .ok()
        .filter(|&gid| gid > 0)?;
    Some(f64::from(font.advance_glyph(gid).ok()?) * 1000.0)
}

pub(crate) fn win_ansi_code(c: char) -> Option<u8> {
    (32u8..=255).find(|&code| BaseEncoding::WinAnsi.char(code) == Some(c))
}

/// Police standard la plus proche : famille (linéale, à empattements, à
/// chasse fixe), graisse et style.
pub(crate) fn standard_substitute(metrics: &FontMetrics) -> &'static str {
    let name = metrics.base_name.to_ascii_lowercase();
    let mono = metrics.is_fixed_pitch()
        || ["courier", "mono", "consol", "menlo"]
            .iter()
            .any(|w| name.contains(w));
    let sans = name.contains("sans") || name.contains("grotesk") || name.contains("gothic");
    let serif = !sans
        && (metrics.is_serif()
            || [
                "times", "serif", "roman", "garamond", "georgia", "minion", "palatino", "book",
                "cambria",
            ]
            .iter()
            .any(|w| name.contains(w)));
    let (bold, italic) = (metrics.is_bold(), metrics.is_italic());
    match (mono, serif, bold, italic) {
        (true, _, false, false) => "Courier",
        (true, _, true, false) => "Courier-Bold",
        (true, _, false, true) => "Courier-Oblique",
        (true, _, true, true) => "Courier-BoldOblique",
        (false, true, false, false) => "Times-Roman",
        (false, true, true, false) => "Times-Bold",
        (false, true, false, true) => "Times-Italic",
        (false, true, true, true) => "Times-BoldItalic",
        (false, false, false, false) => "Helvetica",
        (false, false, true, false) => "Helvetica-Bold",
        (false, false, false, true) => "Helvetica-Oblique",
        (false, false, true, true) => "Helvetica-BoldOblique",
    }
}

/// Noms PostScript sous lesquels chercher une police installée.
fn postscript_candidates(base_name: &str) -> Vec<String> {
    let mut names = vec![base_name.to_owned()];
    // Variante TrueType « Arial,Bold » : nom PostScript « Arial-Bold ».
    if base_name.contains(',') {
        names.push(base_name.replace(',', "-"));
    }
    names.retain(|name| !name.is_empty());
    names
}

/// Police complète installée sur le Mac, si son intégration est autorisée.
fn installed_font(postscript_name: &str) -> Option<Font> {
    use font_kit::handle::Handle;
    use font_kit::source::SystemSource;

    let handle = SystemSource::new()
        .select_by_postscript_name(postscript_name)
        .ok()?;
    let (data, index) = match handle {
        Handle::Path { path, font_index } => {
            if std::fs::metadata(&path).ok()?.len() > MAX_FONT_FILE {
                return None;
            }
            (std::fs::read(&path).ok()?, font_index)
        }
        Handle::Memory { bytes, font_index } => (bytes.to_vec(), font_index),
    };
    if !embeddable(&data, index) {
        return None;
    }
    Font::from_bytes_with_index(postscript_name, index as i32, &data).ok()
}

/// Vrai si la licence de la police (champ `fsType` de la table `OS/2`)
/// autorise son intégration dans un document modifiable.
pub(crate) fn embeddable(data: &[u8], index: u32) -> bool {
    let u16_at = |at: usize| {
        data.get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    // Collection : table des polices, puis répertoire de la police `index`.
    let directory = if data.starts_with(b"ttcf") {
        match u32_at(12 + 4 * index as usize) {
            Some(offset) => offset as usize,
            None => return false,
        }
    } else {
        0
    };
    let Some(tables) = u16_at(directory + 4) else {
        return false;
    };
    for table in 0..tables as usize {
        let record = directory + 12 + 16 * table;
        if data.get(record..record + 4) != Some(b"OS/2") {
            continue;
        }
        let Some(fs_type) = u32_at(record + 8).and_then(|offset| u16_at(offset as usize + 8))
        else {
            return false;
        };
        // Bitmaps seuls (0x200) : pas de contours à intégrer.
        if fs_type & 0x200 != 0 {
            return false;
        }
        // 0 : installable ; 8 : modifiable. 2 (restreinte) et 4 (aperçu et
        // impression seuls) n'autorisent pas un document modifiable.
        return fs_type & 0xF == 0 || fs_type & 0x8 != 0;
    }
    // Pas de table OS/2 (anciennes polices du Mac) : aucune restriction déclarée.
    true
}

// MARK: - Choix de la police

/// Glyphes de la police d'origine relevés en dessinant les codes à l'essai.
#[derive(Default)]
struct Probe {
    glyphs: Arc<Mutex<Vec<bool>>>,
}

struct Segments(usize);

impl PathWalker for Segments {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn close(&mut self) {}
}

impl NativeDevice for Probe {
    fn fill_text(
        &mut self,
        text: &Text,
        _: mupdf::Matrix,
        _: &Colorspace,
        _: &[f32],
        _: f32,
        _: ColorParams,
    ) {
        let mut found = self
            .glyphs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for span in text.spans() {
            let font = span.font();
            for item in span.items() {
                let gid = item.gid();
                // -1 : caractère supplémentaire d'un même code (ligature).
                if gid < 0 {
                    continue;
                }
                let drawn = gid > 0
                    && font.outline_glyph(gid).ok().flatten().is_some_and(|path| {
                        let mut segments = Segments(0);
                        path.walk(&mut segments).is_ok() && segments.0 > 0
                    });
                found.push(drawn);
            }
        }
    }
}

/// Police qui dessinera `text` à la place du bloc `object` : la police
/// d'origine si elle contient tous les caractères, sinon la même police
/// installée sur le Mac, sinon la police standard la plus proche.
pub(crate) fn choose_font(
    doc: &mut PdfDocument,
    page_index: u32,
    model: &PageModel,
    object: u32,
    text: &str,
    scratch: &mut Scratch,
) -> Result<FontChoice> {
    let Some(object) = model.objects.get(object as usize) else {
        return Ok(FontChoice::Original);
    };
    let Some(state) = model.first_state(object) else {
        return Ok(FontChoice::Original);
    };
    let (Some(metrics), Some(font_name)) = (state.font.as_ref(), state.font_name.as_ref()) else {
        return Ok(FontChoice::Original);
    };
    let needed: Vec<char> = {
        let mut seen = HashSet::new();
        text.chars()
            .filter(|c| !c.is_whitespace() && seen.insert(*c))
            .collect()
    };

    let in_font = metrics.supported && !metrics.vertical && metrics.kind != FontKind::Type3;
    let codes: Option<Vec<Vec<u8>>> = needed
        .iter()
        .map(|&c| metrics.encode(c).map(|(code, len)| code_bytes(code, len)))
        .collect();
    if let (true, Some(codes)) = (in_font, codes) {
        let page = doc.find_page(page_index as i32)?;
        let resources = model.resources(doc, &page)?;
        if codes.is_empty()
            || probe(
                doc,
                page_index,
                font_name,
                &codes,
                resources.as_ref(),
                scratch,
            )?
        {
            return Ok(FontChoice::Original);
        }
    }

    for name in postscript_candidates(&metrics.base_name) {
        if let Some(font) = installed_font(&name) {
            if needed
                .iter()
                .all(|&c| font.encode_character(c as i32).is_ok_and(|gid| gid > 0))
            {
                return Ok(FontChoice::Installed {
                    postscript_name: name,
                });
            }
        }
    }
    Ok(FontChoice::Standard {
        name: standard_substitute(metrics).to_owned(),
    })
}

/// Vrai si la police `font_name` de la page (ressources `resources`, sinon
/// celles de la page) dessine un glyphe pour chacun des `codes`.
fn probe(
    doc: &mut PdfDocument,
    page_index: u32,
    font_name: &[u8],
    codes: &[Vec<u8>],
    resources: Option<&PdfObject>,
    scratch: &mut Scratch,
) -> Result<bool> {
    let mut content = Vec::new();
    content.extend_from_slice(b"BT ");
    crate::content::writer::write_name(&mut content, font_name);
    content.extend_from_slice(b" 12 Tf 0 Tr\n");
    for code in codes {
        crate::content::writer::write_hex(&mut content, code);
        content.extend_from_slice(b" Tj\n");
    }
    content.extend_from_slice(b"ET\n");

    let glyphs = Arc::new(Mutex::new(Vec::new()));
    let device = Device::from_native(Probe {
        glyphs: Arc::clone(&glyphs),
    })?;
    with_page_content(doc, page_index, &content, resources, scratch, |doc| {
        let page = doc.load_page(page_index as i32)?;
        page.run_contents(&device, &mupdf::Matrix::IDENTITY)?;
        Ok(())
    })?;
    drop(device);
    let found = glyphs
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(found.len() == codes.len() && found.iter().all(|&drawn| drawn))
}

// MARK: - Application des modifications

/// Flux de contenu de travail, réutilisé pour l'aperçu et les essais.
#[derive(Default)]
pub(crate) struct Scratch {
    stream: Option<PdfObject>,
}

/// Exécute `f` pendant que la page `page_index` affiche `content` (et
/// `resources` s'il est donné), puis rétablit la page.
pub(crate) fn with_page_content<R>(
    doc: &mut PdfDocument,
    page_index: u32,
    content: &[u8],
    resources: Option<&PdfObject>,
    scratch: &mut Scratch,
    f: impl FnOnce(&mut PdfDocument) -> Result<R>,
) -> Result<R> {
    let buffer = Buffer::from_bytes(content)?;
    let stream = match &mut scratch.stream {
        Some(stream) => {
            stream.write_stream_buffer(&buffer)?;
            stream.try_clone()?
        }
        None => {
            let stream = doc.add_stream(&buffer, None, false)?;
            scratch.stream = Some(stream.try_clone()?);
            stream
        }
    };
    let mut page = doc.find_page(page_index as i32)?;
    let saved_contents = page.get_dict("Contents")?;
    let saved_resources = page.get_dict("Resources")?;
    page.dict_put("Contents", stream)?;
    if let Some(resources) = resources {
        page.dict_put("Resources", resources.try_clone()?)?;
    }
    let result = f(doc);
    let restored = restore_key(&mut page, "Contents", saved_contents)
        .and_then(|()| restore_key(&mut page, "Resources", saved_resources));
    let value = result?;
    restored?;
    Ok(value)
}

fn restore_key(page: &mut PdfObject, key: &str, saved: Option<PdfObject>) -> Result<()> {
    match saved {
        Some(value) => page.dict_put(key, value)?,
        None => page.dict_delete(key)?,
    }
    Ok(())
}

/// Polices déjà ajoutées à un document, par choix.
pub(crate) type EmbeddedFonts = HashMap<FontChoice, PdfObject>;

/// Contenu et ressources d'une page modifiée.
pub(crate) struct EditedPage {
    /// Nouveau flux de contenu.
    pub content: Vec<u8>,
    /// Nouvelles ressources (polices ajoutées, ressources des formulaires
    /// dépliés), si elles changent.
    pub resources: Option<PdfObject>,
}

/// Applique `edits` au contenu de la page `page` décrite par `model`.
pub(crate) fn edit_page(
    doc: &mut PdfDocument,
    page: &PdfObject,
    model: &PageModel,
    edits: &PageEdits,
    fonts: &mut FontProvider,
    embedded: &mut EmbeddedFonts,
) -> Result<EditedPage> {
    let merged = model.resources(doc, page)?;
    let unfolded = merged.is_some();
    let resources = match merged {
        Some(merged) => Some(merged),
        None => page.get_dict_inheritable("Resources")?,
    };
    let font_dict = match &resources {
        Some(resources) => resources.get_dict("Font")?,
        None => None,
    };
    let mut taken: HashSet<Vec<u8>> = HashSet::new();
    if let Some(dict) = &font_dict {
        if dict.is_dict()? {
            for entry in dict.dict_iter()? {
                let (key, _) = entry?;
                taken.insert(key.as_name()?);
            }
        }
    }

    let mut names = FontNames {
        taken,
        names: HashMap::new(),
        added: Vec::new(),
        embedded,
    };
    let derived = model.derive(Some(edits));
    let mut changes: BTreeMap<u32, Change> = BTreeMap::new();
    for (&id, effect) in &derived.effects {
        let Some(object) = model.objects.get(id as usize) else {
            continue;
        };
        let mut change = Change {
            deleted: effect.deleted,
            offset: effect.offset,
            text: None,
            reshape: effect.reshape.clone(),
        };
        if let (Some(replacement), false) = (effect.text, effect.deleted) {
            if let Some(plan) = model.plan(object, replacement, fonts) {
                if let Some(font) = names.resource(doc, model, object, &plan)? {
                    change.text = Some(NewText {
                        layout: plan.layout,
                        font,
                    });
                }
            }
        }
        changes.insert(id, change);
    }
    let mut insertions = Vec::new();
    for cell in &derived.cells {
        let object = &model.objects[cell.reference as usize];
        let Some(block) = &object.text else {
            continue;
        };
        let Some(plan) = model.plan_in(object, cell.replacement, fonts, Some(cell.frame)) else {
            continue;
        };
        if let Some(font) = names.resource(doc, model, object, &plan)? {
            insertions.push(Insertion {
                text_object: model.run.glyphs[block.lines[0].style].text_object,
                text: NewText {
                    layout: plan.layout,
                    font,
                },
                offset: cell.offset,
            });
        }
    }
    let content = rewrite(
        &model.source,
        &model.ops,
        &model.run,
        &model.objects,
        &changes,
        &insertions,
    );
    let added = names.added;

    let resources = if added.is_empty() && !unfolded {
        None
    } else {
        let mut copy = match &resources {
            Some(resources) if resources.is_dict()? => crate::writer::detached_copy(resources)?,
            _ => doc.new_dict()?,
        };
        let mut fonts = match &font_dict {
            Some(dict) if dict.is_dict()? => crate::writer::detached_copy(dict)?,
            _ => doc.new_dict()?,
        };
        for (name, reference) in added {
            fonts.dict_put(String::from_utf8_lossy(&name).into_owned(), reference)?;
        }
        copy.dict_put("Font", fonts)?;
        Some(copy)
    };
    Ok(EditedPage { content, resources })
}

/// Noms de ressource des polices qui dessinent les nouveaux textes d'une page.
struct FontNames<'a> {
    taken: HashSet<Vec<u8>>,
    names: HashMap<FontChoice, Vec<u8>>,
    /// Polices ajoutées aux ressources de la page.
    added: Vec<(Vec<u8>, PdfObject)>,
    embedded: &'a mut EmbeddedFonts,
}

impl FontNames<'_> {
    /// Nom de ressource de la police du texte mis en page `plan` dans le style
    /// du bloc `object`.
    fn resource(
        &mut self,
        doc: &mut PdfDocument,
        model: &PageModel,
        object: &PageObject,
        plan: &Plan,
    ) -> Result<Option<Vec<u8>>> {
        let (choice, font) = match (&plan.choice, &plan.font) {
            (FontChoice::Original, _) | (_, None) => {
                return Ok(model
                    .first_state(object)
                    .and_then(|state| state.font_name.clone()));
            }
            (choice, Some(font)) => (choice, font),
        };
        if let Some(name) = self.names.get(choice) {
            return Ok(Some(name.clone()));
        }
        let reference = match self.embedded.get(choice) {
            Some(reference) => reference.try_clone()?,
            None => {
                let reference = add_font(doc, choice, font)?;
                self.embedded.insert(choice.clone(), reference.try_clone()?);
                reference
            }
        };
        let name = unique_name(&mut self.taken);
        self.names.insert(choice.clone(), name.clone());
        self.added.push((name.clone(), reference));
        Ok(Some(name))
    }
}

/// Nom de ressource libre pour une police ajoutée.
fn unique_name(taken: &mut HashSet<Vec<u8>>) -> Vec<u8> {
    let mut index = 1;
    loop {
        let name = format!("PSF{index}").into_bytes();
        if taken.insert(name.clone()) {
            return name;
        }
        index += 1;
    }
}

/// Ajoute au document la police d'un texte remplacé.
pub(crate) fn add_font(
    doc: &mut PdfDocument,
    choice: &FontChoice,
    font: &Font,
) -> Result<PdfObject> {
    match choice {
        FontChoice::Standard { name } => {
            // Police standard non intégrée, comme le permet la norme pour les
            // 14 polices de base ; chasses indiquées pour les lecteurs stricts.
            let mut dict = doc.new_dict()?;
            dict.dict_put("Type", PdfObject::new_name("Font")?)?;
            dict.dict_put("Subtype", PdfObject::new_name("Type1")?)?;
            dict.dict_put("BaseFont", PdfObject::new_name(name)?)?;
            dict.dict_put("Encoding", PdfObject::new_name("WinAnsiEncoding")?)?;
            dict.dict_put("FirstChar", PdfObject::new_int(32)?)?;
            dict.dict_put("LastChar", PdfObject::new_int(255)?)?;
            let mut widths = doc.new_array()?;
            for code in 32u8..=255 {
                let width = BaseEncoding::WinAnsi
                    .char(code)
                    .and_then(|c| glyph_advance(font, c))
                    .unwrap_or(0.0);
                widths.array_push(PdfObject::new_real(width.round() as f32)?)?;
            }
            dict.dict_put("Widths", widths)?;
            Ok(doc.add_object(&dict)?)
        }
        _ => Ok(doc.add_font(font)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formulaire() -> PdfDocument {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/formulaire.pdf");
        PdfDocument::from_bytes(&std::fs::read(path).unwrap()).unwrap()
    }

    fn rendu(doc: &PdfDocument) -> Vec<u8> {
        let page = doc.load_page(0).unwrap();
        let matrix = mupdf::Matrix::new_scale(2.0, 2.0);
        let pixmap = page
            .to_pixmap(&matrix, &Colorspace::device_rgb(), false, false)
            .unwrap();
        pixmap.samples().to_vec()
    }

    #[test]
    fn deplier_un_formulaire_ne_change_pas_l_affichage() {
        let mut doc = formulaire();
        let page = doc.find_page(0).unwrap();
        let model = PageModel::load(&page).unwrap();
        assert!(model.objects.iter().all(|o| o.kind != ObjectKind::Form));
        // Les trois polices G1 (page, formulaire, formulaire imbriqué)
        // gardent chacune leur nom ou en reçoivent un nouveau.
        let fonts: Vec<&[u8]> = model
            .aliases
            .iter()
            .filter(|a| a.category == "Font")
            .map(|a| a.name.as_slice())
            .collect();
        assert_eq!(fonts, [&b"G1_1"[..], b"G2", b"G1_2"]);
        let resources = model.resources(&mut doc, &page).unwrap().unwrap();
        let before = rendu(&doc);
        let after = with_page_content(
            &mut doc,
            0,
            &model.source,
            Some(&resources),
            &mut Scratch::default(),
            |doc| Ok(rendu(doc)),
        )
        .unwrap();
        assert!(before == after, "le contenu déplié s'affiche autrement");
    }

    #[test]
    fn formulaire_a_elimination_reste_entier() {
        let doc = formulaire();
        let page = doc.find_page(0).unwrap();
        let mut group = page
            .get_dict("Resources")
            .unwrap()
            .unwrap()
            .get_dict("XObject")
            .unwrap()
            .unwrap()
            .get_dict("Fm1")
            .unwrap()
            .unwrap()
            .get_dict("Group")
            .unwrap()
            .unwrap();
        group.dict_put("K", PdfObject::new_bool(true)).unwrap();
        let model = PageModel::load(&page).unwrap();
        assert!(model.aliases.is_empty());
        let kinds: Vec<ObjectKind> = model.objects.iter().map(|o| o.kind).collect();
        assert_eq!(kinds, [ObjectKind::Form, ObjectKind::Text]);
    }

    #[test]
    fn police_standard_la_plus_proche() {
        let mut metrics =
            FontMetrics::simple("ABCDEF+Calibri-Bold", HashMap::new(), HashMap::new());
        assert_eq!(standard_substitute(&metrics), "Helvetica-Bold");
        metrics = FontMetrics::simple("Georgia-Italic", HashMap::new(), HashMap::new());
        assert_eq!(standard_substitute(&metrics), "Times-Italic");
        metrics = FontMetrics::simple("SourceSansPro-Regular", HashMap::new(), HashMap::new());
        metrics.flags = 0x2;
        assert_eq!(standard_substitute(&metrics), "Helvetica");
        metrics = FontMetrics::simple("Menlo-Regular", HashMap::new(), HashMap::new());
        assert_eq!(standard_substitute(&metrics), "Courier");
    }

    #[test]
    fn licence_d_integration() {
        // Police minimale : en-tête, une table OS/2 dont fsType vaut `fs_type`.
        fn font(fs_type: u16) -> Vec<u8> {
            let mut data = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
            data.extend_from_slice(b"OS/2");
            data.extend_from_slice(&[0; 4]);
            data.extend_from_slice(&28u32.to_be_bytes());
            data.extend_from_slice(&10u32.to_be_bytes());
            data.extend_from_slice(&[0; 8]);
            data.extend_from_slice(&fs_type.to_be_bytes());
            data
        }
        assert!(embeddable(&font(0), 0));
        assert!(embeddable(&font(8), 0));
        assert!(!embeddable(&font(2), 0));
        assert!(!embeddable(&font(4), 0));
        assert!(!embeddable(&font(0x208), 0));
        assert_eq!(win_ansi_code('é'), Some(0xE9));
        assert_eq!(win_ansi_code('€'), Some(0x80));
        assert_eq!(win_ansi_code('Ω'), None);
    }
}
