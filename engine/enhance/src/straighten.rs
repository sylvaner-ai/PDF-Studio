//! Redressement d'une page scannée : orientation (quarts de tour) puis
//! inclinaison, avec leurs garde-fous (cf. D61).
//!
//! L'image est d'abord égalisée (fond, contraste), comme avant une lecture.
//!
//! - **Page couchée** : les lignes de texte sont verticales
//!   (`ocr::preprocess::is_sideways`, D27) ; la page est tournée d'un quart
//!   de tour avant la suite.
//! - **Inclinaison** : angle qui rend le plus net le profil de projection de
//!   l'encre des caractères (`ocr::preprocess::measure_skew`), cherché entre
//!   −45° et 45°, au centième de degré. La netteté de cet angle, rapportée à
//!   celle des autres, dit si la mesure est fiable : une photographie ou une
//!   page sans texte n'a pas de direction qui se distingue.
//! - **Page à l'envers** : les hampes (b, d, h, l, majuscules) dépassent bien
//!   plus souvent de la bande des lettres basses que les jambages (g, p, q,
//!   y) : leur rapport (`ocr::preprocess::ascender_ratio`), mesuré sur la
//!   page et sur la page retournée, désigne le haut. S'il ne tranche pas et
//!   qu'un moteur de lecture est fourni, la confiance de la lecture dans
//!   chaque sens décide ; sinon la page garde son sens (dans le doute, ne
//!   rien changer).
//! - **Garde-fous** : aucune correction sous `min_degrees` (0,1°) ; au-delà
//!   de `max_degrees` (15°), la page n'est pas redressée et elle est signalée
//!   « à vérifier » ; une mesure peu fiable ne corrige rien.

use ocr::{preprocess, GrayImage, OcrEngine, OcrError};

use crate::settings::StraightenSettings;

/// Plage de recherche de l'inclinaison, en degrés : bien au-delà du plafond,
/// pour reconnaître une page trop inclinée au lieu d'y lire un faux angle.
const SEARCH_DEGREES: f64 = 45.0;

/// Netteté relative en dessous de laquelle l'angle mesuré n'est pas fiable
/// (cf. `docs/AMELIORATION.md` : 9,3 au moins pour les pages du jeu de test,
/// 1,6 pour le faux angle d'une recherche limitée à ±15°).
const MIN_PROMINENCE: f64 = 4.0;

/// Pixels d'encre (à 100 ppp) en dessous desquels la page n'a pas assez de
/// texte pour être mesurée.
const MIN_INK: usize = 500;

/// Rapport des hampes aux jambages au-delà duquel le sens de la page est
/// sûr (et en deçà de son inverse, la page est à l'envers).
const UPRIGHT_RATIO: f64 = 1.5;

/// Conclusion du redressement d'une page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Inclinaison inférieure au seuil : la page est déjà droite.
    Straight,
    /// L'inclinaison est corrigée.
    Corrected,
    /// Inclinaison au-delà du plafond : la page n'est pas redressée et doit
    /// être vérifiée.
    ToCheck,
    /// Pas assez de texte pour mesurer l'inclinaison (photographie, page
    /// presque vide) : la page n'est pas redressée.
    Unmeasurable,
    /// Correction de l'inclinaison désactivée.
    Disabled,
}

/// Comment le sens de la page a été établi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Par la forme des lignes de texte, sans doute possible.
    Sure,
    /// Par la confiance du moteur de lecture, la forme des lignes ne
    /// tranchant pas.
    Read,
    /// Incertain : la page garde son sens.
    Doubtful,
    /// Correction de l'orientation désactivée.
    Disabled,
}

/// Redressement d'une page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Straightening {
    /// Quarts de tour, dans le sens horaire, qui remettent la page à
    /// l'endroit (0 : elle l'est).
    pub quarter_turns: u8,
    /// Comment ce sens a été établi.
    pub orientation: Orientation,
    /// Inclinaison mesurée sur la page remise à l'endroit, en degrés
    /// (positive : les lignes descendent vers la droite) ; `None` si elle
    /// n'a pas pu l'être.
    pub measured_degrees: Option<f64>,
    /// Inclinaison corrigée, en degrés (0 : aucune) : l'image est tournée de
    /// son opposé.
    pub correction_degrees: f64,
    /// Conclusion sur l'inclinaison.
    pub verdict: Verdict,
}

impl Straightening {
    /// Vrai si les pixels de la page changent (inclinaison corrigée).
    pub fn changes_pixels(&self) -> bool {
        self.correction_degrees != 0.0
    }
}

