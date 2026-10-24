//! Relecture des mots oubliés par le moteur.
//!
//! Vision laisse parfois de côté un mot court isolé par de grands blancs
//! (texte justifié à chasse fixe : « Vous », « Les », « ne » en début de
//! ligne). Une ligne lue garde alors, dans sa bande, de l'encre qu'aucun mot
//! ne couvre. Ces morceaux d'encre sont découpés, réunis dans une seule image
//! (un appel au moteur pour toute la page) et relus hors de leur contexte ;
//! les mots trouvés rejoignent leur ligne.

use crate::engine::{OcrEngine, OcrError, Point, Quad, RecognizedWord};
use crate::image::GrayImage;

/// Nombre maximal de morceaux relus par page.
const MAX_PIECES: usize = 200;

/// Une ligne lue : ses mots et sa bande dans l'image.
struct Row {
    /// Indices des mots, de gauche à droite.
    words: Vec<usize>,
    top: f32,
    bottom: f32,
    /// Hauteur médiane des mots.
    height: f32,
}

/// Morceau d'encre non lu : colonnes `[x0, x1)` de la bande de la ligne `row`.
struct Piece {
    row: usize,
    x0: u32,
    x1: u32,
    /// Place du morceau dans l'image recomposée : décalage à ajouter aux
    /// coordonnées de l'image d'origine.
    dx: f32,
    dy: f32,
    /// Rangées de l'image recomposée qu'il occupe.
    slot: (f32, f32),
}

/// Mots que `engine` n'a pas lus dans `image` alors que leur encre est dans
/// la bande d'une ligne de `words` ; `image` est droite (redressée).
pub fn missed_words(
    engine: &dyn OcrEngine,
    image: &GrayImage,
    words: &[RecognizedWord],
    languages: &[String],
) -> Result<Vec<RecognizedWord>, OcrError> {
    let rows = rows(words);
    let limit = ink_limit(image);
    let mut pieces = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        pieces.extend(
            row_pieces(image, words, row, limit)
                .into_iter()
                .map(|(x0, x1)| Piece {
                    row: index,
                    x0,
                    x1,
                    dx: 0.0,
                    dy: 0.0,
                    slot: (0.0, 0.0),
                }),
        );
        if pieces.len() >= MAX_PIECES {
            break;
        }
    }
    if pieces.is_empty() {
        return Ok(Vec::new());
    }
    let composite = compose(image, &rows, &mut pieces);
    let found = engine.recognize(&composite, languages)?;
    let mut out: Vec<RecognizedWord> = Vec::new();
    for word in found {
        let center = word.quad.center();
        let Some(piece) = pieces
            .iter()
            .find(|p| center.y >= p.slot.0 && center.y < p.slot.1)
        else {
            continue;
        };
        let quad = word.quad.map(|p| Point {
            x: p.x - piece.dx,
            y: p.y - piece.dy,
        });
        let c = quad.center();
        let row = &rows[piece.row];
        let text = word.text.trim();
        if c.x < piece.x0 as f32 - 2.0
            || c.x > piece.x1 as f32 + 2.0
            || !text.chars().any(char::is_alphanumeric)
            || covered(words, &quad)
            || covered(&out, &quad)
        {
            continue;
        }
        // Le numéro de ligne du mot lu le plus proche.
        let nearest = row
            .words
            .iter()
            .min_by(|&&a, &&b| {
                let d = |i: usize| (words[i].quad.center().x - c.x).abs();
                d(a).total_cmp(&d(b))
            })
            .map_or(0, |&i| words[i].line);
        out.push(RecognizedWord {
            text: text.to_owned(),
            quad,
            confidence: word.confidence,
            line: nearest,
        });
    }
    Ok(out)
}

/// Ramène à leur ligne les mots dont l'emprise est bien plus haute que les
/// autres : Vision donne parfois à une ligne une emprise qui couvre aussi la
/// ligne du dessus. Le haut est reporté au blanc qui sépare les deux lignes,
/// le plus près d'une hauteur ordinaire au-dessus du bas (qui, lui, est juste).
pub fn tighten(image: &GrayImage, words: &mut [RecognizedWord]) {
    let mut heights: Vec<f32> = words.iter().map(|w| w.quad.height()).collect();
    heights.sort_by(f32::total_cmp);
    let Some(&typical) = heights.get(heights.len() / 2) else {
        return;
    };
    if typical <= 1.0 {
        return;
    }
    let limit = ink_limit(image);
    for word in words.iter_mut() {
        let (x0, top, x1, bottom) = word.quad.bounds();
        if bottom - top <= 1.6 * typical {
            continue;
        }
        let from = (x0.max(0.0) as u32).min(image.width);
        let to = (x1.ceil().max(0.0) as u32).min(image.width);
        let blank = |y: u32| (from..to).all(|x| image.get(x, y) >= limit);
        let target = bottom - 1.1 * typical;
        let low = (bottom - 1.5 * typical).max(0.0) as u32;
        let high = ((bottom - 0.6 * typical).max(0.0) as u32).min(image.height);
        let cut = (low..high)
            .filter(|&y| blank(y))
            .min_by(|&a, &b| {
                (a as f32 - target)
                    .abs()
                    .total_cmp(&(b as f32 - target).abs())
            })
            .map_or(target, |y| y as f32);
        word.quad.top_left.y = word.quad.top_left.y.max(cut);
        word.quad.top_right.y = word.quad.top_right.y.max(cut);
    }
}

