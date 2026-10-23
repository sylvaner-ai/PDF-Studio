//! Images RVB d'une page scannée et rectangles de pixels.

/// Rectangle de pixels, bords gauche et haut compris, droit et bas exclus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Bord gauche.
    pub x0: i32,
    /// Bord supérieur.
    pub y0: i32,
    /// Bord droit (exclu).
    pub x1: i32,
    /// Bord inférieur (exclu).
    pub y1: i32,
}

impl Rect {
    /// Rectangle vide.
    pub const EMPTY: Rect = Rect {
        x0: i32::MAX,
        y0: i32::MAX,
        x1: i32::MIN,
        y1: i32::MIN,
    };

    /// Rectangle de coins donnés.
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    /// Plus petit rectangle de pixels qui couvre le rectangle réel donné.
    pub fn covering(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect {
            x0: x0.min(x1).floor() as i32,
            y0: y0.min(y1).floor() as i32,
            x1: x0.max(x1).ceil() as i32,
            y1: y0.max(y1).ceil() as i32,
        }
    }

    /// Vrai si le rectangle ne contient aucun pixel.
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Largeur (calculée sans débordement, même pour [`Rect::EMPTY`]).
    pub fn width(&self) -> usize {
        (i64::from(self.x1) - i64::from(self.x0)).max(0) as usize
    }

    /// Hauteur.
    pub fn height(&self) -> usize {
        (i64::from(self.y1) - i64::from(self.y0)).max(0) as usize
    }

    /// Nombre de pixels.
    pub fn area(&self) -> usize {
        self.width() * self.height()
    }

    /// Rectangle agrandi de `d` pixels de chaque côté.
    pub fn inflate(&self, d: i32) -> Rect {
        if self.is_empty() {
            return *self;
        }
        Rect::new(
            self.x0.saturating_sub(d),
            self.y0.saturating_sub(d),
            self.x1.saturating_add(d),
            self.y1.saturating_add(d),
        )
    }

    /// Rectangle déplacé.
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        if self.is_empty() {
            return *self;
        }
        Rect::new(
            self.x0.saturating_add(dx),
            self.y0.saturating_add(dy),
            self.x1.saturating_add(dx),
            self.y1.saturating_add(dy),
        )
    }

    /// Partie commune.
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect::new(
            self.x0.max(other.x0),
            self.y0.max(other.y0),
            self.x1.min(other.x1),
            self.y1.min(other.y1),
        )
    }

    /// Plus petit rectangle qui contient les deux.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Rect::new(
            self.x0.min(other.x0),
            self.y0.min(other.y0),
            self.x1.max(other.x1),
            self.y1.max(other.y1),
        )
    }

    /// Vrai si le pixel `(x, y)` est dans le rectangle.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// Vrai si `other` est entièrement dans le rectangle.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        !other.is_empty()
            && other.x0 >= self.x0
            && other.y0 >= self.y0
            && other.x1 <= self.x1
            && other.y1 <= self.y1
    }
}

/// Image RVB 8 bits, rangée par rangée.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbImage {
    /// Largeur en pixels.
    pub width: usize,
    /// Hauteur en pixels.
    pub height: usize,
    /// Pixels : rouge, vert, bleu.
    pub data: Vec<u8>,
}

