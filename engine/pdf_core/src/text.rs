//! Texte des pages et recherche insensible à la casse et aux accents.
//!
//! Le texte de chaque page est « replié » une fois pour toutes : minuscules,
//! accents retirés, ligatures développées, guillemets, apostrophes et tirets
//! unifiés, espaces fusionnées. La requête subit le même traitement, puis une
//! simple recherche de sous-chaîne suffit. Chaque octet du texte replié garde
//! la trace du glyphe dont il provient, ce qui donne les rectangles à surligner.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Rectangle sur une page, en points, origine en haut à gauche (rotation `/Rotate` comprise).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    /// Bord gauche.
    pub x: f32,
    /// Bord supérieur.
    pub y: f32,
    /// Largeur.
    pub width: f32,
    /// Hauteur.
    pub height: f32,
}

impl PageRect {
    pub(crate) fn from_quad(quad: mupdf::Quad) -> Self {
        let xs = [quad.ul.x, quad.ur.x, quad.ll.x, quad.lr.x];
        let ys = [quad.ul.y, quad.ur.y, quad.ll.y, quad.lr.y];
        let x0 = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let x1 = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let y0 = ys.iter().copied().fold(f32::INFINITY, f32::min);
        let y1 = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.width).max(other.x + other.width);
        let y1 = (self.y + self.height).max(other.y + other.height);
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }
}

/// Une ligne de texte de la page et son emprise.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// Texte de la ligne.
    pub text: String,
    /// Emprise, dans l'espace d'affichage de la page.
    pub rect: PageRect,
}

/// Une occurrence trouvée sur une page : un rectangle par ligne de texte touchée.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMatch {
    /// Rectangles à surligner, dans l'ordre de lecture.
    pub rects: Vec<PageRect>,
}

/// Marque un caractère replié qui ne vient d'aucun glyphe (espace entre deux lignes).
const NO_GLYPH: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
struct Glyph {
    rect: PageRect,
    line: u32,
}

/// Texte d'une page, préparé pour la recherche.
#[derive(Debug, Default, Clone)]
pub struct PageText {
    glyphs: Vec<Glyph>,
    folded: String,
    /// Glyphe d'origine de chaque octet de `folded`.
    sources: Vec<u32>,
}

impl PageText {
    /// Extrait le texte d'une page structurée par MuPDF.
    pub(crate) fn from_text_page(page: &mupdf::TextPage) -> Self {
        let mut builder = Builder::default();
        for block in page.blocks() {
            if block.r#type() != mupdf::text_page::TextBlockType::Text {
                continue;
            }
            for line in block.lines() {
                builder.start_line();
                for ch in line.chars() {
                    let Some(c) = ch.char() else { continue };
                    builder.push(c, PageRect::from_quad(ch.quad()));
                }
            }
        }
        builder.finish()
    }

    /// Vrai si la page ne contient aucun texte natif (page scannée, par exemple).
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Ce texte suivi de lignes lues par la reconnaissance de texte : chaque
    /// ligne est une suite de mots avec leur emprise. Les caractères d'un mot
    /// se partagent sa largeur.
    pub fn with_lines(&self, lines: &[Vec<(String, PageRect)>]) -> PageText {
        let mut builder = Builder {
            text: self.clone(),
            line: self.glyphs.last().map_or(0, |glyph| glyph.line),
            ..Builder::default()
        };
        for line in lines {
            builder.start_line();
            let mut previous: Option<PageRect> = None;
            for (word, rect) in line {
                if let Some(before) = previous {
                    let x = before.x + before.width;
                    let gap = PageRect {
                        x,
                        y: rect.y,
                        width: (rect.x - x).max(0.0),
                        height: rect.height,
                    };
                    builder.push(' ', gap);
                }
                let count = word.chars().count().max(1) as f32;
                for (k, c) in word.chars().enumerate() {
                    let width = rect.width / count;
                    let glyph = PageRect {
                        x: rect.x + width * k as f32,
                        width,
                        ..*rect
                    };
                    builder.push(c, glyph);
                }
                previous = Some(*rect);
            }
        }
        builder.finish()
    }

    /// Cherche toutes les occurrences de `query`, sans tenir compte de la casse,
    /// des accents ni des ligatures.
    pub fn search(&self, query: &str) -> Vec<TextMatch> {
        let needle = fold_query(query);
        if needle.is_empty() {
            return Vec::new();
        }
        self.folded
            .match_indices(&needle)
            .filter_map(|(start, found)| self.match_at(start, found.len()))
            .collect()
    }

    fn match_at(&self, start: usize, len: usize) -> Option<TextMatch> {
        let mut rects: Vec<PageRect> = Vec::new();
        let mut current_line = None;
        let mut previous = NO_GLYPH;
        for &index in &self.sources[start..start + len] {
            if index == NO_GLYPH || index == previous {
                continue;
            }
            previous = index;
            let glyph = self.glyphs[index as usize];
            match rects.last_mut() {
                Some(rect) if current_line == Some(glyph.line) => *rect = rect.union(glyph.rect),
                _ => {
                    rects.push(glyph.rect);
                    current_line = Some(glyph.line);
                }
            }
        }
        (!rects.is_empty()).then_some(TextMatch { rects })
    }
}

