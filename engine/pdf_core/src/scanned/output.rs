//! Ce que l'application voit d'une page scannée : ses objets (modifications
//! comprises), son aperçu et son contenu enregistré.

use edit::PageEdits;
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::{Buffer, ColorParams, Colorspace, Device, DisplayList, Image, Pixmap};

use super::retouch::line_zone;
use super::{Patch, Placement, Retouched, ScanModel};
use crate::content::geometry::Matrix;
use crate::content::objects::ObjectKind;
use crate::editing::{CellInfo, ObjectInfo, TableInfo};
use crate::error::Result;
use crate::text::PageRect;
use scan::Rect as PixelRect;

/// Objets de la page scannée `model`, modifications `edits` comprises
/// (`retouched` : leur effet sur les pixels).
pub(crate) fn infos(
    model: &ScanModel,
    edits: Option<&PageEdits>,
    retouched: Option<&Retouched>,
) -> Vec<ObjectInfo> {
    let placement = model.image.placement;
    let edit_of = |id: u32| edits.and_then(|edits| edits.get(id));
    let final_zone = |id: u32, zone: PixelRect| -> Option<PixelRect> {
        match retouched.and_then(|r| r.bounds.get(&id)) {
            Some(bounds) => *bounds,
            None if edit_of(id).is_some_and(|e| e.deleted) => None,
            None => Some(zone),
        }
    };
    let table_offset = |table: Option<u32>| -> Option<(i32, i32)> {
        let Some(edit) = table.and_then(edit_of) else {
            return Some((0, 0));
        };
        if edit.deleted {
            return None;
        }
        Some(placement.pixel_offset(f64::from(edit.dx), f64::from(edit.dy)))
    };
    let mut out = Vec::new();
    for line in &model.lines {
        let Some(offset) = table_offset(line.table) else {
            continue;
        };
        let changed = retouched.is_some_and(|r| r.bounds.contains_key(&line.id));
        let Some(zone) = final_zone(line.id, line.core) else {
            continue;
        };
        let zone = if changed {
            zone
        } else {
            zone.offset(offset.0, offset.1)
        };
        let scan = edit_of(line.id).and_then(|e| e.scan.clone());
        let size = scan
            .as_ref()
            .map_or((line.height() / placement.scale * 0.75) as f32, |s| {
                s.style.size
            });
        out.push(ObjectInfo {
            id: line.id,
            kind: ObjectKind::Text,
            bounds: page_rect(&placement, &zone),
            movable: true,
            editable: true,
            background: false,
            text: Some(
                scan.as_ref()
                    .map_or_else(|| line.text.clone(), |s| s.text.clone()),
            ),
            font_name: scan.as_ref().map(|s| s.style.font.clone()),
            font_size: size,
            substituted: false,
            overflow: false,
            missing: missing(scan.as_ref()),
            fingerprint: fingerprint("line", &line_zone(line)),
            table: line.table,
            grid: None,
            scanned: true,
            scan,
        });
    }
    for element in &model.elements {
        let Some(zone) = final_zone(element.id, element.zone) else {
            continue;
        };
        out.push(ObjectInfo {
            id: element.id,
            kind: element.kind,
            bounds: page_rect(&placement, &zone),
            movable: true,
            editable: false,
            background: false,
            text: None,
            font_name: None,
            font_size: 0.0,
            substituted: false,
            overflow: false,
            missing: String::new(),
            fingerprint: fingerprint("element", &element.zone),
            table: None,
            grid: None,
            scanned: true,
            scan: None,
        });
    }
    for table in &model.tables {
        let Some(zone) = final_zone(table.id, table.zone) else {
            continue;
        };
        let (dx, dy) = table_offset(Some(table.id)).unwrap_or((0, 0));
        let t = &table.table;
        let cells = t
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let rect = model.deskew.pixel_rect(&t.cell_rect(cell)).offset(dx, dy);
                CellInfo {
                    row: cell.row as u32,
                    column: cell.column as u32,
                    row_span: cell.row_span as u32,
                    column_span: cell.column_span as u32,
                    source_row: cell.row as u32,
                    source_column: cell.column as u32,
                    bounds: page_rect(&placement, &rect),
                    objects: table.cells[index]
                        .iter()
                        .copied()
                        .filter(|id| !edit_of(*id).is_some_and(|e| e.deleted))
                        .collect(),
                    text: None,
                    reference: None,
                    overflow: false,
                }
            })
            .collect();
        let grid = TableInfo {
            rows: t.row_count() as u32,
            columns: t.column_count() as u32,
            row_order: (0..t.row_count() as u32).collect(),
            column_order: (0..t.column_count() as u32).collect(),
            source_rows: t.row_count() as u32,
            source_columns: t.column_count() as u32,
            cells,
        };
        out.push(ObjectInfo {
            id: table.id,
            kind: ObjectKind::Table,
            bounds: page_rect(&placement, &zone),
            movable: true,
            editable: false,
            background: false,
            text: None,
            font_name: None,
            font_size: 0.0,
            substituted: false,
            overflow: false,
            missing: String::new(),
            fingerprint: format!(
                "{}:{}x{}",
                fingerprint("table", &table.zone),
                t.row_count(),
                t.column_count()
            ),
            table: None,
            grid: Some(grid),
            scanned: true,
            scan: None,
        });
    }
    out
}

