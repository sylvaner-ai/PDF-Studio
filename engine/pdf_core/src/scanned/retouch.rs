//! Application des modifications d'une page scannée à ses pixels.
//!
//! Deux étapes, sur une copie du rendu du scan :
//!
//! 1. les blocs dont le texte change sont réécrits en place : seuls les
//!    mots modifiés sont effacés et redessinés dans la police retrouvée, les
//!    mots suivants glissent pour leur faire place, et un paragraphe est
//!    remis en page dans sa largeur (cf. [`super::reflow`]) ;
//! 2. les objets supprimés sont effacés, les objets déplacés effacés puis
//!    redéposés (leur encre, détachée de son papier) à leur nouvelle place.
//!
//! Les pixels qui diffèrent du rendu d'origine forment les calques posés sur
//! la page. La couche de texte suit : mots remplacés, déplacés, supprimés.

use std::collections::{BTreeMap, HashMap, HashSet};

use edit::{LayerWord, ObjectEdit, PageEdits, TextLayer};
use scan::ink::Mask;
use scan::retouch::{apply, erase, extract, merge_rects, select, Background, InkPatch, Selecting};
use scan::{Rect as PixelRect, RgbImage};

use super::reflow::rewrite_block;
use super::{ScanBlock, ScanLine, ScanModel};

/// Pixels modifiés d'une zone du scan.
#[derive(Clone, Debug)]
pub(crate) struct Patch {
    /// Zone, en pixels du scan.
    pub rect: PixelRect,
    /// Pixels de la zone (modifiés ou non).
    pub pixels: RgbImage,
    /// Pixels modifiés, à poser ; les autres restent ceux du scan.
    pub changed: Mask,
    /// Tous les pixels modifiés sont gris.
    pub gray: bool,
}

/// Résultat des modifications d'une page scannée.
#[derive(Clone, Debug, Default)]
pub(crate) struct Retouched {
    /// Calques à poser sur la page.
    pub patches: Vec<Patch>,
    /// Couche de texte à jour.
    pub layer: TextLayer,
    /// Emprise de chaque objet modifié, en pixels ; `None` s'il est supprimé.
    pub bounds: HashMap<u32, Option<PixelRect>>,
    /// Blocs dont le texte réécrit occupe plus de lignes qu'à l'origine.
    pub overflow: HashSet<u32>,
}

/// Effet des modifications sur les mots de la couche de texte.
struct LayerChanges {
    words: Vec<Option<LayerWord>>,
    /// Mots ajoutés, à placer avant le mot d'origine d'indice donné, avec le
    /// bloc auquel ils appartiennent.
    inserted: BTreeMap<usize, Vec<(u32, LayerWord)>>,
}

impl LayerChanges {
    fn offset(&mut self, index: usize, offset: (f64, f64)) {
        if let Some(Some(word)) = self.words.get_mut(index) {
            shift(word, offset);
        }
    }

    fn finish(self, engine: &str) -> TextLayer {
        let mut words = Vec::new();
        let mut inserted = self.inserted;
        for (i, word) in self.words.into_iter().enumerate() {
            if let Some(added) = inserted.remove(&i) {
                words.extend(added.into_iter().map(|(_, word)| word));
            }
            words.extend(word);
        }
        for (_, added) in inserted {
            words.extend(added.into_iter().map(|(_, word)| word));
        }
        TextLayer {
            words,
            engine: engine.to_owned(),
        }
    }
}

fn shift(word: &mut LayerWord, (dx, dy): (f64, f64)) {
    for k in 0..4 {
        word.quad[2 * k] += dx as f32;
        word.quad[2 * k + 1] += dy as f32;
    }
}

