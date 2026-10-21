//! Rendu des pages en images bitmap RGBA.
//!
//! Le rendu part d'un [`PageContent`] (liste d'affichage MuPDF) et peut donc être
//! exécuté depuis n'importe quel fil, en parallèle pour plusieurs pages ou
//! plusieurs zones d'une même page. Les forts grossissements passent par
//! [`render_region`] : seule la zone visible est rendue, par tuiles.
#![forbid(unsafe_code)]

use pdf_core::{PageContent, PageSize, PdfError, Result};

/// Plus grande dimension autorisée pour une image rendue, en pixels.
///
/// Au-delà, la mémoire nécessaire devient déraisonnable (16 384² × 4 octets = 1 Gio) ;
/// les zooms plus forts passent par [`render_region`].
pub const MAX_DIMENSION: u32 = 16_384;

/// Plus grande échelle de rendu acceptée (6 400 %).
pub const MAX_SCALE: f32 = 64.0;

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

/// Rotation d'affichage, dans le sens des aiguilles d'une montre.
///
/// Elle s'ajoute à la rotation `/Rotate` enregistrée dans le PDF, déjà comprise
/// dans les dimensions de la page ; le fichier n'est pas modifié.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// Page droite.
    #[default]
    None,
    /// Un quart de tour (90°).
    Quarter,
    /// Un demi-tour (180°).
    Half,
    /// Trois quarts de tour (270°).
    ThreeQuarters,
}

impl Rotation {
    /// Rotation la plus proche d'un angle en degrés, quel qu'il soit.
    pub fn from_degrees(degrees: i32) -> Self {
        match (degrees as f64 / 90.0).round().rem_euclid(4.0) as u8 {
            1 => Rotation::Quarter,
            2 => Rotation::Half,
            3 => Rotation::ThreeQuarters,
            _ => Rotation::None,
        }
    }

    /// Angle en degrés : 0, 90, 180 ou 270.
    pub fn degrees(self) -> u32 {
        match self {
            Rotation::None => 0,
            Rotation::Quarter => 90,
            Rotation::Half => 180,
            Rotation::ThreeQuarters => 270,
        }
    }

    fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Quarter | Rotation::ThreeQuarters)
    }
}

/// Zone d'une image de page, en pixels, origine en haut à gauche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// Bord gauche.
    pub x: u32,
    /// Bord supérieur.
    pub y: u32,
    /// Largeur.
    pub width: u32,
    /// Hauteur.
    pub height: u32,
}

/// Taille en pixels de l'image complète d'une page, rotation d'affichage comprise.
pub fn image_size(size: PageSize, scale: f32, rotation: Rotation) -> (u32, u32) {
    // Même arrondi que MuPDF : un pixel entamé de plus d'un millième compte.
    let pixels = |points: f32| ((points * scale - 0.001).ceil() as u32).max(1);
    let (width, height) = (pixels(size.width), pixels(size.height));
    if rotation.swaps_axes() {
        (height, width)
    } else {
        (width, height)
    }
}

/// Rend une page entière à l'échelle donnée (1,0 = 72 pixels par pouce).
pub fn render(content: &PageContent, scale: f32, rotation: Rotation) -> Result<Bitmap> {
    check_scale(scale)?;
    let (width, height) = image_size(content.size(), scale, rotation);
    let region = Region {
        x: 0,
        y: 0,
        width,
        height,
    };
    render_region(content, scale, rotation, region)
}

/// Rend une page pour une résolution exprimée en points par pouce.
pub fn render_at_dpi(content: &PageContent, dpi: f32) -> Result<Bitmap> {
    render(content, dpi / 72.0, Rotation::None)
}

/// Rend une zone de l'image d'une page : utile pour les tuiles des forts
/// grossissements, où l'image complète serait trop grande.
///
/// La zone est exprimée dans l'image complète à cette échelle et cette rotation
/// (cf. [`image_size`]) ; la partie qui en déborde est ignorée.
pub fn render_region(
    content: &PageContent,
    scale: f32,
    rotation: Rotation,
    region: Region,
) -> Result<Bitmap> {
    check_scale(scale)?;
    let (full_width, full_height) = image_size(content.size(), scale, rotation);
    let width = region.width.min(full_width.saturating_sub(region.x));
    let height = region.height.min(full_height.saturating_sub(region.y));
    if width == 0 || height == 0 {
        return Err(engine_error(format!(
            "zone vide ou hors de la page : {region:?} pour une image de {full_width}×{full_height}"
        )));
    }
    if width.max(height) > MAX_DIMENSION {
        return Err(engine_error(format!(
            "image trop grande : {width}×{height} pixels"
        )));
    }

    let list = content.display_list();
    let ctm = page_transform(content.size(), list.bounds(), scale, rotation);
    let mut pixmap = mupdf::Pixmap::new(
        &mupdf::Colorspace::device_rgb(),
        region.x as i32,
        region.y as i32,
        width as i32,
        height as i32,
        true,
    )?;
    pixmap.clear_with(0xff)?;
    {
        // Le périphérique doit être fermé (libéré) avant de lire les pixels.
        let device = mupdf::Device::from_pixmap(&pixmap)?;
        let area = mupdf::Rect::new(
            region.x as f32,
            region.y as f32,
            (region.x + width) as f32,
            (region.y + height) as f32,
        );
        list.run(&device, &ctm, area)?;
    }
    Ok(to_bitmap(&pixmap))
}

fn check_scale(scale: f32) -> Result<()> {
    if scale.is_finite() && scale > 0.0 && scale <= MAX_SCALE {
        Ok(())
    } else {
        Err(engine_error(format!("échelle de rendu invalide : {scale}")))
    }
}

/// Matrice qui place la page à l'origine, la tourne puis la met à l'échelle.
fn page_transform(
    size: PageSize,
    bounds: mupdf::Rect,
    s: f32,
    rotation: Rotation,
) -> mupdf::Matrix {
    let (w, h) = (size.width, size.height);
    // x' = a·x + c·y + e ; y' = b·x + d·y + f
    let (a, b, c, d, e, f) = match rotation {
        Rotation::None => (s, 0.0, 0.0, s, 0.0, 0.0),
        Rotation::Quarter => (0.0, s, -s, 0.0, s * h, 0.0),
        Rotation::Half => (-s, 0.0, 0.0, -s, s * w, s * h),
        Rotation::ThreeQuarters => (0.0, -s, s, 0.0, 0.0, s * w),
    };
    // Les coordonnées de la page commencent en principe à (0, 0) ; on recale sinon.
    let (x0, y0) = (bounds.x0, bounds.y0);
    mupdf::Matrix::new(a, b, c, d, e - a * x0 - c * y0, f - b * x0 - d * y0)
}

fn to_bitmap(pixmap: &mupdf::Pixmap) -> Bitmap {
    let width = pixmap.width();
    let height = pixmap.height();
    let row_bytes = width as usize * 4;
    let stride = pixmap.stride() as usize;
    let samples = pixmap.samples();
    let pixels = if stride == row_bytes {
        samples[..row_bytes * height as usize].to_vec()
    } else {
        samples
            .chunks_exact(stride)
            .take(height as usize)
            .flat_map(|row| &row[..row_bytes])
            .copied()
            .collect()
    };
    Bitmap {
        width,
        height,
        bytes_per_row: width * 4,
        pixels,
    }
}

fn engine_error(detail: String) -> PdfError {
    PdfError::Engine { detail }
}