/// Caractères du texte retouché absents de sa police.
fn missing(scan: Option<&edit::ScanText>) -> String {
    let Some(scan) = scan else {
        return String::new();
    };
    let catalog = fonts::catalog::shared();
    let Some(file) = catalog.find(&scan.style.font).and_then(|i| catalog.load(i)) else {
        return String::new();
    };
    let face = file.face();
    let mut out = String::new();
    for c in scan.text.chars().filter(|c| !c.is_whitespace()) {
        if face.glyph_index(c).is_none() && !out.contains(c) {
            out.push(c);
        }
    }
    out
}

fn fingerprint(kind: &str, zone: &PixelRect) -> String {
    format!(
        "scan:{kind}:{}:{}:{}:{}",
        zone.x0, zone.y0, zone.x1, zone.y1
    )
}

fn page_rect(placement: &Placement, zone: &PixelRect) -> PageRect {
    let r = placement.display_rect(zone);
    PageRect {
        x: r.x0 as f32,
        y: r.y0 as f32,
        width: r.width() as f32,
        height: r.height() as f32,
    }
}

/// Image MuPDF d'un calque : pixels modifiés opaques, les autres
/// transparents.
fn patch_image(patch: &Patch) -> Result<Image> {
    let (w, h) = (patch.rect.width(), patch.rect.height());
    let mut pixmap = Pixmap::new_with_w_h(&Colorspace::device_rgb(), w as i32, h as i32, true)?;
    let stride = pixmap.stride() as usize;
    let samples = pixmap.samples_mut();
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (patch.rect.x0 + x as i32, patch.rect.y0 + y as i32);
            let i = y * stride + x * 4;
            if patch.changed.get(px, py) {
                let [r, g, b] = patch.pixels.get(x as i32, y as i32);
                samples[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            } else {
                // Pixels prémultipliés : transparent, donc noir.
                samples[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Ok(Image::from_pixmap(&pixmap)?)
}

fn to_mupdf(m: &Matrix) -> mupdf::Matrix {
    mupdf::Matrix::new(
        m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32,
    )
}

/// Aperçu : la page `list` et, par-dessus, les calques de `retouched`.
pub(crate) fn compose(
    list: &DisplayList,
    placement: &Placement,
    retouched: &Retouched,
) -> Result<DisplayList> {
    let mut out = DisplayList::new(list.bounds())?;
    {
        let device = Device::from_display_list(&mut out)?;
        list.run(&device, &mupdf::Matrix::IDENTITY, mupdf::Rect::INF)?;
        for patch in &retouched.patches {
            let image = patch_image(patch)?;
            let ctm = to_mupdf(&placement.display_image_matrix(&patch.rect));
            device.fill_image(&image, &ctm, 1.0, ColorParams::default())?;
        }
    }
    Ok(out)
}

/// Ajoute au document le calque `patch` : image sans perte et masque de ses
/// pixels modifiés.
fn add_patch(doc: &mut PdfDocument, patch: &Patch) -> Result<PdfObject> {
    let (w, h) = (patch.rect.width(), patch.rect.height());
    let channels = if patch.gray { 1 } else { 3 };
    let mut color = Vec::with_capacity(w * h * channels);
    let mut alpha = Vec::with_capacity(w * h);
    for y in 0..h {
        // Les pixels masqués répètent leur voisin : meilleure compression.
        let mut previous = [255u8; 3];
        for x in 0..w {
            let (px, py) = (patch.rect.x0 + x as i32, patch.rect.y0 + y as i32);
            let on = patch.changed.get(px, py);
            let c = if on {
                patch.pixels.get(x as i32, y as i32)
            } else {
                previous
            };
            previous = c;
            if patch.gray {
                color.push(c[0]);
            } else {
                color.extend_from_slice(&c);
            }
            alpha.push(if on { 255 } else { 0 });
        }
    }
    let image_dict = |doc: &mut PdfDocument, space: &str| -> Result<PdfObject> {
        let mut dict = doc.new_dict()?;
        dict.dict_put("Type", doc.new_name("XObject")?)?;
        dict.dict_put("Subtype", doc.new_name("Image")?)?;
        dict.dict_put("Width", doc.new_int(w as i32)?)?;
        dict.dict_put("Height", doc.new_int(h as i32)?)?;
        dict.dict_put("ColorSpace", doc.new_name(space)?)?;
        dict.dict_put("BitsPerComponent", doc.new_int(8)?)?;
        Ok(dict)
    };
    let mask_dict = image_dict(doc, "DeviceGray")?;
    let mask = doc.add_stream(&Buffer::from_bytes(&alpha)?, Some(&mask_dict), false)?;
    let mut dict = image_dict(
        doc,
        if patch.gray {
            "DeviceGray"
        } else {
            "DeviceRGB"
        },
    )?;
    dict.dict_put("SMask", mask)?;
    Ok(doc.add_stream(&Buffer::from_bytes(&color)?, Some(&dict), false)?)
}

/// Pose les calques de `retouched` sur la page `page` : un flux de contenu
/// de plus, après le contenu entouré de `q … Q`, et les images dans les
/// ressources de la page.
pub(crate) fn write_patches(
    doc: &mut PdfDocument,
    page: &mut PdfObject,
    placement: &Placement,
    retouched: &Retouched,
) -> Result<()> {
    if retouched.patches.is_empty() {
        return Ok(());
    }
    let resources = page.get_dict_inheritable("Resources")?;
    let mut copy = match &resources {
        Some(resources) if resources.is_dict()? => crate::writer::detached_copy(resources)?,
        _ => doc.new_dict()?,
    };
    let mut xobjects = match copy.get_dict("XObject")? {
        Some(dict) if dict.is_dict()? => crate::writer::detached_copy(&dict)?,
        _ => doc.new_dict()?,
    };
    let mut content = b"\nQ\n".to_vec();
    let mut number = 1;
    for patch in &retouched.patches {
        let mut name = format!("PSRT{number}");
        while xobjects.get_dict(name.as_str())?.is_some() {
            number += 1;
            name = format!("PSRT{number}");
        }
        number += 1;
        let image = add_patch(doc, patch)?;
        xobjects.dict_put(name.as_str(), image)?;
        let m = placement.page_image_matrix(&patch.rect);
        content.extend_from_slice(
            format!(
                "q {} {} {} {} {} {} cm /{name} Do Q\n",
                num(m.a),
                num(m.b),
                num(m.c),
                num(m.d),
                num(m.e),
                num(m.f)
            )
            .as_bytes(),
        );
    }
    copy.dict_put("XObject", xobjects)?;
    page.dict_put("Resources", copy)?;
    let mut contents = doc.new_array()?;
    contents.array_push(doc.add_stream(&Buffer::from_bytes(b"q\n")?, None, false)?)?;
    match page.get_dict("Contents")? {
        Some(existing) if existing.is_array()? => {
            for item in existing.array_iter()? {
                contents.array_push(item?)?;
            }
        }
        Some(existing) => contents.array_push(existing)?,
        None => {}
    }
    contents.array_push(doc.add_stream(&Buffer::from_bytes(&content)?, None, false)?)?;
    page.dict_put("Contents", contents)?;
    Ok(())
}

/// Nombre écrit dans un flux de contenu.
fn num(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}
