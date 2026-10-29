//! Objets copiés d'une page et objets redimensionnés.
//!
//! Des objets copiés deviennent un PDF d'une page, cadré sur eux : leur
//! contenu (modifications comprises) sans le reste de la page, avec les
//! seules polices et images qu'ils emploient, et les objets ajoutés copiés
//! dessinés par-dessus. La page porte la rotation sous laquelle les objets
//! se montraient : collée dans une autre application, elle apparaît comme à
//! l'écran ; collée dans une page, elle devient une image ajoutée faite de
//! ce PDF (cf. [`crate::added`]), dessinée en formulaire.
//!
//! Un objet d'origine se redimensionne autour du coin inférieur gauche de son
//! emprise d'origine, puis se déplace ([`edit::ObjectEdit::scale`]).

use std::collections::{BTreeMap, HashSet};

use edit::{AddedObject, ObjectEdit, PageEdits};
use mupdf::pdf::{PdfDocument, PdfObject, PdfWriteOptions};
use mupdf::Buffer;

use crate::added::{self, AddedResources};
use crate::content::geometry::Rect;
use crate::content::lexer;
use crate::editing::{edit_page, EmbeddedFonts, FontProvider, PageModel};
use crate::error::{PdfError, Result};
use crate::text::PageRect;

/// Marge autour des objets copiés, en points : l'emprise d'un texte est
/// celle de ses glyphes, à peu près.
const CLIP_MARGIN: f64 = 1.0;

/// Objets copiés d'une page.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// PDF d'une page, cadré sur les objets.
    pub pdf: Vec<u8>,
    /// Cadre de cette page dans l'espace d'affichage de la page d'origine
    /// (origine en haut à gauche, rotation `/Rotate` comprise).
    pub bounds: PageRect,
}

/// Rectangle de l'espace d'affichage, au format de l'application.
fn to_page_rect(rect: Rect) -> PageRect {
    PageRect {
        x: rect.x0 as f32,
        y: rect.y0 as f32,
        width: rect.width() as f32,
        height: rect.height() as f32,
    }
}

fn from_page_rect(rect: &PageRect) -> Rect {
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    Rect::new(x, y, x + f64::from(rect.width), y + f64::from(rect.height))
}

