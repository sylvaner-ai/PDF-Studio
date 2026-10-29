//! Lecture des ressources d'une page avec MuPDF : métriques des polices
//! (`/Widths`, `/W`, encodage, `ToUnicode`, CMap) et nature des objets
//! externes.

use std::collections::HashMap;
use std::sync::Arc;

use mupdf::pdf::PdfObject;

use super::cmap::CMap;
use super::encodings::{glyph_name_text, BaseEncoding};
use super::font::{FontKind, FontMetrics};
use super::fontprog::{self, ProgramFormat};
use super::geometry::{Matrix, Rect};
use super::interp::{Resources, XObject};
use super::unfold::{self, Alias};
use crate::error::Result;

/// Ressources d'une page (`/Resources`), lues à la demande et gardées.
pub struct PageResources {
    dict: Option<PdfObject>,
    /// Ressources des formulaires dépliés, par catégorie et nom dans la page.
    aliases: HashMap<(&'static str, Vec<u8>), PdfObject>,
    fonts: HashMap<Vec<u8>, Option<Arc<FontMetrics>>>,
    xobjects: HashMap<Vec<u8>, XObject>,
}

impl PageResources {
    /// Ressources du dictionnaire `dict` (absent : aucune ressource).
    pub fn new(dict: Option<PdfObject>) -> Self {
        Self {
            dict,
            aliases: HashMap::new(),
            fonts: HashMap::new(),
            xobjects: HashMap::new(),
        }
    }

    /// Ressources du dictionnaire `dict`, augmentées de celles des
    /// formulaires dépliés (cf. [`unfold`](super::unfold)).
    pub fn with_aliases(dict: Option<PdfObject>, aliases: &[Alias]) -> Self {
        let mut resources = Self::new(dict);
        for alias in aliases {
            if let Some(object) = unfold::resolve(resources.dict.as_ref(), alias) {
                resources
                    .aliases
                    .insert((alias.category, alias.name.clone()), object);
            }
        }
        resources
    }

    /// Polices lues, par nom de ressource.
    pub fn fonts(&self) -> impl Iterator<Item = (&[u8], &Arc<FontMetrics>)> {
        self.fonts
            .iter()
            .filter_map(|(name, font)| font.as_ref().map(|font| (name.as_slice(), font)))
    }

    fn entry(&self, category: &'static str, name: &[u8]) -> Option<PdfObject> {
        if let Some(object) = self.aliases.get(&(category, name.to_vec())) {
            return object.try_clone().ok();
        }
        let dict = self.dict.as_ref()?;
        let category = dict.get_dict(category).ok()??;
        category
            .get_dict(String::from_utf8_lossy(name).into_owned())
            .ok()?
    }
}

impl Resources for PageResources {
    fn font(&mut self, name: &[u8]) -> Option<Arc<FontMetrics>> {
        if let Some(known) = self.fonts.get(name) {
            return known.clone();
        }
        let font = self
            .entry("Font", name)
            .and_then(|dict| font_metrics(&dict).ok())
            .map(Arc::new);
        self.fonts.insert(name.to_vec(), font.clone());
        font
    }

