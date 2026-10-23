//! Appariement d'une police au texte d'une ligne scannée.
//!
//! La ligne est d'abord classée d'après ses glyphes : inclinaison (droite ou
//! italique), mesurée par le cisaillement qui rend les fûts les plus
//! verticaux. Chaque police du catalogue de même allure est ensuite mise à
//! l'échelle du scan (hauteur des mots au-dessus de la ligne de base),
//! espacée comme lui (largeur des mots), rendue et comparée à l'encre de la
//! ligne, mot par mot (similarité des couvertures, à un pixel près). Les
//! trois meilleures familles sont gardées ; pour chacune, le crénage, le flou
//! et la graisse qui rapprochent le plus le rendu du scan sont retenus.
//!
//! L'identification reste approximative : une police absente du catalogue,
//! une très faible résolution ou un mot isolé donnent des scores proches et
//! un choix incertain, que le score indique.

use std::collections::HashMap;

use rustybuzz::Face;

use crate::catalog::Catalog;
use crate::text::{rasterize, shape, Coverage, Pen, Shaped};

/// Ligne scannée, redressée (ligne de base horizontale).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    /// Largeur en pixels.
    pub width: usize,
    /// Hauteur en pixels.
    pub height: usize,
    /// Couverture de l'encre, rangée par rangée (0 : papier, 1 : cœur des
    /// traits).
    pub ink: Vec<f32>,
    /// Ligne de base, en pixels depuis le haut.
    pub baseline: f32,
    /// Mots lus, dans l'ordre de la ligne.
    pub words: Vec<ObservedWord>,
}

/// Un mot lu sur la ligne : texte et étendue approximative (reconnaissance).
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedWord {
    /// Texte.
    pub text: String,
    /// Début, en pixels depuis la gauche.
    pub left: f32,
    /// Fin.
    pub right: f32,
}

/// Une police candidate et les réglages qui la font coïncider avec le scan,
/// en pixels du scan.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// Indice de la police dans le catalogue.
    pub face: usize,
    /// Nom PostScript.
    pub postscript_name: String,
    /// Famille.
    pub family: String,
    /// Similarité du rendu avec le scan, de 0 à 1.
    pub score: f32,
    /// Corps : taille du cadratin, en pixels.
    pub em: f32,
    /// Interlettrage, en pixels.
    pub spacing: f32,
    /// Crénage de la police appliqué.
    pub kerning: bool,
    /// Flou (écart type), en pixels.
    pub blur: f32,
    /// Graisse ajoutée, de -1 (traits amincis d'un pixel) à 1 (épaissis).
    pub weight: f32,
    /// Origine du premier glyphe de chaque mot, le long de la ligne (pixels).
    pub pens: Vec<f32>,
}

/// Résultat de l'appariement d'une ligne.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineMatch {
    /// Meilleures polices, une par famille, de la plus proche à la moins
    /// proche (trois au plus).
    pub candidates: Vec<Candidate>,
    /// Inclinaison mesurée des glyphes, en degrés (positive : vers la
    /// droite).
    pub slant: f32,
    /// La ligne est en italique.
    pub italic: bool,
}

/// Nombre de familles proposées.
const CANDIDATES: usize = 3;
/// Mots comparés au plus (les plus longs).
const MAX_WORDS: usize = 10;
/// Inclinaison au-delà de laquelle une ligne est italique (tangente, ≈ 4°).
const ITALIC_SHEAR: f32 = 0.07;
/// Flous essayés, en pixels.
const BLURS: [f32; 8] = [0.0, 0.3, 0.5, 0.75, 1.0, 1.3, 1.7, 2.2];
/// Corrections de taille essayées sur les meilleures polices.
const SCALE_FACTORS: [f32; 6] = [0.91, 0.94, 0.97, 1.03, 1.06, 1.09];
/// Graisses essayées.
const WEIGHTS: [f32; 7] = [-0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75];
/// Flou appliqué au rendu pendant le tri des polices, en part du cadratin.
const SORT_BLUR: f32 = 0.02;

/// Mot préparé pour la comparaison : fenêtre et emprise de son encre.
struct Word<'a> {
    text: &'a str,
    chars: usize,
    /// Colonnes de la fenêtre de comparaison.
    window: (usize, usize),
    /// Emprise de l'encre : gauche, droite, haut.
    ink: (f32, f32, f32),
}

