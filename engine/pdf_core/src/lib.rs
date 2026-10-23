//! Cœur du moteur PDF Studio : ouverture des documents, pages, métadonnées,
//! plan, texte et écriture des documents réorganisés.
//!
//! Le moteur s'appuie sur MuPDF (cf. `docs/DECISIONS.md`). Aucune ligne `unsafe`
//! n'est autorisée dans ce crate.
#![forbid(unsafe_code)]

pub mod content;
mod document;
mod editing;
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

pub use document::Document;

/// Vrai si `id` désigne un objet d'une page scannée (pixels de son image).
pub fn is_scanned_object(id: u32) -> bool {
    scanned::is_scan_id(id)
}
pub use editing::{CellInfo, ObjectInfo, TableInfo};
pub use error::{PdfError, Result};
pub use outline::OutlineEntry;
pub use page::{PageContent, PageSize};
pub use qos::{install_qos_hooks, QosHooks};
pub use service::{DocumentHandle, Source, PANIC_DETAIL};
pub use text::{PageRect, PageText, TextLine, TextMatch};
pub use writer::{is_signed, write_document, write_edited_pdf, write_pdf, OutputPage, SourceData};
