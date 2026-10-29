//! Cœur du moteur PDF Studio : ouverture des documents, pages, métadonnées,
//! plan, texte et écriture des documents réorganisés.
//!
//! Le moteur s'appuie sur MuPDF (cf. `docs/DECISIONS.md`). Aucune ligne `unsafe`
//! n'est autorisée dans ce crate.
#![forbid(unsafe_code)]

mod added;
mod clip;
pub mod content;
mod document;
mod editing;
mod enhanced;
mod error;
mod fonts;
mod outline;
mod page;
mod qos;
mod scanned;
mod service;
mod text;
mod textlayer;
mod writer;

pub use added::{blank_document, image_size, is_pdf, AddedInfo};
pub use clip::Clip;
pub use document::{Document, RecognitionNeed};

/// Vrai si `id` désigne un objet d'une page scannée (pixels de son image).
pub fn is_scanned_object(id: u32) -> bool {
    scanned::is_scan_id(id)
}
pub use editing::{CellInfo, ObjectInfo, TableInfo};
pub use enhanced::{write_enhanced_pages, EnhancedImage, PageGeometry, ScanPixels};
pub use error::{PdfError, Result};
pub use outline::OutlineEntry;
pub use page::{PageContent, PageSize};
pub use qos::{install_qos_hooks, QosHooks};
pub use service::{DocumentHandle, Source, PANIC_DETAIL};
pub use text::{GlyphLine, PageRect, PageText, TextLine, TextMatch};
pub use writer::{is_signed, write_document, write_edited_pdf, write_pdf, OutputPage, SourceData};
