//! Réécriture du texte d'un bloc scanné : les mots inchangés gardent leurs
//! pixels, les autres sont dessinés dans la police retrouvée, et un
//! paragraphe est remis en page (renvoi à la ligne) dans sa largeur.
//!
//! 1. Les mots d'origine et ceux du nouveau texte sont appariés (plus longue
//!    sous-suite commune) : un mot apparié garde son encre, les autres sont
//!    effacés ou dessinés.
//! 2. Les lignes qui précèdent la première différence ne bougent pas. Les
//!    suivantes sont remplies une à une : la largeur d'un mot gardé est celle
//!    de son encre, l'espace entre deux mots voisins d'origine est conservé,
//!    les autres prennent l'espace courant du bloc. Dès que le reste du texte
//!    est celui des lignes d'origine, à partir du début de l'une d'elles, ces
//!    lignes sont reprises telles quelles (décalées d'une ou plusieurs
//!    lignes au besoin).
//! 3. Un paragraphe passe à la ligne au bord droit du bloc ; une liste garde
//!    ses retours à la ligne et ne coupe pas ses lignes. Une ligne justifiée
//!    coupée par manque de place est étirée jusqu'au bord droit ; une ligne
//!    alignée à droite ou centrée garde son bord ou son centre.
//! 4. L'encre des mots qui changent de place est détachée puis reposée, mot
//!    par mot (ou par suite de mots qui glissent ensemble).

use std::collections::HashMap;

use edit::{LayerWord, ScanText, TextLayer};
use fonts::{rasterize, shape, Pen};
use scan::line::LineFrame;
use scan::retouch::{draw, erase, extract, select, Background, InkPatch, Selecting};
use scan::{Rect as PixelRect, RgbImage};

use super::{Alignment, LineGeometry, ScanBlock, ScanModel};

/// Taille maximale du tableau d'appariement (mots d'origine × nouveaux mots).
const MAX_PAIRING: usize = 4_000_000;

/// Effet de la réécriture d'un bloc.
pub(super) struct BlockResult {
    /// Zones de pixels modifiées.
    pub changed: Vec<PixelRect>,
    /// Emprise de chaque ligne du bloc réécrit.
    pub zones: Vec<PixelRect>,
    /// Mots d'origine retirés de la couche (effacés, ou replacés : ils sont
    /// alors parmi `added`).
    pub removed: Vec<usize>,
    /// Mots d'origine qui glissent avec leur ligne, et leur décalage dans
    /// l'espace de la page.
    pub shifted: Vec<(usize, (f64, f64))>,
    /// Mots de la couche, dans l'ordre de lecture des lignes réécrites.
    pub added: Vec<LayerWord>,
    /// Indice du mot d'origine avant lequel placer `added`.
    pub insert_at: usize,
    /// Le texte occupe plus de lignes que le bloc d'origine.
    pub overflow: bool,
    /// Encre détachée, à reposer avec son décalage une fois les nouveaux
    /// mots dessinés.
    pub pending: Vec<(InkPatch, i32, i32)>,
}

/// Élément du texte : un mot ou un retour à la ligne voulu.
#[derive(Clone, Debug, PartialEq)]
enum Token<'a> {
    Word(&'a str),
    Break,
}

/// Mot d'origine : ligne et rang dans la ligne.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Origin {
    line: usize,
    rank: usize,
}

/// Mot à placer.
#[derive(Clone, Debug)]
enum Item {
    /// Mot d'origine, et son texte.
    Kept(Origin),
    /// Nouveau mot et la largeur de son encre (pixels).
    New(String, f64),
    /// Retour à la ligne voulu.
    Break,
}

/// Mot placé sur une ligne : début le long du bloc et largeur.
#[derive(Clone, Debug)]
struct Placed {
    item: Item,
    u: f64,
    width: f64,
}

/// Ligne du bloc réécrit.
struct OutLine {
    /// Ligne du bloc d'origine qui lui donne sa place (même rang), ou ligne
    /// ajoutée sous la dernière.
    words: Vec<Placed>,
    /// Ligne d'origine reprise telle quelle (décalée si son rang change).
    copy: Option<usize>,
}

