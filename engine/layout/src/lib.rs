//! Détection de la structure des pages : filets, quadrillages et tableaux,
//! et réorganisation d'un tableau (lignes et colonnes supprimées ou
//! déplacées).
//!
//! Ce crate ne dépend ni de MuPDF ni du format PDF : il travaille sur des
//! segments (traits, bords de rectangles) et sur l'emprise des glyphes, dans
//! l'espace de la page (ordonnées vers le haut). `pdf_core` les tire du flux
//! de contenu des pages natives ; pour un scan, ils pourront venir de l'image
//! et de la reconnaissance du texte.
#![forbid(unsafe_code)]

pub mod geometry;
mod lattice;
pub mod restructure;
pub mod rulings;
mod table;

pub use geometry::{Rect, Segment, TextBox};
pub use restructure::{keeps_merged_cells, Axis, Placement, Restructure};
pub use table::{detect, Cell, Table, TableKind};
