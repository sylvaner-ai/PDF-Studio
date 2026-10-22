//! Mesure de la qualité d'une lecture par rapport à un texte de référence.
//!
//! La référence est donnée ligne par ligne, avec l'emprise de chaque ligne
//! dans l'image. Chaque mot lu est rattaché à la ligne qui contient son
//! centre ; les mots d'une ligne sont remis dans l'ordre de gauche à droite,
//! puis comparés au texte de référence par distance d'édition (insertions,
//! suppressions, substitutions). Ainsi, l'ordre dans lequel un moteur parcourt
//! les colonnes ou les cases d'un tableau ne compte pas : seule compte la
//! justesse de ce qu'il lit à chaque endroit. Un mot rattaché à aucune ligne
//! (lecture d'une tache, par exemple) compte comme autant d'erreurs que de
//! caractères.

use unicode_normalization::UnicodeNormalization;

use crate::engine::RecognizedWord;

/// Une ligne du texte de référence.
#[derive(Clone, Debug, PartialEq)]
pub struct TruthLine {
    /// Texte de la ligne.
    pub text: String,
    /// Emprise `(x0, y0, x1, y1)` dans l'image, en pixels.
    pub bounds: (f32, f32, f32, f32),
}

/// Erreurs comptées sur une ou plusieurs pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    /// Caractères de la référence (espaces comprises).
    pub chars: usize,
    /// Erreurs de caractères.
    pub char_errors: usize,
    /// Mots de la référence.
    pub words: usize,
    /// Erreurs de mots.
    pub word_errors: usize,
}

impl Score {
    /// Ajoute les erreurs d'une autre page.
    pub fn add(&mut self, other: &Score) {
        self.chars += other.chars;
        self.char_errors += other.char_errors;
        self.words += other.words;
        self.word_errors += other.word_errors;
    }

    /// Taux de caractères corrects, de 0 à 1 (1 − taux d'erreur), borné à 0.
    pub fn char_accuracy(&self) -> f64 {
        accuracy(self.char_errors, self.chars)
    }

    /// Taux de mots corrects, de 0 à 1, borné à 0.
    pub fn word_accuracy(&self) -> f64 {
        accuracy(self.word_errors, self.words)
    }
}

fn accuracy(errors: usize, total: usize) -> f64 {
    if total == 0 {
        return if errors == 0 { 1.0 } else { 0.0 };
    }
    (1.0 - errors as f64 / total as f64).max(0.0)
}

/// Confiance annoncée pour un mot lu, et justesse de ce mot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordOutcome {
    /// Confiance du moteur, de 0 à 1.
    pub confidence: f32,
    /// Le mot figure tel quel dans la ligne de référence.
    pub correct: bool,
}

/// Forme canonique d'un texte pour la comparaison : composition Unicode NFC,
/// ligatures typographiques développées, apostrophes et traits d'union
/// unifiés, espaces fusionnées. Les accents, la casse et la ponctuation
/// comptent.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.nfc() {
        let expanded: &str = match c {
            '\u{FB00}' => "ff",
            '\u{FB01}' => "fi",
            '\u{FB02}' => "fl",
            '\u{FB03}' => "ffi",
            '\u{FB04}' => "ffl",
            '\u{FB05}' | '\u{FB06}' => "st",
            '\u{2019}' | '\u{2018}' | '\u{02BC}' | '\u{2032}' => "'",
            '\u{2010}' | '\u{2011}' => "-",
            _ => "",
        };
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if expanded.is_empty() {
            out.push(c);
        } else {
            out.push_str(expanded);
        }
    }
    out
}