/// Repère et mesures d'une ligne de sortie.
#[derive(Clone)]
struct Seat {
    frame: LineFrame,
    /// Décalage du repère le long du bloc.
    offset: f64,
    baseline: f64,
    top: f64,
    bottom: f64,
}

/// Réécrit `block` avec le texte `text`. `None` si le texte ne change pas
/// ou si aucune police n'est disponible.
pub(super) fn rewrite_block(
    model: &ScanModel,
    layer: &TextLayer,
    block: &ScanBlock,
    text: &ScanText,
    image: &mut RgbImage,
    next_line: &mut u32,
) -> Option<BlockResult> {
    if text.text == block.text {
        return None;
    }
    let placement = model.image.placement;
    let scale = placement.scale;

    // Police.
    let catalog = fonts::catalog::shared();
    let face_index = catalog
        .find(&text.style.font)
        .or_else(|| (!catalog.faces().is_empty()).then_some(0))?;
    let file = catalog.load(face_index)?;
    let face = file.face();
    let upem = f64::from(face.units_per_em() as f32);
    let em = f64::from(text.style.size) * scale;
    let k = em / upem;
    let spacing_units = (f64::from(text.style.spacing) * scale / k) as f32;
    let blur = text.style.blur * scale as f32;
    let space = shape(&face, " ", 0.0, false).advance as f64 * k;
    let ink_width = |word: &str| {
        let shaped = shape(&face, word, spacing_units, text.style.kerning);
        (f64::from(shaped.ink_x.1 - shaped.ink_x.0) * k).max(1.0)
    };

    // 1. Appariement.
    let old: Vec<(Token, Option<Origin>)> = old_tokens(block, layer);
    let new: Vec<Token> = new_tokens(&text.text);
    let pairs = pair(&old, &new);
    // Première différence, en rang de l'ancien texte.
    let mut agree = 0;
    while agree < old.len() && agree < new.len() && pairs[agree] == Some(agree) {
        agree += 1;
    }
    if agree == old.len() && agree == new.len() {
        return None;
    }
    // Ligne de la première différence : celle du mot (ou du retour à la
    // ligne) d'origine où les textes divergent ; un mot ajouté en fin de
    // texte va à la dernière ligne.
    let first = match old.get(agree) {
        Some((_, Some(origin))) => origin.line,
        Some((_, None)) => old[..agree]
            .iter()
            .rev()
            .find_map(|(_, origin)| origin.map(|o| o.line))
            .unwrap_or(0),
        None => block.lines.len() - 1,
    };
    // Rang, dans les deux textes (identiques jusque-là), du début de la
    // ligne `first`.
    let start = old
        .iter()
        .position(|(_, origin)| origin.is_some_and(|o| o.line == first))
        .unwrap_or(old.len());
    let matched: HashMap<usize, Origin> = pairs
        .iter()
        .enumerate()
        .filter_map(|(i, p)| Some(((*p)?, old[i].1?)))
        .collect();

    // 2. Mesures des lignes.
    let n = block.lines.len();
    let geometry: Vec<LineGeometry> = block
        .lines
        .iter()
        .map(|line| model.measure(line, layer))
        .collect();
    let origin0 = block.lines[0].frame.origin;
    let direction = block.lines[0].frame.direction;
    let offset_of = |frame: &LineFrame| {
        (frame.origin.0 - origin0.0) * direction.0 + (frame.origin.1 - origin0.1) * direction.1
    };
    // Étendue de l'encre d'un mot d'origine, le long du bloc.
    let extent = |o: Origin| -> (f64, f64) {
        let (a, b) = geometry[o.line].extents[o.rank];
        let off = offset_of(&block.lines[o.line].frame);
        (a + off, b + off)
    };
    // Espace courant : médiane des espaces mesurés (quartile bas dans un
    // texte justifié, dont les espaces sont élargis).
    let mut gaps: Vec<f64> = geometry
        .iter()
        .flat_map(|g| g.extents.windows(2).map(|p| p[1].0 - p[0].1))
        .filter(|g| *g > 0.0)
        .collect();
    gaps.sort_by(f64::total_cmp);
    let natural = if gaps.is_empty() {
        if space > 0.0 {
            space
        } else {
            0.25 * em
        }
    } else if block.alignment == Alignment::Justified {
        gaps[gaps.len() / 4]
    } else {
        gaps[gaps.len() / 2]
    };
    let edges: Vec<(f64, f64)> = (0..n)
        .map(|j| {
            let count = block.lines[j].words.len();
            (
                extent(Origin { line: j, rank: 0 }).0,
                extent(Origin {
                    line: j,
                    rank: count - 1,
                })
                .1,
            )
        })
        .collect();
    let margin = if n >= 2 {
        median(edges[1..].iter().map(|e| e.0).collect())
    } else {
        edges[0].0
    };
    let right_edge = edges.iter().map(|e| e.1).fold(f64::NEG_INFINITY, f64::max);

    // Places des lignes de sortie.
    let leading = if block.leading > 0.0 {
        block.leading
    } else {
        1.2 * block.em.max(em)
    };
    let last = n - 1;
    let seat = |line: usize| -> Seat {
        let g = &geometry[line.min(last)];
        let base = block.lines[line.min(last)].frame;
        let frame = if line < n {
            base
        } else {
            let m = (line - last) as f64 * leading;
            let (dx, dy) = base.direction;
            LineFrame {
                origin: (base.origin.0 - m * dy, base.origin.1 + m * dx),
                ..base
            }
        };
        Seat {
            offset: offset_of(&frame),
            frame,
            baseline: f64::from(g.ink.observation.baseline),
            top: g.top,
            bottom: g.bottom,
        }
    };

    // 3. Mise en page à partir de la ligne `first`.
    let mut queue: Vec<Item> = Vec::new();
    for (j, token) in new.iter().enumerate().skip(start) {
        match token {
            Token::Break => queue.push(Item::Break),
            Token::Word(word) => match matched.get(&j) {
                Some(origin) => queue.push(Item::Kept(*origin)),
                None => queue.push(Item::New((*word).to_owned(), ink_width(word))),
            },
        }
    }
    // Suite d'origine à partir du début de chaque ligne, pour reprendre les
    // lignes telles quelles.
    let tail_of = |j: usize| -> Vec<Item> {
        old.iter()
            .skip_while(|(_, origin)| !origin.is_some_and(|o| o.line >= j))
            .map(|(token, origin)| match (token, origin) {
                (Token::Break, _) | (_, None) => Item::Break,
                (_, Some(o)) => Item::Kept(*o),
            })
            .collect()
    };
    let same = |a: &[Item], b: &[Item]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| match (x, y) {
                (Item::Kept(p), Item::Kept(q)) => p == q,
                (Item::Break, Item::Break) => true,
                _ => false,
            })
    };
    let mut out: Vec<OutLine> = Vec::new();
    let mut pos = 0;
    let mut line = first;
    let mut after_break = first > 0 && block.hard[first - 1];
    while pos < queue.len() {
        // Reprise des lignes d'origine.
        if line > first {
            if let Some(Item::Kept(head)) = queue.get(pos) {
                if head.rank == 0 && same(&queue[pos..], &tail_of(head.line)) {
                    for j in head.line..n {
                        out.push(OutLine {
                            words: Vec::new(),
                            copy: Some(j),
                        });
                    }
                    break;
                }
            }
        }
        // Une ligne d'origine qui ne part pas de la marge (début lu à part,
        // ou absent de la couche) garde son début : la marge n'est pas à
        // elle.
        let indented = line < n && line > 0 && edges[line].0 > margin + block.em.max(natural);
        let begin = if block.paragraph && !indented {
            if line == 0 && !after_break {
                edges[0].0
            } else {
                margin
            }
        } else if line < n {
            edges[line].0
        } else {
            margin
        };
        let mut words: Vec<Placed> = Vec::new();
        let mut cursor = begin;
        let mut ended_hard = false;
        while pos < queue.len() {
            let item = queue[pos].clone();
            let (width, original) = match &item {
                Item::Break => {
                    pos += 1;
                    ended_hard = true;
                    break;
                }
                Item::Kept(o) => {
                    let (a, b) = extent(*o);
                    (b - a, Some((*o, a)))
                }
                Item::New(_, w) => (*w, None),
            };
            let u = match (words.last(), original) {
                // Premier mot d'origine de sa ligne, à sa place : il y reste.
                (None, Some((o, a))) if o.line == line && o.rank == 0 => a,
                (None, _) => cursor,
                (Some(previous), _) => {
                    let gap = match (&previous.item, original) {
                        (Item::Kept(p), Some((o, a)))
                            if p.line == o.line && p.rank + 1 == o.rank =>
                        {
                            a - extent(*p).1
                        }
                        _ => natural,
                    };
                    previous.u + previous.width + gap
                }
            };
            if block.paragraph && !words.is_empty() && u + width > right_edge + 0.5 * natural {
                break;
            }
            words.push(Placed { item, u, width });
            cursor = u + width;
            pos += 1;
        }
        // Alignement.
        let soft = !ended_hard && pos < queue.len();
        align(&mut words, block, line, soft, &edges, right_edge, natural);
        out.push(OutLine { words, copy: None });
        after_break = ended_hard;
        line += 1;
    }
    if out.is_empty() && first < n {
        // Plus aucun mot à partir de cette ligne.
        out.push(OutLine {
            words: Vec::new(),
            copy: None,
        });
    }

    // 4. Pixels.
    let mut result = BlockResult {
        changed: Vec::new(),
        zones: (0..first)
            .map(|j| super::retouch::line_zone(&block.lines[j]))
            .collect(),
        removed: Vec::new(),
        shifted: Vec::new(),
        added: Vec::new(),
        insert_at: usize::MAX,
        overflow: first + out.len() > n,
        pending: Vec::new(),
    };
    // Décalage en pixels d'un mot d'origine posé à `u` sur la ligne `target`.
    let anchor = |o: Origin| -> (f64, f64) {
        let s = seat(o.line);
        let (a, _) = extent(o);
        s.frame.to_image(a - s.offset, s.baseline)
    };
    let offset_to = |o: Origin, target: usize, u: f64| -> (i32, i32) {
        let s = seat(target);
        let (x, y) = s.frame.to_image(u - s.offset, s.baseline);
        let (x0, y0) = anchor(o);
        ((x - x0).round() as i32, (y - y0).round() as i32)
    };
    // Sort de chaque mot d'origine des lignes `first..` : effacé, gardé en
    // place, ou déplacé.
    #[derive(Clone, Copy, PartialEq)]
    enum Fate {
        Erase,
        Move(i32, i32),
    }
    let mut fate: HashMap<Origin, Fate> = HashMap::new();
    for j in first..n {
        for rank in 0..block.lines[j].words.len() {
            fate.insert(Origin { line: j, rank }, Fate::Erase);
        }
    }
    let mut copies: Vec<(usize, usize)> = Vec::new();
    for (index, out_line) in out.iter().enumerate() {
        let target = first + index;
        if let Some(j) = out_line.copy {
            copies.push((j, target));
            let shift = if j == target {
                (0, 0)
            } else {
                let o = Origin { line: j, rank: 0 };
                let (a, _) = extent(o);
                offset_to(o, target, a)
            };
            for rank in 0..block.lines[j].words.len() {
                fate.insert(Origin { line: j, rank }, Fate::Move(shift.0, shift.1));
            }
            continue;
        }
        for placed in &out_line.words {
            if let Item::Kept(o) = placed.item {
                let (dx, dy) = offset_to(o, target, placed.u);
                fate.insert(o, Fate::Move(dx, dy));
            }
        }
    }
    let settings = |j: usize| {
        let g = &geometry[j];
        Selecting {
            margin: 1,
            background: Background::Closing(super::retouch::radius(g.bottom - g.top)),
            color: None,
            halo: super::retouch::halo(scale),
        }
    };
    let strip = |j: usize, s0: f64, s1: f64| -> PixelRect {
        let g = &geometry[j];
        strip_rect(&block.lines[j].frame, g.top, g.bottom, s0, s1)
    };
    // Détacher puis effacer, ligne par ligne, chaque suite de mots qui ont le
    // même sort.
    for j in first..n {
        let count = block.lines[j].words.len();
        // L'encre des autres lignes est laissée.
        let avoid = |x, y| {
            model
                .owner(x, y)
                .is_some_and(|owner| owner != (block.id, j))
        };
        let mut rank = 0;
        while rank < count {
            let f = fate[&Origin { line: j, rank }];
            if f == Fate::Move(0, 0) {
                rank += 1;
                continue;
            }
            let mut end = rank + 1;
            while end < count && fate[&Origin { line: j, rank: end }] == f {
                end += 1;
            }
            let g = &geometry[j];
            let zone = strip(j, g.extents[rank].0 - 1.0, g.extents[end - 1].1 + 1.0);
            let selection = select(image, zone, &settings(j), &avoid);
            if let Fate::Move(dx, dy) = f {
                result.pending.push((extract(image, &selection), dx, dy));
            }
            result.changed.push(erase(
                image,
                &selection,
                u64::from(block.id) * 97 + (j * 1000 + rank) as u64,
            ));
            rank = end;
        }
    }

    // Nouveaux mots, et couche de texte des lignes réécrites.
    let color = text.style.color.map(|c| c * 255.0);
    let number_of = |line: usize, next_line: &mut u32| -> u32 {
        match block.lines.get(line).and_then(|l| l.words.first()) {
            Some(&i) => layer.words[i].line,
            None => {
                let number = *next_line;
                *next_line += 1;
                number
            }
        }
    };
    let mut fresh: HashMap<usize, u32> = HashMap::new();
    for (index, out_line) in out.iter().enumerate() {
        let target = first + index;
        let s = seat(target);
        if let Some(j) = out_line.copy {
            let (dx, dy) = match fate[&Origin { line: j, rank: 0 }] {
                Fate::Move(dx, dy) => (dx, dy),
                Fate::Erase => (0, 0),
            };
            let zone = super::retouch::line_zone(&block.lines[j]).offset(dx, dy);
            result.zones.push(zone);
            if (dx, dy) != (0, 0) {
                let offset = placement.page_offset(dx, dy);
                for &i in &block.lines[j].words {
                    result.shifted.push((i, offset));
                }
            }
            continue;
        }
        let number = if target < n {
            number_of(target, next_line)
        } else {
            *fresh.entry(target).or_insert_with(|| {
                let number = *next_line;
                *next_line += 1;
                number
            })
        };
        let mut zone = PixelRect::EMPTY;
        for (rank, placed) in out_line.words.iter().enumerate() {
            let s0 = placed.u - s.offset;
            let corners = [
                s.frame.to_image(s0, s.bottom),
                s.frame.to_image(s0 + placed.width, s.bottom),
                s.frame.to_image(s0 + placed.width, s.top),
                s.frame.to_image(s0, s.top),
            ];
            zone = zone.union(&strip_rect(
                &s.frame,
                s.top,
                s.bottom,
                s0 - 1.0,
                s0 + placed.width + 1.0,
            ));
            match &placed.item {
                Item::Kept(o) => {
                    let i = block.lines[o.line].words[o.rank];
                    let mut word = layer.words[i].clone();
                    if let Fate::Move(dx, dy) = fate[o] {
                        let (px, py) = placement.page_offset(dx, dy);
                        for c in 0..4 {
                            word.quad[2 * c] += px as f32;
                            word.quad[2 * c + 1] += py as f32;
                        }
                    }
                    word.line = number;
                    result.added.push(word);
                }
                Item::New(text_word, _) => {
                    let shaped = shape(&face, text_word, spacing_units, text.style.kerning);
                    let pen_s = s0 - f64::from(shaped.ink_x.0) * k;
                    let pen = Pen {
                        origin: s.frame.to_image(pen_s, s.baseline),
                        scale: k,
                        direction: s.frame.direction,
                        shear: 0.0,
                    };
                    let coverage = rasterize(&face, &shaped, &pen)
                        .emboldened(text.style.weight)
                        .blurred(blur);
                    result.changed.push(draw(
                        image,
                        &coverage,
                        color,
                        text.style.noise,
                        u64::from(block.id) * 31 + (target * 1000 + rank) as u64,
                    ));
                    let quad_corners = corners.map(|(x, y)| placement.to_page(x, y));
                    let mut quad = [0.0f32; 8];
                    for (c, (x, y)) in quad_corners.iter().enumerate() {
                        quad[2 * c] = *x as f32;
                        quad[2 * c + 1] = *y as f32;
                    }
                    result.added.push(LayerWord {
                        text: text_word.clone(),
                        quad,
                        confidence: 1.0,
                        line: number,
                    });
                }
                Item::Break => {}
            }
        }
        if zone.is_empty() {
            zone = if target < n {
                super::retouch::line_zone(&block.lines[target])
            } else {
                strip_rect(&s.frame, s.top, s.bottom, 0.0, 1.0)
            };
        }
        result.zones.push(zone);
    }
    // Mots d'origine des lignes réécrites : retirés de la couche (ceux qui
    // restent y reviennent dans `added`, à leur nouvelle place).
    let copied: Vec<usize> = copies.iter().map(|(j, _)| *j).collect();
    for j in first..n {
        if copied.contains(&j) {
            continue;
        }
        result.removed.extend(block.lines[j].words.iter().copied());
    }
    result.insert_at = result
        .removed
        .iter()
        .copied()
        .min()
        .unwrap_or_else(|| block.words().min().unwrap_or(0));
    Some(result)
}

