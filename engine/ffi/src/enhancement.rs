//! Mode Amélioration : redressement des pages scannées (cf.
//! `docs/AMELIORATION.md`).
//!
//! Chaque page est d'abord améliorée sans toucher au document
//! ([`EditSession::enhance_page`]) : le résultat garde l'image améliorée et
//! se montre en aperçu, à côté de la page d'origine. Les pages retenues
//! forment ensuite un document source de plus ([`EditSession::enhanced_document`],
//! que l'application ajoute à la session comme un document inséré), puis
//! remplacent les pages d'origine en une étape d'annulation
//! ([`EditSession::apply_enhancements`]).

use std::sync::{Arc, Mutex};

use edit::{Enhancement, PageOrigin};
use enhance::{Orientation, Settings, StraightenSettings, Verdict};
use ocr::{GrayImage, OcrEngine, OcrError};
use pdf_core::EnhancedImage;

use crate::recognition::{Foreign, TextRecognizer};
use crate::{EditSession, EngineError, PdfDocument, RenderedPage};

/// Réglages du mode Amélioration.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EnhancementSettings {
    /// Corriger l'inclinaison des pages.
    pub deskew: bool,
    /// Remettre à l'endroit les pages couchées ou à l'envers.
    pub orientation: bool,
    /// Inclinaison sous laquelle une page est laissée telle quelle, en degrés.
    pub min_angle: f32,
    /// Plafond de la correction, en degrés : au-delà, la page est « à
    /// vérifier ».
    pub max_angle: f32,
    /// Qualité de l'image améliorée (JPEG, de 50 à 100).
    pub jpeg_quality: u8,
}

impl From<&EnhancementSettings> for Settings {
    fn from(settings: &EnhancementSettings) -> Self {
        Settings {
            straighten: StraightenSettings {
                deskew: settings.deskew,
                orientation: settings.orientation,
                min_degrees: degrees(settings.min_angle),
                max_degrees: degrees(settings.max_angle),
            },
            jpeg_quality: settings.jpeg_quality,
        }
        .bounded()
    }
}

/// Angle de l'application en degrés, au dix-millième (0,1 et non
/// 0,10000000149 une fois passé en `f64`, dans les réglages retenus).
fn degrees(angle: f32) -> f64 {
    (f64::from(angle) * 1e4).round() / 1e4
}

impl From<&Settings> for EnhancementSettings {
    fn from(settings: &Settings) -> Self {
        EnhancementSettings {
            deskew: settings.straighten.deskew,
            orientation: settings.straighten.orientation,
            min_angle: settings.straighten.min_degrees as f32,
            max_angle: settings.straighten.max_degrees as f32,
            jpeg_quality: settings.jpeg_quality,
        }
    }
}

/// Réglages par défaut : redressement et orientation, seuil de 0,1°, plafond
/// de 15°.
#[uniffi::export]
pub fn default_enhancement_settings() -> EnhancementSettings {
    (&Settings::default()).into()
}

/// Réglages au format JSON, pour les retenir.
#[uniffi::export]
pub fn enhancement_settings_to_json(settings: EnhancementSettings) -> String {
    Settings::from(&settings).to_json()
}

/// Réglages retenus au format JSON ; les réglages par défaut s'ils sont
/// illisibles.
#[uniffi::export]
pub fn enhancement_settings_from_json(json: String) -> EnhancementSettings {
    (&Settings::from_json(&json).unwrap_or_default()).into()
}

/// Conclusion du redressement d'une page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StraighteningStatus {
    /// Déjà droite (inclinaison sous le seuil).
    Straight,
    /// Inclinaison corrigée.
    Corrected,
    /// Inclinaison au-delà du plafond : non redressée, à vérifier.
    ToCheck,
    /// Pas assez de texte pour mesurer l'inclinaison : non redressée.
    Unmeasurable,
    /// Correction de l'inclinaison désactivée.
    Disabled,
    /// Pas un scan pleine page (PDF natif, page vierge) : laissée telle quelle.
    NotScanned,
    /// Page déjà retouchée dans la session : laissée telle quelle.
    Retouched,
}

/// Comment le sens d'une page a été établi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OrientationStatus {
    /// Par la forme des lignes de texte.
    Sure,
    /// Par la confiance de la lecture dans chaque sens.
    Read,
    /// Incertain : la page garde son sens.
    Doubtful,
    /// Correction de l'orientation désactivée, ou sans objet.
    Disabled,
}

/// Bilan de l'amélioration d'une page.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct EnhancementReport {
    /// Identifiant de la page.
    pub page: u64,
    /// Conclusion sur l'inclinaison.
    pub status: StraighteningStatus,
    /// Comment le sens de la page a été établi.
    pub orientation: OrientationStatus,
    /// Quarts de tour (sens horaire) qui remettent la page à l'endroit,
    /// sans la rotation de l'organisation ; 0 si elle l'est.
    pub quarter_turns: u8,
    /// Inclinaison mesurée, en degrés (positive : les lignes descendent vers
    /// la droite) ; absente si elle n'a pas pu l'être.
    pub measured_angle: Option<f32>,
    /// Inclinaison corrigée, en degrés (0 : aucune).
    pub corrected_angle: f32,
    /// Vrai si l'image de la page est remplacée.
    pub changes_pixels: bool,
}

