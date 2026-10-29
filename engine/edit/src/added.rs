//! Objets ajoutés à une page en mode Édition : textes et images, dessinés
//! par-dessus son contenu d'origine.
//!
//! Un objet ajouté n'existe pas dans la page source : il est décrit ici en
//! entier (texte, police, corps, couleur ; octets de l'image) et placé dans
//! l'espace de la page PDF. Il est numéroté à partir de [`ADDED_ID`], à part
//! des objets d'origine et de ceux d'un scan, et s'empile dans l'ordre de ses
//! numéros. `pdf_core` le dessine droit à l'affichage, quelle que soit la
//! rotation propre de la page, et compense les quarts de tour que
//! l'organisation donnait à la page quand il a été ajouté : il se lit droit
//! sur la page telle qu'elle était montrée.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::content::FontChoice;

/// Premier numéro des objets ajoutés. Les objets d'origine ont des numéros
/// plus petits ; ceux d'un scan portent le bit `0x4000_0000`.
pub const ADDED_ID: u32 = 0x2000_0000;

/// Vrai si `id` désigne un objet ajouté.
pub fn is_added_id(id: u32) -> bool {
    id & 0x6000_0000 == ADDED_ID
}

/// Objet ajouté à une page.
#[derive(Clone, Debug, PartialEq)]
pub enum AddedObject {
    /// Texte.
    Text(AddedText),
    /// Image.
    Image(AddedImage),
}

impl AddedObject {
    /// Déplace l'objet, en points dans l'espace de la page PDF (ordonnées
    /// vers le haut).
    pub fn offset(&mut self, dx: f32, dy: f32) {
        match self {
            AddedObject::Text(text) => {
                text.x += dx;
                text.y += dy;
            }
            AddedObject::Image(image) => {
                image.rect = [
                    image.rect[0] + dx,
                    image.rect[1] + dy,
                    image.rect[2] + dx,
                    image.rect[3] + dy,
                ];
            }
        }
    }

    /// Quarts de tour de la page à l'écran quand l'objet a été ajouté.
    pub fn turns(&self) -> u8 {
        match self {
            AddedObject::Text(text) => text.turns,
            AddedObject::Image(image) => image.turns,
        }
    }
}

/// Texte ajouté : des lignes séparées par des retours voulus, sans renvoi
/// automatique.
#[derive(Clone, Debug, PartialEq)]
pub struct AddedText {
    /// Texte ; un saut de ligne commence une nouvelle ligne.
    pub text: String,
    /// Police : une police standard (Helvetica, Times, Courier) ou une police
    /// installée sur le Mac, intégrée au document.
    pub font: FontChoice,
    /// Corps, en points.
    pub size: f32,
    /// Couleur : rouge, vert, bleu, de 0 à 1.
    pub color: [f32; 3],
    /// Point de la page PDF qui est le coin supérieur gauche du texte à
    /// l'affichage.
    pub x: f32,
    /// Ordonnée de ce point (vers le haut).
    pub y: f32,
    /// Quarts de tour (sens horaire, 0 à 3) de la page à l'écran quand le
    /// texte a été ajouté : il est tourné d'autant en sens inverse.
    pub turns: u8,
}

/// Image ajoutée.
#[derive(Clone, Debug, PartialEq)]
pub struct AddedImage {
    /// Octets du fichier image (JPEG ou PNG), partagés entre les états.
    pub data: Arc<Vec<u8>>,
    /// Empreinte des octets, pour n'intégrer qu'une fois une même image.
    pub key: u64,
    /// Cadre de l'image dans l'espace de la page PDF : x0, y0, x1, y1.
    pub rect: [f32; 4],
    /// Quarts de tour (sens horaire, 0 à 3) de la page à l'écran quand
    /// l'image a été ajoutée : elle est tournée d'autant en sens inverse dans
    /// son cadre.
    pub turns: u8,
}

impl AddedImage {
    /// Image des octets `data`, placée dans le cadre `rect`, sur une page
    /// tournée de `turns` quarts de tour.
    pub fn new(data: Vec<u8>, rect: [f32; 4], turns: u8) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut hasher);
        Self {
            key: hasher.finish(),
            data: Arc::new(data),
            rect,
            turns: turns & 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeros_des_objets_ajoutes() {
        assert!(is_added_id(ADDED_ID));
        assert!(is_added_id(ADDED_ID + 12));
        assert!(!is_added_id(12));
        assert!(!is_added_id(0x4000_0001));
        assert!(!is_added_id(0x6000_0001));
    }

    #[test]
    fn deplace_un_objet_ajoute() {
        let mut image =
            AddedObject::Image(AddedImage::new(vec![1, 2, 3], [0.0, 0.0, 10.0, 20.0], 0));
        image.offset(5.0, -2.0);
        let AddedObject::Image(image) = image else {
            unreachable!()
        };
        assert_eq!(image.rect, [5.0, -2.0, 15.0, 18.0]);
        assert_eq!(image.key, AddedImage::new(vec![1, 2, 3], [0.0; 4], 1).key);
    }
}
