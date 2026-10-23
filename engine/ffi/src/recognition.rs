//! Reconnaissance du texte des pages scannées.
//!
//! Le moteur de reconnaissance est fourni par l'application : Swift implémente
//! le trait [`TextRecognizer`] avec le framework Vision d'Apple (cf.
//! `docs/OCR_BENCHMARK.md`). Le moteur Rust prépare l'image (rendu, fond,
//! inclinaison, orientation), lui transmet, puis convertit les mots lus en
//! couche de texte dans l'espace de la page PDF.

use std::sync::Arc;

use edit::{LayerWord, TextLayer};
use ocr::{GrayImage, OcrEngine, OcrError, Point, Quad, RecognizedWord};
use pdf_core::content::geometry::{Matrix, Point as PagePoint};
use pdf_core::{DocumentHandle, PageRect as CoreRect, RecognitionNeed, TextLine};

use crate::{EngineError, PageRect};

/// Confiance au-dessous de laquelle un mot est signalé comme douteux.
pub const DOUBTFUL_BELOW: f32 = 0.5;

/// Plus grand côté de l'image d'une page soumise à la reconnaissance, en
/// pixels : la résolution est réduite pour les très grands formats.
const MAX_IMAGE_SIDE: f32 = 8000.0;

/// Image d'une page en niveaux de gris, 8 bits par pixel, sans marge.
#[derive(Debug, Clone, uniffi::Record)]
pub struct OcrImage {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Pixels, rangée par rangée (0 : noir, 255 : blanc).
    pub pixels: Vec<u8>,
}

/// Un mot lu par un moteur de reconnaissance, dans l'image reçue.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RecognizedText {
    /// Texte du mot, sans espace.
    pub text: String,
    /// Confiance, de 0 à 1.
    pub confidence: f32,
    /// Numéro de ligne, commun aux mots d'une même ligne.
    pub line: u32,
    /// Coins supérieur gauche, supérieur droit, inférieur droit et inférieur
    /// gauche (x, y pour chacun), en pixels depuis le haut à gauche.
    pub quad: Vec<f32>,
}

/// Moteur de reconnaissance de texte fourni par l'application.
#[uniffi::export(with_foreign)]
pub trait TextRecognizer: Send + Sync {
    /// Nom du moteur (« Vision »).
    fn name(&self) -> String;

    /// Lit le texte de `image`, dans les langues `languages` (« fr-FR »…).
    fn recognize(
        &self,
        image: OcrImage,
        languages: Vec<String>,
    ) -> Result<Vec<RecognizedText>, EngineError>;
}

impl From<uniffi::UnexpectedUniFFICallbackError> for EngineError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        EngineError::Failed {
            message: "La reconnaissance du texte a échoué.".to_owned(),
            detail: err.reason,
        }
    }
}

/// Le moteur de l'application, vu comme un [`OcrEngine`].
struct Foreign(Arc<dyn TextRecognizer>);

impl OcrEngine for Foreign {
    fn name(&self) -> String {
        self.0.name()
    }

    fn recognize(
        &self,
        image: &GrayImage,
        languages: &[String],
    ) -> Result<Vec<RecognizedWord>, OcrError> {
        let image = OcrImage {
            width: image.width,
            height: image.height,
            pixels: image.pixels.clone(),
        };
        let words = self
            .0
            .recognize(image, languages.to_vec())
            .map_err(|err| OcrError::Failed(err.to_string()))?;
        Ok(words
            .into_iter()
            .filter(|word| word.quad.len() == 8 && !word.text.trim().is_empty())
            .map(|word| {
                let p = |i: usize| Point {
                    x: word.quad[i],
                    y: word.quad[i + 1],
                };
                RecognizedWord {
                    text: word.text.trim().to_owned(),
                    quad: Quad {
                        top_left: p(0),
                        top_right: p(2),
                        bottom_right: p(4),
                        bottom_left: p(6),
                    },
                    confidence: word.confidence.clamp(0.0, 1.0),
                    line: word.line,
                }
            })
            .collect())
    }
}

/// Réglages de la reconnaissance.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecognitionSettings {
    /// Langues attendues, par ordre de préférence (« fr-FR », « en-US »).
    pub languages: Vec<String>,
    /// Résolution de l'image soumise au moteur, en pixels par pouce.
    pub dpi: f32,
    /// Égaliser le fond et étirer le contraste.
    pub flatten: bool,
    /// Filtre médian contre le bruit ponctuel.
    pub denoise: bool,
    /// Binariser l'image.
    pub binarize: bool,
    /// Redresser une page inclinée.
    pub deskew: bool,
    /// Lire une page couchée ou à l'envers, et la remettre à l'endroit.
    pub orientation: bool,
}

