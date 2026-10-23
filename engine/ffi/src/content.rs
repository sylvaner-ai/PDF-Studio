//! Édition du contenu des pages : objets d'une page, modifications et leur
//! conservation pour la récupération.

use std::collections::BTreeMap;
use std::sync::Arc;

use edit::{
    FontChoice, ObjectEdit, PageEdits, ScanStyle, ScanText, TableEdit, TextLayer, TextReplacement,
};
use pdf_core::content::objects::ObjectKind;
use pdf_core::{CellInfo, ObjectInfo, TableInfo};

use crate::PageRect;

/// Nature d'un objet de page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PageObjectKind {
    /// Bloc de texte (paragraphe, titre, ligne isolée).
    Text,
    /// Image.
    Image,
    /// Tracés (filets, cadres, formes) qui se touchent.
    Path,
    /// Formulaire PDF (contenu réutilisable), déplacé d'un bloc.
    Form,
    /// Dégradé.
    Shading,
    /// Tableau : le texte de ses cellules, ses filets et ses fonds.
    Table,
}

impl From<ObjectKind> for PageObjectKind {
    fn from(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Text => PageObjectKind::Text,
            ObjectKind::Image => PageObjectKind::Image,
            ObjectKind::Path => PageObjectKind::Path,
            ObjectKind::Form => PageObjectKind::Form,
            ObjectKind::Shading => PageObjectKind::Shading,
            ObjectKind::Table => PageObjectKind::Table,
        }
    }
}

/// Un objet d'une page, modifications comprises.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PageObjectItem {
    /// Numéro de l'objet dans la page (ordre d'empilement).
    pub id: u32,
    /// Nature.
    pub kind: PageObjectKind,
    /// Emprise sur la page droite (sans les quarts de tour de l'organisation),
    /// en points, origine en haut à gauche.
    pub bounds: PageRect,
    /// L'objet peut être déplacé et supprimé.
    pub movable: bool,
    /// Le texte peut être modifié.
    pub editable: bool,
    /// Fond de page : il n'est pas sélectionné par un clic.
    pub background: bool,
    /// Texte du bloc.
    pub text: Option<String>,
    /// Police du texte.
    pub font_name: Option<String>,
    /// Corps du texte, en points.
    pub font_size: f32,
    /// Le texte modifié est dessiné avec une police de substitution.
    pub substituted: bool,
    /// Le texte modifié ne tient pas dans le bloc d'origine.
    pub overflow: bool,
    /// Caractères qu'aucune police disponible ne contient (omis).
    pub missing: String,
    /// Tableau dont l'objet fait partie (bloc d'une cellule, filet, fond).
    pub table: Option<u32>,
    /// Grille d'un tableau.
    pub grid: Option<TableGridItem>,
    /// Objet d'une page scannée : ses pixels sont retouchés dans l'image (les
    /// lignes et les colonnes de ses tableaux ne se réorganisent pas).
    pub scanned: bool,
    /// Texte retouché d'une ligne scannée : réglages et polices candidates.
    pub scan: Option<ScanTextItem>,
}

/// Réglages du rendu d'un texte dans un scan (longueurs en points).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScanStyleItem {
    /// Nom PostScript de la police.
    pub font: String,
    /// Famille de la police.
    pub family: String,
    /// Corps, en points.
    pub size: f32,
    /// Interlettrage, en points.
    pub spacing: f32,
    /// Graisse ajoutée, de -1 à 1.
    pub weight: f32,
    /// Flou, en points.
    pub blur: f32,
    /// Grain de l'encre (sur 255).
    pub noise: f32,
    /// Couleur : rouge, vert, bleu, de 0 à 1.
    pub red: f32,
    /// Vert.
    pub green: f32,
    /// Bleu.
    pub blue: f32,
    /// Crénage appliqué.
    pub kerning: bool,
}

/// Police retrouvée sur une ligne scannée.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScanCandidateItem {
    /// Nom PostScript.
    pub font: String,
    /// Famille.
    pub family: String,
    /// Ressemblance avec le scan, de 0 à 1.
    pub score: f32,
}

/// Texte retouché d'une ligne scannée.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScanTextItem {
    /// Réglages actuels.
    pub style: ScanStyleItem,
    /// Réglages mesurés sur le scan.
    pub measured: ScanStyleItem,
    /// Polices candidates, de la plus proche à la moins proche.
    pub candidates: Vec<ScanCandidateItem>,
}

impl From<&ScanStyle> for ScanStyleItem {
    fn from(style: &ScanStyle) -> Self {
        let [red, green, blue] = style.color;
        ScanStyleItem {
            font: style.font.clone(),
            family: style.family.clone(),
            size: style.size,
            spacing: style.spacing,
            weight: style.weight,
            blur: style.blur,
            noise: style.noise,
            red,
            green,
            blue,
            kerning: style.kerning,
        }
    }
}