/// Copie les objets `objects` de la page `index` (modifications `edits`
/// comprises) : objets d'origine et objets ajoutés. `quarter_turns` : quarts
/// de tour que l'organisation donne à la page, gardés par la page copiée.
/// `None` si aucun objet ne peut être copié.
#[allow(clippy::too_many_arguments)]
pub(crate) fn clip(
    doc: &mut PdfDocument,
    index: u32,
    model: &PageModel,
    edits: Option<&PageEdits>,
    objects: &[u32],
    quarter_turns: u8,
    fonts: &mut FontProvider,
    embedded: &mut EmbeddedFonts,
) -> Result<Option<Clip>> {
    let selected: HashSet<u32> = objects.iter().copied().collect();
    let to_display = model.display_matrix();

    // Objets d'origine copiés, tels qu'ils se montrent.
    let originals: Vec<_> = model
        .infos(edits, fonts)
        .into_iter()
        .filter(|info| selected.contains(&info.id) && info.movable)
        .collect();
    let added: BTreeMap<u32, AddedObject> = edits
        .map(|edits| {
            edits
                .added()
                .iter()
                .filter(|(id, _)| selected.contains(id))
                .map(|(&id, object)| (id, object.clone()))
                .collect()
        })
        .unwrap_or_default();
    let added_infos = added::infos(&added, &to_display, fonts);
    let shown = originals
        .iter()
        .chain(&added_infos)
        .map(|info| from_page_rect(&info.bounds))
        .fold(Rect::EMPTY, |rect, bounds| rect.union(&bounds));
    if shown.is_empty() {
        return Ok(None);
    }
    let shown = shown.inflate(CLIP_MARGIN);
    let Some(from_display) = to_display.inverse() else {
        return Ok(None);
    };
    let media = from_display.apply_rect(shown);

    // Contenu de la page sans les objets qui ne sont pas copiés.
    let page = doc.find_page(index as i32)?;
    let (content, resources) = if originals.is_empty() {
        (Vec::new(), None)
    } else {
        let kept: HashSet<u32> = originals.iter().map(|info| info.id).collect();
        let mut changes: BTreeMap<u32, ObjectEdit> = edits
            .map(|edits| {
                edits
                    .iter()
                    .filter(|(id, _)| !crate::scanned::is_scan_id(*id))
                    .map(|(id, edit)| (id, edit.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for object in model.objects() {
            let keep = kept.contains(&object.id) || object.table.is_some_and(|t| kept.contains(&t));
            if !keep {
                changes.entry(object.id).or_default().deleted = true;
            }
        }
        let edited = edit_page(
            doc,
            &page,
            model,
            &PageEdits::new(changes, 0),
            fonts,
            embedded,
        )?;
        let resources = match edited.resources {
            Some(resources) => Some(resources),
            None => page.get_dict_inheritable("Resources")?,
        };
        (edited.content, resources)
    };

    let rotation = match page.get_dict_inheritable("Rotate")? {
        Some(value) if value.is_number()? => value.as_int()?,
        _ => 0,
    };
    let rotation =
        ((rotation.rem_euclid(360) + 45) / 90 * 90 + 90 * i32::from(quarter_turns)) % 360;

    let mut out = PdfDocument::new();
    let mut map = out.new_graft_map()?;
    let mut page_dict = out.new_dict()?;
    page_dict.dict_put("Type", PdfObject::new_name("Page")?)?;
    let mut media_box = out.new_array()?;
    for value in [media.x0, media.y0, media.x1, media.y1] {
        media_box.array_push(PdfObject::new_real(value as f32)?)?;
    }
    page_dict.dict_put("MediaBox", media_box)?;
    if rotation != 0 {
        page_dict.dict_put("Rotate", PdfObject::new_int(rotation)?)?;
    }
    let mut kept_resources = out.new_dict()?;
    if let Some(resources) = resources.filter(|r| r.is_dict().unwrap_or(false)) {
        let used = used_names(&content);
        for entry in resources.dict_iter()? {
            let (key, value) = entry?;
            let category = String::from_utf8_lossy(&key.as_name()?).into_owned();
            // Les polices et les images, souvent lourdes, ne sont copiées que
            // si les objets copiés les emploient.
            let names = match category.as_str() {
                "Font" => Some(&used.fonts),
                "XObject" => Some(&used.xobjects),
                _ => None,
            };
            let Some(names) = names else {
                kept_resources.dict_put(category.as_str(), map.graft_object(&value)?)?;
                continue;
            };
            if !value.is_dict()? {
                continue;
            }
            let mut dict = out.new_dict()?;
            for item in value.dict_iter()? {
                let (name, object) = item?;
                if names.contains(&name.as_name()?) {
                    let name = String::from_utf8_lossy(&name.as_name()?).into_owned();
                    dict.dict_put(name.as_str(), map.graft_object(&object)?)?;
                }
            }
            if dict.dict_len()? > 0 {
                kept_resources.dict_put(category.as_str(), dict)?;
            }
        }
    }
    page_dict.dict_put("Resources", kept_resources)?;
    let stream = out.add_stream(&Buffer::from_bytes(&content)?, None, false)?;
    page_dict.dict_put("Contents", stream)?;
    let page_object = out.add_object(&page_dict)?;
    out.insert_page(0, &page_object)?;
    if !added.is_empty() {
        added::append(
            &mut out,
            0,
            quarter_turns,
            &added,
            fonts,
            &mut AddedResources::default(),
        )?;
    }

    let mut options = PdfWriteOptions::default();
    options.set_garbage_level(3).set_compress(true);
    let mut pdf = Vec::new();
    out.write_to_with_options(&mut pdf, options)?;
    Ok(Some(Clip {
        pdf,
        bounds: to_page_rect(shown),
    }))
}

/// Noms des polices et des objets externes qu'emploie un flux de contenu.
struct UsedNames {
    fonts: HashSet<Vec<u8>>,
    xobjects: HashSet<Vec<u8>>,
}

fn used_names(content: &[u8]) -> UsedNames {
    let mut used = UsedNames {
        fonts: HashSet::new(),
        xobjects: HashSet::new(),
    };
    for op in lexer::parse(content) {
        let set = if op.is("Tf") {
            &mut used.fonts
        } else if op.is("Do") {
            &mut used.xobjects
        } else {
            continue;
        };
        if let Some(name) = op.operands.first().and_then(|operand| operand.name()) {
            set.insert(name.to_vec());
        }
    }
    used
}

/// Agrandissement et déplacement (dans l'espace de la page PDF) qui
/// placent l'objet d'origine `object` dans le cadre `target` de l'espace
/// d'affichage, ses autres modifications (texte) gardées. `None` si l'objet
/// ne se redimensionne pas.
pub(crate) fn resize(
    model: &PageModel,
    edits: Option<&PageEdits>,
    object: u32,
    target: &PageRect,
    fonts: &mut FontProvider,
) -> Result<Option<([f32; 2], f32, f32)>> {
    let Some(original) = model
        .objects()
        .get(object as usize)
        .filter(|o| o.id == object)
    else {
        return Ok(None);
    };
    if !original.movable || original.table.is_some() || original.structure.is_some() {
        return Ok(None);
    }
    // Emprise sans le déplacement ni l'agrandissement de l'objet.
    let mut objects = edits.map(PageEdits::objects).unwrap_or_default();
    if let Some(edit) = objects.get_mut(&object) {
        edit.dx = 0.0;
        edit.dy = 0.0;
        edit.scale = None;
    }
    let base = PageEdits::new(objects, 0);
    let Some(info) = model
        .infos(Some(&base), fonts)
        .into_iter()
        .find(|info| info.id == object)
    else {
        return Ok(None);
    };
    let to_display = model.display_matrix();
    let Some(from_display) = to_display.inverse() else {
        return Ok(None);
    };
    let current = from_display.apply_rect(from_page_rect(&info.bounds));
    let wanted = from_display.apply_rect(from_page_rect(target));
    if wanted.is_empty() {
        return Err(PdfError::WriteFailed {
            detail: "cadre vide".to_owned(),
        });
    }
    // Un filet sans épaisseur garde la sienne.
    let factor = |wanted: f64, current: f64| {
        if current < 0.5 {
            1.0
        } else {
            (wanted / current).clamp(0.01, 100.0)
        }
    };
    let (sx, sy) = (
        factor(wanted.width(), current.width()),
        factor(wanted.height(), current.height()),
    );
    // Le coin inférieur gauche de l'emprise d'origine reste en place à
    // l'agrandissement ; le déplacement amène l'emprise dans le cadre.
    let (ax, ay) = (original.bbox.x0, original.bbox.y0);
    let dx = wanted.x0 - ax - sx * (current.x0 - ax);
    let dy = wanted.y0 - ay - sy * (current.y0 - ay);
    Ok(Some(([sx as f32, sy as f32], dx as f32, dy as f32)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::geometry::Matrix;

    #[test]
    fn noms_employes_par_un_flux() {
        let used = used_names(b"q /Im1 Do Q BT /F2 12 Tf (a) Tj ET /GS0 gs /Fm1 Do");
        assert!(used.fonts.contains(&b"F2"[..]));
        assert!(used.xobjects.contains(&b"Im1"[..]) && used.xobjects.contains(&b"Fm1"[..]));
        assert!(!used.xobjects.contains(&b"GS0"[..]));
    }

    #[test]
    fn transformation_d_agrandissement() {
        // La matrice d'affichage et d'agrandissement ne sert qu'à vérifier
        // l'algèbre : le coin inférieur gauche du cadre voulu est atteint.
        let (ax, ay) = (10.0, 20.0);
        let (sx, sy) = (2.0, 0.5);
        let current = Rect::new(10.0, 20.0, 30.0, 60.0);
        let wanted = Rect::new(100.0, 100.0, 140.0, 120.0);
        let dx = wanted.x0 - ax - sx * (current.x0 - ax);
        let dy = wanted.y0 - ay - sy * (current.y0 - ay);
        let m = Matrix::translate(-ax, -ay)
            .then(&Matrix::new(sx, 0.0, 0.0, sy, 0.0, 0.0))
            .then(&Matrix::translate(ax + dx, ay + dy));
        let result = m.apply_rect(current);
        assert!((result.x0 - 100.0).abs() < 1e-9 && (result.y0 - 100.0).abs() < 1e-9);
        assert!((result.x1 - 140.0).abs() < 1e-9 && (result.y1 - 120.0).abs() < 1e-9);
    }
}
