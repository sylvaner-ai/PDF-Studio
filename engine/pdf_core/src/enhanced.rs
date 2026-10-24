//! Pages améliorées (mode Amélioration) : pixels du scan d'une page, et PDF
//! formé des pages dont l'image a été remplacée par sa version améliorée.
//!
//! Une page améliorée garde le format, les cadres et la rotation de la page
//! d'origine ; son contenu se réduit à l'image améliorée (JPEG), posée à la
//! place exacte du scan. Dans la session, elle devient une page d'un document
//! source de plus ; à l'enregistrement, son contenu remplace celui de la page
//! d'origine, dont l'objet est gardé avec ses annotations, ses liens et les
//! signets qui la visent (cf. [`crate::OutputPage::Replaced`]).

use mupdf::pdf::{PdfDocument, PdfObject, PdfWriteOptions};
use mupdf::Buffer;
use scan::{Rect as PixelRect, RgbImage};

use crate::content::geometry::Matrix;
use crate::error::{PdfError, Result};
use crate::scanned::{self, ScanImage};

/// Part de la page que l'image d'un scan doit couvrir pour être améliorée :
/// le contenu de la page améliorée se réduit à cette image.
const MIN_COVERAGE: f64 = 0.9;

/// Cadres et rotation d'une page, et place de l'image de son scan.
#[derive(Clone, Debug, PartialEq)]
pub struct PageGeometry {
    media_box: [f64; 4],
    crop_box: Option<[f64; 4]>,
    rotate: i32,
    /// Matrice qui place l'image dans l'espace de la page PDF (opérateur `cm`
    /// avant `Do`).
    image_matrix: Matrix,
}

/// Pixels du scan d'une page, à la résolution de son image, sur la page
/// droite (sans sa rotation `/Rotate`).
#[derive(Clone, Debug)]
pub struct ScanPixels {
    /// Pixels du scan (contenu de la page compris, sans les annotations).
    pub image: RgbImage,
    /// Résolution, en pixels par pouce.
    pub dpi: f32,
    /// Quarts de tour, dans le sens horaire, qui passent de ces pixels à la
    /// page telle qu'elle s'affiche (rotation `/Rotate`).
    pub display_turns: u8,
    /// Cadres de la page et place du scan, pour écrire la page améliorée.
    pub geometry: PageGeometry,
}

/// Image améliorée d'une page, encodée en JPEG.
#[derive(Clone, Debug)]
pub struct EnhancedImage {
    /// Octets JPEG.
    pub jpeg: Vec<u8>,
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Niveaux de gris (un canal).
    pub gray: bool,
    /// Page d'origine.
    pub geometry: PageGeometry,
}

/// Pixels du scan de la page `index` de `doc` ; `None` si la page n'est pas
/// un scan pleine page (texte visible, image trop petite).
pub(crate) fn scan_pixels(
    doc: &PdfDocument,
    index: u32,
    scan: Option<ScanImage>,
    page_area: f64,
) -> Result<Option<ScanPixels>> {
    let Some(scan) = scan else {
        return Ok(None);
    };
    let placement = scan.placement;
    let (w, h) = (placement.width, placement.height);
    let full = PixelRect::new(0, 0, w as i32, h as i32);
    let shown = placement.display_rect(&full);
    if shown.area() < MIN_COVERAGE * page_area {
        return Ok(None);
    }
    let page = doc.find_page(index as i32)?;
    let geometry = PageGeometry {
        media_box: read_box(&page, "MediaBox")?.unwrap_or([0.0, 0.0, 612.0, 792.0]),
        crop_box: read_box(&page, "CropBox")?,
        rotate: match page.get_dict_inheritable("Rotate")? {
            Some(value) if value.is_number()? => value.as_int()?,
            _ => 0,
        },
        image_matrix: placement.page_image_matrix(&full),
    };
    let image = scanned::render(doc, index, &placement)?;
    Ok(Some(ScanPixels {
        image,
        dpi: (placement.scale * 72.0) as f32,
        display_turns: placement.display_quarter_turns(),
        geometry,
    }))
}

