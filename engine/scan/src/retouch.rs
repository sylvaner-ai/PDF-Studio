//! Opérations de retouche d'un scan : sélection de l'encre d'une zone,
//! effacement, transfert de l'encre ailleurs et dessin d'un nouveau texte.
//!
//! Seuls les pixels d'encre sont touchés : le papier autour garde ses
//! pixels d'origine, ce qui rend les retouches invisibles hors des traits.

use fonts::Coverage;

use crate::image::{Noise, Rect, RgbImage};
use crate::ink::{components, grain, ink_mask, ink_threshold, Mask, Paper};
use crate::inpaint::inpaint;

/// Manière d'estimer le papier sous une zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Background {
    /// Fermeture morphologique du rayon donné (pixels) : texte et traits
    /// fins sur un papier qui varie.
    Closing(usize),
    /// Couleur unie de l'anneau qui entoure la zone : éléments larges.
    Ring,
}

/// Réglages d'une sélection d'encre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selecting {
    /// Marge autour de la zone (pixels) : une composante d'encre est prise
    /// si elle tient entièrement dans la zone agrandie de cette marge.
    pub margin: i32,
    /// Estimation du papier.
    pub background: Background,
    /// Couleur de l'encre visée : les composantes d'une autre couleur
    /// (tampon, surlignage) sont laissées. `None` : toutes.
    pub color: Option<[f32; 3]>,
    /// Halo pris autour des traits (pixels) : bords adoucis et artefacts de
    /// compression.
    pub halo: i32,
}

/// Écart de couleur au-delà duquel une composante n'est pas de l'encre
/// visée.
const COLOR_TOLERANCE: f32 = 90.0;

/// Encre sélectionnée dans une zone.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Pixels d'encre pris, halo compris.
    pub mask: Mask,
    /// Papier estimé autour.
    pub paper: Paper,
    /// Grain du papier (écart type).
    pub grain: f32,
    /// Papier uni (estimé sur l'anneau autour de la zone).
    pub uniform: bool,
}

impl Selection {
    /// Vrai si aucune encre n'a été prise.
    pub fn is_empty(&self) -> bool {
        self.mask.count() == 0
    }

    /// Emprise des pixels pris.
    pub fn extent(&self) -> Rect {
        self.mask.extent()
    }
}

/// Sélectionne l'encre de `zone`. Les composantes dont le centre tombe dans
/// une zone de `avoid` (lignes voisines) sont laissées.
pub fn select(image: &RgbImage, zone: Rect, settings: &Selecting, avoid: &[Rect]) -> Selection {
    let bounds = image.bounds();
    let allowed = zone.inflate(settings.margin);
    let search = allowed.inflate(1 + settings.halo).intersect(&bounds);
    let paper = match settings.background {
        Background::Closing(radius) => Paper::closing(image, search, radius),
        Background::Ring => Paper::ring(image, search, 4),
    };
    let noise = grain(image, &paper, search);
    let raw = ink_mask(
        image,
        &paper,
        search.inflate(-settings.halo),
        ink_threshold(noise),
    );
    let mut mask = Mask::new(raw.rect);
    for part in components(image, &raw) {
        if !allowed.contains_rect(&part.rect) {
            continue;
        }
        let center = (
            (part.rect.x0 + part.rect.x1) / 2,
            (part.rect.y0 + part.rect.y1) / 2,
        );
        if avoid.iter().any(|r| r.contains(center.0, center.1)) {
            continue;
        }
        if let Some(color) = settings.color {
            let d = (0..3)
                .map(|c| (part.color[c] - color[c]).abs())
                .fold(0.0, f32::max);
            if d > COLOR_TOLERANCE && part.pixels > 4 {
                continue;
            }
        }
        for (x, y) in part.points {
            mask.set(x, y, true);
        }
    }
    let mut mask = mask.dilated(settings.halo);
    // Le halo ne prend que des pixels un peu plus sombres que le papier.
    let faint = (2.0 * noise).max(6.0);
    for y in mask.rect.y0..mask.rect.y1 {
        for x in mask.rect.x0..mask.rect.x1 {
            if mask.get(x, y)
                && !raw.get(x, y)
                && paper.rect.contains(x, y)
                && paper.darkness(image, x, y) < faint
            {
                mask.set(x, y, false);
            }
        }
    }
    let mask = clip(&mask, &paper.rect);
    Selection {
        mask,
        paper,
        grain: noise,
        uniform: settings.background == Background::Ring,
    }
}

