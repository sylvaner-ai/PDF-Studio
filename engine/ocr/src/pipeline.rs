//! Lecture d'une page : prétraitement, reconnaissance, puis retour des mots
//! dans le repère de l'image d'origine.

use crate::engine::{OcrEngine, OcrError, RecognizedWord};
use crate::image::{Affine, GrayImage};
use crate::preprocess;

/// Réglages de la lecture d'une page.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Résolution de l'image, en pixels par pouce.
    pub dpi: f32,
    /// Langues attendues, par ordre de préférence (« fr-FR », « en-US »).
    pub languages: Vec<String>,
    /// Égaliser le fond et étirer le contraste.
    pub flatten: bool,
    /// Filtre médian contre le bruit ponctuel.
    pub denoise: bool,
    /// Binariser (noir et blanc) avant la lecture.
    pub binarize: bool,
    /// Redresser une page inclinée.
    pub deskew: bool,
    /// Lire une page couchée ou à l'envers dans le bon sens.
    pub orientation: bool,
}

impl Options {
    /// Image transmise telle quelle au moteur.
    pub fn raw(dpi: f32, languages: Vec<String>) -> Self {
        Self {
            dpi,
            languages,
            flatten: false,
            denoise: false,
            binarize: false,
            deskew: false,
            orientation: false,
        }
    }

    /// Tous les traitements.
    pub fn full(dpi: f32, languages: Vec<String>) -> Self {
        Self {
            dpi,
            languages,
            flatten: true,
            denoise: true,
            binarize: true,
            deskew: true,
            orientation: true,
        }
    }
}

/// Résultat de la lecture d'une page.
#[derive(Clone, Debug, PartialEq)]
pub struct PageOcr {
    /// Mots, dans l'ordre de lecture, leur emprise exprimée dans l'image
    /// d'origine.
    pub words: Vec<RecognizedWord>,
    /// Quarts de tour, dans le sens horaire, qui remettent la page à
    /// l'endroit (0 : elle l'était).
    pub quarter_turns: u8,
    /// Inclinaison des lignes de la page remise à l'endroit, en degrés
    /// (positive : les lignes descendent vers la droite).
    pub skew_degrees: f64,
}

impl PageOcr {
    /// Confiance moyenne, pondérée par la longueur des mots ; 0 sans mot.
    pub fn mean_confidence(&self) -> f32 {
        mean_confidence(&self.words)
    }
}

/// Confiance moyenne de mots, pondérée par leur longueur.
fn mean_confidence(words: &[RecognizedWord]) -> f32 {
    let (weighted, chars) = words.iter().fold((0f32, 0usize), |(w, n), word| {
        let len = word.text.chars().count();
        (w + word.confidence * len as f32, n + len)
    });
    if chars == 0 {
        0.0
    } else {
        weighted / chars as f32
    }
}

/// Qualité d'une lecture : caractères lus, pondérés par leur confiance.
fn quality(words: &[RecognizedWord]) -> f32 {
    words
        .iter()
        .map(|word| word.confidence * word.text.chars().count() as f32)
        .sum()
}

/// Une lecture est douteuse si elle trouve très peu de mots ou des mots peu
/// sûrs : la page est peut-être à l'envers.
fn is_doubtful(words: &[RecognizedWord]) -> bool {
    words.len() < 3 || mean_confidence(words) < 0.6
}