/// Cherche dans `catalog` les polices qui reproduisent le mieux la ligne
/// `observation`.
pub fn match_line(catalog: &Catalog, observation: &Observation) -> LineMatch {
    let words = prepare(observation);
    let slant = measure_shear(observation);
    let italic = slant >= ITALIC_SHEAR;
    let mut result = LineMatch {
        candidates: Vec::new(),
        slant: slant.atan().to_degrees(),
        italic,
    };
    if words.is_empty() {
        return result;
    }

    // Tri de toutes les polices de même allure.
    let same_style: Vec<usize> = (0..catalog.faces().len())
        .filter(|&i| catalog.faces()[i].italic == italic)
        .collect();
    let families = |faces: &[usize]| {
        faces
            .iter()
            .map(|&i| catalog.faces()[i].family.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
    };
    let pool: Vec<usize> = if families(&same_style) >= CANDIDATES {
        same_style
    } else {
        (0..catalog.faces().len()).collect()
    };
    let mut scored: Vec<(f32, usize, Fit)> = Vec::new();
    for index in pool {
        let Some(file) = catalog.load(index) else {
            continue;
        };
        let face = file.face();
        let Some(fit) = fit(&face, observation, &words, true) else {
            continue;
        };
        let blur = SORT_BLUR * fit.em;
        let score = score(&face, observation, &words, &fit, blur, 0.0) * fit.spacing_penalty();
        scored.push((score, index, fit));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    // Meilleure police de chaque famille, puis réglages fins.
    let mut seen: HashMap<String, ()> = HashMap::new();
    for (_, index, sorted_fit) in scored {
        let info = &catalog.faces()[index];
        if seen.insert(info.family.clone(), ()).is_some() {
            continue;
        }
        let Some(file) = catalog.load(index) else {
            continue;
        };
        let face = file.face();
        result
            .candidates
            .push(refine(&face, observation, &words, sorted_fit, index, info));
        if result.candidates.len() == CANDIDATES {
            break;
        }
    }
    result
        .candidates
        .sort_by(|a, b| b.score.total_cmp(&a.score));
    result
}

/// Réglages d'une police pour la ligne.
#[derive(Clone, Debug)]
struct Fit {
    /// Pixels par unité de la police.
    scale: f32,
    /// Cadratin, en pixels.
    em: f32,
    /// Interlettrage, en pixels.
    spacing: f32,
    kerning: bool,
}

impl Fit {
    /// Pénalité d'un interlettrage inhabituel : une police aux proportions
    /// différentes ne s'ajuste qu'en écartant ou en serrant beaucoup ses
    /// lettres.
    fn spacing_penalty(&self) -> f32 {
        let relative = (self.spacing / self.em).abs();
        (-(relative - 0.02).max(0.0) / 0.08).exp()
    }
}

/// Mots de la ligne, avec la fenêtre où chercher leur encre.
fn prepare(observation: &Observation) -> Vec<Word<'_>> {
    let (w, h) = (observation.width, observation.height);
    let mut out = Vec::new();
    let count = observation.words.len();
    for (index, word) in observation.words.iter().enumerate() {
        let chars = word.text.chars().filter(|c| !c.is_whitespace()).count();
        if chars == 0 {
            continue;
        }
        // Fenêtre : jusqu'au milieu des espaces qui la séparent des voisins.
        let previous = index
            .checked_sub(1)
            .map(|i| observation.words[i].right)
            .unwrap_or(0.0);
        let next = observation
            .words
            .get(index + 1)
            .map(|next| next.left)
            .unwrap_or(w as f32);
        let start = if index == 0 {
            (word.left - (word.right - word.left) * 0.1).max(0.0)
        } else {
            (previous + word.left) / 2.0
        };
        let end = if index + 1 == count {
            (word.right + (word.right - word.left) * 0.1).min(w as f32)
        } else {
            (word.right + next) / 2.0
        };
        let window = (start.max(0.0) as usize, (end.ceil() as usize).min(w));
        if window.1 <= window.0 + 1 {
            continue;
        }
        // Emprise de l'encre dans la fenêtre.
        let (mut x0, mut x1, mut top) = (usize::MAX, 0, usize::MAX);
        for y in 0..h {
            for x in window.0..window.1 {
                if observation.ink[y * w + x] >= 0.35 {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    top = top.min(y);
                }
            }
        }
        if x0 == usize::MAX || (top as f32) >= observation.baseline - 1.0 {
            continue;
        }
        out.push(Word {
            text: &word.text,
            chars,
            window,
            ink: (x0 as f32, x1 as f32 + 1.0, top as f32),
        });
    }
    // Les mots les plus longs suffisent.
    if out.len() > MAX_WORDS {
        let mut order: Vec<usize> = (0..out.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(out[i].chars));
        let keep: std::collections::HashSet<usize> = order.into_iter().take(MAX_WORDS).collect();
        out = out
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep.contains(i))
            .map(|(_, word)| word)
            .collect();
    }
    out
}

