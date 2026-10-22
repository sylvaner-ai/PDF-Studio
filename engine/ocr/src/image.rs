//! Image en niveaux de gris d'une page et transformations affines.

use crate::engine::Point;

/// Transformation affine du plan : `(x, y) ↦ (a·x + c·y + e, b·x + d·y + f)`,
/// comme les matrices PDF. Les coordonnées d'image ont l'origine en haut à
/// gauche, l'axe des ordonnées vers le bas, et se comptent en pixels depuis le
/// bord des pixels (le centre du premier pixel est en 0,5 ; 0,5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Coefficient de x dans la nouvelle abscisse.
    pub a: f64,
    /// Coefficient de x dans la nouvelle ordonnée.
    pub b: f64,
    /// Coefficient de y dans la nouvelle abscisse.
    pub c: f64,
    /// Coefficient de y dans la nouvelle ordonnée.
    pub d: f64,
    /// Translation horizontale.
    pub e: f64,
    /// Translation verticale.
    pub f: f64,
}

impl Affine {
    /// Identité.
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Transformation donnée par ses six coefficients.
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// Mise à l'échelle uniforme.
    pub fn scale(factor: f64) -> Self {
        Self::new(factor, 0.0, 0.0, factor, 0.0, 0.0)
    }

    /// Image d'un point.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Image d'un point de mot.
    pub fn apply_point(&self, point: Point) -> Point {
        let (x, y) = self.apply(f64::from(point.x), f64::from(point.y));
        Point {
            x: x as f32,
            y: y as f32,
        }
    }

    /// `self` puis `next`.
    pub fn then(&self, next: &Affine) -> Affine {
        Affine {
            a: self.a * next.a + self.b * next.c,
            b: self.a * next.b + self.b * next.d,
            c: self.c * next.a + self.d * next.c,
            d: self.c * next.b + self.d * next.d,
            e: self.e * next.a + self.f * next.c + next.e,
            f: self.e * next.b + self.f * next.d + next.f,
        }
    }

    /// Transformation réciproque ; `None` si elle n'existe pas.
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let a = self.d / det;
        let b = -self.b / det;
        let c = -self.c / det;
        let d = self.a / det;
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(self.e * a + self.f * c),
            f: -(self.e * b + self.f * d),
        })
    }
}

/// Image en niveaux de gris, 8 bits par pixel (0 : noir, 255 : blanc),
/// rangée par rangée, sans marge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Pixels, `width × height` octets.
    pub pixels: Vec<u8>,
}

impl GrayImage {
    /// Image uniforme.
    pub fn new(width: u32, height: u32, value: u8) -> Self {
        Self {
            width,
            height,
            pixels: vec![value; width as usize * height as usize],
        }
    }