/// Insère les mots relus `found` parmi `words`, chacun dans sa ligne, à sa
/// place de gauche à droite.
pub fn insert(words: &mut Vec<RecognizedWord>, found: Vec<RecognizedWord>) {
    for word in found {
        let x = word.quad.center().x;
        let same: Vec<usize> = (0..words.len())
            .filter(|&i| words[i].line == word.line)
            .collect();
        let at = same
            .iter()
            .find(|&&i| words[i].quad.center().x > x)
            .copied()
            .or_else(|| same.last().map(|&i| i + 1))
            .unwrap_or(words.len());
        words.insert(at, word);
    }
}

/// Lignes de la page : mots dont le centre est à la hauteur de la ligne.
fn rows(words: &[RecognizedWord]) -> Vec<Row> {
    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by(|&a, &b| {
        words[a]
            .quad
            .center()
            .y
            .total_cmp(&words[b].quad.center().y)
    });
    let mut rows: Vec<Row> = Vec::new();
    for i in order {
        let (_, top, _, bottom) = words[i].quad.bounds();
        let center = (top + bottom) / 2.0;
        let height = bottom - top;
        let found = rows.iter_mut().rev().take(4).find(|row| {
            let middle = (row.top + row.bottom) / 2.0;
            (center - middle).abs() <= 0.35 * row.height.min(height)
        });
        match found {
            Some(row) => row.words.push(i),
            None => rows.push(Row {
                words: vec![i],
                top,
                bottom,
                height,
            }),
        }
    }
    for row in &mut rows {
        let median = |mut values: Vec<f32>| {
            values.sort_by(f32::total_cmp);
            values[values.len() / 2]
        };
        let bounds: Vec<_> = row.words.iter().map(|&i| words[i].quad.bounds()).collect();
        row.top = median(bounds.iter().map(|b| b.1).collect());
        row.bottom = median(bounds.iter().map(|b| b.3).collect());
        row.height = (row.bottom - row.top).max(1.0);
        row.words.sort_by(|&a, &b| {
            words[a]
                .quad
                .center()
                .x
                .total_cmp(&words[b].quad.center().x)
        });
    }
    rows
}

/// Niveau au-dessous duquel un pixel est de l'encre : nettement plus sombre
/// que le papier (niveau médian de l'image).
fn ink_limit(image: &GrayImage) -> u8 {
    let mut histogram = [0usize; 256];
    for &v in &image.pixels {
        histogram[v as usize] += 1;
    }
    let half = image.pixels.len() / 2;
    let mut seen = 0;
    for (level, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= half {
            return (level as u8).saturating_sub(80);
        }
    }
    128
}