/// Une page améliorée, pas encore appliquée au document.
#[derive(uniffi::Object)]
pub struct EnhancedPage {
    report: EnhancementReport,
    /// Image améliorée ; `None` si les pixels ne changent pas.
    image: Option<EnhancedImage>,
    /// Page d'origine : document source et indice (aucun pour une page
    /// vierge).
    original: Option<(Arc<PdfDocument>, u32)>,
    /// Page améliorée seule, ouverte pour l'aperçu à la première demande.
    preview: Mutex<Option<Arc<PdfDocument>>>,
}

#[uniffi::export]
impl EnhancedPage {
    /// Bilan de l'amélioration.
    pub fn report(&self) -> EnhancementReport {
        self.report
    }

    /// Rend la page améliorée (comme `PdfDocument::render_page`) ; la page
    /// d'origine si ses pixels ne changent pas.
    pub fn render(&self, scale: f32, rotation: i32) -> Result<RenderedPage, EngineError> {
        let Some(image) = &self.image else {
            return self.render_original(scale, rotation);
        };
        let document = {
            let mut preview = self
                .preview
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match preview.as_ref() {
                Some(document) => Arc::clone(document),
                None => {
                    let bytes = pdf_core::write_enhanced_pages(&[image])?;
                    let document = PdfDocument::from_data(bytes, None)?;
                    *preview = Some(Arc::clone(&document));
                    document
                }
            }
        };
        document.render_page(0, scale, rotation)
    }

    /// Rend la page d'origine, sans les modifications de la session (une
    /// page modifiée n'est pas améliorée).
    pub fn render_original(&self, scale: f32, rotation: i32) -> Result<RenderedPage, EngineError> {
        match &self.original {
            Some((document, index)) => document.render_page(*index, scale, rotation),
            None => Err(EngineError::Failed {
                message: "Cette page n'a pas d'aperçu.".to_owned(),
                detail: "page vierge".to_owned(),
            }),
        }
    }
}

impl EnhancedPage {
    /// Page laissée telle quelle.
    fn unchanged(
        page: u64,
        status: StraighteningStatus,
        original: Option<(Arc<PdfDocument>, u32)>,
    ) -> Self {
        EnhancedPage {
            report: EnhancementReport {
                page,
                status,
                orientation: OrientationStatus::Disabled,
                quarter_turns: 0,
                measured_angle: None,
                corrected_angle: 0.0,
                changes_pixels: false,
            },
            image: None,
            original,
            preview: Mutex::new(None),
        }
    }
}

#[uniffi::export]
impl EditSession {
    /// Améliore la page `page` selon `settings`, sans modifier le document :
    /// orientation et inclinaison mesurées sur l'image de son scan, image
    /// redressée si elle doit l'être. Avec `recognizer`, une page dont le
    /// sens reste douteux est lue dans les deux sens. Une page qui n'est pas
    /// un scan pleine page, ou dont le contenu a été modifié dans la session,
    /// est laissée telle quelle.
    pub fn enhance_page(
        &self,
        page: u64,
        settings: EnhancementSettings,
        recognizer: Option<Arc<dyn TextRecognizer>>,
    ) -> Result<Arc<EnhancedPage>, EngineError> {
        let (document, index, edits) = self.page_source(page)?;
        let Some(document) = document else {
            return Ok(Arc::new(EnhancedPage::unchanged(
                page,
                StraighteningStatus::NotScanned,
                None,
            )));
        };
        let original = Some((Arc::clone(&document), index));
        if edits.is_some_and(|edits| !edits.is_empty()) {
            return Ok(Arc::new(EnhancedPage::unchanged(
                page,
                StraighteningStatus::Retouched,
                original,
            )));
        }
        let Some(scan) = document.handle.scan_pixels(index)? else {
            return Ok(Arc::new(EnhancedPage::unchanged(
                page,
                StraighteningStatus::NotScanned,
                original,
            )));
        };
        let settings = Settings::from(&settings);
        let gray = GrayImage::from_rgb(
            scan.image.width as u32,
            scan.image.height as u32,
            &scan.image.data,
        );
        let gray = if scan.display_turns == 0 {
            gray
        } else {
            gray.rotate_quarters(scan.display_turns).0
        };
        let foreign = recognizer.map(Foreign);
        let engine = foreign.as_ref().map(|engine| engine as &dyn OcrEngine);
        let straightening = enhance::analyze(&gray, scan.dpi, &settings.straighten, engine)
            .map_err(failed_reading)?;
        drop(gray);

        // La rotation tourne les pixels de la page droite : le quart de tour
        // de `/Rotate` ne change pas le sens d'une petite rotation.
        let image = if straightening.changes_pixels() {
            // Bords que la page inclinée laisse au couvercle du scanner.
            let (w, h) = (scan.image.width, scan.image.height);
            let tilt = straightening.correction_degrees.abs().to_radians().sin();
            let margin = (w.max(h) as f64 * tilt).ceil() as usize + 2;
            let paper = enhance::Paper::estimate(&scan.image, margin);
            let straight = enhance::rotate(&scan.image, -straightening.correction_degrees, &paper);
            let encoded = enhance::encode(&straight, settings.jpeg_quality).map_err(|err| {
                EngineError::Failed {
                    message: "L'image redressée n'a pas pu être enregistrée.".to_owned(),
                    detail: err.to_string(),
                }
            })?;
            Some(EnhancedImage {
                jpeg: encoded.data,
                width: encoded.width,
                height: encoded.height,
                gray: encoded.gray,
                geometry: scan.geometry,
            })
        } else {
            None
        };
        let report = EnhancementReport {
            page,
            status: match straightening.verdict {
                Verdict::Straight => StraighteningStatus::Straight,
                Verdict::Corrected => StraighteningStatus::Corrected,
                Verdict::ToCheck => StraighteningStatus::ToCheck,
                Verdict::Unmeasurable => StraighteningStatus::Unmeasurable,
                Verdict::Disabled => StraighteningStatus::Disabled,
            },
            orientation: match straightening.orientation {
                Orientation::Sure => OrientationStatus::Sure,
                Orientation::Read => OrientationStatus::Read,
                Orientation::Doubtful => OrientationStatus::Doubtful,
                Orientation::Disabled => OrientationStatus::Disabled,
            },
            quarter_turns: straightening.quarter_turns,
            measured_angle: straightening.measured_degrees.map(|d| d as f32),
            corrected_angle: straightening.correction_degrees as f32,
            changes_pixels: image.is_some(),
        };
        Ok(Arc::new(EnhancedPage {
            report,
            image,
            original,
            preview: Mutex::new(None),
        }))
    }

