//! Cœur du moteur PDF Studio : ouverture des documents, pages et métadonnées.
//!
//! Le moteur s'appuie sur MuPDF (cf. `docs/DECISIONS.md`). Aucune ligne `unsafe`
//! n'est autorisée dans ce crate.
#![forbid(unsafe_code)]

mod document;
mod error;
mod page;
mod service;

pub use document::Document;
pub use error::{PdfError, Result};
pub use page::{PageContent, PageSize};
pub use service::{DocumentHandle, Source};
