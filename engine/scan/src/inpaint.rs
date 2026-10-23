//! Comblement des pixels effacés (inpainting) par la méthode de Telea :
//! une marche rapide (« fast marching ») progresse du bord vers le centre de
//! la zone, et chaque pixel atteint reçoit la moyenne pondérée des pixels
//! déjà connus de son voisinage, favorisant les plus proches et ceux situés
//! dans la direction de progression.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::image::{Noise, Rect, RgbImage};
use crate::ink::Mask;

/// État d'un pixel pendant la marche.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Known,
    Band,
    Inside,
}

/// Entrée du tas : distance au bord et pixel.
#[derive(PartialEq)]
struct Front(f32, i32, i32);

impl Eq for Front {}

impl PartialOrd for Front {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Front {
    fn cmp(&self, other: &Self) -> Ordering {
        // Tas minimal sur la distance.
        other.0.total_cmp(&self.0)
    }
}

/// Comble les pixels marqués de `mask` à partir des pixels voisins, dans un
/// rayon de `radius` pixels, puis leur ajoute un grain gaussien d'écart type
/// `grain` (tirage reproductible depuis `seed`).
pub fn inpaint(image: &mut RgbImage, mask: &Mask, radius: f32, grain: f32, seed: u64) {
    let bounds = image.bounds();
    let work = mask
        .rect
        .inflate(radius.ceil() as i32 + 2)
        .intersect(&bounds);
    if work.is_empty() || mask.count() == 0 {
        return;
    }
    let (w, h) = (work.width(), work.height());
    let index = |x: i32, y: i32| (y - work.y0) as usize * w + (x - work.x0) as usize;
    let mut state = vec![State::Known; w * h];
    let mut dist = vec![0.0f32; w * h];
    for y in work.y0..work.y1 {
        for x in work.x0..work.x1 {
            if mask.get(x, y) {
                state[index(x, y)] = State::Inside;
                dist[index(x, y)] = f32::INFINITY;
            }
        }
    }
    let inside = |state: &[State], x: i32, y: i32| {
        work.contains(x, y) && state[index(x, y)] == State::Inside
    };
    // Front initial : pixels connus voisins de la zone.
    let mut heap = BinaryHeap::new();
    for y in work.y0..work.y1 {
        for x in work.x0..work.x1 {
            let i = index(x, y);
            if state[i] == State::Known
                && NEIGHBORS
                    .iter()
                    .any(|(dx, dy)| inside(&state, x + dx, y + dy))
            {
                state[i] = State::Band;
                heap.push(Front(0.0, x, y));
            }
        }
    }
    // Pixels de la zone sans voisin connu dans l'image (zone collée au bord) :
    // ils reçoivent quand même une valeur, celle du pixel le plus proche
    // atteint par la marche.
    let r = radius.max(1.0);
    let reach = r.ceil() as i32;
    let mut noise = Noise::new(seed);
    while let Some(Front(_, x, y)) = heap.pop() {
        let i = index(x, y);
        if state[i] == State::Known {
            continue;
        }
        state[i] = State::Known;
        for (dx, dy) in NEIGHBORS {
            let (nx, ny) = (x + dx, y + dy);
            if !inside(&state, nx, ny) {
                continue;
            }
            let n = index(nx, ny);
            let t = solve(&dist, &state, work, nx, ny);
            dist[n] = t;
            state[n] = State::Band;
            let color = estimate(image, &dist, &state, work, nx, ny, reach, r);
            let color = if grain > 0.0 {
                let g = grain * noise.gaussian();
                color.map(|c| c + g)
            } else {
                color
            };
            image.set_f(nx, ny, color);
            heap.push(Front(t, nx, ny));
        }
    }
}

const NEIGHBORS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// Distance au bord du pixel `(x, y)`, solution de l'équation eikonale à
/// partir de ses voisins déjà atteints.
fn solve(dist: &[f32], state: &[State], work: Rect, x: i32, y: i32) -> f32 {
    let value = |x: i32, y: i32| {
        if !work.contains(x, y) {
            return f32::INFINITY;
        }
        let i = (y - work.y0) as usize * work.width() + (x - work.x0) as usize;
        if state[i] == State::Inside {
            f32::INFINITY
        } else {
            dist[i]
        }
    };
    let a = value(x - 1, y).min(value(x + 1, y));
    let b = value(x, y - 1).min(value(x, y + 1));
    if a.is_finite() && b.is_finite() {
        let d = a - b;
        if d.abs() < 1.0 {
            return (a + b + (2.0 - d * d).sqrt()) / 2.0;
        }
    }
    a.min(b) + 1.0
}

/// Couleur estimée du pixel `(x, y)` à partir des pixels connus voisins.
#[allow(clippy::too_many_arguments)]
fn estimate(
    image: &RgbImage,
    dist: &[f32],
    state: &[State],
    work: Rect,
    x: i32,
    y: i32,
    reach: i32,
    radius: f32,
) -> [f32; 3] {
    let at = |x: i32, y: i32| (y - work.y0) as usize * work.width() + (x - work.x0) as usize;
    let here = dist[at(x, y)];
    // Gradient de la distance : direction de progression.
    let grad = |ax: i32, ay: i32, bx: i32, by: i32| {
        let ok = |x: i32, y: i32| work.contains(x, y) && state[at(x, y)] != State::Inside;
        match (ok(ax, ay), ok(bx, by)) {
            (true, true) => (dist[at(bx, by)] - dist[at(ax, ay)]) / 2.0,
            (true, false) => here - dist[at(ax, ay)],
            (false, true) => dist[at(bx, by)] - here,
            (false, false) => 0.0,
        }
    };
    let gx = grad(x - 1, y, x + 1, y);
    let gy = grad(x, y - 1, x, y + 1);
    let mut sum = [0.0f32; 3];
    let mut total = 0.0f32;
    for sy in y - reach..=y + reach {
        for sx in x - reach..=x + reach {
            if !work.contains(sx, sy) || state[at(sx, sy)] != State::Known {
                continue;
            }
            let (rx, ry) = ((x - sx) as f32, (y - sy) as f32);
            let len2 = rx * rx + ry * ry;
            if len2 > radius * radius || len2 == 0.0 {
                continue;
            }
            let len = len2.sqrt();
            let direction = ((rx * gx + ry * gy) / len).abs().max(1e-3);
            let level = 1.0 / (1.0 + (dist[at(sx, sy)] - here).abs());
            let weight = direction * level / len2;
            let c = image.get_f(sx, sy);
            for (s, v) in sum.iter_mut().zip(c) {
                *s += weight * v;
            }
            total += weight;
        }
    }
    if total <= 0.0 {
        // Aucun voisin connu dans le rayon : le plus proche connu.
        let mut best = (f32::INFINITY, [255.0; 3]);
        for sy in y - reach..=y + reach {
            for sx in x - reach..=x + reach {
                if work.contains(sx, sy) && state[at(sx, sy)] == State::Known {
                    let d = ((x - sx).pow(2) + (y - sy).pow(2)) as f32;
                    if d < best.0 {
                        best = (d, image.get_f(sx, sy));
                    }
                }
            }
        }
        return best.1;
    }
    sum.map(|s| s / total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comble_un_trait_sur_un_degrade() {
        let mut image = RgbImage::new(40, 20, [0, 0, 0]);
        for y in 0..20 {
            for x in 0..40 {
                let v = 200.0 + x as f32;
                image.set_f(x, y, [v, v, v]);
            }
        }
        let mut mask = Mask::new(Rect::new(0, 0, 40, 20));
        for y in 4..16 {
            for x in 18..22 {
                image.set(x, y, [0, 0, 0]);
                mask.set(x, y, true);
            }
        }
        inpaint(&mut image, &mask, 4.0, 0.0, 1);
        for y in 4..16 {
            for x in 18..22 {
                let v = image.get_f(x, y)[0];
                assert!((v - (200.0 + x as f32)).abs() < 3.0, "{x},{y}: {v}");
            }
        }
    }

    #[test]
    fn zone_collee_au_bord() {
        let mut image = RgbImage::new(10, 10, [240, 240, 240]);
        let mut mask = Mask::new(Rect::new(0, 0, 10, 10));
        for y in 0..10 {
            for x in 0..4 {
                image.set(x, y, [0, 0, 0]);
                mask.set(x, y, true);
            }
        }
        inpaint(&mut image, &mask, 3.0, 2.0, 3);
        let v = image.get_f(0, 5)[0];
        assert!((v - 240.0).abs() < 10.0, "{v}");
    }
}