/// Taille et interlettrage d'une police qui donnent aux mots la hauteur et
/// la largeur de leur encre.
fn fit(face: &Face, observation: &Observation, words: &[Word], kerning: bool) -> Option<Fit> {
    fit_scaled(face, observation, words, kerning, 1.0)
}

/// Comme [`fit`], la taille mesurée multipliée par `factor` (l'interlettrage
/// est recalculé pour garder la largeur des mots).
fn fit_scaled(
    face: &Face,
    observation: &Observation,
    words: &[Word],
    kerning: bool,
    factor: f32,
) -> Option<Fit> {
    let upem = face.units_per_em() as f32;
    let mut scales = Vec::new();
    let mut shaped = Vec::with_capacity(words.len());
    for word in words {
        let s = shape(face, word.text, 0.0, kerning);
        if !s.missing.is_empty() {
            return None;
        }
        // Hauteur de l'encre au-dessus de la ligne de base : les mots en
        // capitales ou à hampes comme ceux en lettres basses.
        if s.ink_y.1 > 0.3 * upem {
            scales.push((observation.baseline - word.ink.2) / s.ink_y.1);
        }
        shaped.push(s);
    }
    let scale = median(&mut scales)? * factor;
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let mut spacings: Vec<f32> = words
        .iter()
        .zip(&shaped)
        .filter(|(word, s)| word.chars >= 2 && !s.is_blank())
        .map(|(word, s)| {
            let observed = word.ink.1 - word.ink.0;
            let drawn = (s.ink_x.1 - s.ink_x.0) * scale;
            (observed - drawn) / (word.chars - 1) as f32
        })
        .collect();
    let spacing = median(&mut spacings).unwrap_or(0.0);
    Some(Fit {
        scale,
        em: scale * upem,
        spacing,
        kerning,
    })
}

/// Rendu d'un mot avec les réglages `fit`, son encre alignée sur celle du
/// scan.
fn render_word(face: &Face, observation: &Observation, word: &Word, fit: &Fit) -> Coverage {
    let shaped = shape(face, word.text, fit.spacing / fit.scale, fit.kerning);
    let origin = word.ink.0 - shaped.ink_x.0 * fit.scale;
    rasterize(
        face,
        &shaped,
        &Pen::horizontal(
            (f64::from(origin), f64::from(observation.baseline)),
            f64::from(fit.scale),
        ),
    )
}

/// Similarité moyenne des mots rendus avec le scan, pondérée par leur
/// longueur.
fn score(
    face: &Face,
    observation: &Observation,
    words: &[Word],
    fit: &Fit,
    blur: f32,
    weight: f32,
) -> f32 {
    let (mut total, mut chars) = (0.0, 0);
    for word in words {
        let rendered = render_word(face, observation, word, fit)
            .emboldened(weight)
            .blurred(blur);
        total += similarity(observation, word.window, &rendered) * word.chars as f32;
        chars += word.chars;
    }
    if chars == 0 {
        0.0
    } else {
        total / chars as f32
    }
}