/// Réglages retenus par le banc d'essai pour Vision (cf.
/// `docs/OCR_BENCHMARK.md`).
#[uniffi::export]
pub fn default_recognition_settings() -> RecognitionSettings {
    RecognitionSettings {
        languages: vec!["fr-FR".to_owned(), "en-US".to_owned()],
        dpi: 300.0,
        flatten: true,
        denoise: false,
        binarize: false,
        deskew: true,
        orientation: true,
    }
}

/// Un mot de la couche de texte, dans l'espace de la page PDF.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LayerWordItem {
    /// Texte du mot.
    pub text: String,
    /// Coins inférieur gauche, inférieur droit, supérieur droit et supérieur
    /// gauche (x, y), en points, ordonnées vers le haut, sans rotation.
    pub quad: Vec<f32>,
    /// Confiance, de 0 à 1.
    pub confidence: f32,
    /// Numéro de ligne.
    pub line: u32,
}

impl From<&LayerWord> for LayerWordItem {
    fn from(word: &LayerWord) -> Self {
        LayerWordItem {
            text: word.text.clone(),
            quad: word.quad.to_vec(),
            confidence: word.confidence,
            line: word.line,
        }
    }
}

/// Couche de texte formée de mots retrouvés (récupération) ; les mots mal
/// formés sont ignorés.
pub(crate) fn layer_from_items(items: &[LayerWordItem], engine: &str) -> TextLayer {
    TextLayer {
        words: items
            .iter()
            .filter_map(|item| {
                let quad: [f32; 8] = item.quad.as_slice().try_into().ok()?;
                quad.iter().all(|v| v.is_finite()).then(|| LayerWord {
                    text: item.text.clone(),
                    quad,
                    confidence: item.confidence.clamp(0.0, 1.0),
                    line: item.line,
                })
            })
            .collect(),
        engine: engine.to_owned(),
    }
}

/// Résultat de la reconnaissance d'une page, à appliquer au document par
/// `EditSession::apply_recognitions`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PageRecognition {
    /// Identifiant de la page.
    pub page: u64,
    /// Mots reconnus, ligne après ligne.
    pub words: Vec<LayerWordItem>,
    /// Quarts de tour (sens horaire) qui remettent la page à l'endroit ;
    /// 0 si elle l'est déjà.
    pub quarter_turns: u8,
    /// Nom du moteur.
    pub engine: String,
    /// Confiance moyenne, pondérée par la longueur des mots.
    pub mean_confidence: f32,
    /// Nombre de mots douteux (confiance inférieure au seuil).
    pub doubtful_words: u32,
}

impl PageRecognition {
    pub(crate) fn empty(page: u64, engine: String) -> Self {
        PageRecognition {
            page,
            words: Vec::new(),
            quarter_turns: 0,
            engine,
            mean_confidence: 0.0,
            doubtful_words: 0,
        }
    }
}

/// Un mot reconnu, tel que l'application le signale sur la page.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RecognizedWordItem {
    /// Texte lu.
    pub text: String,
    /// Emprise sur la page droite (sans les quarts de tour de l'organisation),
    /// en points, origine en haut à gauche.
    pub bounds: PageRect,
    /// Confiance, de 0 à 1.
    pub confidence: f32,
    /// Le mot est douteux : confiance inférieure au seuil.
    pub doubtful: bool,
}

/// Emprise d'un mot de la couche dans l'espace d'affichage de la page.
pub(crate) fn display_bounds(word: &LayerWord, to_display: &Matrix) -> PageRect {
    let corners = [
        word.bottom_left(),
        word.bottom_right(),
        word.top_right(),
        word.top_left(),
    ]
    .map(|(x, y)| to_display.apply(PagePoint::new(f64::from(x), f64::from(y))));
    let x0 = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let y0 = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let x1 = corners
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let y1 = corners
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);
    PageRect {
        x: x0 as f32,
        y: y0 as f32,
        width: (x1 - x0) as f32,
        height: (y1 - y0) as f32,
    }
}

