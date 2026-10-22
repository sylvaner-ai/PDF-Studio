//! Édition du contenu des pages : objets d'une page, modifications et leur
//! conservation pour la récupération.

use std::collections::BTreeMap;
use std::sync::Arc;

use edit::{FontChoice, ObjectEdit, PageEdits, TextReplacement};
use pdf_core::content::objects::ObjectKind;
use pdf_core::ObjectInfo;

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
}

impl From<ObjectKind> for PageObjectKind {
    fn from(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Text => PageObjectKind::Text,
            ObjectKind::Image => PageObjectKind::Image,
            ObjectKind::Path => PageObjectKind::Path,
            ObjectKind::Form => PageObjectKind::Form,
            ObjectKind::Shading => PageObjectKind::Shading,
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
}

impl From<ObjectInfo> for PageObjectItem {
    fn from(info: ObjectInfo) -> Self {
        PageObjectItem {
            id: info.id,
            kind: info.kind.into(),
            bounds: PageRect {
                x: info.bounds.x,
                y: info.bounds.y,
                width: info.bounds.width,
                height: info.bounds.height,
            },
            movable: info.movable,
            editable: info.editable,
            background: info.background,
            text: info.text,
            font_name: info.font_name,
            font_size: info.font_size,
            substituted: info.substituted,
            overflow: info.overflow,
            missing: info.missing,
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
        }
    }
}

/// Modifications du contenu d'une page à un instant donné, pour la rendre.
#[derive(uniffi::Object)]
pub struct ContentEdits {
    pub(crate) edits: Arc<PageEdits>,
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