/// Trait d'union terminant une ligne.
#[derive(Clone, Copy, PartialEq)]
enum Hyphen {
    /// Trait d'union visible, recopié dans le texte replié.
    Visible,
    /// Trait d'union conditionnel (U+00AD), absent du texte replié.
    Soft,
}

#[derive(Default)]
struct Builder {
    text: PageText,
    line: u32,
    /// Une nouvelle ligne commence : séparer du texte précédent.
    at_line_start: bool,
    /// Trait d'union qui terminait la ligne précédente.
    hyphen: Option<Hyphen>,
    /// Dernier caractère source de la ligne en cours.
    last_char: Option<char>,
}

impl Builder {
    fn start_line(&mut self) {
        if !self.text.glyphs.is_empty() {
            self.line += 1;
            self.at_line_start = true;
            self.hyphen = match self.last_char {
                Some('\u{00AD}') => Some(Hyphen::Soft),
                Some('-' | '\u{2010}') => Some(Hyphen::Visible),
                _ => None,
            };
        }
        self.last_char = None;
    }

    fn push(&mut self, c: char, rect: PageRect) {
        if std::mem::take(&mut self.at_line_start) {
            self.separate_lines(c);
        }
        let index = self.text.glyphs.len() as u32;
        self.text.glyphs.push(Glyph {
            rect,
            line: self.line,
        });
        self.last_char = Some(c);
        if c.is_whitespace() {
            self.push_space(index);
        } else {
            fold_char(c, |folded| self.push_folded(folded, index));
        }
    }

    /// Sépare deux lignes par une espace, sauf pour un mot coupé en fin de ligne
    /// (lettre, trait d'union, puis minuscule au début de la ligne suivante),
    /// qui est recollé.
    fn separate_lines(&mut self, next: char) {
        let folded = &self.text.folded;
        let letter_before = |skip| {
            folded
                .chars()
                .rev()
                .nth(skip)
                .is_some_and(char::is_alphabetic)
        };
        let rejoin = next.is_lowercase()
            && match self.hyphen {
                Some(Hyphen::Visible) => letter_before(1),
                Some(Hyphen::Soft) => letter_before(0),
                None => false,
            };
        if !rejoin {
            self.push_space(NO_GLYPH);
        } else if self.hyphen == Some(Hyphen::Visible) {
            self.text.folded.pop();
            self.text.sources.pop();
        }
    }

    fn push_space(&mut self, source: u32) {
        if self.text.folded.is_empty() || self.text.folded.ends_with(' ') {
            return;
        }
        self.text.folded.push(' ');
        self.text.sources.push(source);
    }

    fn push_folded(&mut self, c: char, source: u32) {
        self.text.folded.push(c);
        self.text
            .sources
            .extend(std::iter::repeat_n(source, c.len_utf8()));
    }

    fn finish(mut self) -> PageText {
        if self.text.folded.ends_with(' ') {
            self.text.folded.pop();
            self.text.sources.pop();
        }
        self.text
    }
}