    /// Document formé des pages améliorées de `pages` dont l'image change,
    /// dans cet ordre ; `None` s'il n'y en a aucune. L'application l'ajoute
    /// aux sources de la session (`add_source`) avant d'appliquer les pages
    /// ([`EditSession::apply_enhancements`]), comme un document inséré.
    pub fn enhanced_document(
        &self,
        pages: Vec<Arc<EnhancedPage>>,
    ) -> Result<Option<Arc<PdfDocument>>, EngineError> {
        let images: Vec<&EnhancedImage> = pages.iter().filter_map(|p| p.image.as_ref()).collect();
        if images.is_empty() {
            return Ok(None);
        }
        let bytes = pdf_core::write_enhanced_pages(&images)?;
        PdfDocument::from_data(bytes, None).map(Some)
    }

    /// Applique les pages améliorées `pages` en une étape d'annulation
    /// (« l'amélioration des pages ») : les pages dont l'image change
    /// deviennent celles du document source `source`
    /// ([`EditSession::enhanced_document`], dans le même ordre), et les pages
    /// couchées ou retournées sont remises à l'endroit. Les pages supprimées
    /// entre-temps sont ignorées.
    pub fn apply_enhancements(
        &self,
        pages: Vec<Arc<EnhancedPage>>,
        source: Option<u32>,
    ) -> Result<(), EngineError> {
        let changed = pages.iter().filter(|page| page.image.is_some()).count() as u32;
        if changed > 0 {
            let count = source.and_then(|source| self.source_page_count(source));
            if count != Some(changed) {
                return Err(EngineError::Failed {
                    message: "Les pages améliorées n'ont pas pu être appliquées.".to_owned(),
                    detail: "document des pages améliorées absent ou incomplet".to_owned(),
                });
            }
        }
        let mut next = 0;
        let results: Vec<Enhancement> = pages
            .iter()
            .map(|page| {
                let origin = page.image.as_ref().zip(source).map(|(_, source)| {
                    next += 1;
                    PageOrigin::Source {
                        source,
                        index: next - 1,
                    }
                });
                Enhancement {
                    page: page.report.page,
                    origin,
                    quarter_turns: page.report.quarter_turns % 4,
                }
            })
            .collect();
        self.with_editor(|editor| editor.enhance(&results));
        Ok(())
    }

    /// Rend aux pages améliorées désignées leur page d'origine, avec sa
    /// rotation, en une étape d'annulation (« le retour aux pages
    /// d'origine »).
    pub fn restore_original_pages(&self, pages: Vec<u64>) -> Result<(), EngineError> {
        Ok(self.with_editor(|editor| editor.restore_originals(&pages))?)
    }
}

/// Erreur de la lecture qui départage le sens d'une page.
fn failed_reading(err: OcrError) -> EngineError {
    EngineError::Failed {
        message: "Le sens de la page n'a pas pu être établi.".to_owned(),
        detail: err.to_string(),
    }
}
