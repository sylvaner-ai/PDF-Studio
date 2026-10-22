//! Interface commune des moteurs de reconnaissance.

use crate::image::GrayImage;

/// Point d'une image, en pixels, origine en haut à gauche.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Abscisse.
    pub x: f32,
    /// Ordonnée, vers le bas.
    pub y: f32,
}

/// Quadrilatère qui entoure un mot, dans le sens de lecture : le bord
/// supérieur va de `top_left` à `top_right`. Un mot incliné ou tourné garde
/// ainsi son orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quad {
    /// Coin supérieur gauche (début du mot, en haut).
    pub top_left: Point,
    /// Coin supérieur droit.
    pub top_right: Point,
    /// Coin inférieur droit.
    pub bottom_right: Point,
    /// Coin inférieur gauche (début du mot, ligne de pied).
    pub bottom_left: Point,
}

impl Quad {
    /// Rectangle droit `(x0, y0)–(x1, y1)`.
    pub fn from_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            top_left: Point { x: x0, y: y0 },
            top_right: Point { x: x1, y: y0 },
            bottom_right: Point { x: x1, y: y1 },
            bottom_left: Point { x: x0, y: y1 },
        }
    }

    /// Les quatre coins, dans l'ordre.
    pub fn corners(&self) -> [Point; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }

    /// Centre (moyenne des coins).
    pub fn center(&self) -> Point {
        let corners = self.corners();
        Point {
            x: corners.iter().map(|p| p.x).sum::<f32>() / 4.0,
            y: corners.iter().map(|p| p.y).sum::<f32>() / 4.0,
        }
    }

    /// Rectangle droit qui contient le quadrilatère : `(x0, y0, x1, y1)`.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let corners = self.corners();
        let x0 = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let y0 = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let x1 = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let y1 = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        (x0, y0, x1, y1)
    }

    /// Hauteur du mot : distance moyenne entre bord supérieur et ligne de pied.
    pub fn height(&self) -> f32 {
        let left = distance(self.top_left, self.bottom_left);
        let right = distance(self.top_right, self.bottom_right);
        (left + right) / 2.0
    }

    /// Longueur du mot, le long de sa ligne de pied.
    pub fn width(&self) -> f32 {
        let top = distance(self.top_left, self.top_right);
        let bottom = distance(self.bottom_left, self.bottom_right);
        (top + bottom) / 2.0
    }

    /// Le même quadrilatère, chaque coin transformé par `map`.
    pub fn map(&self, map: impl Fn(Point) -> Point) -> Quad {
        Quad {
            top_left: map(self.top_left),
            top_right: map(self.top_right),
            bottom_right: map(self.bottom_right),
            bottom_left: map(self.bottom_left),
        }
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Un mot reconnu.
#[derive(Clone, Debug, PartialEq)]
pub struct RecognizedWord {
    /// Texte du mot, sans espace.
    pub text: String,
    /// Emprise du mot dans l'image.
    pub quad: Quad,
    /// Confiance du moteur, de 0 à 1.
    pub confidence: f32,
    /// Numéro de la ligne du mot ; les mots d'une même ligne partagent ce
    /// numéro et se suivent dans l'ordre de lecture.
    pub line: u32,
}

/// Échec d'une reconnaissance.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OcrError {
    /// Le moteur n'est pas disponible (absent, modèle manquant).
    #[error("le moteur de reconnaissance n'est pas disponible : {0}")]
    Unavailable(String),
    /// Le moteur a échoué sur cette image.
    #[error("la reconnaissance a échoué : {0}")]
    Failed(String),
    /// La reconnaissance a été interrompue.
    #[error("la reconnaissance a été interrompue")]
    Cancelled,
}

/// Moteur de reconnaissance de texte. Chaque moteur (Vision d'Apple,
/// Tesseract, `ocrs`…) reçoit l'image d'une page, déjà préparée, et renvoie
/// ses mots, avec leur emprise dans cette image et une confiance.
pub trait OcrEngine {
    /// Nom du moteur, pour les rapports.
    fn name(&self) -> String;

    /// Lit le texte de `image`. `languages` liste les langues attendues par
    /// ordre de préférence (codes BCP 47 : « fr-FR », « en-US »).
    fn recognize(
        &self,
        image: &GrayImage,
        languages: &[String],
    ) -> Result<Vec<RecognizedWord>, OcrError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesures_d_un_quadrilatere() {
        let quad = Quad::from_rect(10.0, 20.0, 50.0, 30.0);
        assert_eq!(quad.center(), Point { x: 30.0, y: 25.0 });
        assert_eq!(quad.bounds(), (10.0, 20.0, 50.0, 30.0));
        assert_eq!(quad.width(), 40.0);
        assert_eq!(quad.height(), 10.0);
    }
}