/// Lit le texte de `image` avec `engine`.
///
/// L'image est d'abord préparée selon `options` ; une page couchée est
/// tournée d'un quart de tour, une page inclinée redressée. Si la lecture est
/// douteuse, la page est relue à l'envers et la meilleure lecture retenue.
/// Les mots sont rendus avec leur emprise dans `image`.
pub fn recognize(
    engine: &dyn OcrEngine,
    image: &GrayImage,
    options: &Options,
) -> Result<PageOcr, OcrError> {
    let mut prepared = if options.flatten {
        preprocess::flatten_background(image, options.dpi)
    } else {
        image.clone()
    };
    if options.denoise {
        prepared = preprocess::median3(&prepared);
    }

    let mut turns = 0u8;
    let mut to_source = Affine::IDENTITY;
    if options.orientation && preprocess::is_sideways(&prepared, options.dpi) {
        let (rotated, back) = prepared.rotate_quarters(1);
        prepared = rotated;
        to_source = back;
        turns = 1;
    }

    let mut skew = 0.0;
    if options.deskew {
        skew = preprocess::estimate_skew(&prepared, options.dpi);
        if skew.abs() >= 0.1 {
            let (straight, back) = prepared.rotate(-skew, 255);
            prepared = straight;
            to_source = back.then(&to_source);
        }
    }

    if options.binarize {
        prepared = preprocess::binarize(&prepared, options.dpi);
    }

    let mut words = engine.recognize(&prepared, &options.languages)?;
    if options.orientation && is_doubtful(&words) {
        let (upside_down, back) = prepared.rotate_quarters(2);
        let other = engine.recognize(&upside_down, &options.languages)?;
        if quality(&other) > quality(&words) {
            words = other
                .into_iter()
                .map(|mut word| {
                    word.quad = word.quad.map(|p| back.apply_point(p));
                    word
                })
                .collect();
            turns = (turns + 2) % 4;
        }
    }

    for word in &mut words {
        word.quad = word.quad.map(|p| to_source.apply_point(p));
    }
    Ok(PageOcr {
        words,
        quarter_turns: turns,
        skew_degrees: skew,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Quad;
    use std::cell::RefCell;

    /// Moteur factice : trouve un « mot » par bande de pixels noirs, avec une
    /// confiance élevée seulement si le premier pixel noir de la page est
    /// en haut à gauche (page à l'endroit).
    struct FakeEngine {
        sizes: RefCell<Vec<(u32, u32)>>,
    }

    impl OcrEngine for FakeEngine {
        fn name(&self) -> String {
            "factice".into()
        }

        fn recognize(
            &self,
            image: &GrayImage,
            _languages: &[String],
        ) -> Result<Vec<RecognizedWord>, OcrError> {
            self.sizes.borrow_mut().push((image.width, image.height));
            let first = image.pixels.iter().position(|&v| v < 128);
            let last = image.pixels.iter().rposition(|&v| v < 128);
            let (Some(first), Some(last)) = (first, last) else {
                return Ok(Vec::new());
            };
            let (x0, y0) = (first as u32 % image.width, first as u32 / image.width);
            let (x1, y1) = (last as u32 % image.width, last as u32 / image.width);
            let upright = y0 < image.height / 2;
            let confidence = if upright { 0.95 } else { 0.2 };
            let word = |n: u32| RecognizedWord {
                text: format!("mot{n}"),
                quad: Quad::from_rect(x0 as f32, y0 as f32, x1 as f32 + 1.0, y1 as f32 + 1.0),
                confidence,
                line: 0,
            };
            Ok((0..3).map(word).collect())
        }
    }

    #[test]
    fn relit_une_page_a_l_envers() {
        // Une barre noire en haut de la page ; la même page à l'envers.
        let mut page = GrayImage::new(200, 300, 255);
        for y in 20..30 {
            for x in 30..170 {
                page.pixels[y * 200 + x] = 0;
            }
        }
        let engine = FakeEngine {
            sizes: RefCell::new(Vec::new()),
        };
        let mut options = Options::raw(100.0, vec!["fr-FR".into()]);
        options.orientation = true;

        let upright = recognize(&engine, &page, &options).unwrap();
        assert_eq!(upright.quarter_turns, 0);
        assert_eq!(engine.sizes.borrow().len(), 1);

        let (flipped, _) = page.rotate_quarters(2);
        let result = recognize(&engine, &flipped, &options).unwrap();
        assert_eq!(result.quarter_turns, 2);
        assert!(result.mean_confidence() > 0.9);
        // L'emprise est rendue dans l'image reçue : la barre y est en bas.
        let (x0, y0, x1, y1) = result.words[0].quad.bounds();
        assert_eq!((x0, y0, x1, y1), (30.0, 270.0, 170.0, 280.0));
    }
}
