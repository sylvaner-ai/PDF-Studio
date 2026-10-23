//! Lignes de texte d'un scan : bande redressée le long de la ligne et
//! mesures de son encre (couleur, ligne de base, grain), pour l'appariement
//! d'une police.

use fonts::{Observation, ObservedWord};

use crate::image::{luma, RgbImage};
use crate::ink::{grain, Paper};

/// Repère d'une ligne dans l'image : bande de `width × height` pixels dont
/// le coin supérieur gauche est `origin` et dont les rangées suivent
/// `direction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFrame {
    /// Coin supérieur gauche de la bande, en pixels de l'image.
    pub origin: (f64, f64),
    /// Direction de la ligne (vecteur unitaire ; `(1, 0)` si droite).
    pub direction: (f64, f64),
    /// Longueur de la bande.
    pub width: f64,
    /// Hauteur de la bande.
    pub height: f64,
}

impl LineFrame {
    /// Point de l'image d'un point de la bande (`s` le long de la ligne,
    /// `t` vers le bas).
    pub fn to_image(&self, s: f64, t: f64) -> (f64, f64) {
        let (dx, dy) = self.direction;
        (
            self.origin.0 + s * dx - t * dy,
            self.origin.1 + s * dy + t * dx,
        )
    }

    /// Point de la bande d'un point de l'image.
    pub fn to_line(&self, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = self.direction;
        let (rx, ry) = (x - self.origin.0, y - self.origin.1);
        (rx * dx + ry * dy, -rx * dy + ry * dx)
    }

    /// Angle de la ligne en degrés (positif : descend vers la droite dans
    /// l'image).
    pub fn angle(&self) -> f64 {
        self.direction.1.atan2(self.direction.0).to_degrees()
    }
}

/// Mesures d'une ligne scannée.
#[derive(Clone, Debug)]
pub struct LineInk {
    /// Bande redressée, prête pour l'appariement.
    pub observation: Observation,
    /// Couleur du cœur des traits.
    pub color: [f32; 3],
    /// Grain du papier (écart type).
    pub grain: f32,
    /// Grain de l'encre (écart type au cœur des traits).
    pub ink_grain: f32,
}

/// Échantillonne la ligne `frame` (interpolation bilinéaire) et mesure son
/// encre. `words` : texte de chaque mot et son étendue le long de la bande.
pub fn observe(image: &RgbImage, frame: &LineFrame, words: &[(String, f64, f64)]) -> LineInk {
    let w = frame.width.ceil().max(1.0) as usize;
    let h = frame.height.ceil().max(1.0) as usize;
    let mut strip = RgbImage::new(w, h, [255, 255, 255]);
    for t in 0..h {
        for s in 0..w {
            let (x, y) = frame.to_image(s as f64 + 0.5, t as f64 + 0.5);
            strip.set_f(s as i32, t as i32, bilinear(image, x - 0.5, y - 0.5));
        }
    }
    let radius = (h / 5).clamp(2, 12);
    let paper = Paper::closing(&strip, strip.bounds(), radius);
    let noise = grain(&strip, &paper, strip.bounds());
    let darkness: Vec<f32> = (0..h as i32)
        .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
        .map(|(x, y)| paper.darkness(&strip, x, y))
        .collect();
    // Contraste du cœur des traits : quantile haut des pixels d'encre.
    let mut inked: Vec<f32> = darkness.iter().copied().filter(|d| *d > 40.0).collect();
    inked.sort_by(f32::total_cmp);
    let core = inked
        .get(inked.len().saturating_sub(1) * 9 / 10)
        .copied()
        .unwrap_or(255.0)
        .max(60.0);
    let ink: Vec<f32> = darkness
        .iter()
        .map(|d| (d / core).clamp(0.0, 1.0))
        .collect();
    // Couleur et grain du cœur des traits.
    let mut cores: Vec<[f32; 3]> = Vec::new();
    for (i, d) in darkness.iter().enumerate() {
        if *d >= 0.8 * core {
            cores.push(strip.get_f((i % w) as i32, (i / w) as i32));
        }
    }
    let (color, ink_grain) = if cores.is_empty() {
        ([0.0; 3], 0.0)
    } else {
        let mut color = [0.0; 3];
        for (c, value) in color.iter_mut().enumerate() {
            let mut v: Vec<f32> = cores.iter().map(|p| p[c]).collect();
            v.sort_by(f32::total_cmp);
            *value = v[v.len() / 2];
        }
        let mean = luma(color);
        let var = cores.iter().map(|p| (luma(*p) - mean).powi(2)).sum::<f32>() / cores.len() as f32;
        (color, var.sqrt())
    };
    let baseline = baseline(&ink, w, h);
    LineInk {
        observation: Observation {
            width: w,
            height: h,
            ink,
            baseline,
            words: words
                .iter()
                .map(|(text, left, right)| ObservedWord {
                    text: text.clone(),
                    left: *left as f32,
                    right: *right as f32,
                })
                .collect(),
        },
        color,
        grain: noise,
        ink_grain,
    }
}

