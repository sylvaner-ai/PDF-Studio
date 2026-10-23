//! Modifications du document et historique d'annulation : organisation des
//! pages, contenu de chaque page et couche de texte des pages reconnues.
//!
//! Ce crate ne dépend pas de MuPDF : il décrit les modifications, que
//! `pdf_core` applique au contenu des pages pour l'affichage et au fichier lors
//! de l'enregistrement.
#![forbid(unsafe_code)]

mod content;
mod editor;
mod history;
mod layer;

pub use content::{FontChoice, ObjectEdit, PageEdits, TableEdit, TextReplacement};
pub use editor::{EditError, Editor, PageEntry, PageOrigin, Recognition, RestoredPage, Result};
pub use history::{Command, History};
pub use layer::{LayerWord, TextLayer};
