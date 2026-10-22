//! Mise en page d'un texte remplacé dans son bloc : même origine, même
//! largeur, même alignement et même interligne que le texte d'origine.
//!
//! La mise en page se fait dans l'espace du texte du premier glyphe du bloc :
//! le nouveau texte reprend sa matrice, son corps, son échelle horizontale et
//! son espacement des caractères, si bien qu'il occupe la même place et suit
//! la même orientation.

use super::geometry::{Matrix, Rect};
use super::interp::Interpretation;
use super::objects::{Alignment, TextBlock};

/// Police qui dessine le nouveau texte.
pub trait Shaper {
    /// Code (octets) et chasse en millièmes de corps du caractère `c` ; `None`
    /// si la police ne le contient pas.
    fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)>;
    /// Hauteur des ascendantes, en millièmes de corps.
    fn ascent(&self) -> f64;
    /// Profondeur des descendantes (négative), en millièmes de corps.
    fn descent(&self) -> f64;
}

/// Élément d'une ligne mise en page.
#[derive(Clone, Debug, PartialEq)]
pub enum LaidItem {
    /// Codes de glyphes consécutifs.
    Codes(Vec<u8>),
    /// Décalage, en millièmes de corps (valeur d'un tableau `TJ`).
    Kern(f64),
}

/// Ligne mise en page, dans l'espace du texte du bloc.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidLine {
    /// Début de la ligne de base.
    pub x: f64,
    /// Ordonnée de la ligne de base.
    pub y: f64,
    /// Glyphes et décalages.
    pub items: Vec<LaidItem>,
    /// Longueur de la ligne, dans l'espace du texte.
    pub width: f64,
}

/// Texte mis en page.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Lignes, de haut en bas.
    pub lines: Vec<LaidLine>,
    /// Matrice de texte du premier glyphe d'origine (espace du texte →
    /// espace utilisateur).
    pub frame: Matrix,
    /// Indice de l'état graphique du premier glyphe d'origine.
    pub state: usize,
    /// Emprise du nouveau texte, dans l'espace de la page.
    pub bbox: Rect,
    /// Le texte ne tient pas dans le bloc d'origine (lignes en plus, ou mot
    /// plus large que le bloc).
    pub overflow: bool,
    /// Caractères absents de la police, omis.
    pub missing: Vec<char>,
}

