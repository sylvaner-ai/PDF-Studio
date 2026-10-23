//! Retouche des pages scannées, sur les pixels de l'image du scan.
//!
//! - [`ink`] : couleur locale du papier, masque des pixels d'encre,
//!   composantes connexes ;
//! - [`inpaint`] : comblement des pixels effacés (méthode de Telea) ;
//! - [`retouch`] : sélection, effacement et transfert de l'encre d'une zone,
//!   dessin d'un nouveau texte ;
//! - [`line`] : bande redressée d'une ligne de texte et mesures de son encre
//!   (pour l'appariement de police du crate `fonts`) ;
//! - [`rulings`] et [`elements`] : filets (tableaux) et éléments graphiques
//!   trouvés dans l'image.
//!
//! Les coordonnées sont des pixels de l'image, ordonnées vers le bas. Ce
//! crate ne dépend ni de MuPDF ni du format PDF.
#![forbid(unsafe_code)]

pub mod elements;
pub mod image;
pub mod ink;
pub mod inpaint;
pub mod line;
pub mod retouch;
pub mod rulings;

pub use image::{Noise, Rect, RgbImage};