impl From<&ScanStyleItem> for ScanStyle {
    fn from(item: &ScanStyleItem) -> Self {
        ScanStyle {
            font: item.font.clone(),
            family: item.family.clone(),
            size: item.size.clamp(2.0, 400.0),
            spacing: item.spacing.clamp(-10.0, 50.0),
            weight: item.weight.clamp(-1.0, 1.0),
            blur: item.blur.clamp(0.0, 10.0),
            noise: item.noise.clamp(0.0, 80.0),
            color: [item.red, item.green, item.blue].map(|c| c.clamp(0.0, 1.0)),
            kerning: item.kerning,
        }
    }
}

impl From<&ScanText> for ScanTextItem {
    fn from(text: &ScanText) -> Self {
        ScanTextItem {
            style: ScanStyleItem::from(&text.style),
            measured: ScanStyleItem::from(&text.measured),
            candidates: text
                .candidates
                .iter()
                .map(|c| ScanCandidateItem {
                    font: c.font.clone(),
                    family: c.family.clone(),
                    score: c.score,
                })
                .collect(),
        }
    }
}

/// Grille d'un tableau, modifications comprises.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TableGridItem {
    /// Nombre de lignes.
    pub rows: u32,
    /// Nombre de colonnes.
    pub columns: u32,
    /// Cellules, ligne par ligne.
    pub cells: Vec<TableCellItem>,
}

/// Une cellule d'un tableau.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TableCellItem {
    /// Ligne de son coin supérieur gauche.
    pub row: u32,
    /// Colonne de son coin supérieur gauche.
    pub column: u32,
    /// Nombre de lignes couvertes.
    pub row_span: u32,
    /// Nombre de colonnes couvertes.
    pub column_span: u32,
    /// Emprise sur la page droite, en points, origine en haut à gauche.
    pub bounds: PageRect,
    /// Blocs de texte de la cellule.
    pub objects: Vec<u32>,
    /// Texte saisi dans la cellule, vide à l'origine.
    pub text: Option<String>,
    /// Un texte peut être saisi dans la cellule (vide à l'origine).
    pub fillable: bool,
    /// Le texte saisi ne tient pas dans la cellule.
    pub overflow: bool,
}

impl From<TableInfo> for TableGridItem {
    fn from(grid: TableInfo) -> Self {
        TableGridItem {
            rows: grid.rows,
            columns: grid.columns,
            cells: grid.cells.into_iter().map(TableCellItem::from).collect(),
        }
    }
}

impl From<CellInfo> for TableCellItem {
    fn from(cell: CellInfo) -> Self {
        TableCellItem {
            row: cell.row,
            column: cell.column,
            row_span: cell.row_span,
            column_span: cell.column_span,
            bounds: rect(cell.bounds),
            objects: cell.objects,
            text: cell.text,
            fillable: cell.reference.is_some(),
            overflow: cell.overflow,
        }
    }
}

fn rect(rect: pdf_core::PageRect) -> PageRect {
    PageRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

impl From<ObjectInfo> for PageObjectItem {
    fn from(info: ObjectInfo) -> Self {
        PageObjectItem {
            id: info.id,
            kind: info.kind.into(),
            bounds: rect(info.bounds),
            movable: info.movable,
            editable: info.editable,
            background: info.background,
            text: info.text,
            font_name: info.font_name,
            font_size: info.font_size,
            substituted: info.substituted,
            overflow: info.overflow,
            missing: info.missing,
            table: info.table,
            grid: info.grid.map(TableGridItem::from),
            scanned: info.scanned,
            scan: info.scan.as_ref().map(ScanTextItem::from),
        }
    }
}

/// Police d'un texte modifié.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TextFont {
    /// Police d'origine.
    Original,
    /// Police installée sur le Mac, intégrée au document.
    Installed {
        /// Nom PostScript.
        postscript_name: String,
    },
    /// Police standard (Helvetica, Times, Courier).
    Standard {
        /// Nom de la police standard.
        name: String,
    },
}

impl From<&FontChoice> for TextFont {
    fn from(choice: &FontChoice) -> Self {
        match choice {
            FontChoice::Original => TextFont::Original,
            FontChoice::Installed { postscript_name } => TextFont::Installed {
                postscript_name: postscript_name.clone(),
            },
            FontChoice::Standard { name } => TextFont::Standard { name: name.clone() },
        }
    }
}