/// Redressement de la page `image` (niveaux de gris, `dpi` pixels par
/// pouce), tournée comme elle s'affiche. Avec `engine`, une page dont le
/// sens reste douteux est lue dans chaque sens.
pub fn analyze(
    image: &GrayImage,
    dpi: f32,
    settings: &StraightenSettings,
    engine: Option<&dyn OcrEngine>,
) -> Result<Straightening, OcrError> {
    let flat = preprocess::flatten_background(image, dpi);
    let mut turns = 0u8;
    let mut upright = flat;
    if settings.orientation && preprocess::is_sideways(&upright, dpi) {
        turns = 1;
        upright = upright.rotate_quarters(1).0;
    }
    let skew = preprocess::measure_skew(&upright, dpi, SEARCH_DEGREES);
    let reliable = skew.prominence >= MIN_PROMINENCE && skew.ink >= MIN_INK;

    let mut orientation = Orientation::Disabled;
    if settings.orientation {
        let angle = if reliable { skew.degrees } else { 0.0 };
        orientation = match upright_score(&upright, dpi, angle) {
            Some(score) if score >= UPRIGHT_RATIO => Orientation::Sure,
            Some(score) if score <= 1.0 / UPRIGHT_RATIO => {
                turns = (turns + 2) % 4;
                Orientation::Sure
            }
            _ => Orientation::Doubtful,
        };
        if orientation == Orientation::Doubtful {
            if let Some(engine) = engine {
                if let Some(read) = read_orientation(engine, &upright, dpi)? {
                    turns = (turns + read) % 4;
                    orientation = Orientation::Read;
                }
            }
        }
    }

    let measured = reliable.then_some(skew.degrees);
    let verdict = if !settings.deskew {
        Verdict::Disabled
    } else {
        match measured {
            None => Verdict::Unmeasurable,
            Some(degrees) if degrees.abs() > settings.max_degrees => Verdict::ToCheck,
            Some(degrees) if degrees.abs() < settings.min_degrees => Verdict::Straight,
            Some(_) => Verdict::Corrected,
        }
    };
    // Une page retournée garde son angle : la rotation d'un demi-tour change
    // le sens du texte, pas la pente des lignes.
    let correction = match (verdict, measured) {
        (Verdict::Corrected, Some(degrees)) => degrees,
        _ => 0.0,
    };
    Ok(Straightening {
        quarter_turns: turns,
        orientation,
        measured_degrees: measured,
        correction_degrees: correction,
        verdict,
    })
}

/// Sens de la page `image`, dont les lignes sont horizontales (inclinées de
/// `skew`) : plus de 1 si elle est à l'endroit, moins de 1 si elle est à
/// l'envers. Le rapport des hampes aux jambages est mesuré sur la page et
/// sur la page retournée, et les deux mesures combinées (moyenne
/// géométrique) ; `None` si l'une d'elles est impossible.
fn upright_score(image: &GrayImage, dpi: f32, skew: f64) -> Option<f64> {
    let direct = preprocess::ascender_ratio(image, dpi, skew)?;
    let flipped = preprocess::ascender_ratio(&image.rotate_quarters(2).0, dpi, skew)?;
    Some((direct / flipped).sqrt())
}

