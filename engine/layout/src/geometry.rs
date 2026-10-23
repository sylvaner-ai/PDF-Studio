//! Géométrie du crate : rectangles et segments dans l'espace de la page PDF
//! (ordonnées vers le haut), en double précision.

/// Rectangle aux côtés parallèles aux axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Bord gauche.
    pub x0: f64,
    /// Bord inférieur.
    pub y0: f64,
    /// Bord droit.
    pub x1: f64,
    /// Bord supérieur.
    pub y1: f64,
}

impl Rect {
    /// Rectangle vide (neutre pour [`Rect::union`]).
    pub const EMPTY: Rect = Rect {
        x0: f64::INFINITY,
        y0: f64::INFINITY,
        x1: f64::NEG_INFINITY,
        y1: f64::NEG_INFINITY,
    };

    /// Rectangle de coins donnés, dans n'importe quel ordre.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    /// Vrai si le rectangle ne contient aucun point.
    pub fn is_empty(&self) -> bool {
        !(self.x0 <= self.x1 && self.y0 <= self.y1)
    }

    /// Largeur.
    pub fn width(&self) -> f64 {
        (self.x1 - self.x0).max(0.0)
    }

    /// Hauteur.
    pub fn height(&self) -> f64 {
        (self.y1 - self.y0).max(0.0)
    }

    /// Centre.
    pub fn center(&self) -> (f64, f64) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    /// Plus petit rectangle qui contient les deux.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Rectangle agrandi de `d` de chaque côté (rétréci si `d` est négatif).
    pub fn inflate(&self, d: f64) -> Rect {
        Rect {
            x0: self.x0 - d,
            y0: self.y0 - d,
            x1: self.x1 + d,
            y1: self.y1 + d,
        }
    }

    /// Vrai si le point est dans le rectangle (bords compris).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }

    /// Vrai si `other` est entièrement dans le rectangle.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        !other.is_empty()
            && other.x0 >= self.x0
            && other.x1 <= self.x1
            && other.y0 >= self.y0
            && other.y1 <= self.y1
    }

    /// Vrai si les deux rectangles ont un point commun.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }
}

/// Segment de droite, d'un trait ou du bord d'un rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// Première extrémité (`x`, `y`).
    pub a: (f64, f64),
    /// Seconde extrémité.
    pub b: (f64, f64),
}

impl Segment {
    /// Segment d'extrémités données.
    pub fn new(a: (f64, f64), b: (f64, f64)) -> Self {
        Segment { a, b }
    }

    /// Les quatre bords d'un rectangle.
    pub fn edges(rect: &Rect) -> [Segment; 4] {
        let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
        [
            Segment::new((x0, y0), (x1, y0)),
            Segment::new((x1, y0), (x1, y1)),
            Segment::new((x0, y1), (x1, y1)),
            Segment::new((x0, y0), (x0, y1)),
        ]
    }
}

/// Emprise d'un glyphe visible, pour situer le texte dans un tableau.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBox {
    /// Emprise du glyphe.
    pub bbox: Rect,
    /// Ordonnée de la ligne de base.
    pub baseline: f64,
    /// Corps du texte.
    pub size: f64,
}
