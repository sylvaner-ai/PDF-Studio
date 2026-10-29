//! Modifications du contenu d'une page : objets supprimés, déplacés, texte
//! remplacé.
//!
//! Les objets d'une page (blocs de texte, images, tracés…) sont désignés par leur
//! numéro dans l'ordre d'empilement, tel que l'établit `pdf_core` à partir du
//! contenu de la page source. Ce numéro ne dépend que du contenu d'origine : il
//! reste valable tant que la page source est la même, d'une session à l'autre.
//! Une modification ne touche donc jamais au document : elle est décrite ici et
//! appliquée au contenu de la page lors de l'affichage et de l'enregistrement.

use std::collections::BTreeMap;

use crate::added::{AddedObject, ADDED_ID};
use crate::scan::ScanText;

/// Police d'un texte remplacé.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontChoice {
    /// Police d'origine, intégrée au document : tous les caractères y figurent.
    Original,
    /// Police complète installée sur le Mac, désignée par son nom PostScript ;
    /// elle est intégrée au document à l'enregistrement.
    Installed {
        /// Nom PostScript (« Calibri-Bold »).
        postscript_name: String,
    },
    /// Police standard la plus proche (clone URW d'Helvetica, Times ou Courier).
    Standard {
        /// Nom de la police standard (« Times-Bold »).
        name: String,
    },
}

/// Nouveau texte d'un bloc.
#[derive(Clone, Debug, PartialEq)]
pub struct TextReplacement {
    /// Texte saisi ; un saut de ligne force un retour à la ligne.
    pub text: String,
    /// Police retenue pour tout le bloc.
    pub font: FontChoice,
}

/// Modifications d'un tableau : lignes et colonnes supprimées ou déplacées,
/// texte saisi dans des cellules vides.
///
/// Lignes et colonnes sont désignées par leur numéro dans le tableau d'origine,
/// tel que `pdf_core` le détecte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableEdit {
    /// Lignes d'origine conservées, dans leur nouvel ordre ; `None` : toutes,
    /// dans l'ordre d'origine.
    pub rows: Option<Vec<u32>>,
    /// Colonnes d'origine conservées, dans leur nouvel ordre.
    pub columns: Option<Vec<u32>>,
    /// Texte saisi dans des cellules vides, par cellule d'origine (ligne et
    /// colonne de son coin supérieur gauche).
    pub cells: BTreeMap<(u32, u32), TextReplacement>,
}

impl TableEdit {
    /// Vrai si le tableau est intact.
    pub fn is_unchanged(&self) -> bool {
        self.rows.is_none() && self.columns.is_none() && self.cells.is_empty()
    }
}

/// État modifié d'un objet ; un objet absent des modifications est intact.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectEdit {
    /// L'objet est supprimé.
    pub deleted: bool,
    /// Déplacement horizontal, en points, dans l'espace de la page PDF (axe des
    /// ordonnées vers le haut, sans rotation).
    pub dx: f32,
    /// Déplacement vertical, en points, vers le haut.
    pub dy: f32,
    /// Nouveau texte, pour un bloc de texte.
    pub text: Option<TextReplacement>,
    /// Modifications d'un tableau.
    pub table: Option<TableEdit>,
    /// Nouveau texte d'une ligne d'une page scannée, dessiné dans l'image.
    pub scan: Option<ScanText>,
}

impl ObjectEdit {
    /// Vrai si l'objet est déplacé.
    pub fn is_moved(&self) -> bool {
        self.dx != 0.0 || self.dy != 0.0
    }

    /// Vrai si l'objet est inchangé.
    pub fn is_unchanged(&self) -> bool {
        !self.deleted
            && !self.is_moved()
            && self.text.is_none()
            && self.scan.is_none()
            && self.table.as_ref().is_none_or(TableEdit::is_unchanged)
    }
}

/// Modifications du contenu d'une page, par numéro d'objet, et objets
/// ajoutés à la page (textes, images).
///
/// Chaque état est figé : une modification produit un nouvel état, numéroté
/// ([`PageEdits::revision`]), ce qui permet de garder en cache les images d'une
/// page pour chacun de ses états.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageEdits {
    objects: BTreeMap<u32, ObjectEdit>,
    added: BTreeMap<u32, AddedObject>,
    revision: u64,
}

impl PageEdits {
    /// Modifications données, numérotées `revision`.
    pub fn new(objects: BTreeMap<u32, ObjectEdit>, revision: u64) -> Self {
        Self::with_added(objects, BTreeMap::new(), revision)
    }

    /// Modifications et objets ajoutés donnés, numérotés `revision`.
    pub fn with_added(
        objects: BTreeMap<u32, ObjectEdit>,
        added: BTreeMap<u32, AddedObject>,
        revision: u64,
    ) -> Self {
        let objects = objects
            .into_iter()
            .filter(|(_, edit)| !edit.is_unchanged())
            .collect();
        Self {
            objects,
            added,
            revision,
        }
    }

    /// Numéro de cet état, unique dans la session (0 : aucune modification).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Modification d'un objet, s'il en a une.
    pub fn get(&self, object: u32) -> Option<&ObjectEdit> {
        self.objects.get(&object)
    }

    /// Objets modifiés, dans l'ordre de leurs numéros.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &ObjectEdit)> {
        self.objects.iter().map(|(&id, edit)| (id, edit))
    }

    /// Vrai si aucun objet n'est modifié ni ajouté.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.added.is_empty()
    }

    /// Copie des modifications, pour en dériver un nouvel état.
    pub fn objects(&self) -> BTreeMap<u32, ObjectEdit> {
        self.objects.clone()
    }

    /// Objets ajoutés, par numéro (ordre d'empilement).
    pub fn added(&self) -> &BTreeMap<u32, AddedObject> {
        &self.added
    }

    /// Numéro du prochain objet ajouté : au-dessus de tous les autres.
    pub fn next_added_id(&self) -> u32 {
        self.added
            .keys()
            .next_back()
            .map_or(ADDED_ID, |&last| last + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_objet_intact_n_est_pas_retenu() {
        let mut objects = BTreeMap::new();
        objects.insert(1, ObjectEdit::default());
        objects.insert(
            2,
            ObjectEdit {
                dx: 3.0,
                ..ObjectEdit::default()
            },
        );
        let edits = PageEdits::new(objects, 7);
        assert_eq!(edits.revision(), 7);
        assert!(edits.get(1).is_none());
        assert!(edits.get(2).unwrap().is_moved());
        assert_eq!(edits.iter().count(), 1);
    }
}
