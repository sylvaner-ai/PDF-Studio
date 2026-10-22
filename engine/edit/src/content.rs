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
}

impl ObjectEdit {
    /// Vrai si l'objet est déplacé.
    pub fn is_moved(&self) -> bool {
        self.dx != 0.0 || self.dy != 0.0
    }

    /// Vrai si l'objet est inchangé.
    pub fn is_unchanged(&self) -> bool {
        !self.deleted && !self.is_moved() && self.text.is_none()
    }
}

/// Modifications du contenu d'une page, par numéro d'objet.
///
/// Chaque état est figé : une modification produit un nouvel état, numéroté
/// ([`PageEdits::revision`]), ce qui permet de garder en cache les images d'une
/// page pour chacun de ses états.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageEdits {
    objects: BTreeMap<u32, ObjectEdit>,
    revision: u64,
}

impl PageEdits {
    /// Modifications données, numérotées `revision`.
    pub fn new(objects: BTreeMap<u32, ObjectEdit>, revision: u64) -> Self {
        let objects = objects
            .into_iter()
            .filter(|(_, edit)| !edit.is_unchanged())
            .collect();
        Self { objects, revision }
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

    /// Vrai si aucun objet n'est modifié.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Copie des modifications, pour en dériver un nouvel état.
    pub fn objects(&self) -> BTreeMap<u32, ObjectEdit> {
        self.objects.clone()
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