/// Distance d'édition entre deux suites.
pub fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cost = usize::from(x != y);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Compare les mots lus sur une page à sa référence ; renvoie les erreurs et,
/// pour chaque mot rattaché à une ligne, sa confiance et sa justesse.
pub fn score(truth: &[TruthLine], words: &[RecognizedWord]) -> (Score, Vec<WordOutcome>) {
    let mut assigned: Vec<Vec<&RecognizedWord>> = vec![Vec::new(); truth.len()];
    let mut stray = 0usize;
    for word in words {
        let center = word.quad.center();
        let mut best: Option<(usize, f32)> = None;
        for (index, line) in truth.iter().enumerate() {
            let (x0, y0, x1, y1) = line.bounds;
            let height = (y1 - y0).max(1.0);
            let (mx, my) = (height * 0.5, height * 0.25);
            if center.x < x0 - mx || center.x > x1 + mx || center.y < y0 - my || center.y > y1 + my
            {
                continue;
            }
            let distance = (center.y - (y0 + y1) / 2.0).abs();
            if best.is_none_or(|(_, d)| distance < d) {
                best = Some((index, distance));
            }
        }
        match best {
            Some((index, _)) => assigned[index].push(word),
            None => stray += normalize(&word.text).chars().count(),
        }
    }

    let mut total = Score::default();
    let mut outcomes = Vec::new();
    for (line, words) in truth.iter().zip(assigned.iter_mut()) {
        let reference = normalize(&line.text);
        if reference.is_empty() && words.is_empty() {
            continue;
        }
        words.sort_by(|a, b| a.quad.center().x.total_cmp(&b.quad.center().x));
        let read: Vec<String> = words.iter().map(|word| normalize(&word.text)).collect();
        let read_line = read.join(" ");

        let expected: Vec<char> = reference.chars().collect();
        let got: Vec<char> = read_line.chars().collect();
        total.chars += expected.len();
        total.char_errors += levenshtein(&expected, &got);

        let expected_words: Vec<&str> = reference.split(' ').filter(|w| !w.is_empty()).collect();
        let got_words: Vec<&str> = read_line.split(' ').filter(|w| !w.is_empty()).collect();
        total.words += expected_words.len();
        total.word_errors += levenshtein(&expected_words, &got_words);

        let mut remaining = expected_words.clone();
        for (word, text) in words.iter().zip(&read) {
            let correct = match remaining.iter().position(|w| w == text) {
                Some(position) => {
                    remaining.swap_remove(position);
                    true
                }
                None => false,
            };
            outcomes.push(WordOutcome {
                confidence: word.confidence,
                correct,
            });
        }
    }
    total.char_errors += stray;
    (total, outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Quad;

    fn word(text: &str, x: f32, y: f32, confidence: f32) -> RecognizedWord {
        RecognizedWord {
            text: text.into(),
            quad: Quad::from_rect(x, y, x + 40.0, y + 20.0),
            confidence,
            line: 0,
        }
    }

    #[test]
    fn forme_canonique() {
        assert_eq!(
            normalize("  l’eﬃcacité\u{a0} \n du  cœur "),
            "l'efficacité du cœur"
        );
        // Accent composé (e + accent aigu) et précomposé : même forme.
        assert_eq!(normalize("e\u{301}t\u{e9}"), "\u{e9}t\u{e9}");
    }

    #[test]
    fn distance_d_edition() {
        assert_eq!(levenshtein(&['c', 'h', 'a', 't'], &['c', 'h', 'a', 't']), 0);
        assert_eq!(levenshtein(&['c', 'h', 'a', 't'], &['c', 'a', 't', 's']), 2);
        assert_eq!(levenshtein::<char>(&[], &['a']), 1);
    }

    #[test]
    fn compte_les_erreurs_ligne_par_ligne() {
        let truth = [
            TruthLine {
                text: "Le café coûte".into(),
                bounds: (0.0, 0.0, 300.0, 20.0),
            },
            TruthLine {
                text: "12,50 €".into(),
                bounds: (0.0, 40.0, 300.0, 60.0),
            },
        ];
        // Mots dans le désordre, un accent perdu, une tache lue en plus.
        let words = [
            word("coûte", 200.0, 0.0, 0.9),
            word("Le", 0.0, 0.0, 0.99),
            word("cafe", 100.0, 0.0, 0.4),
            word("12,50", 0.0, 40.0, 0.95),
            word("€", 100.0, 40.0, 0.95),
            word("~", 100.0, 200.0, 0.1),
        ];
        let (score, outcomes) = score(&truth, &words);
        assert_eq!(score.chars, 13 + 7);
        assert_eq!(score.char_errors, 2);
        assert_eq!(score.words, 5);
        assert_eq!(score.word_errors, 1);
        assert!((score.char_accuracy() - 0.9).abs() < 1e-9);
        let wrong: Vec<f32> = outcomes
            .iter()
            .filter(|o| !o.correct)
            .map(|o| o.confidence)
            .collect();
        assert_eq!(wrong, [0.4]);
    }
}