/// Applique `edits` au scan `model` dont la couche de texte d'origine est
/// `layer`.
pub(crate) fn retouch(model: &ScanModel, layer: &TextLayer, edits: &PageEdits) -> Retouched {
    let placement = model.image.placement;
    let mut image = (*model.base).clone();
    let mut changed: Vec<PixelRect> = Vec::new();
    let mut changes = LayerChanges {
        words: layer.words.iter().cloned().map(Some).collect(),
        inserted: BTreeMap::new(),
    };
    let mut bounds: HashMap<u32, Option<PixelRect>> = HashMap::new();
    let deleted_tables: HashSet<u32> = model
        .tables
        .iter()
        .filter(|t| edits.get(t.id).is_some_and(|e| e.deleted))
        .map(|t| t.id)
        .collect();
    let in_deleted_table =
        |block: &ScanBlock| block.table.is_some_and(|t| deleted_tables.contains(&t));
    let mut next_line = layer.words.iter().map(|w| w.line + 1).max().unwrap_or(0);
    let mut overflow = HashSet::new();

    // 1. Texte réécrit en place.
    let mut zones: HashMap<u32, Vec<PixelRect>> = HashMap::new();
    let mut pending: Vec<(InkPatch, i32, i32)> = Vec::new();
    for block in &model.blocks {
        let Some(ObjectEdit {
            deleted: false,
            scan: Some(text),
            ..
        }) = edits.get(block.id)
        else {
            continue;
        };
        if in_deleted_table(block) {
            continue;
        }
        let Some(result) = rewrite_block(model, layer, block, text, &mut image, &mut next_line)
        else {
            continue;
        };
        changed.extend(result.changed);
        pending.extend(result.pending);
        let zone = union(&result.zones);
        bounds.insert(block.id, Some(zone));
        zones.insert(block.id, result.zones);
        if result.overflow {
            overflow.insert(block.id);
        }
        for index in result.removed {
            changes.words[index] = None;
        }
        for (index, offset) in result.shifted {
            changes.offset(index, offset);
        }
        changes
            .inserted
            .entry(result.insert_at)
            .or_default()
            .extend(result.added.into_iter().map(|word| (block.id, word)));
    }
    // L'encre des mots déplacés se repose une fois tous les blocs réécrits.
    for (patch, dx, dy) in pending.drain(..) {
        changed.push(apply(&mut image, &patch, dx, dy));
    }

    // 2. Suppressions et déplacements.
    let mut process = |zone: PixelRect,
                       settings: Selecting,
                       avoid: &dyn Fn(i32, i32) -> bool,
                       edit: &ObjectEdit,
                       id: u32,
                       image: &mut RgbImage,
                       changed: &mut Vec<PixelRect>| {
        let selection = select(image, zone, &settings, avoid);
        if edit.is_moved() && !edit.deleted {
            let (dx, dy) = placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy));
            pending.push((extract(image, &selection), dx, dy));
        }
        changed.push(erase(image, &selection, u64::from(id)));
    };
    for block in &model.blocks {
        let Some(edit) = edits.get(block.id) else {
            continue;
        };
        if in_deleted_table(block) {
            bounds.insert(block.id, None);
            continue;
        }
        if !edit.deleted && !edit.is_moved() {
            continue;
        }
        let line_zones = zones
            .get(&block.id)
            .cloned()
            .unwrap_or_else(|| block.lines.iter().map(line_zone).collect());
        // L'encre des lignes des autres blocs est évitée, pas celle du bloc.
        let avoid = |x, y| model.owner(x, y).is_some_and(|(id, _)| id != block.id);
        let settings = Selecting {
            margin: (0.1 * block.height()).ceil() as i32,
            background: Background::Closing(radius(block.height())),
            color: None,
            halo: halo(placement.scale),
        };
        for (k, zone) in line_zones.iter().enumerate() {
            process(
                *zone,
                settings,
                &avoid,
                edit,
                block.id.wrapping_add(k as u32 * 7919),
                &mut image,
                &mut changed,
            );
        }
        if edit.deleted {
            bounds.insert(block.id, None);
            for index in block.words() {
                changes.words[index] = None;
            }
            remove_inserted(&mut changes, block.id);
        } else {
            let (dx, dy) = placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy));
            bounds.insert(block.id, Some(union(&line_zones).offset(dx, dy)));
            let offset = placement.page_offset(dx, dy);
            for index in block.words() {
                changes.offset(index, offset);
            }
            shift_inserted(&mut changes, block.id, offset);
        }
    }
    for element in &model.elements {
        let Some(edit) = edits.get(element.id) else {
            continue;
        };
        if !edit.deleted && !edit.is_moved() {
            continue;
        }
        let settings = Selecting {
            margin: 2,
            background: if element.kind == crate::content::objects::ObjectKind::Path {
                Background::Closing(
                    (element.zone.width().min(element.zone.height()) / 2 + 3).min(24),
                )
            } else {
                Background::Ring
            },
            color: None,
            halo: halo(placement.scale) + 1,
        };
        process(
            element.zone,
            settings,
            &|_, _| false,
            edit,
            element.id,
            &mut image,
            &mut changed,
        );
        let (dx, dy) = placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy));
        bounds.insert(
            element.id,
            (!edit.deleted).then(|| element.zone.offset(dx, dy)),
        );
    }
    for table in &model.tables {
        let Some(edit) = edits.get(table.id) else {
            continue;
        };
        if !edit.deleted && !edit.is_moved() {
            continue;
        }
        let settings = Selecting {
            margin: 3,
            background: Background::Ring,
            color: None,
            halo: halo(placement.scale) + 1,
        };
        process(
            table.zone,
            settings,
            &|_, _| false,
            edit,
            table.id,
            &mut image,
            &mut changed,
        );
        let (dx, dy) = placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy));
        bounds.insert(table.id, (!edit.deleted).then(|| table.zone.offset(dx, dy)));
        // Mots du tableau.
        let offset = placement.page_offset(dx, dy);
        let zone = table.zone;
        let inside = |word: &LayerWord| {
            let (x, y) = word_center(word, &placement);
            zone.contains(x as i32, y as i32)
        };
        for word in changes.words.iter_mut() {
            if word.as_ref().is_some_and(inside) {
                if edit.deleted {
                    *word = None;
                } else if let Some(word) = word {
                    shift(word, offset);
                }
            }
        }
        for added in changes.inserted.values_mut() {
            if edit.deleted {
                added.retain(|(_, w)| !inside(w));
            } else {
                for (_, word) in added.iter_mut().filter(|(_, w)| inside(w)) {
                    shift(word, offset);
                }
            }
        }
    }
    for (patch, dx, dy) in &pending {
        changed.push(apply(&mut image, patch, *dx, *dy));
    }

    Retouched {
        patches: patches(&model.base, &image, &changed),
        layer: changes.finish(&layer.engine),
        bounds,
        overflow,
    }
}