/// Cadre `key` de la page (hérité compris).
fn read_box(page: &PdfObject, key: &str) -> Result<Option<[f64; 4]>> {
    let Some(array) = page.get_dict_inheritable(key)? else {
        return Ok(None);
    };
    if !array.is_array()? || array.len()? != 4 {
        return Ok(None);
    }
    let mut out = [0.0; 4];
    for (index, slot) in out.iter_mut().enumerate() {
        match array.get_array(index as i32)? {
            Some(item) if item.is_number()? => *slot = f64::from(item.as_float()?),
            _ => return Ok(None),
        }
    }
    Ok(Some(out))
}

/// PDF formé des pages améliorées `pages`, dans l'ordre : pour chacune, le
/// format et la rotation de la page d'origine, et l'image à la place du
/// scan.
pub fn write_enhanced_pages(pages: &[&EnhancedImage]) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(PdfError::Engine {
            detail: "aucune page améliorée à écrire".into(),
        });
    }
    let mut doc = PdfDocument::new();
    for (position, page) in pages.iter().enumerate() {
        let object = enhanced_page(&mut doc, page)?;
        doc.insert_page(position as i32, &object)?;
    }
    let mut options = PdfWriteOptions::default();
    options.set_garbage_level(1).set_compress(true);
    let mut output = Vec::new();
    doc.write_to_with_options(&mut output, options)?;
    Ok(output)
}

/// Objet page d'une page améliorée, ajouté à `doc`.
fn enhanced_page(doc: &mut PdfDocument, page: &EnhancedImage) -> Result<PdfObject> {
    let mut image = doc.new_dict()?;
    image.dict_put("Type", doc.new_name("XObject")?)?;
    image.dict_put("Subtype", doc.new_name("Image")?)?;
    image.dict_put("Width", doc.new_int(page.width as i32)?)?;
    image.dict_put("Height", doc.new_int(page.height as i32)?)?;
    image.dict_put(
        "ColorSpace",
        doc.new_name(if page.gray { "DeviceGray" } else { "DeviceRGB" })?,
    )?;
    image.dict_put("BitsPerComponent", doc.new_int(8)?)?;
    image.dict_put("Filter", doc.new_name("DCTDecode")?)?;
    // Données déjà compressées : MuPDF les écrit telles quelles.
    let image = doc.add_stream(&Buffer::from_bytes(&page.jpeg)?, Some(&image), true)?;

    let mut xobjects = doc.new_dict()?;
    xobjects.dict_put("PSEnh", image)?;
    let mut resources = doc.new_dict()?;
    resources.dict_put("XObject", xobjects)?;
    let m = &page.geometry.image_matrix;
    let content = format!(
        "q {} {} {} {} {} {} cm /PSEnh Do Q\n",
        num(m.a),
        num(m.b),
        num(m.c),
        num(m.d),
        num(m.e),
        num(m.f)
    );
    let contents = doc.add_stream(&Buffer::from_bytes(content.as_bytes())?, None, false)?;

    let mut dict = doc.new_dict()?;
    dict.dict_put("Type", doc.new_name("Page")?)?;
    dict.dict_put("MediaBox", rect_array(doc, &page.geometry.media_box)?)?;
    if let Some(crop) = &page.geometry.crop_box {
        dict.dict_put("CropBox", rect_array(doc, crop)?)?;
    }
    if page.geometry.rotate != 0 {
        dict.dict_put("Rotate", doc.new_int(page.geometry.rotate)?)?;
    }
    dict.dict_put("Resources", resources)?;
    dict.dict_put("Contents", contents)?;
    Ok(doc.add_object(&dict)?)
}

fn rect_array(doc: &PdfDocument, rect: &[f64; 4]) -> Result<PdfObject> {
    let mut array = doc.new_array()?;
    for &value in rect {
        array.array_push(doc.new_real(value as f32)?)?;
    }
    Ok(array)
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
