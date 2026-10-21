//! Rendu des pages en images bitmap RGBA.
//!
//! Le rendu part d'un [`PageContent`] (liste d'affichage MuPDF) et peut donc être
//! exécuté depuis n'importe quel fil, en parallèle pour plusieurs pages.
#![forbid(unsafe_code)]

use pdf_core::{PageContent, PdfError, Result};

/// Plus grande dimension autorisée pour une image rendue, en pixels.
///
/// Au-delà, la mémoire nécessaire devient déraisonnable (16 384² × 4 octets = 1 Gio) ;
/// les zooms extrêmes devront passer par un rendu par tuiles.
pub const MAX_DIMENSION: u32 = 16_384;

/// Image d'une page : pixels RGBA 8 bits, opaques, rangée par rangée.
#[derive(Debug, Clone)]
pub struct Bitmap {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Nombre d'octets par rangée de pixels.
    pub bytes_per_row: u32,
    /// Pixels au format RGBA, alpha toujours à 255.
    pub pixels: Vec<u8>,
}

/// Rend une page à l'échelle donnée (1,0 = 72 pixels par pouce).
pub fn render(content: &PageContent, scale: f32) -> Result<Bitmap> {
    let size = content.size();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PdfError::Engine {
            detail: format!("échelle de rendu invalide : {scale}"),
        });
    }
    let longest = size.width.max(size.height) * scale;
    if longest > MAX_DIMENSION as f32 {
        return Err(PdfError::Engine {
            detail: format!("image trop grande : {longest:.0} pixels de côté"),
        });
    }

    let ctm = mupdf::Matrix::new_scale(scale, scale);
    let pixmap = content
        .display_list()
        .to_pixmap(&ctm, &mupdf::Colorspace::device_rgb(), false)?;
    Ok(rgb_to_rgba(&pixmap))
}

/// Rend une page pour une résolution exprimée en points par pouce.
pub fn render_at_dpi(content: &PageContent, dpi: f32) -> Result<Bitmap> {
    render(content, dpi / 72.0)
}

fn rgb_to_rgba(pixmap: &mupdf::Pixmap) -> Bitmap {
    let width = pixmap.width();
    let height = pixmap.height();
    let src_stride = pixmap.stride() as usize;
    let samples = pixmap.samples();
    let row_px = width as usize;

    let mut pixels = Vec::with_capacity(row_px * height as usize * 4);
    for row in samples.chunks_exact(src_stride).take(height as usize) {
        for rgb in row[..row_px * 3].chunks_exact(3) {
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
        }
    }
    Bitmap {
        width,
        height,
        bytes_per_row: width * 4,
        pixels,
    }
}