/// Zone d'une ligne intacte : emprise de ses mots, avec une marge.
pub(crate) fn line_zone(line: &ScanLine) -> PixelRect {
    line.core.inflate((0.1 * line.height()).ceil() as i32)
}

/// Zone d'un bloc intact.
pub(crate) fn block_zone(block: &ScanBlock) -> PixelRect {
    union(&block.lines.iter().map(line_zone).collect::<Vec<_>>())
}

fn union(zones: &[PixelRect]) -> PixelRect {
    zones
        .iter()
        .fold(PixelRect::EMPTY, |acc, zone| acc.union(zone))
}

pub(super) fn radius(height: f64) -> usize {
    (height * 0.15).round().clamp(2.0, 14.0) as usize
}

pub(super) fn halo(scale: f64) -> i32 {
    if scale > 5.0 {
        2
    } else {
        1
    }
}

fn word_center(word: &LayerWord, placement: &super::Placement) -> (f64, f64) {
    let x = (word.quad[0] + word.quad[2] + word.quad[4] + word.quad[6]) / 4.0;
    let y = (word.quad[1] + word.quad[3] + word.quad[5] + word.quad[7]) / 4.0;
    placement.to_pixels(f64::from(x), f64::from(y))
}

fn remove_inserted(changes: &mut LayerChanges, block: u32) {
    for added in changes.inserted.values_mut() {
        added.retain(|(owner, _)| *owner != block);
    }
}

fn shift_inserted(changes: &mut LayerChanges, block: u32, offset: (f64, f64)) {
    for added in changes.inserted.values_mut() {
        for (_, word) in added.iter_mut().filter(|(owner, _)| *owner == block) {
            shift(word, offset);
        }
    }
}

/// Calques des pixels modifiés : zones regroupées, pixels qui diffèrent du
/// rendu d'origine.
fn patches(base: &RgbImage, image: &RgbImage, changed: &[PixelRect]) -> Vec<Patch> {
    let mut out = Vec::new();
    for rect in merge_rects(changed, 6) {
        let rect = rect.intersect(&image.bounds());
        if rect.is_empty() {
            continue;
        }
        let mut mask = Mask::new(rect);
        let mut gray = true;
        let mut any = false;
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let c = image.get(x, y);
                if c != base.get(x, y) {
                    mask.set(x, y, true);
                    any = true;
                    gray &= c[0] == c[1] && c[1] == c[2];
                }
            }
        }
        if !any {
            continue;
        }
        let rect = mask.extent();
        let changed = {
            let mut tight = Mask::new(rect);
            tight.merge(&mask);
            tight
        };
        out.push(Patch {
            rect,
            pixels: image.crop(rect),
            changed,
            gray,
        });
    }
    out
}
