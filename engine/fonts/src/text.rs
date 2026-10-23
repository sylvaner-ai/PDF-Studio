//! Mise en forme d'un texte dans une police (crénage, interlettrage) et rendu
//! de ses glyphes en couverture anticrénelée, à n'importe quelle échelle et
//! dans n'importe quelle direction.

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};
use rustybuzz::{Face, Feature, UnicodeBuffer};

/// Glyphe placé, en unités de la police, ordonnées vers le haut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    /// Identifiant du glyphe.
    pub id: u16,
    /// Origine le long de la ligne de base.
    pub x: f32,
    /// Élévation au-dessus de la ligne de base.
    pub y: f32,
}

/// Texte mis en forme, en unités de la police.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shaped {
    /// Glyphes, dans l'ordre.
    pub glyphs: Vec<PlacedGlyph>,
    /// Avance totale, de l'origine du premier glyphe à la fin du dernier
    /// (sans l'interlettrage qui le suivrait).
    pub advance: f32,
    /// Unités par cadratin.
    pub units_per_em: f32,
    /// Caractères absents de la police (hors espaces).
    pub missing: Vec<char>,
    /// Emprise de l'encre le long de la ligne : début et fin.
    pub ink_x: (f32, f32),
    /// Emprise de l'encre en hauteur : bas et haut (ordonnées vers le haut).
    pub ink_y: (f32, f32),
}

impl Shaped {
    /// Vrai si aucun glyphe ne dessine quelque chose.
    pub fn is_blank(&self) -> bool {
        self.ink_x.0 > self.ink_x.1
    }
}

/// Met en forme `text` dans `face`, ligatures désactivées (comme le font les
/// traitements de texte), crénage selon `kerning`, `spacing` unités ajoutées
/// après chaque caractère sauf le dernier.
pub fn shape(face: &Face, text: &str, spacing: f32, kerning: bool) -> Shaped {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let mut features = vec![
        feature(b"liga", false),
        feature(b"clig", false),
        feature(b"dlig", false),
    ];
    if !kerning {
        features.push(feature(b"kern", false));
    }
    let output = rustybuzz::shape(face, &features, buffer);
    let infos = output.glyph_infos();
    let positions = output.glyph_positions();
    let mut shaped = Shaped {
        glyphs: Vec::with_capacity(infos.len()),
        advance: 0.0,
        units_per_em: face.units_per_em() as f32,
        missing: text
            .chars()
            .filter(|c| !c.is_whitespace() && face.glyph_index(*c).is_none())
            .collect(),
        ink_x: (f32::INFINITY, f32::NEG_INFINITY),
        ink_y: (f32::INFINITY, f32::NEG_INFINITY),
    };
    let mut pen = 0.0f32;
    let mut end = 0.0f32;
    for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
        let id = info.glyph_id as u16;
        let x = pen + position.x_offset as f32;
        let y = position.y_offset as f32;
        shaped.glyphs.push(PlacedGlyph { id, x, y });
        if let Some(bbox) = face.glyph_bounding_box(GlyphId(id)) {
            shaped.ink_x.0 = shaped.ink_x.0.min(x + f32::from(bbox.x_min));
            shaped.ink_x.1 = shaped.ink_x.1.max(x + f32::from(bbox.x_max));
            shaped.ink_y.0 = shaped.ink_y.0.min(y + f32::from(bbox.y_min));
            shaped.ink_y.1 = shaped.ink_y.1.max(y + f32::from(bbox.y_max));
        }
        pen += position.x_advance as f32;
        end = pen;
        // L'interlettrage suit chaque caractère (et non chaque glyphe d'une
        // grappe), sauf le dernier.
        let next_cluster = infos.get(index + 1).map(|next| next.cluster);
        if next_cluster.is_some_and(|next| next != info.cluster) {
            pen += spacing;
        }
    }
    shaped.advance = end;
    shaped
}

fn feature(tag: &[u8; 4], on: bool) -> Feature {
    Feature::new(
        rustybuzz::ttf_parser::Tag::from_bytes(tag),
        u32::from(on),
        ..,
    )
}

/// Placement d'un texte mis en forme dans une image (ordonnées vers le bas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    /// Origine de la ligne de base, en pixels de l'image.
    pub origin: (f64, f64),
    /// Pixels par unité de la police.
    pub scale: f64,
    /// Direction de la ligne de base dans l'image (vecteur unitaire ;
    /// `(1, 0)` pour un texte horizontal).
    pub direction: (f64, f64),
    /// Inclinaison synthétique : décalage le long de la ligne par unité de
    /// hauteur (0 : droit).
    pub shear: f64,
}

impl Pen {
    /// Texte horizontal, à l'origine `origin`, à `scale` pixels par unité.
    pub fn horizontal(origin: (f64, f64), scale: f64) -> Self {
        Self {
            origin,
            scale,
            direction: (1.0, 0.0),
            shear: 0.0,
        }
    }

