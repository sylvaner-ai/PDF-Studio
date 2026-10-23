//! Papier et encre : couleur locale du fond, masque des pixels d'encre et
//! composantes connexes.

use crate::image::{luma, Rect, RgbImage};

/// Couleur du papier sous chaque pixel d'un rectangle.
#[derive(Clone, Debug)]
pub struct Paper {
    /// Rectangle couvert, dans l'image.
    pub rect: Rect,
    /// Couleurs, rangée par rangée (réels de 0 à 255).
    pub colors: Vec<[f32; 3]>,
}

impl Paper {
    /// Fond estimé par une fermeture morphologique : chaque canal prend le
    /// maximum puis le minimum de son voisinage de rayon `radius`, ce qui
    /// efface les traits plus fins que `2 × radius + 1` pixels, puis une
    /// moyenne de même rayon lisse le grain.
    pub fn closing(image: &RgbImage, rect: Rect, radius: usize) -> Paper {
        let rect = rect.intersect(&image.bounds());
        let r = radius.max(1);
        // Marge de calcul pour que les bords du rectangle voient un
        // voisinage complet.
        let outer = rect.inflate(2 * r as i32 + 1).intersect(&image.bounds());
        let (w, h) = (outer.width(), outer.height());
        let mut channels: Vec<Vec<f32>> = (0..3)
            .map(|c| {
                let mut v = Vec::with_capacity(w * h);
                for y in outer.y0..outer.y1 {
                    for x in outer.x0..outer.x1 {
                        v.push(image.get_f(x, y)[c]);
                    }
                }
                v
            })
            .collect();
        for channel in &mut channels {
            *channel = filter(channel, w, h, r, f32::max);
            *channel = filter(channel, w, h, r, f32::min);
            *channel = box_blur(channel, w, h, r);
        }
        let mut colors = Vec::with_capacity(rect.area());
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let i = (y - outer.y0) as usize * w + (x - outer.x0) as usize;
                colors.push([channels[0][i], channels[1][i], channels[2][i]]);
            }
        }
        Paper { rect, colors }
    }

    /// Fond uni : médiane des pixels clairs d'un anneau de `width` pixels
    /// autour de `rect`. Convient aux éléments trop grands pour une
    /// fermeture (logos, aplats).
    pub fn ring(image: &RgbImage, rect: Rect, width: i32) -> Paper {
        let color = ring_color(image, rect, width);
        let rect = rect.intersect(&image.bounds());
        Paper {
            rect,
            colors: vec![color; rect.area()],
        }
    }

    /// Couleur du papier au pixel `(x, y)`, qui doit être dans le rectangle.
    pub fn at(&self, x: i32, y: i32) -> [f32; 3] {
        let i = (y - self.rect.y0) as usize * self.rect.width() + (x - self.rect.x0) as usize;
        self.colors[i]
    }

    /// Écart d'encre du pixel : plus forte baisse d'un canal par rapport au
    /// papier (0 sur le papier).
    pub fn darkness(&self, image: &RgbImage, x: i32, y: i32) -> f32 {
        let p = self.at(x, y);
        let c = image.get_f(x, y);
        (p[0] - c[0]).max(p[1] - c[1]).max(p[2] - c[2]).max(0.0)
    }
}

/// Couleur médiane (par canal) de la moitié la plus claire des pixels d'un
/// anneau autour de `rect`.
pub fn ring_color(image: &RgbImage, rect: Rect, width: i32) -> [f32; 3] {
    let bounds = image.bounds();
    let outer = rect.inflate(width).intersect(&bounds);
    let mut pixels: Vec<[f32; 3]> = Vec::new();
    for y in outer.y0..outer.y1 {
        for x in outer.x0..outer.x1 {
            if !rect.contains(x, y) {
                pixels.push(image.get_f(x, y));
            }
        }
    }
    if pixels.is_empty() {
        return [255.0; 3];
    }
    pixels.sort_by(|a, b| luma(*b).total_cmp(&luma(*a)));
    pixels.truncate(pixels.len().div_ceil(2));
    let mut color = [0.0; 3];
    for (c, value) in color.iter_mut().enumerate() {
        let mut v: Vec<f32> = pixels.iter().map(|p| p[c]).collect();
        v.sort_by(f32::total_cmp);
        *value = v[v.len() / 2];
    }
    color
}