/// Morceaux d'encre de la bande de `row` qu'aucun de ses mots ne couvre,
/// en colonnes `[x0, x1)`.
fn row_pieces(
    image: &GrayImage,
    words: &[RecognizedWord],
    row: &Row,
    limit: u8,
) -> Vec<(u32, u32)> {
    let h = row.height;
    let (w, ih) = (image.width as usize, image.height as i64);
    let clamp_y = |y: f32| (y.round() as i64).clamp(0, ih) as u32;
    // Cœur de la bande : les jambages des lignes voisines n'y entrent pas.
    let (c0, c1) = (clamp_y(row.top + 0.15 * h), clamp_y(row.bottom - 0.15 * h));
    // Bande élargie, pour la hauteur de l'encre.
    let (b0, b1) = (clamp_y(row.top - 0.1 * h), clamp_y(row.bottom + 0.1 * h));
    if c1 <= c0 {
        return Vec::new();
    }
    let mut covered = vec![false; w];
    for &i in &row.words {
        let (x0, _, x1, _) = words[i].quad.bounds();
        let from = (x0.floor().max(0.0) as usize).saturating_sub(1).min(w);
        let to = ((x1.ceil().max(0.0) as usize) + 1).min(w);
        covered[from..to].iter_mut().for_each(|c| *c = true);
    }
    let ink: Vec<bool> = (0..w)
        .map(|x| (c0..c1).any(|y| image.get(x as u32, y) < limit))
        .collect();
    // Suites de colonnes d'encre, coupées aux blancs d'au moins le quart de la
    // hauteur (l'espace entre deux mots).
    let letter_gap = (0.25 * h).ceil() as usize;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut x = 0;
    while x < w {
        if !ink[x] {
            x += 1;
            continue;
        }
        let start = x;
        let mut end = x + 1;
        let mut blank = 0;
        x += 1;
        while x < w && blank < letter_gap {
            if ink[x] {
                end = x + 1;
                blank = 0;
            } else {
                blank += 1;
            }
            x += 1;
        }
        runs.push((start, end));
        x = end;
    }
    // Un morceau surtout couvert prolonge un mot lu (lettre hors de son
    // emprise) : il n'est pas relu. Un filet ou des tirets n'ont pas la
    // hauteur d'un texte.
    let tall = |a: usize, b: usize| {
        let inked: Vec<u32> = (b0..b1)
            .filter(|&y| (a..b).any(|x| image.get(x as u32, y) < limit))
            .collect();
        matches!((inked.first(), inked.last()), (Some(f), Some(l)) if (l - f + 1) as f32 >= 0.35 * h)
    };
    let pieces: Vec<(usize, usize)> = runs
        .into_iter()
        .filter(|&(a, b)| {
            let inked = (a..b).filter(|&x| ink[x]).count();
            let free = (a..b).filter(|&x| ink[x] && !covered[x]).count();
            2 * free > inked && tall(a, b)
        })
        .collect();
    // Morceaux voisins (une suite de mots oubliés), sans mot lu entre eux.
    let word_gap = (1.2 * h) as usize;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in pieces {
        if let Some(last) = merged.last_mut() {
            if a - last.1 <= word_gap && !covered[last.1..a].iter().any(|&c| c) {
                last.1 = b;
                continue;
            }
        }
        merged.push((a, b));
    }
    merged
        .into_iter()
        .filter(|&(a, b)| (b - a) as f32 >= 0.3 * h)
        .map(|(a, b)| (a as u32, b as u32))
        .collect()
}

/// Vrai si `quad` recouvre en grande partie un mot de `words`.
fn covered(words: &[RecognizedWord], quad: &Quad) -> bool {
    let (x0, y0, x1, y1) = quad.bounds();
    let area = ((x1 - x0) * (y1 - y0)).max(1.0);
    words.iter().any(|word| {
        let (a0, b0, a1, b1) = word.quad.bounds();
        let w = (x1.min(a1) - x0.max(a0)).max(0.0);
        let h = (y1.min(b1) - y0.max(b0)).max(0.0);
        w * h > 0.5 * area.min(((a1 - a0) * (b1 - b0)).max(1.0))
    })
}

