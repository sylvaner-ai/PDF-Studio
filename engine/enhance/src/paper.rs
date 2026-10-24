//! Couleur du papier d'un scan, pour remplir les coins découverts par la
//! rotation.
//!
//! L'éclairage d'un scan est rarement uniforme (bord assombri, lumière en
//! coin) : un coin découvert prend la couleur du papier à cet endroit de la
//! page, estimée par blocs, et non une couleur unique qui se verrait comme
//! une pièce rapportée.

use scan::RgbImage;

/// Couleur du papier : médiane, canal par canal, de la moitié la plus claire
/// des pixels (une page est surtout du papier), prise sur un pixel sur
/// quatre dans chaque sens. Blanc pour une image vide.
pub fn paper_color(image: &RgbImage) -> [u8; 3] {
    let pixels = sample(image, 0, 0, image.width, image.height, 4);
    bright_median(pixels).unwrap_or([255; 3])
}

/// Pixels de la zone `[x0, x1[ × [y0, y1[`, un sur `step` dans chaque sens.
fn sample(
    image: &RgbImage,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    step: usize,
) -> Vec<[u8; 3]> {
    let mut pixels = Vec::new();
    for y in (y0..y1).step_by(step) {
        for x in (x0..x1).step_by(step) {
            pixels.push(image.get(x as i32, y as i32));
        }
    }
    pixels
}

fn luma(c: [u8; 3]) -> u16 {
    (u16::from(c[0]) * 77 + u16::from(c[1]) * 150 + u16::from(c[2]) * 29) >> 8
}

/// Médiane, canal par canal, de la moitié la plus claire de `pixels`.
fn bright_median(mut pixels: Vec<[u8; 3]>) -> Option<[u8; 3]> {
    if pixels.is_empty() {
        return None;
    }
    pixels.sort_unstable_by_key(|&c| luma(c));
    let bright = &pixels[pixels.len() / 2..];
    let mut out = [0u8; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        let mut values: Vec<u8> = bright.iter().map(|c| c[channel]).collect();
        values.sort_unstable();
        *value = values[values.len() / 2];
    }
    Some(out)
}

/// Médiane, canal par canal, des pixels de `pixels` qui ne sont pas de
/// l'encre (au moins 80 % de la clarté des plus clairs). Contrairement à
/// [`bright_median`], un bloc à cheval sur le papier et sur un fond plus
/// clair (couvercle du scanner) prend une couleur proportionnée à la part
/// de chacun.
fn paper_median(mut pixels: Vec<[u8; 3]>) -> Option<[u8; 3]> {
    if pixels.is_empty() {
        return None;
    }
    pixels.sort_unstable_by_key(|&c| luma(c));
    let light = luma(pixels[pixels.len() * 9 / 10]);
    let start = pixels.partition_point(|&c| u32::from(luma(c)) * 5 < u32::from(light) * 4);
    let paper = &pixels[start..];
    let mut out = [0u8; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        let mut values: Vec<u8> = paper.iter().map(|c| c[channel]).collect();
        values.sort_unstable();
        *value = values[values.len() / 2];
    }
    Some(out)
}

/// Couleur du papier en tout point d'une page : couleur de chaque bloc,
/// interpolée entre les centres des blocs.
#[derive(Clone, Debug)]
pub struct Paper {
    width: usize,
    height: usize,
    /// Côté d'un bloc, en pixels.
    block: f64,
    cols: usize,
    rows: usize,
    colors: Vec<[f32; 3]>,
    /// Couleur du papier le long de chaque bord (haut, bas, gauche, droite),
    /// par tronçons de la longueur d'un bloc, prise sur une bande étroite
    /// juste à l'intérieur de la marge : l'éclairage change souvent vite
    /// près du bord (ombre, vignettage), et c'est à ce papier que les coins
    /// découverts se raccordent.
    edges: [Vec<[f32; 3]>; 4],
}

/// Nombre de blocs sur le grand côté de la page.
const BLOCKS: usize = 24;

/// Un bloc plus sombre que cette part du papier de la page est couvert
/// (photo, aplat, bandeau) : il prend la couleur de ses voisins.
const COVERED: f64 = 0.55;

impl Paper {
    /// Même couleur partout.
    pub fn uniform(color: [u8; 3]) -> Self {
        Paper {
            width: 1,
            height: 1,
            block: 1.0,
            cols: 1,
            rows: 1,
            colors: vec![color.map(f32::from)],
            edges: Default::default(),
        }
    }