/// Écart type du grain du papier dans `rect` : dispersion des pixels autour
/// du fond, pixels d'encre exclus.
pub fn grain(image: &RgbImage, paper: &Paper, rect: Rect) -> f32 {
    let rect = rect.intersect(&paper.rect);
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let p = luma(paper.at(x, y));
            let d = luma(image.get_f(x, y)) - p;
            if d.abs() < 25.0 {
                sum += f64::from(d * d);
                count += 1;
            }
        }
    }
    if count < 16 {
        return 0.0;
    }
    (sum / count as f64).sqrt() as f32
}

/// Masque binaire de pixels, sur un rectangle de l'image.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// Rectangle couvert.
    pub rect: Rect,
    /// Valeurs, rangée par rangée.
    pub data: Vec<bool>,
}

impl Mask {
    /// Masque vide.
    pub fn new(rect: Rect) -> Mask {
        Mask {
            rect,
            data: vec![false; rect.area()],
        }
    }

    /// Valeur au pixel `(x, y)` ; faux hors du rectangle.
    pub fn get(&self, x: i32, y: i32) -> bool {
        self.rect.contains(x, y) && self.data[self.index(x, y)]
    }

    /// Change la valeur au pixel `(x, y)`, ignoré hors du rectangle.
    pub fn set(&mut self, x: i32, y: i32, value: bool) {
        if self.rect.contains(x, y) {
            let i = self.index(x, y);
            self.data[i] = value;
        }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        (y - self.rect.y0) as usize * self.rect.width() + (x - self.rect.x0) as usize
    }

    /// Nombre de pixels marqués.
    pub fn count(&self) -> usize {
        self.data.iter().filter(|v| **v).count()
    }

    /// Plus petit rectangle des pixels marqués.
    pub fn extent(&self) -> Rect {
        let mut extent = Rect::EMPTY;
        for y in self.rect.y0..self.rect.y1 {
            for x in self.rect.x0..self.rect.x1 {
                if self.get(x, y) {
                    extent = extent.union(&Rect::new(x, y, x + 1, y + 1));
                }
            }
        }
        extent
    }

    /// Masque épaissi de `radius` pixels (voisinage carré), agrandi
    /// d'autant.
    pub fn dilated(&self, radius: i32) -> Mask {
        let rect = self.rect.inflate(radius);
        let mut out = Mask::new(rect);
        if radius <= 0 {
            for y in self.rect.y0..self.rect.y1 {
                for x in self.rect.x0..self.rect.x1 {
                    out.set(x, y, self.get(x, y));
                }
            }
            return out;
        }
        // Passe horizontale puis verticale.
        let mut row = Mask::new(rect);
        for y in self.rect.y0..self.rect.y1 {
            for x in rect.x0..rect.x1 {
                let hit = (x - radius..=x + radius).any(|sx| self.get(sx, y));
                row.set(x, y, hit);
            }
        }
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let hit = (y - radius..=y + radius).any(|sy| row.get(x, sy));
                out.set(x, y, hit);
            }
        }
        out
    }

    /// Ajoute les pixels marqués de `other`.
    pub fn merge(&mut self, other: &Mask) {
        for y in other.rect.y0..other.rect.y1 {
            for x in other.rect.x0..other.rect.x1 {
                if other.get(x, y) {
                    self.set(x, y, true);
                }
            }
        }
    }
}

/// Pixels d'encre de `rect` : ceux qu'un canal assombrit d'au moins
/// `threshold` par rapport au papier.
pub fn ink_mask(image: &RgbImage, paper: &Paper, rect: Rect, threshold: f32) -> Mask {
    let rect = rect.intersect(&paper.rect);
    let mut mask = Mask::new(rect);
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            if paper.darkness(image, x, y) >= threshold {
                mask.set(x, y, true);
            }
        }
    }
    mask
}

/// Seuil d'encre adapté au grain du papier.
pub fn ink_threshold(grain: f32) -> f32 {
    (4.0 * grain).clamp(28.0, 60.0)
}

/// Composante connexe (8 voisins) d'un masque.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    /// Rectangle englobant.
    pub rect: Rect,
    /// Nombre de pixels.
    pub pixels: usize,
    /// Couleur moyenne des pixels dans l'image.
    pub color: [f32; 3],
    /// Pixels de la composante.
    pub points: Vec<(i32, i32)>,
}