/// Met en page `text` dans le bloc `block`, avec la police `shaper`.
pub fn layout(run: &Interpretation, block: &TextBlock, text: &str, shaper: &dyn Shaper) -> Layout {
    let first_glyph = &run.glyphs[block.lines[0].glyphs[0]];
    let state = &run.states[first_glyph.state];
    let frame = first_glyph.tm;
    let to_page = frame.then(&state.ctm);
    let to_text = to_page.inverse().unwrap_or(Matrix::IDENTITY);
    let size = state.font_size;
    let scale = state.h_scale;
    let spacing = state.char_spacing;

    // Lignes d'origine dans l'espace du texte : début, fin, ordonnée.
    let originals: Vec<(f64, f64, f64)> = block
        .lines
        .iter()
        .map(|line| {
            let start = to_text.apply(line.start);
            let end = to_text.apply(line.end);
            (start.x, end.x, start.y)
        })
        .collect();
    let count = originals.len();
    let leading = if count >= 2 {
        let mut steps: Vec<f64> = originals.windows(2).map(|p| p[0].2 - p[1].2).collect();
        steps.sort_by(f64::total_cmp);
        steps[steps.len() / 2]
    } else if state.leading > 0.0 {
        state.leading
    } else {
        1.2 * size
    };
    let left_first = originals[0].0;
    let left = originals.get(1).map_or(left_first, |line| line.0);
    let lowest_left = originals.iter().map(|l| l.0).fold(f64::INFINITY, f64::min);
    let right = originals
        .iter()
        .map(|l| l.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let wraps = count >= 2;

    let mut missing = Vec::new();
    let mut measure = |c: char| -> Option<(Vec<u8>, f64)> {
        match shaper.glyph(c) {
            Some((code, width)) => Some((code, (width / 1000.0 * size + spacing) * scale)),
            None => {
                if !missing.contains(&c) {
                    missing.push(c);
                }
                None
            }
        }
    };
    let space = shaper
        .glyph(' ')
        .map(|(code, width)| (Some(code), (width / 1000.0 * size + spacing) * scale))
        .unwrap_or((None, 0.25 * size * scale));
    let kern_for = |distance: f64| -> f64 {
        if size == 0.0 || scale == 0.0 {
            0.0
        } else {
            -distance * 1000.0 / (size * scale)
        }
    };

    struct Word {
        codes: Vec<Vec<u8>>,
        width: f64,
    }
    let mut lines: Vec<LaidLine> = Vec::new();
    let mut overflow = false;
    for paragraph in text.split('\n') {
        let words: Vec<Word> = paragraph
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(|word| {
                let mut codes = Vec::new();
                let mut width = 0.0;
                for c in word.chars() {
                    if let Some((code, advance)) = measure(c) {
                        codes.push(code);
                        width += advance;
                    }
                }
                Word { codes, width }
            })
            .collect();

        // Découpage glouton en lignes.
        let mut breaks: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        let mut current_width = 0.0;
        for (index, word) in words.iter().enumerate() {
            let line_index = lines.len() + breaks.len();
            let available = match block.alignment {
                Alignment::Left | Alignment::Justified if line_index == 0 => right - left_first,
                Alignment::Left | Alignment::Justified => right - left,
                Alignment::Right | Alignment::Center => right - lowest_left,
            };
            let added = if current.is_empty() {
                word.width
            } else {
                current_width + space.1 + word.width
            };
            if wraps && !current.is_empty() && added > available + 0.01 * size {
                breaks.push(std::mem::take(&mut current));
                current_width = word.width;
            } else {
                current_width = added;
            }
            if wraps && word.width > available + 0.01 * size {
                overflow = true;
            }
            current.push(index);
        }
        breaks.push(current);

        let last = breaks.len() - 1;
        for (break_index, members) in breaks.into_iter().enumerate() {
            let k = lines.len();
            let y = match originals.get(k) {
                Some(line) => line.2,
                None => originals[count - 1].2 - leading * (k + 1 - count) as f64,
            };
            let width: f64 = members.iter().map(|&w| words[w].width).sum::<f64>()
                + space.1 * members.len().saturating_sub(1) as f64;
            let line_left = if k == 0 { left_first } else { left };
            let available = right - line_left;
            let x = match block.alignment {
                _ if !wraps => left_first,
                Alignment::Left | Alignment::Justified => line_left,
                Alignment::Right => right - width,
                Alignment::Center => (lowest_left + right) / 2.0 - width / 2.0,
            };
            let gaps = members.len().saturating_sub(1);
            let extra = if block.alignment == Alignment::Justified
                && wraps
                && break_index != last
                && gaps > 0
                && available > width
            {
                (available - width) / gaps as f64
            } else {
                0.0
            };
            let mut items: Vec<LaidItem> = Vec::new();
            for (position, &w) in members.iter().enumerate() {
                if position > 0 {
                    match &space.0 {
                        Some(code) => push_codes(&mut items, code),
                        None => push_kern(&mut items, kern_for(space.1)),
                    }
                    if extra > 0.0 {
                        push_kern(&mut items, kern_for(extra));
                    }
                }
                for code in &words[w].codes {
                    push_codes(&mut items, code);
                }
            }
            let width = width + extra * gaps as f64;
            lines.push(LaidLine { x, y, items, width });
        }
    }
    if lines.len() > count {
        overflow = true;
    }

    let (ascent, descent) = (
        shaper.ascent() / 1000.0 * size,
        shaper.descent() / 1000.0 * size,
    );
    let bbox = lines.iter().fold(Rect::EMPTY, |rect, line| {
        let extent = Rect::new(
            line.x,
            line.y + descent,
            line.x + line.width,
            line.y + ascent,
        );
        rect.union(&to_page.apply_rect(extent))
    });

    Layout {
        lines,
        frame,
        state: first_glyph.state,
        bbox,
        overflow,
        missing,
    }
}

fn push_codes(items: &mut Vec<LaidItem>, code: &[u8]) {
    if let Some(LaidItem::Codes(codes)) = items.last_mut() {
        codes.extend_from_slice(code);
    } else {
        items.push(LaidItem::Codes(code.to_vec()));
    }
}

fn push_kern(items: &mut Vec<LaidItem>, kern: f64) {
    if kern == 0.0 {
        return;
    }
    if let Some(LaidItem::Kern(previous)) = items.last_mut() {
        *previous += kern;
    } else {
        items.push(LaidItem::Kern(kern));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::interp::{interpret, tests::test_font, tests::TestResources};
    use crate::content::lexer::parse;
    use crate::content::objects::build;

    /// Police de test : caractères ASCII, un octet par code, chasse 500.
    struct Ascii;

    impl Shaper for Ascii {
        fn glyph(&self, c: char) -> Option<(Vec<u8>, f64)> {
            c.is_ascii().then(|| (vec![c as u8], 500.0))
        }
        fn ascent(&self) -> f64 {
            800.0
        }
        fn descent(&self) -> f64 {
            -200.0
        }
    }

    fn lay(source: &[u8], object: usize, text: &str) -> Layout {
        let ops = parse(source);
        let mut resources = TestResources { font: test_font() };
        let page = Rect::new(0.0, 0.0, 600.0, 800.0);
        let run = interpret(&ops, &mut resources, page);
        let objects = build(&ops, &run, page);
        layout(&run, objects[object].text.as_ref().unwrap(), text, &Ascii)
    }

    const JUSTIFIED: &[u8] =
        b"BT /F1 10 Tf 12 TL 100 700 Td (aaaa bbbb cc) Tj T* (dddd eeee ff) Tj \
        T* (gggg hhhh ii) Tj T* (jj kk) Tj ET";

    #[test]
    fn justifie_les_lignes_sauf_la_derniere() {
        let layout = lay(JUSTIFIED, 0, "aa bb cccc dddd");
        assert_eq!(layout.lines.len(), 2);
        let first = &layout.lines[0];
        assert_eq!((first.x, first.y), (0.0, 0.0));
        assert_eq!(
            first.items,
            [
                LaidItem::Codes(b"aa ".to_vec()),
                LaidItem::Kern(-500.0),
                LaidItem::Codes(b"bb ".to_vec()),
                LaidItem::Kern(-500.0),
                LaidItem::Codes(b"cccc".to_vec()),
            ]
        );
        assert!((first.width - 60.0).abs() < 1e-9);
        let second = &layout.lines[1];
        assert_eq!((second.x, second.y), (0.0, -12.0));
        assert_eq!(second.items, [LaidItem::Codes(b"dddd".to_vec())]);
        assert!(!layout.overflow);
        assert_eq!(layout.frame, Matrix::new(1.0, 0.0, 0.0, 1.0, 100.0, 700.0));
    }

    #[test]
    fn signale_le_debordement() {
        let layout = lay(
            JUSTIFIED,
            0,
            "aaaa bbbb cc dddd eeee ff gggg hhhh ii jjjj kkkk ll mm",
        );
        assert_eq!(layout.lines.len(), 5);
        assert_eq!(layout.lines[4].y, -48.0);
        assert!(layout.overflow);
        // Un mot plus large que le bloc déborde aussi.
        assert!(lay(JUSTIFIED, 0, "aaaaaaaaaaaaaaaaaaaa").overflow);
    }

    #[test]
    fn ligne_isolee_et_caracteres_absents() {
        let layout = lay(b"BT /F1 20 Tf 100 750 Td (Titre) Tj ET", 0, "abc\u{e9}");
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].items, [LaidItem::Codes(b"abc".to_vec())]);
        assert_eq!(layout.missing, ['\u{e9}']);
        assert!(!layout.overflow);
        let bbox = layout.bbox;
        assert!((bbox.x0 - 100.0).abs() < 1e-9 && (bbox.x1 - 130.0).abs() < 1e-9);
        assert!((bbox.y0 - 746.0).abs() < 1e-9 && (bbox.y1 - 766.0).abs() < 1e-9);
    }

    #[test]
    fn paragraphes_et_ligne_vide() {
        let layout = lay(
            b"BT /F1 10 Tf 12 TL 50 500 Td (Jean Dupont) Tj T* (12 rue) Tj T* (Paris) Tj ET",
            0,
            "Jean\n\nLyon",
        );
        assert_eq!(layout.lines.len(), 3);
        assert!(layout.lines[1].items.is_empty());
        assert_eq!(layout.lines[2].y, -24.0);
    }
}
