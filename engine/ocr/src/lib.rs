//! Reconnaissance optique de caractères (OCR) des pages scannées.
//!
//! Ce crate ne dépend ni de MuPDF ni d'un moteur de reconnaissance précis :
//!
//! - [`GrayImage`] et [`Affine`] décrivent l'image d'une page et les
//!   transformations qu'elle subit ;
//! - [`preprocess`] améliore l'image avant la lecture : fond égalisé et
//!   contraste, bruit, binarisation adaptative, inclinaison et orientation ;
//! - [`OcrEngine`] est le trait que chaque moteur implémente (Vision d'Apple,
//!   Tesseract, `ocrs`…) : les moteurs sont interchangeables ;
//! - [`recognize`] enchaîne prétraitement, lecture et retour des mots dans le
//!   repère de l'image d'origine ;
//! - [`metrics`] mesure le taux de caractères corrects par rapport à un texte
//!   de référence, pour le banc d'essai.
#![forbid(unsafe_code)]

mod engine;
mod image;
pub mod metrics;
mod pipeline;
pub mod preprocess;

pub use engine::{OcrEngine, OcrError, Point, Quad, RecognizedWord};
pub use image::{Affine, GrayImage};
pub use pipeline::{recognize, Options, PageOcr};