/// Composantes connexes de `mask`.
pub fn components(image: &RgbImage, mask: &Mask) -> Vec<Component> {
    let rect = mask.rect;
    let mut seen = vec![false; rect.area()];
    let index = |x: i32, y: i32| (y - rect.y0) as usize * rect.width() + (x - rect.x0) as usize;
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            if !mask.get(x, y) || seen[index(x, y)] {
                continue;
            }
            seen[index(x, y)] = true;
            stack.push((x, y));
            let mut points = Vec::new();
            let mut bounds = Rect::EMPTY;
            let mut color = [0.0f64; 3];
            while let Some((px, py)) = stack.pop() {
                points.push((px, py));
                bounds = bounds.union(&Rect::new(px, py, px + 1, py + 1));
                let c = image.get_f(px, py);
                for (sum, value) in color.iter_mut().zip(c) {
                    *sum += f64::from(value);
                }
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (px + dx, py + dy);
                        if mask.get(nx, ny) && !seen[index(nx, ny)] {
                            seen[index(nx, ny)] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            let n = points.len() as f64;
            out.push(Component {
                rect: bounds,
                pixels: points.len(),
                color: color.map(|c| (c / n) as f32),
                points,
            });
        }
    }
    out
}

/// Filtre glissant séparable (maximum ou minimum) de rayon `r`.
fn filter(values: &[f32], w: usize, h: usize, r: usize, op: fn(f32, f32) -> f32) -> Vec<f32> {
    let mut tmp = vec![0.0; values.len()];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let row = &values[y * w..];
            tmp[y * w + x] = row[a..=b].iter().copied().fold(row[a], op);
        }
    }
    let mut out = vec![0.0; values.len()];
    for y in 0..h {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            out[y * w + x] = (a..=b).map(|sy| tmp[sy * w + x]).fold(tmp[a * w + x], op);
        }
    }
    out
}

/// Moyenne glissante séparable de rayon `r`, par sommes cumulées.
fn box_blur(values: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0; values.len()];
    for y in 0..h {
        let mut prefix = vec![0.0f64; w + 1];
        for x in 0..w {
            prefix[x + 1] = prefix[x] + f64::from(values[y * w + x]);
        }
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r + 1).min(w));
            tmp[y * w + x] = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
        }
    }
    let mut out = vec![0.0; values.len()];
    for x in 0..w {
        let mut prefix = vec![0.0f64; h + 1];
        for y in 0..h {
            prefix[y + 1] = prefix[y] + f64::from(tmp[y * w + x]);
        }
        for y in 0..h {
            let (a, b) = (y.saturating_sub(r), (y + r + 1).min(h));
            out[y * w + x] = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Papier beige avec un dégradé et deux traits sombres.
    fn page() -> RgbImage {
        let mut image = RgbImage::new(60, 30, [0, 0, 0]);
        for y in 0..30 {
            for x in 0..60 {
                let v = 230.0 + x as f32 * 0.2;
                image.set_f(x, y, [v, v - 5.0, v - 20.0]);
            }
        }
        for y in 8..20 {
            image.set(10, y, [20, 20, 20]);
            image.set(11, y, [20, 20, 20]);
        }
        for x in 30..50 {
            image.set(x, 15, [30, 30, 200]);
        }
        image
    }

    #[test]
    fn fond_sans_les_traits() {
        let image = page();
        let paper = Paper::closing(&image, image.bounds(), 3);
        let p = paper.at(10, 12);
        assert!((p[0] - 232.0).abs() < 3.0, "{p:?}");
        let mask = ink_mask(&image, &paper, image.bounds(), 40.0);
        assert_eq!(mask.count(), 24 + 20);
        let parts = components(&image, &mask);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().any(|c| c.rect == Rect::new(10, 8, 12, 20)));
        let blue = parts.iter().find(|c| c.pixels == 20).unwrap();
        assert!(blue.color[2] > 150.0);
        assert!(grain(&image, &paper, image.bounds()) < 1.0);
    }

    #[test]
    fn anneau_et_dilatation() {
        let image = page();
        let color = ring_color(&image, Rect::new(8, 6, 14, 22), 3);
        assert!(color[0] > 225.0);
        let mut mask = Mask::new(Rect::new(0, 0, 5, 5));
        mask.set(2, 2, true);
        let big = mask.dilated(1);
        assert_eq!(big.count(), 9);
        assert_eq!(big.extent(), Rect::new(1, 1, 4, 4));
    }
}