/// Image recomposée : chaque morceau, entouré de papier, l'un sous l'autre.
/// Seule l'encre du morceau est recopiée, pas celle des mots voisins.
fn compose(image: &GrayImage, rows: &[Row], pieces: &mut [Piece]) -> GrayImage {
    let tallest = pieces
        .iter()
        .map(|p| rows[p.row].height)
        .fold(0.0f32, f32::max);
    let margin = tallest.ceil();
    let width = pieces
        .iter()
        .map(|p| (p.x1 - p.x0) as f32 + 2.0 * margin)
        .fold(0.0f32, f32::max)
        .ceil() as u32;
    let mut cursor = margin;
    let mut spans = Vec::with_capacity(pieces.len());
    for piece in pieces.iter_mut() {
        let row = &rows[piece.row];
        let top = (row.top - 0.25 * row.height).max(0.0).floor();
        let bottom = (row.bottom + 0.25 * row.height)
            .min(image.height as f32)
            .ceil();
        piece.dx = margin - piece.x0 as f32;
        piece.dy = cursor - top;
        piece.slot = (
            cursor - 0.5 * margin,
            cursor + (bottom - top) + 0.5 * margin,
        );
        spans.push((top as u32, bottom as u32));
        cursor += (bottom - top) + margin;
    }
    let height = (cursor + margin).ceil() as u32;
    let mut out = GrayImage::new(width.max(1), height.max(1), 255);
    for (piece, &(top, bottom)) in pieces.iter().zip(&spans) {
        let from = piece.x0.saturating_sub(2);
        let to = (piece.x1 + 2).min(image.width);
        for y in top..bottom {
            let ty = (y as f32 + piece.dy) as u32;
            for x in from..to {
                let tx = (x as f32 + piece.dx).round();
                if tx >= 0.0 && (tx as u32) < out.width && ty < out.height {
                    out.pixels[(ty * out.width + tx as u32) as usize] = image.get(x, y);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Page : trois « mots » (blocs d'encre) sur une ligne ; le moteur ne lit
    /// d'abord que les deux derniers, puis, dans l'image recomposée, tout bloc
    /// d'encre.
    struct Forgetful {
        calls: Cell<u32>,
    }

    fn blocks(image: &GrayImage) -> Vec<(u32, u32, u32, u32)> {
        // Colonnes d'encre regroupées en blocs, sur toute la hauteur.
        let mut out = Vec::new();
        let mut x = 0;
        while x < image.width {
            let inked = |x: u32| (0..image.height).any(|y| image.get(x, y) < 128);
            if !inked(x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < image.width && inked(x) {
                x += 1;
            }
            let rows: Vec<u32> = (0..image.height)
                .filter(|&y| (start..x).any(|c| image.get(c, y) < 128))
                .collect();
            out.push((start, rows[0], x, rows[rows.len() - 1] + 1));
        }
        out
    }

    impl OcrEngine for Forgetful {
        fn name(&self) -> String {
            "oublieux".into()
        }

        fn recognize(
            &self,
            image: &GrayImage,
            _languages: &[String],
        ) -> Result<Vec<RecognizedWord>, OcrError> {
            let call = self.calls.get();
            self.calls.set(call + 1);
            // Les tirets ne sont pas lus.
            let found: Vec<_> = blocks(image)
                .into_iter()
                .filter(|b| b.3 - b.1 >= 10)
                .collect();
            let skip = if call == 0 { 1 } else { 0 };
            Ok(found
                .into_iter()
                .skip(skip)
                .enumerate()
                .map(|(n, (x0, y0, x1, y1))| RecognizedWord {
                    text: format!("mot{n}"),
                    // Emprise de ligne : plus haute que l'encre.
                    quad: Quad::from_rect(x0 as f32, y0 as f32 - 6.0, x1 as f32, y1 as f32 + 6.0),
                    confidence: 0.9,
                    line: 3,
                })
                .collect())
        }
    }

    fn page() -> GrayImage {
        let mut image = GrayImage::new(400, 120, 250);
        // Trois mots de 24 pixels de haut.
        for (x0, x1) in [(20, 60), (150, 230), (250, 330)] {
            for y in 40..64 {
                for x in x0..x1 {
                    image.pixels[y * 400 + x] = 20;
                }
            }
        }
        image
    }

    #[test]
    fn relit_un_mot_isole() {
        let image = page();
        let engine = Forgetful {
            calls: Cell::new(0),
        };
        let mut words = engine.recognize(&image, &[]).unwrap();
        assert_eq!(words.len(), 2);
        let found = missed_words(&engine, &image, &words, &[]).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        let (x0, _, x1, _) = found[0].quad.bounds();
        assert!(
            (x0 - 20.0).abs() < 1.0 && (x1 - 60.0).abs() < 2.0,
            "{found:?}"
        );
        assert_eq!(found[0].line, 3);
        insert(&mut words, found);
        let order: Vec<f32> = words.iter().map(|w| w.quad.bounds().0).collect();
        assert!(order.windows(2).all(|p| p[0] < p[1]), "{order:?}");
    }

    #[test]
    fn ramene_une_emprise_trop_haute() {
        let image = page();
        let word = |x0: f32, y0: f32| RecognizedWord {
            text: "mot".into(),
            quad: Quad::from_rect(x0, y0, x0 + 30.0, 70.0),
            confidence: 0.9,
            line: 0,
        };
        // Deux mots ordinaires, un troisième qui monte deux fois plus haut.
        let mut words = vec![word(20.0, 34.0), word(150.0, 34.0), word(250.0, 0.0)];
        // Encre au-dessus de la ligne, dans la colonne du troisième mot.
        let mut image = image;
        for y in 2..14 {
            for x in 250..280 {
                image.pixels[y * 400 + x] = 20;
            }
        }
        tighten(&image, &mut words);
        let (_, top, _, bottom) = words[2].quad.bounds();
        assert!((14.0..=40.0).contains(&top) && bottom == 70.0, "{top}");
        assert_eq!(words[0].quad.bounds().1, 34.0);
    }

    #[test]
    fn ignore_les_tirets_et_les_mots_lus() {
        let mut image = page();
        // Tirets à hauteur de la ligne : trop bas pour un texte.
        for x in 70..140 {
            if x % 8 < 5 {
                for y in 54..57 {
                    image.pixels[y * 400 + x] = 20;
                }
            }
        }
        let engine = Forgetful {
            calls: Cell::new(0),
        };
        let words = engine.recognize(&image, &[]).unwrap();
        let found = missed_words(&engine, &image, &words, &[]).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        let (x0, _, x1, _) = found[0].quad.bounds();
        assert!(x0 >= 19.0 && x1 <= 62.0, "{found:?}");
        // Tout est lu : rien à relire.
        let engine = Forgetful {
            calls: Cell::new(1),
        };
        let words = engine.recognize(&image, &[]).unwrap();
        assert!(missed_words(&engine, &image, &words, &[])
            .unwrap()
            .is_empty());
    }
}