    /// Point de l'image d'un point en unités de la police (ordonnées vers le
    /// haut, depuis l'origine de la ligne de base).
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let along = (x + self.shear * y) * self.scale;
        let up = y * self.scale;
        let (dx, dy) = self.direction;
        // Le haut du texte est à gauche de sa direction (ordonnées vers le bas).
        (
            self.origin.0 + along * dx + up * dy,
            self.origin.1 + along * dy - up * dx,
        )
    }
}

/// Couverture de l'encre (0 à 1), rangée par rangée, placée dans une image.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    /// Abscisse du premier pixel dans l'image.
    pub left: i32,
    /// Ordonnée de la première rangée dans l'image.
    pub top: i32,
    /// Largeur.
    pub width: usize,
    /// Hauteur.
    pub height: usize,
    /// Couverture de chaque pixel.
    pub data: Vec<f32>,
}

impl Coverage {
    /// Couverture vide de taille donnée.
    pub fn new(left: i32, top: i32, width: usize, height: usize) -> Self {
        Self {
            left,
            top,
            width,
            height,
            data: vec![0.0; width * height],
        }
    }

    /// Couverture du pixel `(x, y)` de l'image (0 en dehors).
    pub fn at(&self, x: i32, y: i32) -> f32 {
        let (cx, cy) = (x - self.left, y - self.top);
        if cx < 0 || cy < 0 || cx as usize >= self.width || cy as usize >= self.height {
            return 0.0;
        }
        self.data[cy as usize * self.width + cx as usize]
    }

    /// Somme de la couverture.
    pub fn total(&self) -> f32 {
        self.data.iter().sum()
    }

    /// Traits épaissis (`weight` > 0) ou amincis (< 0) d'au plus un pixel :
    /// mélange avec la couverture dilatée ou érodée en croix.
    pub fn emboldened(&self, weight: f32) -> Coverage {
        let weight = weight.clamp(-1.0, 1.0);
        if weight.abs() < 1e-3 || self.data.is_empty() {
            return self.clone();
        }
        // Une marge d'un pixel reçoit l'encre étendue.
        let grown = self.padded(1);
        let (w, h) = (grown.width, grown.height);
        let mut out = grown.clone();
        for y in 0..h {
            for x in 0..w {
                let center = grown.data[y * w + x];
                let mut extreme = center;
                for (nx, ny) in [
                    (x.wrapping_sub(1), y),
                    (x + 1, y),
                    (x, y.wrapping_sub(1)),
                    (x, y + 1),
                ] {
                    let value = if nx < w && ny < h {
                        grown.data[ny * w + nx]
                    } else {
                        0.0
                    };
                    extreme = if weight > 0.0 {
                        extreme.max(value)
                    } else {
                        extreme.min(value)
                    };
                }
                out.data[y * w + x] = center + weight.abs() * (extreme - center);
            }
        }
        out
    }

    /// Couverture adoucie par un flou gaussien d'écart type `sigma` pixels.
    pub fn blurred(&self, sigma: f32) -> Coverage {
        if sigma < 0.05 || self.data.is_empty() {
            return self.clone();
        }
        let radius = (3.0 * sigma).ceil() as usize;
        let kernel: Vec<f32> = {
            let raw: Vec<f32> = (0..=2 * radius)
                .map(|i| {
                    let d = i as f32 - radius as f32;
                    (-d * d / (2.0 * sigma * sigma)).exp()
                })
                .collect();
            let sum: f32 = raw.iter().sum();
            raw.into_iter().map(|v| v / sum).collect()
        };
        let source = self.padded(radius);
        let (w, h) = (source.width, source.height);
        let mut horizontal = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let sx = x as isize + k as isize - radius as isize;
                    if sx >= 0 && (sx as usize) < w {
                        acc += weight * source.data[y * w + sx as usize];
                    }
                }
                horizontal[y * w + x] = acc;
            }
        }
        let mut out = Coverage::new(source.left, source.top, w, h);
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let sy = y as isize + k as isize - radius as isize;
                    if sy >= 0 && (sy as usize) < h {
                        acc += weight * horizontal[sy as usize * w + x];
                    }
                }
                out.data[y * w + x] = acc;
            }
        }
        out
    }

    /// Même couverture, entourée d'une marge vide de `margin` pixels.
    pub fn padded(&self, margin: usize) -> Coverage {
        let (w, h) = (self.width + 2 * margin, self.height + 2 * margin);
        let mut out = Coverage::new(self.left - margin as i32, self.top - margin as i32, w, h);
        for y in 0..self.height {
            let row = &self.data[y * self.width..(y + 1) * self.width];
            out.data[(y + margin) * w + margin..(y + margin) * w + margin + self.width]
                .copy_from_slice(row);
        }
        out
    }
}

