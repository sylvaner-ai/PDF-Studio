//! Éléments graphiques d'un scan (logos, signatures, tampons, figures) :
//! l'encre qui n'appartient ni aux mots reconnus ni aux filets.

use crate::image::{Rect, RgbImage};
use crate::ink::{components, Mask};
use crate::retouch::merge_rects;

/// Réglages de la recherche des éléments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finding {
    /// Facteur de réduction du masque pendant la recherche (vitesse ; les
    /// parties proches d'un élément se rejoignent).
    pub factor: usize,
    /// Écart en pixels de l'image sous lequel deux parties forment un même
    /// élément.
    pub gap: i32,
    /// Côté minimal d'un élément en pixels de l'image (les poussières plus
    /// petites sont ignorées).
    pub min_size: i32,
}

/// Rectangles des éléments de `dark` (masque des pixels sombres de toute
/// l'image), hors des zones `excluded` (mots, filets).
pub fn find(image: &RgbImage, dark: &Mask, excluded: &[Rect], settings: &Finding) -> Vec<Rect> {
    let f = settings.factor.max(1) as i32;
    let small = Rect::new(
        0,
        0,
        (image.width as i32 + f - 1) / f,
        (image.height as i32 + f - 1) / f,
    );
    let mut hidden = Mask::new(dark.rect);
    for r in excluded {
        let r = r.intersect(&dark.rect);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                hidden.set(x, y, true);
            }
        }
    }
    let mut reduced = Mask::new(small);
    for y in dark.rect.y0..dark.rect.y1 {
        for x in dark.rect.x0..dark.rect.x1 {
            if dark.get(x, y) && !hidden.get(x, y) {
                reduced.set(x / f, y / f, true);
            }
        }
    }
    let parts: Vec<Rect> = components(&reduced_image(&reduced), &reduced)
        .into_iter()
        // Un pixel isolé de la réduction est une poussière.
        .filter(|c| c.pixels > 1)
        .map(|c| {
            Rect::new(c.rect.x0 * f, c.rect.y0 * f, c.rect.x1 * f, c.rect.y1 * f)
                .intersect(&image.bounds())
        })
        .collect();
    merge_rects(&parts, settings.gap)
        .into_iter()
        .filter(|r| r.width() as i32 >= settings.min_size || r.height() as i32 >= settings.min_size)
        .collect()
}

/// Image neutre de la taille du masque (les composantes n'ont besoin que de
/// leur emprise ici).
fn reduced_image(mask: &Mask) -> RgbImage {
    RgbImage::new(mask.rect.width(), mask.rect.height(), [0, 0, 0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rulings::dark_mask;

    #[test]
    fn trouve_un_logo_hors_des_mots() {
        let mut image = RgbImage::new(200, 100, [240, 240, 240]);
        // Logo : deux formes proches.
        for y in 10..30 {
            for x in 10..25 {
                image.set(x, y, [200, 20, 20]);
            }
            for x in 28..40 {
                image.set(x, y, [20, 20, 200]);
            }
        }
        // Mot reconnu.
        for y in 60..70 {
            for x in 100..140 {
                image.set(x, y, [20, 20, 20]);
            }
        }
        // Poussière.
        image.set(180, 90, [0, 0, 0]);
        let dark = dark_mask(&image, 60.0);
        let found = find(
            &image,
            &dark,
            &[Rect::new(98, 58, 142, 72)],
            &Finding {
                factor: 2,
                gap: 6,
                min_size: 6,
            },
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains_rect(&Rect::new(10, 10, 40, 30)));
    }
}