impl RgbImage {
    /// Image unie.
    pub fn new(width: usize, height: usize, color: [u8; 3]) -> Self {
        let mut data = Vec::with_capacity(width * height * 3);
        for _ in 0..width * height {
            data.extend_from_slice(&color);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Image tirée de pixels RVBA (`stride` octets par rangée), alpha ignoré.
    pub fn from_rgba(width: usize, height: usize, stride: usize, rgba: &[u8]) -> Self {
        let mut data = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            let row = &rgba[y * stride..y * stride + width * 4];
            for pixel in row.chunks_exact(4) {
                data.extend_from_slice(&pixel[..3]);
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Rectangle de toute l'image.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// Pixel `(x, y)`, qui doit être dans l'image.
    pub fn get(&self, x: i32, y: i32) -> [u8; 3] {
        let i = (y as usize * self.width + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Pixel `(x, y)` en réels (0 à 255).
    pub fn get_f(&self, x: i32, y: i32) -> [f32; 3] {
        let [r, g, b] = self.get(x, y);
        [f32::from(r), f32::from(g), f32::from(b)]
    }

    /// Remplace le pixel `(x, y)`, qui doit être dans l'image.
    pub fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        let i = (y as usize * self.width + x as usize) * 3;
        self.data[i..i + 3].copy_from_slice(&color);
    }

    /// Remplace le pixel `(x, y)` par une couleur réelle, arrondie et bornée.
    pub fn set_f(&mut self, x: i32, y: i32, color: [f32; 3]) {
        self.set(x, y, color.map(|c| c.round().clamp(0.0, 255.0) as u8));
    }

    /// Luminance du pixel `(x, y)` (0 à 255).
    pub fn luma(&self, x: i32, y: i32) -> f32 {
        luma(self.get_f(x, y))
    }

    /// Partie de l'image dans `rect` (bornée à l'image).
    pub fn crop(&self, rect: Rect) -> RgbImage {
        let rect = rect.intersect(&self.bounds());
        let (w, h) = (rect.width(), rect.height());
        let mut data = Vec::with_capacity(w * h * 3);
        for y in rect.y0..rect.y1 {
            let start = (y as usize * self.width + rect.x0 as usize) * 3;
            data.extend_from_slice(&self.data[start..start + w * 3]);
        }
        RgbImage {
            width: w,
            height: h,
            data,
        }
    }

    /// Vrai si tous les pixels du rectangle sont gris (rouge = vert = bleu).
    pub fn is_gray(&self, rect: Rect) -> bool {
        let rect = rect.intersect(&self.bounds());
        (rect.y0..rect.y1).all(|y| {
            (rect.x0..rect.x1).all(|x| {
                let [r, g, b] = self.get(x, y);
                r == g && g == b
            })
        })
    }

    /// Image réduite d'un facteur entier, chaque pixel gardant le plus sombre
    /// de son bloc : les filets fins restent visibles.
    pub fn darkest_downscale(&self, factor: usize) -> RgbImage {
        let factor = factor.max(1);
        let (w, h) = (self.width.div_ceil(factor), self.height.div_ceil(factor));
        let mut out = RgbImage::new(w, h, [255, 255, 255]);
        for y in 0..h {
            for x in 0..w {
                let mut best = ([255u8; 3], f32::INFINITY);
                for sy in y * factor..((y + 1) * factor).min(self.height) {
                    for sx in x * factor..((x + 1) * factor).min(self.width) {
                        let pixel = self.get(sx as i32, sy as i32);
                        let l = luma(pixel.map(f32::from));
                        if l < best.1 {
                            best = (pixel, l);
                        }
                    }
                }
                out.set(x as i32, y as i32, best.0);
            }
        }
        out
    }
}

/// Luminance d'une couleur (0 à 255).
pub fn luma(color: [f32; 3]) -> f32 {
    0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2]
}

/// Générateur pseudo-aléatoire déterministe (xorshift), pour un grain
/// reproductible : le même aperçu et le même fichier.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u64,
}

impl Noise {
    /// Générateur initialisé par `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Valeur uniforme dans [0, 1[.
    pub fn uniform(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Valeur d'une loi normale centrée réduite (approximation par somme de
    /// quatre uniformes).
    pub fn gaussian(&mut self) -> f32 {
        let sum: f32 = (0..4).map(|_| self.uniform()).sum();
        (sum - 2.0) * 1.732_050_8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_rectangle_vide_ou_extreme_ne_deborde_pas() {
        assert_eq!(Rect::EMPTY.width(), 0);
        assert_eq!(Rect::EMPTY.area(), 0);
        let huge = Rect::new(i32::MIN, 0, i32::MAX, 1);
        assert_eq!(huge.width(), u32::MAX as usize);
        assert_eq!(huge.inflate(4).x1, i32::MAX);
        assert_eq!(huge.offset(-3, 0).x0, i32::MIN);
    }

    #[test]
    fn rectangles() {
        let a = Rect::new(0, 0, 10, 5);
        let b = Rect::new(8, 2, 20, 9);
        assert_eq!(a.intersect(&b), Rect::new(8, 2, 10, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 20, 9));
        assert!(Rect::EMPTY.is_empty());
        assert_eq!(Rect::EMPTY.union(&a), a);
        assert_eq!(Rect::covering(1.2, 3.8, 4.1, 2.0), Rect::new(1, 2, 5, 4));
        assert!(a.contains(9, 4) && !a.contains(10, 4));
    }

    #[test]
    fn reduction_qui_garde_les_filets() {
        let mut image = RgbImage::new(8, 8, [250, 250, 250]);
        for x in 0..8 {
            image.set(x, 5, [10, 10, 10]);
        }
        let small = image.darkest_downscale(4);
        assert_eq!((small.width, small.height), (2, 2));
        assert_eq!(small.get(0, 1), [10, 10, 10]);
        assert_eq!(small.get(1, 0), [250, 250, 250]);
    }

    #[test]
    fn bruit_reproductible() {
        let (mut a, mut b) = (Noise::new(7), Noise::new(7));
        let values: Vec<f32> = (0..1000).map(|_| a.gaussian()).collect();
        assert!(values
            .iter()
            .zip((0..1000).map(|_| b.gaussian()))
            .all(|(x, y)| *x == y));
        let mean = values.iter().sum::<f32>() / 1000.0;
        let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / 1000.0;
        assert!(mean.abs() < 0.1 && (var - 1.0).abs() < 0.15, "{mean} {var}");
    }
}
