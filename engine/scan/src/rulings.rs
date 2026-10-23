//! Filets d'un scan : longues suites de pixels sombres horizontales ou
//! verticales, en tenant compte de l'inclinaison de la page.

use crate::image::{luma, Rect, RgbImage};
use crate::ink::Mask;

/// Filet trouvé, en pixels de l'image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruling {
    /// Début (gauche ou haut).
    pub from: (f64, f64),
    /// Fin (droite ou bas).
    pub to: (f64, f64),
    /// Épaisseur en pixels.
    pub thickness: f64,
    /// Filet horizontal (sinon vertical).
    pub horizontal: bool,
}

impl Ruling {
    /// Rectangle de pixels qui couvre le filet, épaisseur comprise.
    pub fn bounds(&self) -> Rect {
        let half = self.thickness / 2.0 + 0.5;
        Rect::covering(
            self.from.0.min(self.to.0) - half,
            self.from.1.min(self.to.1) - half,
            self.from.0.max(self.to.0) + half,
            self.from.1.max(self.to.1) + half,
        )
    }

    /// Longueur en pixels.
    pub fn length(&self) -> f64 {
        (self.to.0 - self.from.0).hypot(self.to.1 - self.from.1)
    }
}

/// Pixels sombres de l'image : luminance inférieure à celle du papier
/// (médiane de la page) d'au moins `contrast`.
pub fn dark_mask(image: &RgbImage, contrast: f32) -> Mask {
    let mut histogram = [0usize; 256];
    for y in 0..image.height as i32 {
        for x in 0..image.width as i32 {
            histogram[image.luma(x, y).round().clamp(0.0, 255.0) as usize] += 1;
        }
    }
    let half = image.width * image.height / 2;
    let mut seen = 0;
    let mut paper = 255.0;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= half {
            paper = value as f32;
            break;
        }
    }
    let limit = paper - contrast;
    let mut mask = Mask::new(image.bounds());
    for y in 0..image.height as i32 {
        for x in 0..image.width as i32 {
            if luma(image.get_f(x, y)) < limit {
                mask.set(x, y, true);
            }
        }
    }
    mask
}

/// Trous tolérés dans un filet (pixels).
const GAP: usize = 3;

/// Filets de `mask` d'au moins `min_length` pixels et d'au plus
/// `max_thickness`, pour une page inclinée de `skew` degrés (positif :
/// lignes descendant vers la droite).
pub fn detect(mask: &Mask, skew: f64, min_length: usize, max_thickness: usize) -> Vec<Ruling> {
    let slope = skew.to_radians().tan();
    let mut out = horizontal(mask, slope, min_length, max_thickness);
    out.extend(vertical(mask, slope, min_length, max_thickness));
    out
}

/// Suite de pixels sombres sur une rangée inclinée : `(rangée, début, fin)`.
type Run = (i32, usize, usize);

fn horizontal(mask: &Mask, slope: f64, min_length: usize, max_thickness: usize) -> Vec<Ruling> {
    let r = mask.rect;
    let w = r.width();
    // Rangées décalées pour couvrir toute l'image malgré la pente.
    let drift = (slope * w as f64).abs().ceil() as i32 + 1;
    let mut runs: Vec<Run> = Vec::new();
    for row in r.y0 - drift..r.y1 + drift {
        let dark = |x: usize| {
            let y = row as f64 + slope * x as f64;
            mask.get(r.x0 + x as i32, y.round() as i32)
        };
        collect_runs(w, dark, row, min_length, &mut runs);
    }
    group(&runs, max_thickness)
        .into_iter()
        .map(|(row, start, end, thickness)| {
            let point = |x: f64| (r.x0 as f64 + x, row + slope * x);
            Ruling {
                from: point(start as f64),
                to: point(end as f64),
                thickness,
                horizontal: true,
            }
        })
        .collect()
}

fn vertical(mask: &Mask, slope: f64, min_length: usize, max_thickness: usize) -> Vec<Ruling> {
    let r = mask.rect;
    let h = r.height();
    let drift = (slope * h as f64).abs().ceil() as i32 + 1;
    let mut runs: Vec<Run> = Vec::new();
    for column in r.x0 - drift..r.x1 + drift {
        let dark = |y: usize| {
            let x = column as f64 - slope * y as f64;
            mask.get(x.round() as i32, r.y0 + y as i32)
        };
        collect_runs(h, dark, column, min_length, &mut runs);
    }
    group(&runs, max_thickness)
        .into_iter()
        .map(|(column, start, end, thickness)| {
            let point = |y: f64| (column - slope * y, r.y0 as f64 + y);
            Ruling {
                from: point(start as f64),
                to: point(end as f64),
                thickness,
                horizontal: false,
            }
        })
        .collect()
}