/// Replie une requête comme le texte des pages.
fn fold_query(query: &str) -> String {
    let mut folded = String::with_capacity(query.len());
    for c in query.chars() {
        if c.is_whitespace() {
            if !folded.is_empty() && !folded.ends_with(' ') {
                folded.push(' ');
            }
        } else {
            fold_char(c, |f| folded.push(f));
        }
    }
    folded.truncate(folded.trim_end().len());
    folded
}

/// Replie un caractère (hors espaces) : il peut en produire zéro, un ou plusieurs.
fn fold_char(c: char, mut emit: impl FnMut(char)) {
    let expansion = match c {
        // Caractères invisibles.
        '\u{00AD}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' => "",
        'ﬀ' => "ff",
        'ﬁ' => "fi",
        'ﬂ' => "fl",
        'ﬃ' => "ffi",
        'ﬄ' => "ffl",
        'ﬅ' | 'ﬆ' => "st",
        'œ' | 'Œ' => "oe",
        'æ' | 'Æ' => "ae",
        'ß' | 'ẞ' => "ss",
        'ſ' => "s",
        'ø' | 'Ø' => "o",
        'ł' | 'Ł' => "l",
        'đ' | 'Đ' => "d",
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' | '\u{02BC}' => "'",
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' | '«' | '»' => "\"",
        '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{FE63}' | '\u{FF0D}' => "-",
        _ => {
            for lower in c.to_lowercase() {
                for part in std::iter::once(lower).nfd() {
                    if !is_combining_mark(part) {
                        emit(part);
                    }
                }
            }
            return;
        }
    };
    expansion.chars().for_each(emit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32) -> PageRect {
        PageRect {
            x,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    fn page(lines: &[&str]) -> PageText {
        let mut builder = Builder::default();
        for line in lines {
            builder.start_line();
            for (i, c) in line.chars().enumerate() {
                builder.push(c, rect(i as f32));
            }
        }
        builder.finish()
    }

    #[test]
    fn repli_des_accents_ligatures_et_signes() {
        assert_eq!(fold_query("  L'Œuvre  ÉTÉ "), "l'oeuvre ete");
        assert_eq!(fold_query("ﬁn — « ça »"), "fin - \" ca \"");
        assert_eq!(fold_query("Straße"), "strasse");
        assert_eq!(fold_query("l’été"), "l'ete");
    }

    #[test]
    fn recherche_sur_plusieurs_lignes() {
        let text = page(&["Le chat dort", "sur le tapis."]);
        let found = text.search("DORT SUR");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rects.len(), 2, "une zone par ligne");
        assert_eq!(text.search("dortsur").len(), 0);
    }

    #[test]
    fn mot_coupe_en_fin_de_ligne() {
        let text = page(&["une grande effi-", "cacité"]);
        assert_eq!(text.search("efficacite").len(), 1);
        let text = page(&["Paris-", "Lyon"]);
        assert_eq!(
            text.search("paris- lyon").len(),
            1,
            "majuscule : pas de recollage"
        );
    }

    #[test]
    fn occurrences_multiples_et_requete_vide() {
        let text = page(&["abc abc", "ABC"]);
        assert_eq!(text.search("abc").len(), 3);
        assert!(text.search("   ").is_empty());
        assert!(PageText::default().search("abc").is_empty());
    }

    #[test]
    fn rectangle_d_une_ligature() {
        let text = page(&["ﬁn"]);
        let found = text.search("in");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].rects[0].x, 0.0,
            "la ligature entière est surlignée"
        );
        assert_eq!(found[0].rects[0].width, 2.0);
    }

    #[test]
    fn recherche_dans_une_couche_reconnue() {
        let native = page(&["Titre"]);
        let word = |text: &str, x: f32| {
            (
                text.to_owned(),
                PageRect {
                    x,
                    y: 50.0,
                    width: 30.0,
                    height: 10.0,
                },
            )
        };
        let text = native.with_lines(&[vec![word("Facture", 0.0), word("n°", 40.0)]]);
        assert_eq!(text.search("titre").len(), 1);
        let found = text.search("facture n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rects.len(), 1);
        assert_eq!(found[0].rects[0].x, 0.0);
        // « n » : premier des deux caractères de « n° ».
        assert_eq!(found[0].rects[0].width, 55.0);
        assert!(
            text.search("titre facture").len() == 1,
            "lignes séparées par une espace"
        );
    }
}
