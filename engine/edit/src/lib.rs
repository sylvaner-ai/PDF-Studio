//! Modifications du document : organisation des pages et historique
//! d'annulation.
//!
//! Ce crate ne dépend pas de MuPDF : il décrit les modifications, que
//! `pdf_core` applique au fichier lors de l'enregistrement. L'historique
//! ([`History`]) servira aussi à l'édition du contenu (jalon 4).
#![forbid(unsafe_code)]

mod history;
mod pages;

pub use history::{Command, History};
pub use pages::{EditError, Organizer, PageEntry, PageOrigin, Result};
