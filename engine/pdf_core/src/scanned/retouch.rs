//! Application des modifications d'une page scannée à ses pixels.
//!
//! Deux étapes, sur une copie du rendu du scan :
//!
//! 1. les lignes dont le texte change sont réécrites en place : seuls les
//!    mots modifiés sont effacés et redessinés dans la police retrouvée, les
//!    mots suivants glissent pour leur faire place ;
//! 2. les objets supprimés sont effacés, les objets déplacés effacés puis
//!    redéposés (leur encre, détachée de son papier) à leur nouvelle place.
//!
//! Les pixels qui diffèrent du rendu d'origine forment les calques posés sur
//! la page. La couche de texte suit : mots remplacés, déplacés, supprimés.

use std::collections::{BTreeMap, HashMap, HashSet};

use edit::{LayerWord, ObjectEdit, PageEdits, ScanText, TextLayer};
use fonts::{rasterize, shape, Pen};
use scan::ink::Mask;
use scan::retouch::{
    apply, draw, erase, extract, merge_rects, select, Background, InkPatch, Selecting,
};
use scan::{Rect as PixelRect, RgbImage};

use super::{ScanLine, ScanModel};

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
}

/// Effet des modifications sur les mots de la couche de texte.
struct LayerChanges {
    words: Vec<Option<LayerWord>>,
    /// Mots ajoutés, à placer avant le mot d'origine d'indice donné.
    inserted: BTreeMap<usize, Vec<LayerWord>>,
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
                words.extend(added);
            }
            words.extend(word);
        }
        for (_, added) in inserted {
            words.extend(added);
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
        |line: &ScanLine| line.table.is_some_and(|t| deleted_tables.contains(&t));

    // 1. Texte réécrit en place.
    let mut zones: HashMap<u32, PixelRect> = HashMap::new();
    for line in &model.lines {
        let Some(ObjectEdit {
            deleted: false,
            scan: Some(text),
            ..
        }) = edits.get(line.id)
        else {
            continue;
        };
        if in_deleted_table(line) {
            continue;
        }
        let avoid = avoid_for(model, line.id);
        if let Some(result) = rewrite_line(model, layer, line, text, &mut image, &avoid) {
            changed.extend(result.changed);
            zones.insert(line.id, result.zone);
            bounds.insert(line.id, Some(result.zone));
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
                .extend(result.added);
        }
    }

    // 2. Suppressions et déplacements.
    let mut pending: Vec<(InkPatch, i32, i32)> = Vec::new();
    let mut process = |zone: PixelRect,
                       settings: Selecting,
                       avoid: &[PixelRect],
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
    let line_settings = |line: &ScanLine| Selecting {
        margin: (0.1 * line.height()).ceil() as i32,
        background: Background::Closing(radius(line.height())),
        color: None,
        halo: halo(placement.scale),
    };
    for line in &model.lines {
        let Some(edit) = edits.get(line.id) else {
            continue;
        };
        if in_deleted_table(line) {
            bounds.insert(line.id, None);
            continue;
        }
        if !edit.deleted && !edit.is_moved() {
            continue;
        }
        let zone = zones
            .get(&line.id)
            .copied()
            .unwrap_or_else(|| line_zone(line));
        let avoid = avoid_for(model, line.id);
        process(
            zone,
            line_settings(line),
            &avoid,
            edit,
            line.id,
            &mut image,
            &mut changed,
        );
        if edit.deleted {
            bounds.insert(line.id, None);
            for &index in &line.words {
                changes.words[index] = None;
            }
            remove_inserted(&mut changes, line);
        } else {
            let (dx, dy) = placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy));
            bounds.insert(line.id, Some(zone.offset(dx, dy)));
            let offset = placement.page_offset(dx, dy);
            for &index in &line.words {
                changes.offset(index, offset);
            }
            shift_inserted(&mut changes, line, offset);
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
            &[],
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
            &[],
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
            if word.as_ref().is_some_and(&inside) {
                if edit.deleted {
                    *word = None;
                } else if let Some(word) = word {
                    shift(word, offset);
                }
            }
        }
        for added in changes.inserted.values_mut() {
            if edit.deleted {
                added.retain(|w| !inside(w));
            } else {
                for word in added.iter_mut().filter(|w| inside(w)) {
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
    }
}

/// Zone d'une ligne intacte : emprise de ses mots, avec une marge.
pub(crate) fn line_zone(line: &ScanLine) -> PixelRect {
    line.core.inflate((0.1 * line.height()).ceil() as i32)
}

/// Emprises des autres lignes : leur encre n'est jamais prise.
fn avoid_for(model: &ScanModel, id: u32) -> Vec<PixelRect> {
    model
        .lines
        .iter()
        .filter(|line| line.id != id)
        .map(|line| line.core)
        .collect()
}

fn radius(height: f64) -> usize {
    (height * 0.15).round().clamp(2.0, 14.0) as usize
}

fn halo(scale: f64) -> i32 {
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

/// Les mots ajoutés à une ligne sont ceux insérés parmi ses mots.
fn line_slots(line: &ScanLine) -> std::ops::RangeInclusive<usize> {
    let first = line.words.first().copied().unwrap_or(0);
    let last = line.words.last().copied().unwrap_or(0);
    first..=last + 1
}

fn remove_inserted(changes: &mut LayerChanges, line: &ScanLine) {
    let slots = line_slots(line);
    changes.inserted.retain(|k, _| !slots.contains(k));
}

fn shift_inserted(changes: &mut LayerChanges, line: &ScanLine, offset: (f64, f64)) {
    let slots = line_slots(line);
    for (_, added) in changes.inserted.range_mut(slots) {
        for word in added {
            shift(word, offset);
        }
    }
}

/// Résultat de la réécriture d'une ligne.
struct LineResult {
    changed: Vec<PixelRect>,
    /// Emprise de la ligne réécrite.
    zone: PixelRect,
    /// Mots d'origine remplacés.
    removed: Vec<usize>,
    /// Mots d'origine décalés, et leur décalage dans l'espace de la page.
    shifted: Vec<(usize, (f64, f64))>,
    /// Nouveaux mots.
    added: Vec<LayerWord>,
    /// Indice du mot d'origine avant lequel placer les nouveaux mots.
    insert_at: usize,
}

/// Réécrit la ligne `line` avec le texte `text` : seuls les mots qui
/// changent sont effacés et redessinés ; les mots suivants glissent.
fn rewrite_line(
    model: &ScanModel,
    layer: &TextLayer,
    line: &ScanLine,
    text: &ScanText,
    image: &mut RgbImage,
    avoid: &[PixelRect],
) -> Option<LineResult> {
    let old: Vec<&str> = line
        .words
        .iter()
        .map(|&i| layer.words[i].text.as_str())
        .collect();
    let new: Vec<&str> = text.text.split_whitespace().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    if prefix == old.len() && prefix == new.len() {
        return None;
    }
    let placement = model.image.placement;
    let scale = placement.scale;
    let geometry = model.measure(line, layer);
    let frame = line.frame;
    let ext = &geometry.extents;
    let baseline = f64::from(geometry.ink.observation.baseline);
    let height = (geometry.bottom - geometry.top).max(1.0);
    let (band_top, band_bottom) = (
        geometry.top - 0.15 * height,
        geometry.bottom + 0.15 * height,
    );
    let strip_rect = |s0: f64, s1: f64| {
        let corners = [
            frame.to_image(s0, band_top),
            frame.to_image(s1, band_top),
            frame.to_image(s1, band_bottom),
            frame.to_image(s0, band_bottom),
        ];
        let (x0, y0, x1, y1) = super::bounds(&corners);
        PixelRect::covering(x0, y0, x1, y1)
    };
    let settings = Selecting {
        margin: 1,
        background: Background::Closing(radius(height)),
        color: Some(geometry.ink.color),
        halo: halo(scale),
    };

    // Police.
    let catalog = fonts::catalog::shared();
    let face_index = catalog
        .find(&text.style.font)
        .or_else(|| (!catalog.faces().is_empty()).then_some(0))?;
    let file = catalog.load(face_index)?;
    let face = file.face();
    let upem = f64::from(face.units_per_em() as f32);
    let em = f64::from(text.style.size) * scale;
    let k = em / upem;
    let spacing_units = (f64::from(text.style.spacing) * scale / k) as f32;
    let blur = text.style.blur * scale as f32;
    let space = shape(&face, " ", 0.0, false).advance as f64 * k;
    let gap = geometry
        .gap
        .unwrap_or(if space > 0.0 { space } else { 0.25 * em });

    let mut changed = Vec::new();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    // Mots remplacés.
    if prefix < old_end {
        let zone = strip_rect(ext[prefix].0 - 1.0, ext[old_end - 1].1 + 1.0);
        let selection = select(image, zone, &settings, avoid);
        changed.push(erase(image, &selection, u64::from(line.id)));
    }
    // Mots suivants, détachés pour glisser.
    let tail = (suffix > 0).then(|| {
        let zone = strip_rect(ext[old_end].0 - 1.0, ext[old.len() - 1].1 + 1.0);
        let selection = select(image, zone, &settings, avoid);
        let patch = extract(image, &selection);
        changed.push(erase(image, &selection, u64::from(line.id) + 1));
        patch
    });

    // Nouveaux mots, un à un : l'espace entre eux est celui mesuré sur la
    // ligne.
    let start = if prefix > 0 {
        ext[prefix - 1].1 + gap
    } else if !ext.is_empty() {
        ext[0].0
    } else {
        0.5 * height
    };
    let mut cursor = start;
    let mut added = Vec::new();
    let color = text.style.color.map(|c| c * 255.0);
    let line_number = line.words.first().map_or(0, |&i| layer.words[i].line);
    let mut drawn_end = start;
    for (j, word) in new[prefix..new_end].iter().enumerate() {
        let shaped = shape(&face, word, spacing_units, text.style.kerning);
        let ink = (f64::from(shaped.ink_x.1 - shaped.ink_x.0) * k).max(1.0);
        let pen_s = cursor - f64::from(shaped.ink_x.0) * k;
        let pen = Pen {
            origin: frame.to_image(pen_s, baseline),
            scale: k,
            direction: frame.direction,
            shear: 0.0,
        };
        let coverage = rasterize(&face, &shaped, &pen)
            .emboldened(text.style.weight)
            .blurred(blur);
        changed.push(draw(
            image,
            &coverage,
            color,
            text.style.noise,
            u64::from(line.id) * 31 + j as u64,
        ));
        let corners = [
            frame.to_image(cursor, geometry.bottom),
            frame.to_image(cursor + ink, geometry.bottom),
            frame.to_image(cursor + ink, geometry.top),
            frame.to_image(cursor, geometry.top),
        ]
        .map(|(x, y)| placement.to_page(x, y));
        let mut quad = [0.0f32; 8];
        for (n, (x, y)) in corners.iter().enumerate() {
            quad[2 * n] = *x as f32;
            quad[2 * n + 1] = *y as f32;
        }
        added.push(LayerWord {
            text: (*word).to_owned(),
            quad,
            confidence: 1.0,
            line: line_number,
        });
        drawn_end = cursor + ink;
        cursor += ink + gap;
    }
    if prefix == new_end && prefix > 0 {
        // Aucun nouveau mot : les suivants reprennent après le dernier gardé.
        cursor = ext[prefix - 1].1 + gap;
    }

    // Les mots suivants glissent.
    let mut shifted = Vec::new();
    let mut end = if prefix > 0 { ext[prefix - 1].1 } else { start };
    end = end.max(drawn_end);
    if let Some(patch) = tail {
        let delta = cursor - ext[old_end].0;
        let (dx, dy) = (
            (delta * frame.direction.0).round() as i32,
            (delta * frame.direction.1).round() as i32,
        );
        changed.push(apply(image, &patch, dx, dy));
        let offset = placement.page_offset(dx, dy);
        for &index in &line.words[old_end..] {
            shifted.push((index, offset));
        }
        end = end.max(ext[old.len() - 1].1 + delta);
    }
    let begin = ext.first().map_or(start, |e| e.0.min(start));
    let zone = strip_rect(begin - 1.0, end + 1.0);
    let insert_at = line
        .words
        .get(old_end)
        .copied()
        .unwrap_or_else(|| line.words.last().map_or(0, |&i| i + 1));
    Some(LineResult {
        changed,
        zone,
        removed: line.words[prefix..old_end].to_vec(),
        shifted,
        added,
        insert_at,
    })
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
