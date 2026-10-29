//! Modifications du document et historique d'annulation : organisation des
//! pages, contenu de chaque page (texte retouché des pages scannées et
//! objets ajoutés compris) et couche de texte des pages reconnues.
//!
//! Ce crate ne dépend pas de MuPDF : il décrit les modifications, que
//! `pdf_core` applique au contenu des pages pour l'affichage et au fichier lors
//! de l'enregistrement.
#![forbid(unsafe_code)]

mod added;
mod content;
mod editor;
mod history;
mod layer;
mod scan;

pub use added::{is_added_id, AddedImage, AddedObject, AddedText, ADDED_ID};
pub use content::{FontChoice, ObjectEdit, PageEdits, TableEdit, TextReplacement};
pub use editor::{
    EditError, Editor, Enhancement, PageEntry, PageOrigin, Recognition, ReplacedPage, RestoredPage,
    Result,
};
pub use history::{Command, History};
pub use layer::{LayerWord, TextLayer};
pub use scan::{ScanCandidate, ScanStyle, ScanText};