impl From<TextFont> for FontChoice {
    fn from(font: TextFont) -> Self {
        match font {
            TextFont::Original => FontChoice::Original,
            TextFont::Installed { postscript_name } => FontChoice::Installed { postscript_name },
            TextFont::Standard { name } => FontChoice::Standard { name },
        }
    }
}

/// Modification d'un objet, telle que la conserve la récupération.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ObjectEditItem {
    /// Numéro de l'objet.
    pub object: u32,
    /// L'objet est supprimé.
    pub deleted: bool,
    /// Déplacement horizontal, en points dans l'espace de la page PDF.
    pub dx: f32,
    /// Déplacement vertical, vers le haut.
    pub dy: f32,
    /// Nouveau texte.
    pub text: Option<String>,
    /// Police du nouveau texte.
    pub font: Option<TextFont>,
    /// Empreinte de l'objet d'origine : une modification dont l'objet ne
    /// correspond plus est ignorée à la restauration.
    pub fingerprint: String,
    /// Modifications d'un tableau.
    pub table: Option<TableEditItem>,
    /// Texte retouché d'une ligne scannée, sous forme texte.
    pub scan: Option<String>,
}

/// Modifications d'un tableau, telles que les conserve la récupération.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TableEditItem {
    /// Lignes d'origine conservées, dans leur ordre (`None` : toutes).
    pub rows: Option<Vec<u32>>,
    /// Colonnes d'origine conservées, dans leur ordre (`None` : toutes).
    pub columns: Option<Vec<u32>>,
    /// Textes saisis dans des cellules vides.
    pub cells: Vec<CellTextItem>,
}

/// Texte saisi dans une cellule vide.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CellTextItem {
    /// Ligne d'origine de la cellule.
    pub row: u32,
    /// Colonne d'origine de la cellule.
    pub column: u32,
    /// Texte.
    pub text: String,
    /// Police du texte.
    pub font: TextFont,
}

impl From<&TableEdit> for TableEditItem {
    fn from(edit: &TableEdit) -> Self {
        TableEditItem {
            rows: edit.rows.clone(),
            columns: edit.columns.clone(),
            cells: edit
                .cells
                .iter()
                .map(|(&(row, column), text)| CellTextItem {
                    row,
                    column,
                    text: text.text.clone(),
                    font: TextFont::from(&text.font),
                })
                .collect(),
        }
    }
}

impl From<&TableEditItem> for TableEdit {
    fn from(item: &TableEditItem) -> Self {
        TableEdit {
            rows: item.rows.clone(),
            columns: item.columns.clone(),
            cells: item
                .cells
                .iter()
                .map(|cell| {
                    (
                        (cell.row, cell.column),
                        TextReplacement {
                            text: cell.text.clone(),
                            font: FontChoice::from(cell.font.clone()),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl ObjectEditItem {
    pub(crate) fn new(object: u32, edit: &ObjectEdit, fingerprint: String) -> Self {
        ObjectEditItem {
            object,
            deleted: edit.deleted,
            dx: edit.dx,
            dy: edit.dy,
            text: edit.text.as_ref().map(|text| text.text.clone()),
            font: edit.text.as_ref().map(|text| TextFont::from(&text.font)),
            fingerprint,
            table: edit.table.as_ref().map(TableEditItem::from),
            scan: edit.scan.as_ref().map(ScanText::to_record),
        }
    }

    pub(crate) fn edit(&self) -> ObjectEdit {
        ObjectEdit {
            deleted: self.deleted,
            dx: self.dx,
            dy: self.dy,
            text: self.text.as_ref().map(|text| TextReplacement {
                text: text.clone(),
                font: self
                    .font
                    .clone()
                    .map_or(FontChoice::Original, FontChoice::from),
            }),
            table: self.table.as_ref().map(TableEdit::from),
            scan: self.scan.as_deref().and_then(ScanText::from_record),
        }
    }
}

/// Modifications du contenu d'une page à un instant donné, pour la rendre.
#[derive(uniffi::Object)]
pub struct ContentEdits {
    pub(crate) edits: Arc<PageEdits>,
    /// Couche de texte d'une page reconnue : les objets d'un scan en
    /// dépendent.
    pub(crate) layer: Option<Arc<TextLayer>>,
}

#[uniffi::export]
impl ContentEdits {
    /// Numéro de cet état des modifications, unique dans la session.
    pub fn revision(&self) -> u64 {
        self.edits.revision()
    }
}

/// Modifications retrouvées d'une page, par numéro d'objet.
pub(crate) fn restored_content(items: &[ObjectEditItem]) -> BTreeMap<u32, ObjectEdit> {
    items
        .iter()
        .map(|item| (item.object, item.edit()))
        .collect()
}