/// Ligne de base : bas de la dernière rangée dense en encre (les
/// jambages descendants, épars, sont ignorés).
fn baseline(ink: &[f32], w: usize, h: usize) -> f32 {
    let rows: Vec<f32> = (0..h)
        .map(|y| ink[y * w..(y + 1) * w].iter().sum())
        .collect();
    let peak = rows.iter().copied().fold(0.0, f32::max);
    if peak <= 0.0 {
        return h as f32 * 0.8;
    }
    let last = rows.iter().rposition(|r| *r >= 0.3 * peak).unwrap_or(h - 1);
    // Bord inférieur adouci : fraction de la rangée suivante.
    let next = rows.get(last + 1).copied().unwrap_or(0.0);
    last as f32 + 1.0 + (next / rows[last].max(1e-3)).min(0.5)
}

/// Couleur interpolée au point `(x, y)` (centres des pixels aux entiers).
pub fn bilinear(image: &RgbImage, x: f64, y: f64) -> [f32; 3] {
    let max_x = image.width as i32 - 1;
    let max_y = image.height as i32 - 1;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let pixel = |x: i32, y: i32| image.get_f(x.clamp(0, max_x), y.clamp(0, max_y));
    let (a, b, c, d) = (
        pixel(x0, y0),
        pixel(x0 + 1, y0),
        pixel(x0, y0 + 1),
        pixel(x0 + 1, y0 + 1),
    );
    [0, 1, 2].map(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bottom = c[k] + (d[k] - c[k]) * fx;
        top + (bottom - top) * fy
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repere_incline() {
        let angle = 3.0f64.to_radians();
        let frame = LineFrame {
            origin: (10.0, 20.0),
            direction: (angle.cos(), angle.sin()),
            width: 100.0,
            height: 20.0,
        };
        let (x, y) = frame.to_image(50.0, 10.0);
        let (s, t) = frame.to_line(x, y);
        assert!((s - 50.0).abs() < 1e-9 && (t - 10.0).abs() < 1e-9);
        assert!((frame.angle() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn mesure_une_ligne() {
        // Papier, « texte » : barres sombres de la rangée 5 à 14, un
        // jambage jusqu'à 18.
        let mut image = RgbImage::new(60, 24, [235, 232, 225]);
        for x in (4..56).step_by(4) {
            for y in 5..15 {
                image.set(x, y, [20, 30, 90]);
                image.set(x + 1, y, [20, 30, 90]);
            }
        }
        for y in 15..19 {
            image.set(8, y, [20, 30, 90]);
        }
        let frame = LineFrame {
            origin: (0.0, 0.0),
            direction: (1.0, 0.0),
            width: 60.0,
            height: 24.0,
        };
        let line = observe(&image, &frame, &[("mot".into(), 4.0, 56.0)]);
        assert!(
            (line.observation.baseline - 15.0).abs() < 0.6,
            "{}",
            line.observation.baseline
        );
        assert!((line.color[2] - 90.0).abs() < 5.0, "{:?}", line.color);
        assert_eq!(line.observation.words.len(), 1);
        assert!(line.grain < 1.0);
    }
}