/// Lit la page `index` de `document`, modifications `edits` comprises.
pub(crate) fn recognize(
    page: u64,
    handle: &DocumentHandle,
    index: u32,
    edits: Option<Arc<edit::PageEdits>>,
    recognizer: Arc<dyn TextRecognizer>,
    settings: &RecognitionSettings,
) -> Result<PageRecognition, EngineError> {
    let engine = Foreign(recognizer);
    let name = engine.name();
    let content = match edits {
        Some(edits) if !edits.is_empty() => handle.edited_page_content(index, edits, None)?,
        _ => handle.page_content(index)?,
    };
    let size = content.size();
    let largest = size.width.max(size.height).max(1.0);
    let dpi = settings.dpi.min(MAX_IMAGE_SIDE * 72.0 / largest).max(72.0);
    let bitmap = render::render_at_dpi(&content, dpi)?;
    let image = GrayImage::from_rgba(
        bitmap.width,
        bitmap.height,
        bitmap.bytes_per_row,
        &bitmap.pixels,
    );
    drop(bitmap);
    let options = ocr::Options {
        dpi,
        languages: settings.languages.clone(),
        flatten: settings.flatten,
        denoise: settings.denoise,
        binarize: settings.binarize,
        deskew: settings.deskew,
        orientation: settings.orientation,
    };
    let result = ocr::recognize(&engine, &image, &options).map_err(|err| match err {
        OcrError::Cancelled => EngineError::Failed {
            message: "La reconnaissance du texte a été interrompue.".to_owned(),
            detail: err.to_string(),
        },
        other => EngineError::Failed {
            message: "La reconnaissance du texte a échoué.".to_owned(),
            detail: other.to_string(),
        },
    })?;

    // Le texte invisible d'un OCR précédent est remplacé par cette lecture :
    // il n'écarte aucun mot.
    let native = match handle.recognition_need(index)? {
        RecognitionNeed::Replaceable => Vec::new(),
        _ => handle.text_lines(index)?,
    };
    let to_display = handle.display_matrix(index)?;
    let to_page = to_display.inverse().ok_or_else(|| EngineError::Failed {
        message: "La reconnaissance du texte a échoué.".to_owned(),
        detail: "page dégénérée".to_owned(),
    })?;
    let scale = f64::from(dpi) / 72.0;
    let to_points = |p: Point| PagePoint::new(f64::from(p.x) / scale, f64::from(p.y) / scale);

    let mut words = Vec::with_capacity(result.words.len());
    let mut lines: Vec<u32> = Vec::new();
    for word in &result.words {
        let center = to_points(word.quad.center());
        if covers_native_text(&native, center) {
            continue;
        }
        let corner = |p: Point| {
            let q = to_page.apply(to_points(p));
            [q.x as f32, q.y as f32]
        };
        let [blx, bly] = corner(word.quad.bottom_left);
        let [brx, bry] = corner(word.quad.bottom_right);
        let [trx, try_] = corner(word.quad.top_right);
        let [tlx, tly] = corner(word.quad.top_left);
        let line = match lines.iter().position(|&known| known == word.line) {
            Some(position) => position as u32,
            None => {
                lines.push(word.line);
                (lines.len() - 1) as u32
            }
        };
        words.push(LayerWordItem {
            text: word.text.clone(),
            quad: vec![blx, bly, brx, bry, trx, try_, tlx, tly],
            confidence: word.confidence,
            line,
        });
    }
    let (weighted, chars) = words.iter().fold((0f32, 0usize), |(w, n), word| {
        let len = word.text.chars().count();
        (w + word.confidence * len as f32, n + len)
    });
    Ok(PageRecognition {
        page,
        doubtful_words: words
            .iter()
            .filter(|word| word.confidence < DOUBTFUL_BELOW)
            .count() as u32,
        mean_confidence: if chars == 0 {
            0.0
        } else {
            weighted / chars as f32
        },
        words,
        quarter_turns: result.quarter_turns,
        engine: name,
    })
}

/// Vrai si `point` (espace d'affichage) tombe dans une ligne de texte natif :
/// le mot reconnu doublerait un texte déjà cherchable.
fn covers_native_text(native: &[TextLine], point: PagePoint) -> bool {
    const MARGIN: f32 = 1.0;
    let (x, y) = (point.x as f32, point.y as f32);
    native.iter().any(|line| {
        let CoreRect {
            x: x0,
            y: y0,
            width,
            height,
        } = line.rect;
        x >= x0 - MARGIN
            && x <= x0 + width + MARGIN
            && y >= y0 - MARGIN
            && y <= y0 + height + MARGIN
    })
}