/// Aligne les mots d'une ligne réécrite. `edges` : bords de l'encre de
/// chaque ligne d'origine, le long du bloc.
fn align(
    words: &mut [Placed],
    block: &ScanBlock,
    line: usize,
    soft: bool,
    edges: &[(f64, f64)],
    right_edge: f64,
    natural: f64,
) {
    let Some(last) = words.last() else {
        return;
    };
    let end = last.u + last.width;
    let edge = edges[line.min(edges.len() - 1)];
    let shift = |words: &mut [Placed], delta: f64| {
        for w in words.iter_mut() {
            w.u += delta;
        }
    };
    match block.alignment {
        Alignment::Right => {
            let target = if line < edges.len() {
                edge.1
            } else {
                right_edge
            };
            shift(words, target - end);
        }
        Alignment::Center => {
            let center = (edge.0 + edge.1) / 2.0;
            shift(words, center - (words[0].u + end) / 2.0);
        }
        Alignment::Justified if soft && block.paragraph && words.len() >= 2 => {
            // Les mots restés à leur place d'origine (début de la ligne) ne
            // bougent pas ; les espaces suivants s'élargissent également.
            let anchored = words
                .iter()
                .enumerate()
                .take_while(
                    |(k, w)| matches!(w.item, Item::Kept(o) if o.line == line && o.rank == *k),
                )
                .count()
                .max(1);
            let spaces = (words.len() - anchored) as f64;
            let extra = right_edge - end;
            if spaces > 0.0 && extra > 0.0 && extra / spaces <= 4.0 * natural.max(1.0) {
                let step = extra / spaces;
                for (k, w) in words.iter_mut().enumerate().skip(anchored) {
                    w.u += step * (k + 1 - anchored) as f64;
                }
            }
        }
        Alignment::Left | Alignment::Justified => {}
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// Bande d'une ligne entre `s0` et `s1`, en pixels, avec une marge de 15 %
/// en hauteur.
pub(super) fn strip_rect(frame: &LineFrame, top: f64, bottom: f64, s0: f64, s1: f64) -> PixelRect {
    let height = (bottom - top).max(1.0);
    let (t0, t1) = (top - 0.15 * height, bottom + 0.15 * height);
    let corners = [
        frame.to_image(s0, t0),
        frame.to_image(s1, t0),
        frame.to_image(s1, t1),
        frame.to_image(s0, t1),
    ];
    let (x0, y0, x1, y1) = super::bounds(&corners);
    PixelRect::covering(x0, y0, x1, y1)
}

/// Mots d'origine du bloc, ligne après ligne, avec un retour à la ligne
/// après chaque ligne qui en demande un (sauf la dernière).
fn old_tokens<'a>(block: &ScanBlock, layer: &'a TextLayer) -> Vec<(Token<'a>, Option<Origin>)> {
    let mut out = Vec::new();
    let n = block.lines.len();
    for (j, line) in block.lines.iter().enumerate() {
        for (rank, &i) in line.words.iter().enumerate() {
            out.push((
                Token::Word(layer.words[i].text.as_str()),
                Some(Origin { line: j, rank }),
            ));
        }
        if j + 1 < n && block.hard[j] {
            out.push((Token::Break, None));
        }
    }
    out
}

/// Mots et retours à la ligne du nouveau texte.
fn new_tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    for (k, segment) in text.split('\n').enumerate() {
        if k > 0 {
            out.push(Token::Break);
        }
        out.extend(segment.split_whitespace().map(Token::Word));
    }
    // Retours à la ligne en tête et en fin : sans effet.
    while out.first() == Some(&Token::Break) {
        out.remove(0);
    }
    while out.last() == Some(&Token::Break) {
        out.pop();
    }
    out
}