    /// Couleur du papier de `image`, bloc par bloc, sans les `margin` pixels
    /// du bord de l'image : sur un scan incliné, ils montrent souvent le
    /// couvercle du scanner autour de la page. Les blocs du bord prennent
    /// alors la couleur de leurs voisins de l'intérieur.
    pub fn estimate(image: &RgbImage, margin: usize) -> Self {
        let (w, h) = (image.width, image.height);
        let global = paper_color(image);
        if w == 0 || h == 0 {
            return Paper::uniform(global);
        }
        let block = (w.max(h) / BLOCKS).max(16);
        let (cols, rows) = (w.div_ceil(block), h.div_ceil(block));
        let floor = COVERED * f64::from(luma(global));
        let margin = margin.min(w.min(h) / 4);
        let mut colors: Vec<Option<[f32; 3]>> = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            for col in 0..cols {
                let (x0, y0) = ((col * block).max(margin), (row * block).max(margin));
                let x1 = ((col + 1) * block).min(w - margin);
                let y1 = ((row + 1) * block).min(h - margin);
                // Un bloc presque tout entier dans la marge est inconnu.
                if x1 < x0 + block / 4 || y1 < y0 + block / 4 {
                    colors.push(None);
                    continue;
                }
                let pixels = sample(image, x0, y0, x1, y1, 2);
                colors.push(
                    paper_median(pixels)
                        .filter(|&c| f64::from(luma(c)) >= floor)
                        .map(|c| c.map(f32::from)),
                );
            }
        }
        // Blocs couverts : moyenne des voisins connus, de proche en proche ;
        // couleur de la page s'il n'y en a aucun.
        while colors.iter().any(Option::is_none) {
            let previous = colors.clone();
            let mut changed = false;
            for row in 0..rows {
                for col in 0..cols {
                    if previous[row * cols + col].is_some() {
                        continue;
                    }
                    let known: Vec<[f32; 3]> = neighbours(col, row, cols, rows)
                        .filter_map(|(c, r)| previous[r * cols + c])
                        .collect();
                    if !known.is_empty() {
                        colors[row * cols + col] = Some(mean(&known));
                        changed = true;
                    }
                }
            }
            if !changed {
                let fill = global.map(f32::from);
                for color in &mut colors {
                    color.get_or_insert(fill);
                }
            }
        }
        // Lissage (3 × 3) : le grain du papier et le bruit ne doivent pas
        // former de taches.
        let colors: Vec<[f32; 3]> = colors.into_iter().map(Option::unwrap_or_default).collect();
        let smooth = (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (col, row)))
            .map(|(col, row)| {
                let around: Vec<[f32; 3]> = neighbours(col, row, cols, rows)
                    .chain([(col, row)])
                    .map(|(c, r)| colors[r * cols + c])
                    .collect();
                mean(&around)
            })
            .collect();
        let mut paper = Paper {
            width: w,
            height: h,
            block: block as f64,
            cols,
            rows,
            colors: smooth,
            edges: Default::default(),
        };
        let depth = (block / 4).max(8);
        for side in 0..4 {
            let horizontal = side < 2;
            let count = if horizontal { cols } else { rows };
            let mut profile: Vec<[f32; 3]> = (0..count)
                .map(|segment| {
                    let (a0, a1) = (
                        segment * block,
                        ((segment + 1) * block).min(if horizontal { w } else { h }),
                    );
                    let (b0, b1) = match side % 2 {
                        0 => (margin, margin + depth),
                        _ => {
                            let end = if horizontal { h } else { w } - margin;
                            (end.saturating_sub(depth), end)
                        }
                    };
                    let (x0, x1, y0, y1) = if horizontal {
                        (a0, a1, b0, b1)
                    } else {
                        (b0, b1, a0, a1)
                    };
                    let (x1, y1) = (x1.min(w), y1.min(h));
                    let center = ((a0 + a1) as f64 / 2.0, (b0 + b1) as f64 / 2.0);
                    let (cx, cy) = if horizontal {
                        center
                    } else {
                        (center.1, center.0)
                    };
                    paper_median(sample(image, x0, y0, x1, y1, 2))
                        .filter(|&c| f64::from(luma(c)) >= floor)
                        .map_or_else(|| paper.at(cx, cy), |c| c.map(f32::from))
                })
                .collect();
            // Lissage le long du bord.
            if profile.len() > 2 {
                let raw = profile.clone();
                for (i, color) in profile.iter_mut().enumerate() {
                    let around = &raw[i.saturating_sub(1)..(i + 2).min(raw.len())];
                    *color = mean(around);
                }
            }
            paper.edges[side] = profile;
        }
        paper
    }

    /// Couleur du papier au point `(x, y)` (centres des pixels) ; un point
    /// hors de l'image prend celle du papier au bord le plus proche.
    pub fn at(&self, x: f64, y: f64) -> [f32; 3] {
        if self.colors.len() == 1 {
            return self.colors[0];
        }
        let (right, bottom) = ((self.width - 1) as f64, (self.height - 1) as f64);
        let out_x = (-x).max(x - right).max(0.0);
        let out_y = (-y).max(y - bottom).max(0.0);
        if out_x > 0.0 || out_y > 0.0 {
            let (side, along) = if out_y >= out_x {
                (if y < 0.0 { 0 } else { 1 }, x.clamp(0.0, right))
            } else {
                (if x < 0.0 { 2 } else { 3 }, y.clamp(0.0, bottom))
            };
            return self.along(side, along);
        }
        self.inside(x, y)
    }

    /// Couleur du papier le long du bord `side`, à la position `along`.
    fn along(&self, side: usize, along: f64) -> [f32; 3] {
        let profile = &self.edges[side];
        let g = ((along + 0.5) / self.block - 0.5).clamp(0.0, (profile.len() - 1) as f64);
        let i = g.floor() as usize;
        let (a, b) = (profile[i], profile[(i + 1).min(profile.len() - 1)]);
        let f = (g - i as f64) as f32;
        [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * f)
    }

    /// Couleur du papier au point `(x, y)` de l'image, d'après les blocs.
    fn inside(&self, x: f64, y: f64) -> [f32; 3] {
        let x = x.clamp(0.0, (self.width - 1) as f64);
        let y = y.clamp(0.0, (self.height - 1) as f64);
        // Position parmi les centres des blocs.
        let gx = ((x + 0.5) / self.block - 0.5).clamp(0.0, (self.cols - 1) as f64);
        let gy = ((y + 0.5) / self.block - 0.5).clamp(0.0, (self.rows - 1) as f64);
        let (c0, r0) = (gx.floor() as usize, gy.floor() as usize);
        let (c1, r1) = ((c0 + 1).min(self.cols - 1), (r0 + 1).min(self.rows - 1));
        let (fx, fy) = ((gx - c0 as f64) as f32, (gy - r0 as f64) as f32);
        let at = |c: usize, r: usize| self.colors[r * self.cols + c];
        let (a, b, c, d) = (at(c0, r0), at(c1, r0), at(c0, r1), at(c1, r1));
        let mut out = [0f32; 3];
        for channel in 0..3 {
            let top = a[channel] + (b[channel] - a[channel]) * fx;
            let bottom = c[channel] + (d[channel] - c[channel]) * fx;
            out[channel] = top + (bottom - top) * fy;
        }
        out
    }
}

