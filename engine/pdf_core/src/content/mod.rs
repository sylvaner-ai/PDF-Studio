//! Contenu des pages : lecture du flux, interprétation, objets éditables et
//! réécriture après modification.
//!
//! Ce module ne dépend pas de MuPDF, sauf pour lire les ressources de la page
//! (`resources`) : son cœur se teste sur des flux écrits à la main.

pub mod cmap;
pub mod encodings;
pub mod font;
pub mod fontprog;
pub mod geometry;
pub mod interp;
pub mod layout;
pub mod lexer;
pub mod objects;
pub mod resources;
pub mod rewrite;
pub mod writer;
