//! Géométrie de l'espace PDF : matrices de transformation, points et
//! rectangles, en double précision.
//!
//! Convention du PDF : un point est un vecteur ligne, `p' = p × M`, et la
//! matrice `[a b c d e f]` représente `[[a b 0] [c d 0] [e f 1]]`.

/// Matrice de transformation affine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    /// Coefficients `[a b c d e f]`.
    pub a: f64,
    /// Coefficient b.
    pub b: f64,
    /// Coefficient c.
    pub c: f64,
    /// Coefficient d.
    pub d: f64,
    /// Translation horizontale.
    pub e: f64,
    /// Translation verticale.
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    /// Matrice identité.
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Matrice de coefficients donnés.
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// Matrice lue dans six nombres ; l'identité s'il en manque.
    pub fn from_slice(values: &[f64]) -> Self {
        match values {
            [a, b, c, d, e, f] => Self::new(*a, *b, *c, *d, *e, *f),
            _ => Self::IDENTITY,
        }
    }

    /// Translation.
    pub fn translate(x: f64, y: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    /// `self × other` : `self` d'abord, puis `other`.
    pub fn then(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Déterminant.
    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Inverse ; `None` pour une matrice dégénérée.
    pub fn inverse(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det.abs() < 1e-12 || !det.is_finite() {
            return None;
        }
        let a = self.d / det;
        let b = -self.b / det;
        let c = -self.c / det;
        let d = self.a / det;
        Some(Matrix {
            a,
            b,
            c,
            d,
            e: -(self.e * a + self.f * c),
            f: -(self.e * b + self.f * d),
        })
    }

    /// Image d'un point.
    pub fn apply(&self, point: Point) -> Point {
        Point {
            x: point.x * self.a + point.y * self.c + self.e,
            y: point.x * self.b + point.y * self.d + self.f,
        }
    }

    /// Image d'un vecteur (sans la translation).
    pub fn apply_vector(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.a + y * self.c, x * self.b + y * self.d)
    }

    /// Partie linéaire seule (translation nulle).
    pub fn linear(&self) -> Matrix {
        Matrix {
            e: 0.0,
            f: 0.0,
            ..*self
        }
    }

    /// Facteur d'échelle vertical : longueur de l'image du vecteur (0, 1).
    pub fn vertical_scale(&self) -> f64 {
        self.c.hypot(self.d)
    }

    /// Facteur d'échelle horizontal : longueur de l'image du vecteur (1, 0).
    pub fn horizontal_scale(&self) -> f64 {
        self.a.hypot(self.b)
    }

    /// Image d'un rectangle : le plus petit rectangle qui contient l'image de
    /// ses quatre coins.
    pub fn apply_rect(&self, rect: Rect) -> Rect {
        let corners = [
            Point::new(rect.x0, rect.y0),
            Point::new(rect.x1, rect.y0),
            Point::new(rect.x0, rect.y1),
            Point::new(rect.x1, rect.y1),
        ];
        Rect::bounding(corners.iter().map(|&p| self.apply(p)))
    }
}

/// Point du plan.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Abscisse.
    pub x: f64,
    /// Ordonnée.
    pub y: f64,
}

impl Point {
    /// Point de coordonnées données.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Rectangle aux côtés parallèles aux axes, `x0 ≤ x1` et `y0 ≤ y1` (sauf vide).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Bord gauche.
    pub x0: f64,
    /// Bord bas (dans l'espace PDF) ou haut (dans l'espace de l'écran).
    pub y0: f64,
    /// Bord droit.
    pub x1: f64,
    /// Bord opposé à `y0`.
    pub y1: f64,
}

impl Rect {
    /// Rectangle vide : neutre pour l'union.
    pub const EMPTY: Rect = Rect {
        x0: f64::INFINITY,
        y0: f64::INFINITY,
        x1: f64::NEG_INFINITY,
        y1: f64::NEG_INFINITY,
    };

    /// Rectangle de coins donnés, dans n'importe quel ordre.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    /// Plus petit rectangle contenant des points.
    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Self {
        points.into_iter().fold(Self::EMPTY, |rect, p| Rect {
            x0: rect.x0.min(p.x),
            y0: rect.y0.min(p.y),
            x1: rect.x1.max(p.x),
            y1: rect.y1.max(p.y),
        })
    }

    /// Vrai si le rectangle ne contient aucun point.
    pub fn is_empty(&self) -> bool {
        !(self.x0 <= self.x1 && self.y0 <= self.y1)
    }

    /// Largeur.
    pub fn width(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.x1 - self.x0
        }
    }

    /// Hauteur.
    pub fn height(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.y1 - self.y0
        }
    }

    /// Union.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Intersection ; vide si les rectangles sont disjoints.
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// Agrandi de `margin` de chaque côté.
    pub fn inflate(&self, margin: f64) -> Rect {
        if self.is_empty() {
            return *self;
        }
        Rect {
            x0: self.x0 - margin,
            y0: self.y0 - margin,
            x1: self.x1 + margin,
            y1: self.y1 + margin,
        }
    }

    /// Décalé de `(dx, dy)`.
    pub fn offset(&self, dx: f64, dy: f64) -> Rect {
        Rect {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    /// Aire ; nulle pour un rectangle vide.
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }
}

impl From<Rect> for layout::Rect {
    fn from(rect: Rect) -> Self {
        layout::Rect {
            x0: rect.x0,
            y0: rect.y0,
            x1: rect.x1,
            y1: rect.y1,
        }
    }
}

impl From<layout::Rect> for Rect {
    fn from(rect: layout::Rect) -> Self {
        Rect {
            x0: rect.x0,
            y0: rect.y0,
            x1: rect.x1,
            y1: rect.y1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_et_inverse() {
        let scale = Matrix::new(2.0, 0.0, 0.0, 3.0, 0.0, 0.0);
        let shift = Matrix::translate(10.0, 20.0);
        // Mise à l'échelle puis translation.
        let both = scale.then(&shift);
        assert_eq!(both.apply(Point::new(1.0, 1.0)), Point::new(12.0, 23.0));
        let back = both.inverse().unwrap().apply(Point::new(12.0, 23.0));
        assert!((back.x - 1.0).abs() < 1e-9 && (back.y - 1.0).abs() < 1e-9);
        assert!(Matrix::new(0.0, 0.0, 0.0, 0.0, 1.0, 1.0)
            .inverse()
            .is_none());
    }

    #[test]
    fn rectangle_tourne() {
        let quarter = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let rect = quarter.apply_rect(Rect::new(0.0, 0.0, 2.0, 1.0));
        assert_eq!(rect, Rect::new(-1.0, 0.0, 0.0, 2.0));
        assert!(Rect::EMPTY.is_empty());
        assert_eq!(Rect::EMPTY.union(&rect), rect);
    }
}
