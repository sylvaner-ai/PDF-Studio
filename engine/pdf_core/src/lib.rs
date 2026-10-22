//! Cœur du moteur PDF Studio : ouverture des documents, pages, métadonnées,
//! plan, texte et écriture des documents réorganisés.
//!
//! Le moteur s'appuie sur MuPDF (cf. `docs/DECISIONS.md`). Aucune ligne `unsafe`
//! n'est autorisée dans ce crate.
#![forbid(unsafe_code)]

mod document;
mod error;
mod fonts;
mod outline;
mod page;
mod qos;
mod service;
mod text;
mod writer;

pub use document::Document;
pub use error::{PdfError, Result};
pub use outline::OutlineEntry;
pub use page::{PageContent, PageSize};
pub use qos::{install_qos_hooks, QosHooks};
pub use service::{DocumentHandle, Source};
pub use text::{PageRect, PageText, TextMatch};
pub use writer::{is_signed, write_pdf, OutputPage, SourceData};
