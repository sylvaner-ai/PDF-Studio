//! Interface entre le moteur Rust et l'application SwiftUI, générée par UniFFI.
//!
//! C'est la seule couche du moteur où du code `unsafe` existe. Il est presque
//! entièrement produit par les macros UniFFI (`setup_scaffolding!`,
//! `#[uniffi::export]`), qui convertissent les types et gèrent la mémoire de part
//! et d'autre de la frontière C. Le seul code `unsafe` écrit à la main est celui
//! du module `qos` : deux appels système de macOS, documentés.

use std::path::PathBuf;
use std::sync::Arc;

use pdf_core::{DocumentHandle, PdfError, Source, SourceData};
use render::{Region, Rotation};

mod qos;
mod session;

pub use session::{EditSession, PageItem, PageKind};

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
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
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

impl From<render::Bitmap> for RenderedPage {
    fn from(bitmap: render::Bitmap) -> Self {
        RenderedPage {
            width: bitmap.width,
            height: bitmap.height,
            bytes_per_row: bitmap.bytes_per_row,
            pixels: bitmap.pixels,
        }
    }
}

/// Rectangle sur une page, en points, origine en haut à gauche de la page
/// droite (sans la rotation d'affichage).
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct PageRect {
    /// Bord gauche.
    pub x: f32,
    /// Bord supérieur.
    pub y: f32,
    /// Largeur.
    pub width: f32,
    /// Hauteur.
    pub height: f32,
}

/// Une occurrence trouvée par la recherche : un rectangle par ligne touchée.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchMatch {
    /// Rectangles à surligner, dans l'ordre de lecture.
    pub rects: Vec<PageRect>,
}

/// Entrée du plan du document (signet), en parcours en profondeur.
#[derive(Debug, Clone, uniffi::Record)]
pub struct OutlineItem {
    /// Titre affiché.
    pub title: String,
    /// Profondeur, 0 pour le premier niveau.
    pub level: u32,
    /// Page visée, à partir de 0 ; absente si le signet ne mène nulle part.
    pub page: Option<u32>,
    /// Position visée, en points depuis le haut de la page.
    pub top: Option<f32>,
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
        qos::install();
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

    /// Rend une page entière ; `scale` = 1,0 pour 72 pixels par pouce,
    /// `rotation` en degrés dans le sens horaire (arrondie au quart de tour).
    pub fn render_page(
        &self,
        index: u32,
        scale: f32,
        rotation: i32,
    ) -> Result<RenderedPage, EngineError> {
        let content = self.handle.page_content(index)?;
        Ok(render::render(&content, scale, Rotation::from_degrees(rotation))?.into())
    }

    /// Rend une zone de l'image d'une page, en pixels de l'image complète à
    /// cette échelle et cette rotation : tuiles des forts grossissements.
    #[allow(clippy::too_many_arguments)]
    pub fn render_region(
        &self,
        index: u32,
        scale: f32,
        rotation: i32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<RenderedPage, EngineError> {
        let content = self.handle.page_content(index)?;
        let region = Region {
            x,
            y,
            width,
            height,
        };
        Ok(
            render::render_region(&content, scale, Rotation::from_degrees(rotation), region)?
                .into(),
        )
    }

    /// Plan du document (signets) ; vide s'il n'en a pas.
    pub fn outline(&self) -> Result<Vec<OutlineItem>, EngineError> {
        Ok(self
            .handle
            .outline()?
            .into_iter()
            .map(|entry| OutlineItem {
                title: entry.title,
                level: entry.level,
                page: entry.page,
                top: entry.top,
            })
            .collect())
    }

    /// Texte brut d'une page, dans l'ordre de lecture ; vide pour une page scannée.
    pub fn plain_text(&self, index: u32) -> Result<String, EngineError> {
        Ok(self.handle.plain_text(index)?)
    }

    /// Cherche `query` dans une page, sans tenir compte de la casse, des accents
    /// ni des ligatures. L'application parcourt les pages une à une, ce qui lui
    /// permet d'afficher les résultats au fil de l'eau et d'interrompre la recherche.
    pub fn search_page(&self, index: u32, query: String) -> Result<Vec<SearchMatch>, EngineError> {
        Ok(self
            .handle
            .search_page(index, &query)?
            .into_iter()
            .map(|found| SearchMatch {
                rects: found
                    .rects
                    .into_iter()
                    .map(|r| PageRect {
                        x: r.x,
                        y: r.y,
                        width: r.width,
                        height: r.height,
                    })
                    .collect(),
            })
            .collect())
    }
}

impl PdfDocument {
    /// Octets lus à l'ouverture et mot de passe, d'où part toute écriture.
    pub(crate) fn source_data(&self) -> SourceData {
        self.handle.source_data().clone()
    }
}

/// Version du moteur, affichée dans « À propos ».
#[uniffi::export]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