/// Similarité de la couverture rendue avec l'encre du scan dans la fenêtre
/// de colonnes `window`, au meilleur décalage d'un pixel près : 1 moins
/// l'écart des couvertures rapporté à leur somme.
fn similarity(observation: &Observation, window: (usize, usize), rendered: &Coverage) -> f32 {
    let (w, h) = (observation.width, observation.height);
    let mut best = 0.0f32;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let (mut diff, mut sum) = (0.0f32, 0.0f32);
            for y in 0..h {
                for x in window.0..window.1 {
                    let a = observation.ink[y * w + x];
                    let b = rendered.at(x as i32 - dx, y as i32 - dy);
                    diff += (a - b).abs();
                    sum += a + b;
                }
            }
            if sum > 0.0 {
                best = best.max(1.0 - diff / sum);
            }
        }
    }
    best
}

/// Réglages fins de la police `index` : crénage, flou puis graisse.
fn refine(
    face: &Face,
    observation: &Observation,
    words: &[Word],
    fit: Fit,
    index: usize,
    info: &crate::catalog::FaceInfo,
) -> Candidate {
    let base_blur = SORT_BLUR * fit.em;
    let mut best_fit = fit.clone();
    let mut best = score(face, observation, words, &fit, base_blur, 0.0);
    if let Some(unkerned) = fit_kerning(face, observation, words, false) {
        let value = score(face, observation, words, &unkerned, base_blur, 0.0);
        if value > best {
            best = value;
            best_fit = unkerned;
        }
    }
    // La hauteur mesurée de l'encre est un peu faussée par le flou du scan :
    // tailles voisines, à interlettrage recalculé.
    let mut penalized = best * best_fit.spacing_penalty();
    for factor in SCALE_FACTORS {
        let Some(scaled) = fit_scaled(face, observation, words, best_fit.kerning, factor) else {
            continue;
        };
        let value = score(face, observation, words, &scaled, base_blur, 0.0);
        if value * scaled.spacing_penalty() > penalized {
            penalized = value * scaled.spacing_penalty();
            best = value;
            best_fit = scaled;
        }
    }
    let mut blur = base_blur;
    for candidate in BLURS {
        let value = score(face, observation, words, &best_fit, candidate, 0.0);
        if value > best {
            best = value;
            blur = candidate;
        }
    }
    let mut weight = 0.0;
    for candidate in WEIGHTS {
        let value = score(face, observation, words, &best_fit, blur, candidate);
        if value > best {
            best = value;
            weight = candidate;
        }
    }
    let pens = observation
        .words
        .iter()
        .map(|word| pen_of(face, observation, word, &best_fit))
        .collect();
    Candidate {
        face: index,
        postscript_name: info.postscript_name.clone(),
        family: info.family.clone(),
        score: (best * best_fit.spacing_penalty()).clamp(0.0, 1.0),
        em: best_fit.em,
        spacing: best_fit.spacing,
        kerning: best_fit.kerning,
        blur,
        weight,
        pens,
    }
}

fn fit_kerning(
    face: &Face,
    observation: &Observation,
    words: &[Word],
    kerning: bool,
) -> Option<Fit> {
    fit(face, observation, words, kerning)
}

/// Origine du premier glyphe d'un mot : son encre commence où commence celle
/// du scan (à défaut, au début lu par la reconnaissance).
fn pen_of(face: &Face, observation: &Observation, word: &ObservedWord, fit: &Fit) -> f32 {
    let shaped: Shaped = shape(face, &word.text, fit.spacing / fit.scale, fit.kerning);
    let (w, h) = (observation.width, observation.height);
    let (from, to) = (
        word.left.max(0.0) as usize,
        (word.right.ceil() as usize).min(w),
    );
    let first_ink = (from..to).find(|&x| (0..h).any(|y| observation.ink[y * w + x] >= 0.35));
    let left = first_ink.map_or(word.left, |x| x as f32);
    if shaped.is_blank() {
        left
    } else {
        left - shaped.ink_x.0 * fit.scale
    }
}