    fn xobject(&mut self, name: &[u8]) -> XObject {
        if let Some(&known) = self.xobjects.get(name) {
            return known;
        }
        let kind = self
            .entry("XObject", name)
            .and_then(|object| xobject_kind(&object).ok())
            .unwrap_or(XObject::Other);
        self.xobjects.insert(name.to_vec(), kind);
        kind
    }
}

fn xobject_kind(object: &PdfObject) -> Result<XObject> {
    Ok(match name(object, "Subtype")?.as_deref() {
        Some(b"Image") => XObject::Image,
        Some(b"Form") => {
            let bbox = numbers(object, "BBox")?
                .filter(|values| values.len() == 4)
                .map(|v| Rect::new(v[0], v[1], v[2], v[3]))
                .unwrap_or(Rect::EMPTY);
            let matrix = numbers(object, "Matrix")?
                .filter(|values| values.len() == 6)
                .map(|v| Matrix::from_slice(&v))
                .unwrap_or(Matrix::IDENTITY);
            XObject::Form { bbox, matrix }
        }
        _ => XObject::Other,
    })
}

/// Métriques d'une police d'après son dictionnaire.
pub fn font_metrics(font: &PdfObject) -> Result<FontMetrics> {
    let subtype = name(font, "Subtype")?.unwrap_or_default();
    let base_name = name(font, "BaseFont")?
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .unwrap_or_default();
    let mut metrics = match subtype.as_slice() {
        b"Type0" => composite_metrics(font, &base_name)?,
        b"Type3" => simple_metrics(font, &base_name, FontKind::Type3)?,
        _ => simple_metrics(font, &base_name, FontKind::Simple)?,
    };
    if let Some(stream) = font.get_dict("ToUnicode")? {
        if stream.is_stream()? {
            let cmap = CMap::parse(&stream.read_stream()?);
            for (code, text) in cmap.unicode {
                metrics.unicode.insert(code, text);
            }
        }
    }
    metrics.index_unicode();
    Ok(metrics)
}

/// Police simple (Type 1, TrueType, Type 3).
fn simple_metrics(font: &PdfObject, base_name: &str, kind: FontKind) -> Result<FontMetrics> {
    let descriptor = font.get_dict("FontDescriptor")?;
    // Matrice des glyphes d'une police Type 3 : chasse exprimée dans son espace.
    let scale = if kind == FontKind::Type3 {
        numbers(font, "FontMatrix")?
            .and_then(|m| m.first().copied())
            .map_or(1.0, |a| a * 1000.0)
    } else {
        1.0
    };

    // Encodage : base, puis différences.
    let mut base = None;
    let mut differences: HashMap<u32, String> = HashMap::new();
    if let Some(encoding) = font.get_dict("Encoding")? {
        if encoding.is_name()? {
            base = BaseEncoding::from_name(&encoding.as_name()?);
        } else if encoding.is_dict()? {
            if let Some(name) = name(&encoding, "BaseEncoding")? {
                base = BaseEncoding::from_name(&name);
            }
            if let Some(array) = encoding.get_dict("Differences")? {
                let mut code = 0u32;
                for item in array.array_iter()? {
                    let item = item?;
                    if item.is_number()? {
                        code = item.as_float()?.max(0.0) as u32;
                    } else if item.is_name()? {
                        if let Some(text) =
                            glyph_name_text(&String::from_utf8_lossy(&item.as_name()?))
                        {
                            differences.insert(code, text);
                        }
                        code += 1;
                    }
                }
            }
        }
    }
    // Sans encodage de base désigné, une police Type 1 ou CFF intégrée suit
    // l'encodage de son programme (StandardEncoding s'il n'en porte pas).
    let builtin = match (base, kind, &descriptor) {
        (None, FontKind::Simple, Some(descriptor)) => builtin_encoding(font, descriptor)?,
        _ => None,
    };
    let base = base.unwrap_or(BaseEncoding::Standard);
    let symbolic = base_name.starts_with("Symbol") || base_name.starts_with("ZapfDingbats");
    let mut unicode: HashMap<u32, String> = HashMap::new();
    for code in 0..256u32 {
        if let Some(text) = differences.remove(&code) {
            unicode.insert(code, text);
        } else if let Some(builtin) = &builtin {
            if let Some(text) = builtin
                .get(&(code as u8))
                .and_then(|name| glyph_name_text(name))
            {
                unicode.insert(code, text);
            }
        } else if !symbolic && kind != FontKind::Type3 {
            if let Some(c) = base.char(code as u8) {
                unicode.insert(code, c.to_string());
            }
        }
    }

    // Chasses déclarées, ou à défaut celles de la police standard.
    let mut widths: HashMap<u32, f64> = HashMap::new();
    let first = font
        .get_dict("FirstChar")?
        .map(|value| value.as_float())
        .transpose()?
        .unwrap_or(0.0)
        .max(0.0) as u32;
    if let Some(array) = font.get_dict("Widths")? {
        for (offset, item) in array.array_iter()?.enumerate() {
            let item = item?;
            if item.is_number()? {
                widths.insert(first + offset as u32, f64::from(item.as_float()?) * scale);
            }
        }
    } else if kind == FontKind::Simple {
        if let Some(standard) = crate::fonts::standard_font(base_name) {
            for (&code, text) in &unicode {
                let Some(c) = text.chars().next() else {
                    continue;
                };
                let gid = standard.encode_character(c as i32).unwrap_or(0);
                if gid > 0 {
                    let advance = standard.advance_glyph(gid).unwrap_or(0.0);
                    widths.insert(code, f64::from(advance) * 1000.0);
                }
            }
        }
    }

    let mut metrics = FontMetrics::new(base_name, kind, widths, unicode, None);
    if let Some(descriptor) = &descriptor {
        read_descriptor(&mut metrics, descriptor)?;
    } else if let Some(standard) = crate::fonts::standard_font(base_name) {
        metrics.ascent = f64::from(standard.ascender()) * 1000.0;
        metrics.descent = f64::from(standard.descender()) * 1000.0;
        metrics.flags = if standard.is_serif() { 0x2 } else { 0 }
            | if standard.is_monospaced() { 0x1 } else { 0 };
    }
    Ok(metrics)
}

/// Encodage interne du programme intégré d'une police Type 1 ou CFF.
fn builtin_encoding(
    font: &PdfObject,
    descriptor: &PdfObject,
) -> Result<Option<HashMap<u8, String>>> {
    // Une police TrueType sans encodage suit sa table `cmap`, que MuPDF lit.
    if name(font, "Subtype")?.as_deref() == Some(b"TrueType") {
        return Ok(None);
    }
    let (program, format) = if let Some(program) = descriptor.get_dict("FontFile")? {
        (program, ProgramFormat::Type1)
    } else if let Some(program) = descriptor.get_dict("FontFile3")? {
        let format = match name(&program, "Subtype")?.as_deref() {
            Some(b"Type1C") => ProgramFormat::Cff,
            Some(b"OpenType") => ProgramFormat::OpenType,
            _ => return Ok(None),
        };
        (program, format)
    } else {
        return Ok(None);
    };
    if !program.is_stream()? {
        return Ok(None);
    }
    Ok(fontprog::builtin_encoding(&program.read_stream()?, format))
}

/// Police composite (Type 0).
fn composite_metrics(font: &PdfObject, base_name: &str) -> Result<FontMetrics> {
    let mut supported = true;
    let mut vertical = false;
    let mut cmap = None;
    match font.get_dict("Encoding")? {
        Some(encoding) if encoding.is_name()? => {
            let name = encoding.as_name()?;
            vertical = name.ends_with(b"-V");
            if name != b"Identity-H" && name != b"Identity-V" {
                // CMap prédéfinie (Adobe-Japan1…) : les codes ne sont pas connus.
                supported = false;
            }
        }
        Some(encoding) if encoding.is_stream()? => {
            let parsed = CMap::parse(&encoding.read_stream()?);
            vertical = encoding
                .get_dict("WMode")?
                .map(|mode| mode.as_int())
                .transpose()?
                == Some(1);
            if let Some(base) = &parsed.use_cmap {
                if base != b"Identity-H" && base != b"Identity-V" {
                    supported = false;
                }
            }
            if parsed.ranges.is_empty() {
                supported = false;
            }
            cmap = Some(parsed);
        }
        _ => supported = false,
    }

    let descendant = font
        .get_dict("DescendantFonts")?
        .and_then(|array| array.get_array(0).ok().flatten());
    let mut widths: HashMap<u32, f64> = HashMap::new();
    let mut default_width = 1000.0;
    if let Some(descendant) = &descendant {
        if let Some(dw) = descendant.get_dict("DW")? {
            default_width = f64::from(dw.as_float()?);
        }
        if let Some(w) = descendant.get_dict("W")? {
            read_cid_widths(&w, &mut widths)?;
        }
    }

    let mut metrics =
        FontMetrics::new(base_name, FontKind::Composite, widths, HashMap::new(), cmap);
    metrics.default_width = default_width;
    metrics.vertical = vertical;
    metrics.supported = supported;
    if let Some(descriptor) = descendant
        .as_ref()
        .map(|d| d.get_dict("FontDescriptor"))
        .transpose()?
        .flatten()
    {
        read_descriptor(&mut metrics, &descriptor)?;
    }
    Ok(metrics)
}

/// Lit un tableau `/W` : `c [w1 w2 …]` ou `c_premier c_dernier w`.
fn read_cid_widths(array: &PdfObject, widths: &mut HashMap<u32, f64>) -> Result<()> {
    let items: Vec<PdfObject> = array.array_iter()?.collect::<std::result::Result<_, _>>()?;
    let mut index = 0;
    while index + 1 < items.len() {
        if !items[index].is_number()? {
            index += 1;
            continue;
        }
        let first = items[index].as_float()?.max(0.0) as u32;
        let next = &items[index + 1];
        if next.is_array()? {
            for (offset, width) in next.array_iter()?.enumerate() {
                let width = width?;
                if width.is_number()? {
                    widths.insert(first + offset as u32, f64::from(width.as_float()?));
                }
            }
            index += 2;
        } else if index + 2 < items.len() && next.is_number()? && items[index + 2].is_number()? {
            let last = next.as_float()?.max(0.0) as u32;
            let width = f64::from(items[index + 2].as_float()?);
            // Plage bornée contre les tableaux absurdes.
            for cid in first..=last.min(first.saturating_add(0xFFFF)) {
                widths.insert(cid, width);
            }
            index += 3;
        } else {
            break;
        }
    }
    Ok(())
}

fn read_descriptor(metrics: &mut FontMetrics, descriptor: &PdfObject) -> Result<()> {
    let number = |key: &str| -> Result<Option<f64>> {
        Ok(descriptor
            .get_dict(key)?
            .map(|value| value.as_float())
            .transpose()?
            .map(f64::from))
    };
    metrics.flags = number("Flags")?.unwrap_or(0.0).max(0.0) as u32;
    metrics.italic_angle = number("ItalicAngle")?.unwrap_or(0.0);
    metrics.weight = number("FontWeight")?;
    if let Some(ascent) = number("Ascent")?.filter(|&a| a > 0.0) {
        metrics.ascent = ascent;
    }
    if let Some(descent) = number("Descent")?.filter(|&d| d < 0.0) {
        metrics.descent = descent;
    }
    if metrics.kind != FontKind::Composite {
        if let Some(missing) = number("MissingWidth")? {
            metrics.default_width = missing;
        }
    }
    metrics.embedded = ["FontFile", "FontFile2", "FontFile3"]
        .iter()
        .any(|key| descriptor.get_dict(*key).ok().flatten().is_some());
    Ok(())
}

fn name(object: &PdfObject, key: &str) -> Result<Option<Vec<u8>>> {
    match object.get_dict(key)? {
        Some(value) if value.is_name()? => Ok(Some(value.as_name()?)),
        _ => Ok(None),
    }
}

fn numbers(object: &PdfObject, key: &str) -> Result<Option<Vec<f64>>> {
    let Some(array) = object.get_dict(key)? else {
        return Ok(None);
    };
    if !array.is_array()? {
        return Ok(None);
    }
    let mut values = Vec::new();
    for item in array.array_iter()? {
        let item = item?;
        values.push(if item.is_number()? {
            f64::from(item.as_float()?)
        } else {
            0.0
        });
    }
    Ok(Some(values))
}