/// Masque restreint à `rect`.
fn clip(mask: &Mask, rect: &Rect) -> Mask {
    let r = mask.rect.intersect(rect);
    let mut out = Mask::new(r);
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            out.set(x, y, mask.get(x, y));
        }
    }
    out
}

/// Efface l'encre sélectionnée : comblement depuis le papier voisin et
/// grain assorti. Renvoie le rectangle modifié.
pub fn erase(image: &mut RgbImage, selection: &Selection, seed: u64) -> Rect {
    if selection.is_empty() {
        return Rect::EMPTY;
    }
    let far = if selection.uniform && !selection.paper.colors.is_empty() {
        Some(selection.paper.colors[0])
    } else {
        None
    };
    inpaint(image, &selection.mask, 5.0, selection.grain, seed, far);
    selection.extent()
}

/// Encre détachée de son papier : pour chaque pixel, rapport de sa couleur à
/// celle du papier (1 : papier). Transférée ailleurs, elle assombrit le
/// papier d'arrivée comme elle assombrissait celui de départ.
#[derive(Clone, Debug)]
pub struct InkPatch {
    /// Rectangle couvert.
    pub rect: Rect,
    /// Rapports par canal, rangée par rangée.
    pub ratio: Vec<[f32; 3]>,
}

/// Encre de la sélection, avant son effacement.
pub fn extract(image: &RgbImage, selection: &Selection) -> InkPatch {
    let rect = selection.extent();
    let mut ratio = vec![[1.0f32; 3]; rect.area()];
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            if !selection.mask.get(x, y) {
                continue;
            }
            let p = selection.paper.at(x, y);
            let c = image.get_f(x, y);
            let i = (y - rect.y0) as usize * rect.width() + (x - rect.x0) as usize;
            ratio[i] = [0, 1, 2].map(|k| (c[k] / p[k].max(1.0)).clamp(0.0, 1.0));
        }
    }
    InkPatch { rect, ratio }
}

/// Dépose l'encre décalée de `(dx, dy)` pixels. Renvoie le rectangle
/// modifié.
pub fn apply(image: &mut RgbImage, patch: &InkPatch, dx: i32, dy: i32) -> Rect {
    let target = patch.rect.offset(dx, dy).intersect(&image.bounds());
    for y in target.y0..target.y1 {
        for x in target.x0..target.x1 {
            let (sx, sy) = (x - dx, y - dy);
            let i =
                (sy - patch.rect.y0) as usize * patch.rect.width() + (sx - patch.rect.x0) as usize;
            let r = patch.ratio[i];
            if r == [1.0; 3] {
                continue;
            }
            let c = image.get_f(x, y);
            image.set_f(x, y, [c[0] * r[0], c[1] * r[1], c[2] * r[2]]);
        }
    }
    target
}

/// Dessine une couverture d'encre de couleur `ink`, avec un grain d'encre
/// d'écart type `noise` : chaque pixel prend `fond × (1 − a) + encre × a`.
/// Renvoie le rectangle modifié.
pub fn draw(
    image: &mut RgbImage,
    coverage: &Coverage,
    ink: [f32; 3],
    noise: f32,
    seed: u64,
) -> Rect {
    let rect = Rect::new(
        coverage.left,
        coverage.top,
        coverage.left + coverage.width as i32,
        coverage.top + coverage.height as i32,
    )
    .intersect(&image.bounds());
    let mut random = Noise::new(seed);
    let mut changed = Rect::EMPTY;
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let a = coverage.at(x, y).clamp(0.0, 1.0);
            if a < 0.004 {
                continue;
            }
            let g = if noise > 0.0 {
                noise * random.gaussian()
            } else {
                0.0
            };
            let c = image.get_f(x, y);
            image.set_f(x, y, [0, 1, 2].map(|k| c[k] * (1.0 - a) + (ink[k] + g) * a));
            changed = changed.union(&Rect::new(x, y, x + 1, y + 1));
        }
    }
    changed
}