/// Ajoute à `runs` les suites sombres d'au moins `min_length` pixels d'une
/// rangée.
fn collect_runs(
    len: usize,
    dark: impl Fn(usize) -> bool,
    row: i32,
    min_length: usize,
    runs: &mut Vec<Run>,
) {
    let mut start: Option<usize> = None;
    let mut last = 0;
    for i in 0..=len {
        let on = i < len && dark(i);
        if on {
            if start.is_none() {
                start = Some(i);
            }
            last = i;
        } else if let Some(s) = start {
            if i - last > GAP || i == len {
                if last + 1 - s >= min_length {
                    runs.push((row, s, last + 1));
                }
                start = None;
            }
        }
    }
}

/// Réunit les suites des rangées voisines qui se recouvrent en un filet :
/// `(rangée moyenne, début, fin, épaisseur)`. Les groupes trop épais (aplats)
/// sont écartés.
fn group(runs: &[Run], max_thickness: usize) -> Vec<(f64, usize, usize, f64)> {
    struct Group {
        rows: Vec<i32>,
        start: usize,
        end: usize,
        last: i32,
    }
    let mut open: Vec<Group> = Vec::new();
    let mut done: Vec<Group> = Vec::new();
    for &(row, start, end) in runs {
        // Les groupes qui n'ont rien reçu à la rangée précédente sont clos.
        let (keep, closed): (Vec<Group>, Vec<Group>) =
            open.into_iter().partition(|g| g.last >= row - 1);
        open = keep;
        done.extend(closed);
        let hit = open.iter_mut().find(|g| {
            let overlap = end.min(g.end).saturating_sub(start.max(g.start));
            overlap * 2 >= (end - start).min(g.end - g.start)
        });
        match hit {
            Some(g) => {
                if g.last != row {
                    g.rows.push(row);
                }
                g.last = row;
                g.start = g.start.min(start);
                g.end = g.end.max(end);
            }
            None => open.push(Group {
                rows: vec![row],
                start,
                end,
                last: row,
            }),
        }
    }
    done.extend(open);
    done.into_iter()
        .filter(|g| g.rows.len() <= max_thickness)
        .map(|g| {
            let mean = g.rows.iter().map(|r| f64::from(*r)).sum::<f64>() / g.rows.len() as f64;
            (mean, g.start, g.end, g.rows.len() as f64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trouve_un_cadre_incline() {
        let mut image = RgbImage::new(200, 120, [240, 240, 240]);
        let slope = 1.5f64.to_radians().tan();
        // Cadre de 150 × 80 incliné, filets de 2 pixels.
        for i in 0..150 {
            for t in 0..2 {
                let x = 20 + i;
                for y0 in [20.0, 100.0] {
                    let y = (y0 + t as f64 + slope * x as f64).round() as i32;
                    image.set(x, y, [30, 30, 30]);
                }
            }
        }
        for j in 0..80 {
            for t in 0..2 {
                let y = 21 + j;
                for x0 in [20.0, 170.0] {
                    let x = (x0 + t as f64 - slope * y as f64).round() as i32;
                    image.set(x, y, [30, 30, 30]);
                }
            }
        }
        // Un « mot » : ne doit pas donner de filet.
        for x in 60..90 {
            if x % 3 != 0 {
                for y in 50..60 {
                    image.set(x, y, [30, 30, 30]);
                }
            }
        }
        let mask = dark_mask(&image, 60.0);
        let found = detect(&mask, 1.5, 40, 6);
        let h: Vec<_> = found.iter().filter(|r| r.horizontal).collect();
        let v: Vec<_> = found.iter().filter(|r| !r.horizontal).collect();
        assert_eq!((h.len(), v.len()), (2, 2), "{found:?}");
        assert!(h.iter().all(|r| r.length() > 140.0 && r.thickness <= 3.0));
        assert!(v.iter().all(|r| r.length() > 70.0));
    }
}