/// Blocs voisins (huit au plus) du bloc `(col, row)`.
fn neighbours(
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
) -> impl Iterator<Item = (usize, usize)> {
    (-1i64..=1)
        .flat_map(|dr| (-1i64..=1).map(move |dc| (dc, dr)))
        .filter(|&(dc, dr)| dc != 0 || dr != 0)
        .filter_map(move |(dc, dr)| {
            let (c, r) = (col as i64 + dc, row as i64 + dr);
            (c >= 0 && r >= 0 && c < cols as i64 && r < rows as i64)
                .then_some((c as usize, r as usize))
        })
}

fn mean(colors: &[[f32; 3]]) -> [f32; 3] {
    let n = colors.len().max(1) as f32;
    let mut out = [0f32; 3];
    for color in colors {
        for channel in 0..3 {
            out[channel] += color[channel] / n;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn papier_sous_le_texte() {
        // Papier crème, un tiers de la page couvert d'encre.
        let mut image = RgbImage::new(60, 60, [240, 236, 220]);
        for y in 0..20 {
            for x in 0..60 {
                image.set(x, y, [20, 20, 30]);
            }
        }
        assert_eq!(paper_color(&image), [240, 236, 220]);
    }

    #[test]
    fn suit_l_eclairage_de_la_page() {
        // Papier assombri de 30 % vers la droite, avec un aplat noir en haut
        // à droite et quelques lignes de texte.
        let (w, h) = (600, 800);
        let mut image = RgbImage::new(w, h, [0; 3]);
        for y in 0..h {
            for x in 0..w {
                let light = 1.0 - 0.3 * x as f64 / (w - 1) as f64;
                let v = (230.0 * light).round() as u8;
                let ink = y % 40 < 6 && (60..540).contains(&x);
                let dark = y < 120 && x > 400;
                image.set(
                    x as i32,
                    y as i32,
                    if ink || dark { [15; 3] } else { [v; 3] },
                );
            }
        }
        let paper = Paper::estimate(&image, 0);
        for (x, y) in [
            (0.0, 0.0),
            (300.0, 400.0),
            (599.0, 799.0),
            (599.0, 0.0),
            (-50.0, 400.0),
        ] {
            let expected = 230.0 * (1.0 - 0.3 * f64::clamp(x, 0.0, 599.0) / 599.0);
            let got = paper.at(x, y)[0];
            assert!(
                (got - expected as f32).abs() < 10.0,
                "({x}, {y}) : {got} au lieu de {expected}"
            );
        }
        assert_eq!(Paper::uniform([1, 2, 3]).at(5.0, 5.0), [1.0, 2.0, 3.0]);

        // Couvercle clair sur 20 pixels autour de la page : écarté.
        for y in 0..h {
            for x in 0..w {
                if x < 20 || y < 20 || x >= w - 20 || y >= h - 20 {
                    image.set(x as i32, y as i32, [250, 250, 250]);
                }
            }
        }
        let paper = Paper::estimate(&image, 24);
        let got = paper.at(0.0, 400.0)[0];
        assert!((got - 230.0).abs() < 10.0, "{got}");
    }
}
