//! Modifications du document et historique d'annulation : organisation des
//! pages et contenu de chaque page.
//!
//! Ce crate ne dépend pas de MuPDF : il décrit les modifications, que
//! `pdf_core` applique au contenu des pages pour l'affichage et au fichier lors
//! de l'enregistrement.
#![forbid(unsafe_code)]

mod content;
mod editor;
mod history;

pub use content::{FontChoice, ObjectEdit, PageEdits, TextReplacement};
pub use editor::{EditError, Editor, PageEntry, PageOrigin, RestoredPage, Result};
pub use history::{Command, History};