/// Quarts de tour (0 ou 2) qui remettent à l'endroit la page `image`, dont
/// les lignes sont horizontales, d'après la lecture de `engine` dans les
/// deux sens ; `None` si aucune des deux lectures n'est assez sûre.
fn read_orientation(
    engine: &dyn OcrEngine,
    image: &GrayImage,
    dpi: f32,
) -> Result<Option<u8>, OcrError> {
    let mut options = ocr::Options::raw(dpi, vec!["fr-FR".into(), "en-US".into()]);
    options.deskew = true;
    let quality = |turns: u8| -> Result<(f32, f32), OcrError> {
        let source = if turns == 0 {
            image.clone()
        } else {
            image.rotate_quarters(turns).0
        };
        let read = ocr::recognize(engine, &source, &options)?;
        let chars: usize = read.words.iter().map(|w| w.text.chars().count()).sum();
        Ok((
            read.mean_confidence(),
            read.mean_confidence() * chars as f32,
        ))
    };
    let (upright_confidence, upright) = quality(0)?;
    let (flipped_confidence, flipped) = quality(2)?;
    Ok(if upright_confidence >= 0.6 && upright >= 1.2 * flipped {
        Some(0)
    } else if flipped_confidence >= 0.6 && flipped >= 1.2 * upright {
        Some(2)
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocr::{Quad, RecognizedWord};

    /// Page de 1100 × 1400 pixels à 150 ppp : lignes de « lettres » dont un
    /// tiers a une hampe et un dixième un jambage, comme un texte latin.
    fn text_page() -> GrayImage {
        let mut image = GrayImage::new(1100, 1400, 245);
        for line in 0..24 {
            let base = 120 + line * 50;
            let mut x = 100;
            let mut k = line * 7;
            while x < 1000 {
                k += 1;
                if k % 6 == 0 {
                    x += 12;
                    continue;
                }
                let top = if k % 3 == 0 { base - 22 } else { base - 12 };
                let bottom = if k % 10 == 3 { base + 8 } else { base };
                for y in top..bottom {
                    for dx in 0..4 {
                        image.pixels[(y * 1100 + x + dx) as usize] = 20;
                    }
                }
                x += 9;
            }
        }
        image
    }

    fn settings() -> StraightenSettings {
        StraightenSettings::default()
    }

    #[test]
    fn page_droite_laissee_telle_quelle() {
        let result = analyze(&text_page(), 150.0, &settings(), None).unwrap();
        assert_eq!(result.quarter_turns, 0);
        assert_eq!(result.orientation, Orientation::Sure);
        assert_eq!(result.verdict, Verdict::Straight);
        assert!(!result.changes_pixels());
    }

    #[test]
    fn inclinaison_corrigee_et_plafond() {
        let (tilted, _) = text_page().rotate(4.0, 245);
        let result = analyze(&tilted, 150.0, &settings(), None).unwrap();
        assert_eq!(result.verdict, Verdict::Corrected);
        assert!((result.correction_degrees - 4.0).abs() < 0.1, "{result:?}");

        let (steep, _) = text_page().rotate(20.0, 245);
        let result = analyze(&steep, 150.0, &settings(), None).unwrap();
        assert_eq!(result.verdict, Verdict::ToCheck);
        assert!((result.measured_degrees.unwrap() - 20.0).abs() < 0.2);
        assert_eq!(result.correction_degrees, 0.0);

        let mut off = settings();
        off.deskew = false;
        let result = analyze(&tilted, 150.0, &off, None).unwrap();
        assert_eq!(result.verdict, Verdict::Disabled);
        assert!(!result.changes_pixels());
    }

    #[test]
    fn orientation_retrouvee() {
        let page = text_page();
        for (turns, expected) in [(1u8, 3u8), (2, 2), (3, 1)] {
            let (rotated, _) = page.rotate_quarters(turns);
            let result = analyze(&rotated, 150.0, &settings(), None).unwrap();
            assert_eq!(result.quarter_turns, expected, "{turns}");
            assert_eq!(result.verdict, Verdict::Straight);
        }
    }

    #[test]
    fn page_sans_texte_non_mesuree() {
        let blank = GrayImage::new(800, 1000, 240);
        let result = analyze(&blank, 150.0, &settings(), None).unwrap();
        assert_eq!(result.verdict, Verdict::Unmeasurable);
        assert_eq!(result.quarter_turns, 0);
        assert_eq!(result.orientation, Orientation::Doubtful);
    }

    /// Moteur factice : lit trois mots sûrs si la page a de l'encre en haut à
    /// gauche, peu sûrs sinon.
    struct Reader;

    impl OcrEngine for Reader {
        fn name(&self) -> String {
            "factice".into()
        }

        fn recognize(
            &self,
            image: &GrayImage,
            _languages: &[String],
        ) -> Result<Vec<RecognizedWord>, OcrError> {
            let first = image.pixels.iter().position(|&v| v < 128).unwrap_or(0) as u32;
            let upright = first / image.width < image.height / 2;
            Ok((0..3)
                .map(|n| RecognizedWord {
                    text: format!("mot{n}"),
                    quad: Quad::from_rect(0.0, 0.0, 10.0, 10.0),
                    confidence: if upright { 0.95 } else { 0.2 },
                    line: 0,
                })
                .collect())
        }
    }

    #[test]
    fn la_lecture_tranche_un_sens_douteux() {
        // Lignes de capitales seules (aucune hampe ni jambage), en haut de
        // la page puis la même page retournée.
        let mut page = GrayImage::new(900, 1200, 245);
        for line in 0..8 {
            let top = 100 + line * 40;
            for x in (100..800).step_by(9) {
                for y in top..top + 20 {
                    for dx in 0..4 {
                        page.pixels[(y * 900 + x + dx) as usize] = 20;
                    }
                }
            }
        }
        let (flipped, _) = page.rotate_quarters(2);
        let result = analyze(&flipped, 150.0, &settings(), None).unwrap();
        assert_eq!(result.orientation, Orientation::Doubtful);
        assert_eq!(result.quarter_turns, 0);
        let result = analyze(&flipped, 150.0, &settings(), Some(&Reader)).unwrap();
        assert_eq!(result.orientation, Orientation::Read);
        assert_eq!(result.quarter_turns, 2);
    }
}