/// Plus longue sous-suite commune : pour chaque élément de `old`, l'indice de
/// l'élément de `new` apparié, sinon `None`. Rangé par indice de `old`.
fn pair(old: &[(Token, Option<Origin>)], new: &[Token]) -> Vec<Option<usize>> {
    let (n, m) = (old.len(), new.len());
    let mut out = vec![None; n];
    // Tête et queue communes, puis appariement du milieu.
    let mut head = 0;
    while head < n && head < m && old[head].0 == new[head] {
        out[head] = Some(head);
        head += 1;
    }
    let mut tail = 0;
    while tail < n - head && tail < m - head && old[n - 1 - tail].0 == new[m - 1 - tail] {
        out[n - 1 - tail] = Some(m - 1 - tail);
        tail += 1;
    }
    let (a, b) = (n - head - tail, m - head - tail);
    if a == 0 || b == 0 || a * b > MAX_PAIRING {
        return out;
    }
    // Longueurs des sous-suites communes des suffixes.
    let mut table = vec![0u32; (a + 1) * (b + 1)];
    let at = |i: usize, j: usize| i * (b + 1) + j;
    for i in (0..a).rev() {
        for j in (0..b).rev() {
            table[at(i, j)] = if old[head + i].0 == new[head + j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a && j < b {
        if old[head + i].0 == new[head + j] {
            out[head + i] = Some(head + j);
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(words: &[&str]) -> Vec<(Token<'static>, Option<Origin>)> {
        words
            .iter()
            .enumerate()
            .map(|(rank, w)| {
                let w: &'static str = Box::leak(w.to_string().into_boxed_str());
                (Token::Word(w), Some(Origin { line: 0, rank }))
            })
            .collect()
    }

    #[test]
    fn apparie_les_mots_gardes() {
        let old = tokens(&["Le", "contrat", "est", "signé", "le", "2", "mai"]);
        let new = new_tokens("Le nouveau contrat est signé le 3 mai");
        let pairs = pair(&old, &new);
        assert_eq!(
            pairs,
            vec![Some(0), Some(2), Some(3), Some(4), Some(5), None, Some(7)]
        );
        // Deux changements éloignés : les mots entre les deux restent appariés.
        let new = new_tokens("La contrat est signé le 2 juin");
        let pairs = pair(&old, &new);
        assert_eq!(
            pairs,
            vec![None, Some(1), Some(2), Some(3), Some(4), Some(5), None]
        );
    }

    #[test]
    fn retours_a_la_ligne_du_texte() {
        assert_eq!(
            new_tokens("\nun  deux\ntrois\n"),
            vec![
                Token::Word("un"),
                Token::Word("deux"),
                Token::Break,
                Token::Word("trois")
            ]
        );
    }
}
