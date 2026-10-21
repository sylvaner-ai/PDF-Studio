//! Interface entre le moteur Rust et l'application SwiftUI, générée par UniFFI.
//!
//! C'est la seule couche du moteur où du code `unsafe` existe : il est entièrement
//! produit par les macros UniFFI (`setup_scaffolding!`, `#[uniffi::export]`), qui
//! convertissent les types et gèrent la mémoire de part et d'autre de la frontière
//! C. Le code écrit à la main ici n'en contient pas.

use std::path::PathBuf;
use std::sync::Arc;

use pdf_core::{DocumentHandle, PdfError, Source};

uniffi::setup_scaffolding!();

/// Erreur renvoyée à l'application. `message` est affichable tel quel.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    /// Le document est protégé : l'application doit demander le mot de passe.
    #[error("{message}")]
    PasswordRequired {
        /// Message pour l'utilisateur.
        message: String,
    },
    /// Le mot de passe saisi est incorrect.
    #[error("{message}")]
    WrongPassword {
        /// Message pour l'utilisateur.
        message: String,
    },
    /// Toute autre erreur.
    #[error("{message}")]
    Failed {
        /// Message pour l'utilisateur.
        message: String,
        /// Détail technique, pour le journal.
        detail: String,
    },
}

impl From<PdfError> for EngineError {
    fn from(err: PdfError) -> Self {
        let message = err.to_string();
        match err {
            PdfError::PasswordRequired => EngineError::PasswordRequired { message },
            PdfError::WrongPassword => EngineError::WrongPassword { message },
            other => EngineError::Failed {
                detail: other.detail().unwrap_or_default().to_owned(),
                message,
            },
        }
    }
}

/// Dimensions d'une page en points.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct PageSize {
    /// Largeur en points.
    pub width: f32,
    /// Hauteur en points.
    pub height: f32,
}

/// Image d'une page rendue : pixels RGBA 8 bits, opaques.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RenderedPage {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Octets par rangée de pixels.
    pub bytes_per_row: u32,
    /// Pixels RGBA.
    pub pixels: Vec<u8>,
}

/// Document PDF ouvert. Toutes les méthodes sont bloquantes et peuvent être
/// appelées depuis n'importe quel fil ; l'application les appelle hors du fil
/// principal.
#[derive(uniffi::Object)]
pub struct PdfDocument {
    handle: DocumentHandle,
}

#[uniffi::export]
impl PdfDocument {
    /// Ouvre un fichier PDF en lecture seule. Le fichier n'est lu qu'une fois et
    /// n'est jamais modifié.
    #[uniffi::constructor]
    pub fn open(path: String, password: Option<String>) -> Result<Arc<Self>, EngineError> {
        let handle = DocumentHandle::open(Source::File(PathBuf::from(path)), password)?;
        Ok(Arc::new(Self { handle }))
    }

    /// Nombre de pages.
    pub fn page_count(&self) -> u32 {
        self.handle.page_count()
    }

    /// Dimensions de toutes les pages, dans l'ordre.
    pub fn page_sizes(&self) -> Vec<PageSize> {
        self.handle
            .page_sizes()
            .iter()
            .map(|s| PageSize {
                width: s.width,
                height: s.height,
            })
            .collect()
    }

    /// Titre déclaré dans les métadonnées du document.
    pub fn title(&self) -> Option<String> {
        self.handle.title().map(str::to_owned)
    }

    /// Rend une page ; `scale` = 1,0 pour 72 pixels par pouce.
    pub fn render_page(&self, index: u32, scale: f32) -> Result<RenderedPage, EngineError> {
        let content = self.handle.page_content(index)?;
        let bitmap = render::render(&content, scale)?;
        Ok(RenderedPage {
            width: bitmap.width,
            height: bitmap.height,
            bytes_per_row: bitmap.bytes_per_row,
            pixels: bitmap.pixels,
        })
    }
}

/// Version du moteur, affichée dans « À propos ».
#[uniffi::export]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