/// Regroupe des rectangles qui se touchent ou sont séparés de moins de
/// `gap` pixels.
pub fn merge_rects(rects: &[Rect], gap: i32) -> Vec<Rect> {
    let mut out: Vec<Rect> = rects.iter().filter(|r| !r.is_empty()).copied().collect();
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < out.len() {
            let mut j = i + 1;
            while j < out.len() {
                if !out[i].inflate(gap).intersect(&out[j]).is_empty() {
                    out[i] = out[i].union(&out[j]);
                    out.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            return out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Papier gris clair bruité, un mot (bloc sombre) et un filet qui
    /// traverse la zone.
    fn page() -> RgbImage {
        let mut image = RgbImage::new(80, 40, [0, 0, 0]);
        let mut noise = Noise::new(5);
        for y in 0..40 {
            for x in 0..80 {
                let v = 235.0 + 3.0 * noise.gaussian();
                image.set_f(x, y, [v, v, v - 10.0]);
            }
        }
        for y in 10..20 {
            for x in 20..24 {
                image.set(x, y, [30, 30, 30]);
            }
            for x in 30..32 {
                image.set(x, y, [30, 30, 30]);
            }
        }
        for x in 0..80 {
            image.set(x, 25, [40, 40, 40]);
        }
        image
    }

    fn settings() -> Selecting {
        Selecting {
            margin: 2,
            background: Background::Closing(4),
            color: Some([30.0, 30.0, 30.0]),
            halo: 1,
        }
    }

    #[test]
    fn efface_le_mot_et_garde_le_filet() {
        let mut image = page();
        let selection = select(&image, Rect::new(18, 8, 34, 27), &settings(), &[]);
        assert!(!selection.is_empty());
        assert!(!selection.mask.get(50, 25));
        let changed = erase(&mut image, &selection, 1);
        assert!(changed.contains_rect(&Rect::new(20, 10, 32, 20)));
        for y in 10..20 {
            for x in 20..24 {
                assert!(image.luma(x, y) > 215.0, "{x},{y}");
            }
        }
        assert!(image.luma(26, 25) < 60.0);
    }

    #[test]
    fn deplace_l_encre() {
        let mut image = page();
        let selection = select(&image, Rect::new(18, 8, 34, 22), &settings(), &[]);
        let patch = extract(&image, &selection);
        erase(&mut image, &selection, 2);
        apply(&mut image, &patch, 40, 2);
        assert!(image.luma(61, 15) < 60.0);
        assert!(image.luma(21, 15) > 215.0);
    }

    #[test]
    fn dessine_et_regroupe() {
        let mut image = RgbImage::new(20, 20, [240, 240, 240]);
        let mut coverage = Coverage::new(5, 5, 3, 3);
        coverage.data[4] = 1.0;
        let changed = draw(&mut image, &coverage, [10.0, 20.0, 30.0], 0.0, 1);
        assert_eq!(changed, Rect::new(6, 6, 7, 7));
        assert_eq!(image.get(6, 6), [10, 20, 30]);
        let merged = merge_rects(
            &[
                Rect::new(0, 0, 5, 5),
                Rect::new(6, 0, 9, 3),
                Rect::new(30, 30, 31, 31),
            ],
            2,
        );
        assert_eq!(merged.len(), 2);
        assert!(merged.contains(&Rect::new(0, 0, 9, 5)));
    }
}