    /// Image formée de pixels déjà en niveaux de gris ; `None` si leur nombre
    /// ne correspond pas aux dimensions.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == width as usize * height as usize).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Convertit une image RGBA (luminance de la recommandation BT.601).
    pub fn from_rgba(width: u32, height: u32, bytes_per_row: u32, rgba: &[u8]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height as usize {
            let row = &rgba[y * bytes_per_row as usize..][..width as usize * 4];
            pixels.extend(row.chunks_exact(4).map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 + 500)
                    / 1000) as u8
            }));
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Valeur du pixel `(x, y)`.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    /// Image tournée de `turns` quarts de tour dans le sens horaire, et la
    /// transformation qui ramène un point de l'image tournée dans celle-ci.
    pub fn rotate_quarters(&self, turns: u8) -> (GrayImage, Affine) {
        let (w, h) = (self.width as usize, self.height as usize);
        let (wf, hf) = (f64::from(self.width), f64::from(self.height));
        match turns % 4 {
            0 => (self.clone(), Affine::IDENTITY),
            2 => {
                let pixels = self.pixels.iter().rev().copied().collect();
                let image = GrayImage {
                    width: self.width,
                    height: self.height,
                    pixels,
                };
                (image, Affine::new(-1.0, 0.0, 0.0, -1.0, wf, hf))
            }
            turns => {
                // Image tournée : largeur h, hauteur w.
                let mut pixels = vec![0u8; w * h];
                for y in 0..h {
                    let row = &self.pixels[y * w..][..w];
                    for (x, &value) in row.iter().enumerate() {
                        let (nx, ny) = if turns == 1 {
                            (h - 1 - y, x)
                        } else {
                            (y, w - 1 - x)
                        };
                        pixels[ny * h + nx] = value;
                    }
                }
                let image = GrayImage {
                    width: self.height,
                    height: self.width,
                    pixels,
                };
                let to_source = if turns == 1 {
                    // x = y', y = h − x'
                    Affine::new(0.0, -1.0, 1.0, 0.0, 0.0, hf)
                } else {
                    // x = w − y', y = x'
                    Affine::new(0.0, 1.0, -1.0, 0.0, wf, 0.0)
                };
                (image, to_source)
            }
        }
    }

    /// Image tournée de `degrees` degrés dans le sens horaire autour de son
    /// centre, agrandie pour tout contenir, les coins remplis de `fill`, et la
    /// transformation qui ramène un point de l'image tournée dans celle-ci.
    pub fn rotate(&self, degrees: f64, fill: u8) -> (GrayImage, Affine) {
        if degrees.abs() < 1e-3 {
            return (self.clone(), Affine::IDENTITY);
        }
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (wf, hf) = (f64::from(self.width), f64::from(self.height));
        let out_w = (wf * cos.abs() + hf * sin.abs()).ceil();
        let out_h = (wf * sin.abs() + hf * cos.abs()).ceil();
        // Point de l'image tournée → point de l'image d'origine : rotation
        // inverse autour des centres.
        let to_source = Affine::new(1.0, 0.0, 0.0, 1.0, -out_w / 2.0, -out_h / 2.0)
            .then(&Affine::new(cos, -sin, sin, cos, 0.0, 0.0))
            .then(&Affine::new(1.0, 0.0, 0.0, 1.0, wf / 2.0, hf / 2.0));
        let (ow, oh) = (out_w as u32, out_h as u32);
        let mut pixels = vec![fill; ow as usize * oh as usize];
        for y in 0..oh {
            for x in 0..ow {
                let (sx, sy) = to_source.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                pixels[y as usize * ow as usize + x as usize] = self.sample(sx, sy, fill);
            }
        }
        let image = GrayImage {
            width: ow,
            height: oh,
            pixels,
        };
        (image, to_source)
    }

    /// Valeur interpolée (bilinéaire) au point `(x, y)` ; `fill` hors de l'image.
    fn sample(&self, x: f64, y: f64, fill: u8) -> u8 {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |px: f64, py: f64| -> f64 {
            if px < 0.0 || py < 0.0 || px >= f64::from(self.width) || py >= f64::from(self.height) {
                f64::from(fill)
            } else {
                f64::from(self.get(px as u32, py as u32))
            }
        };
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1.0, y0) * tx;
        let bottom = at(x0, y0 + 1.0) * (1.0 - tx) + at(x0 + 1.0, y0 + 1.0) * tx;
        (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8
    }

    /// Image réduite d'un facteur entier (moyenne de chaque carré de pixels).
    pub fn downscale(&self, factor: u32) -> GrayImage {
        let factor = factor.max(1);
        if factor == 1 {
            return self.clone();
        }
        let (w, h) = (self.width / factor, self.height / factor);
        let (w, h) = (w.max(1), h.max(1));
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0u32;
                let mut count = 0u32;
                for dy in 0..factor {
                    let sy = y * factor + dy;
                    if sy >= self.height {
                        break;
                    }
                    for dx in 0..factor {
                        let sx = x * factor + dx;
                        if sx >= self.width {
                            break;
                        }
                        sum += u32::from(self.get(sx, sy));
                        count += 1;
                    }
                }
                pixels.push((sum / count.max(1)) as u8);
            }
        }
        GrayImage {
            width: w,
            height: h,
            pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn composition_et_inverse() {
        let m = Affine::new(2.0, 0.5, -1.0, 3.0, 4.0, -2.0);
        let inv = m.inverse().unwrap();
        assert!(close(m.then(&inv).apply(7.0, 9.0), (7.0, 9.0)));
        let p = m.apply(1.0, 2.0);
        assert!(close(inv.apply(p.0, p.1), (1.0, 2.0)));
    }

    #[test]
    fn quarts_de_tour() {
        // Image 3 × 2, un pixel noir en haut à gauche.
        let mut image = GrayImage::new(3, 2, 255);
        image.pixels[0] = 0;
        for turns in 0..4 {
            let (rotated, to_source) = image.rotate_quarters(turns);
            let dark = rotated.pixels.iter().position(|&v| v == 0).unwrap() as u32;
            let (x, y) = (dark % rotated.width, dark / rotated.width);
            // Le centre du pixel noir revient au centre du pixel (0, 0).
            let back = to_source.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
            assert!(close(back, (0.5, 0.5)), "{turns} : {back:?}");
        }
        assert_eq!(image.rotate_quarters(1).0.width, 2);
    }

    #[test]
    fn rotation_quelconque() {
        let mut image = GrayImage::new(40, 20, 255);
        for x in 5..35 {
            image.pixels[10 * 40 + x] = 0;
        }
        let (rotated, to_source) = image.rotate(10.0, 255);
        assert!(rotated.width > 40 && rotated.height > 20);
        // Chaque pixel sombre de l'image tournée vient de la ligne d'origine.
        for (index, &value) in rotated.pixels.iter().enumerate() {
            if value < 64 {
                let (x, y) = (index as u32 % rotated.width, index as u32 / rotated.width);
                let (_, sy) = to_source.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                assert!((sy - 10.5).abs() < 1.5);
            }
        }
    }

    #[test]
    fn conversion_rgba() {
        let rgba = [255, 255, 255, 255, 0, 0, 0, 255, 9, 9, 9, 9];
        let image = GrayImage::from_rgba(2, 1, 12, &rgba);
        assert_eq!(image.pixels, [255, 0]);
        assert_eq!(image.downscale(2).pixels, [127]);
    }
}