/// Rend un texte mis en forme : couverture des pixels de l'image touchés par
/// ses glyphes, avec une marge d'un pixel.
pub fn rasterize(face: &Face, shaped: &Shaped, pen: &Pen) -> Coverage {
    // Emprise : coins des boîtes des glyphes, transformés.
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut boxes = Vec::with_capacity(shaped.glyphs.len());
    for glyph in &shaped.glyphs {
        let Some(bbox) = face.glyph_bounding_box(GlyphId(glyph.id)) else {
            continue;
        };
        boxes.push(glyph);
        for (u, v) in [
            (bbox.x_min, bbox.y_min),
            (bbox.x_max, bbox.y_min),
            (bbox.x_max, bbox.y_max),
            (bbox.x_min, bbox.y_max),
        ] {
            let (x, y) = pen.map(
                f64::from(glyph.x) + f64::from(u),
                f64::from(glyph.y) + f64::from(v),
            );
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if boxes.is_empty() || !(x0.is_finite() && y1.is_finite()) {
        return Coverage::default();
    }
    let left = x0.floor() as i32 - 1;
    let top = y0.floor() as i32 - 1;
    let width = (x1.ceil() as i32 + 1 - left).max(1) as usize;
    let height = (y1.ceil() as i32 + 1 - top).max(1) as usize;
    let mut rasterizer = Rasterizer::new(width, height);
    for glyph in boxes {
        let mut builder = Builder {
            rasterizer: &mut rasterizer,
            pen,
            glyph: (f64::from(glyph.x), f64::from(glyph.y)),
            offset: (f64::from(left), f64::from(top)),
            start: point(0.0, 0.0),
            last: point(0.0, 0.0),
        };
        face.outline_glyph(GlyphId(glyph.id), &mut builder);
        builder.close();
    }
    let mut coverage = Coverage::new(left, top, width, height);
    rasterizer.for_each_pixel_2d(|x, y, alpha| {
        coverage.data[y as usize * width + x as usize] = alpha.clamp(0.0, 1.0);
    });
    coverage
}

/// Trace les contours d'un glyphe dans le rasteriseur.
struct Builder<'a> {
    rasterizer: &'a mut Rasterizer,
    pen: &'a Pen,
    glyph: (f64, f64),
    offset: (f64, f64),
    start: Point,
    last: Point,
}

impl Builder<'_> {
    fn point(&self, x: f32, y: f32) -> Point {
        let (px, py) = self
            .pen
            .map(self.glyph.0 + f64::from(x), self.glyph.1 + f64::from(y));
        point((px - self.offset.0) as f32, (py - self.offset.1) as f32)
    }
}

impl OutlineBuilder for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = self.point(x, y);
        self.last = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.rasterizer.draw_line(self.last, to);
        self.last = to;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (control, to) = (self.point(x1, y1), self.point(x, y));
        self.rasterizer.draw_quad(self.last, control, to);
        self.last = to;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, to) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        self.rasterizer.draw_cubic(self.last, c1, c2, to);
        self.last = to;
    }

    fn close(&mut self) {
        if self.last != self.start {
            self.rasterizer.draw_line(self.last, self.start);
            self.last = self.start;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::bundled;

    #[test]
    fn met_en_forme_avec_interlettrage() {
        let catalog = bundled();
        let file = catalog
            .load(catalog.find("Carlito-Regular").unwrap())
            .unwrap();
        let face = file.face();
        let plain = shape(&face, "Facture", 0.0, true);
        let spaced = shape(&face, "Facture", 50.0, true);
        assert_eq!(plain.glyphs.len(), 7);
        assert!((spaced.advance - plain.advance - 6.0 * 50.0).abs() < 0.5);
        assert!(plain.missing.is_empty());
        assert_eq!(
            shape(&face, "a\u{4E2D}", 0.0, true).missing,
            vec!['\u{4E2D}']
        );
        // « fi » reste en deux glyphes, ligatures désactivées.
        assert_eq!(shape(&face, "fi", 0.0, true).glyphs.len(), 2);
        assert!(plain.ink_y.1 > 0.0 && plain.ink_y.0 >= -0.05 * plain.units_per_em);
    }

    #[test]
    fn rend_un_texte_horizontal_et_incline() {
        let catalog = bundled();
        let file = catalog
            .load(catalog.find("Caladea-Regular").unwrap())
            .unwrap();
        let face = file.face();
        let shaped = shape(&face, "Hello", 0.0, true);
        let scale = 40.0 / f64::from(shaped.units_per_em);
        let flat = rasterize(&face, &shaped, &Pen::horizontal((10.0, 50.0), scale));
        // Le texte est au-dessus de la ligne de base.
        assert!(flat.top < 50 && flat.top + flat.height as i32 <= 52);
        assert!(flat.total() > 100.0);
        let angle = 10f64.to_radians();
        let tilted = rasterize(
            &face,
            &shaped,
            &Pen {
                origin: (10.0, 50.0),
                scale,
                direction: (angle.cos(), -angle.sin()),
                shear: 0.0,
            },
        );
        // Même quantité d'encre, emprise plus haute.
        assert!((tilted.total() - flat.total()).abs() / flat.total() < 0.03);
        assert!(tilted.height > flat.height);
    }
}