/// Cisaillement (tangente de l'inclinaison) qui rend les traits verticaux
/// les plus nets : le profil des colonnes est alors le plus contrasté.
fn measure_shear(observation: &Observation) -> f32 {
    let (w, h) = (observation.width, observation.height);
    let mut best = (f32::NEG_INFINITY, 0.0f32);
    for step in -4i32..=18 {
        let shear = step as f32 * 0.025;
        let margin = (shear.abs() * h as f32).ceil() as usize + 1;
        let mut profile = vec![0.0f32; w + 2 * margin];
        for y in 0..h {
            let lift = observation.baseline - y as f32;
            for x in 0..w {
                let value = observation.ink[y * w + x];
                if value <= 0.0 {
                    continue;
                }
                // Le haut d'un trait penché est à droite : on le ramène.
                let column = x as f32 - shear * lift + margin as f32;
                if column >= 0.0 && (column as usize) < profile.len() {
                    profile[column as usize] += value;
                }
            }
        }
        let sharpness: f32 = profile.iter().map(|v| v * v).sum();
        if sharpness > best.0 {
            best = (sharpness, shear);
        }
    }
    best.1
}

fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    Some(values[values.len() / 2])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::catalog::tests::bundled;

    /// Ligne « scannée » : texte rendu dans une police du catalogue, flou et
    /// légèrement espacé, comme l'imprimerait puis le lirait un scanner.
    pub(crate) fn scanned_line(
        catalog: &Catalog,
        postscript: &str,
        text: &str,
        em: f32,
    ) -> Observation {
        let file = catalog.load(catalog.find(postscript).unwrap()).unwrap();
        let face = file.face();
        let upem = face.units_per_em() as f32;
        let scale = em / upem;
        let spacing = 0.01 * em / scale;
        let height = (em * 1.6) as usize;
        let baseline = em * 1.15;
        let mut words = Vec::new();
        let mut pen = 6.0f32;
        let mut ink_layers = Vec::new();
        let space = shape(&face, " ", 0.0, true).advance * scale;
        for word in text.split(' ') {
            let shaped = shape(&face, word, spacing, true);
            let coverage = rasterize(
                &face,
                &shaped,
                &Pen::horizontal((f64::from(pen), f64::from(baseline)), f64::from(scale)),
            )
            .blurred(0.7);
            words.push(ObservedWord {
                text: word.to_owned(),
                left: pen + shaped.ink_x.0 * scale - 1.0,
                right: pen + shaped.ink_x.1 * scale + 1.0,
            });
            pen += shaped.advance * scale + space;
            ink_layers.push(coverage);
        }
        let width = pen as usize + 6;
        let mut ink = vec![0.0f32; width * height];
        for layer in ink_layers {
            for y in 0..height {
                for x in 0..width {
                    ink[y * width + x] =
                        (ink[y * width + x] + layer.at(x as i32, y as i32)).min(1.0);
                }
            }
        }
        Observation {
            width,
            height,
            ink,
            baseline,
            words,
        }
    }

    #[test]
    fn retrouve_la_police_d_une_ligne() {
        let catalog = bundled();
        let line = "Montant total de la facture : 1 245,80 euros";
        for (postscript, family) in [("Caladea-Regular", "Caladea"), ("Carlito-Bold", "Carlito")] {
            let observation = scanned_line(&catalog, postscript, line, 38.0);
            let found = match_line(&catalog, &observation);
            assert!(!found.italic, "{}", found.slant);
            let best = &found.candidates[0];
            assert_eq!(best.family, family, "{found:?}");
            assert_eq!(best.postscript_name, postscript);
            assert!(best.score > 0.85, "{}", best.score);
            assert!((best.em - 38.0).abs() < 1.5, "{}", best.em);
            assert!(best.blur > 0.3 && best.blur < 1.4, "{}", best.blur);
            assert_eq!(best.pens.len(), observation.words.len());
            assert!((best.pens[0] - 6.0).abs() < 1.5, "{:?}", best.pens);
            // L'autre famille, moins proche, est proposée.
            assert_eq!(found.candidates.len(), 2);
            assert!(found.candidates[1].score < best.score);
        }
    }

    #[test]
    fn reconnait_l_italique() {
        let catalog = bundled();
        let observation = scanned_line(
            &catalog,
            "Caladea-Italic",
            "Veuillez agréer nos salutations",
            40.0,
        );
        let found = match_line(&catalog, &observation);
        assert!(found.italic, "{}", found.slant);
        assert_eq!(found.candidates[0].postscript_name, "Caladea-Italic");
    }

    #[test]
    fn ligne_vide() {
        let catalog = bundled();
        let found = match_line(&catalog, &Observation::default());
        assert!(found.candidates.is_empty());
    }
}
