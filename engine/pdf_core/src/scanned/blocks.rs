//! Blocs de texte d'un scan : les mots reconnus sont regroupés en lignes,
//! puis en paragraphes homogènes, comme le texte d'une page native (D25).
//!
//! Vision lit une ligne en plusieurs morceaux dès que les espaces s'élargissent
//! (texte justifié, chasse fixe) et donne aux mots des emprises d'une hauteur
//! variable. Le regroupement ne se fie donc ni à ses numéros de ligne ni à la
//! hauteur de ses emprises : les mots sont rangés par la hauteur de leur
//! centre, sur la page redressée ; le corps, la ligne de base, la graisse et
//! la couleur de chaque ligne sont mesurés sur l'encre du scan.
//!
//! - Une rangée est coupée en lignes aux grands blancs : plus d'une fois et
//!   demie la hauteur d'un mot, ou deux fois et demie si la rangée a plusieurs
//!   espaces élargis de largeur voisine (texte justifié ; des écarts très
//!   inégaux séparent des colonnes), et à chaque changement de cellule.
//! - Chaque ligne a sa bande d'encre (des accents aux jambages, bornée en
//!   hauteurs d'x quand elle touche ses voisines) : la police y est mesurée
//!   et l'encre y est prise, sans celle des lignes voisines.
//! - Une ligne rejoint le bloc ouvert dont elle continue le mieux la colonne :
//!   même corps (à 30 % près, la mesure sur un scan est bruitée), même graisse,
//!   même couleur, interligne régulier, bords alignés (ou alinéa sous une
//!   première ligne seule).
//! - Un morceau isolé sur la rangée d'une ligne d'un paragraphe, dans sa
//!   largeur, rejoint cette ligne (premier mot séparé par un grand blanc).

use scan::ink::Mask;
use scan::RgbImage;

/// Cellule d'un tableau : (tableau, cellule), par indice.
pub(crate) type Cell = Option<(usize, usize)>;

/// Nombre de blocs récents auxquels une ligne peut encore se joindre.
const OPEN_BLOCKS: usize = 8;

/// Largeur minimale d'un paragraphe, en cadratins (cf. D25).
const PARAGRAPH_EMS: f64 = 12.0;

/// Rapport du corps à la hauteur mesurée (ligne de base jusqu'au haut des
/// lettres montantes).
const EM_PER_HEIGHT: f64 = 1.0 / 0.72;

/// Écart maximal entre les espaces d'une ligne justifiée, en hauteurs de
/// ligne.
const JUSTIFIED_SPREAD: f64 = 1.2;

/// Bande de l'encre d'une ligne, au-dessus et au-dessous de sa ligne de base,
/// en hauteurs d'x : accents des capitales, jambages.
const BAND_ABOVE: f64 = 2.2;
const BAND_BELOW: f64 = 0.6;

/// Alignement des lignes d'un bloc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alignment {
    Left,
    Right,
    Center,
    Justified,
}

/// Un mot à regrouper.
pub(crate) struct WordInput {
    /// Coins inférieur gauche, inférieur droit, supérieur droit, supérieur
    /// gauche, en pixels du scan.
    pub corners: [(f64, f64); 4],
    pub cell: Cell,
}

/// Une ligne d'un bloc.
#[derive(Clone, Debug)]
pub(crate) struct LinePlan {
    /// Mots, de gauche à droite.
    pub words: Vec<usize>,
    /// Bande de l'encre de la ligne (coins dans le scan, en bas à gauche
    /// d'abord) : des accents aux jambages, sans l'encre des lignes voisines.
    pub band: [(f64, f64); 4],
    /// Début de la ligne de base, dans le scan, et longueur de la ligne.
    pub baseline: (f64, f64),
    pub length: f64,
    /// Hauteur d'x (rangées d'encre les plus denses), en pixels.
    pub x_height: f64,
}

/// Un bloc de texte d'un scan.
#[derive(Clone, Debug)]
pub(crate) struct BlockPlan {
    /// Lignes, de haut en bas.
    pub lines: Vec<LinePlan>,
    pub cell: Cell,
    pub alignment: Alignment,
    /// Paragraphe dont le texte se remet en page ; sinon liste dont chaque
    /// ligne garde son retour à la ligne.
    pub paragraph: bool,
    /// Retour à la ligne voulu après chaque ligne (le dernier est vrai).
    pub hard: Vec<bool>,
    /// Corps du texte, en pixels.
    pub em: f64,
    /// Distance entre deux lignes de base, en pixels (0 pour une ligne seule).
    pub leading: f64,
}

/// Page redressée : rotation de `-angle` autour du centre de l'image.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Upright {
    cos: f64,
    sin: f64,
    center: (f64, f64),
}

impl Upright {
    /// `skew` : inclinaison du texte, en degrés (positive : les lignes
    /// descendent vers la droite).
    pub fn new(skew: f64, width: usize, height: usize) -> Self {
        let angle = skew.to_radians();
        Upright {
            cos: angle.cos(),
            sin: angle.sin(),
            center: (width as f64 / 2.0, height as f64 / 2.0),
        }
    }

    /// Point redressé d'un pixel du scan.
    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (dx, dy) = (x - self.center.0, y - self.center.1);
        (
            dx * self.cos + dy * self.sin + self.center.0,
            -dx * self.sin + dy * self.cos + self.center.1,
        )
    }

    /// Direction des lignes dans le scan (vecteur unitaire).
    pub fn direction(&self) -> (f64, f64) {
        (self.cos, self.sin)
    }

    /// Pixel du scan d'un point redressé.
    pub fn invert(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (dx, dy) = (x - self.center.0, y - self.center.1);
        (
            dx * self.cos - dy * self.sin + self.center.0,
            dx * self.sin + dy * self.cos + self.center.1,
        )
    }
}

/// Mot placé sur la page redressée.
#[derive(Clone, Debug)]
struct Word {
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
    /// Étendue de son encre.
    ink0: f64,
    ink1: f64,
    cell: Cell,
}

impl Word {
    fn height(&self) -> f64 {
        self.bottom - self.top
    }

    fn center(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

/// Ligne mesurée.
#[derive(Clone, Debug)]
struct Line {
    words: Vec<usize>,
    left: f64,
    right: f64,
    /// Haut et bas des emprises (médianes).
    top: f64,
    bottom: f64,
    baseline: f64,
    /// Haut et bas de l'encre de la ligne.
    ink_top: f64,
    ink_bottom: f64,
    /// Hauteur d'x (rangées les plus denses).
    x_height: f64,
    /// Hauteur des lettres montantes au-dessus de la ligne de base.
    size: f64,
    /// Épaisseur des traits (longueur médiane des passages dans l'encre).
    stem: f64,
    /// Couleur de l'encre la plus sombre.
    color: [f32; 3],
    cell: Cell,
}

impl Line {
    fn em(&self) -> f64 {
        self.size * EM_PER_HEIGHT
    }

    fn center(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

/// Accès à l'encre du scan depuis la page redressée.
struct Ink<'a> {
    image: &'a RgbImage,
    dark: &'a Mask,
    upright: Upright,
}

impl Ink<'_> {
    fn pixel(&self, x: f64, y: f64) -> (i32, i32) {
        let (px, py) = self.upright.invert((x + 0.5, y + 0.5));
        (px.floor() as i32, py.floor() as i32)
    }

    fn is_ink(&self, x: f64, y: f64) -> bool {
        let (px, py) = self.pixel(x, y);
        self.dark.get(px, py)
    }
}

/// Regroupe les mots `words` en blocs de texte. `image` est le rendu du scan,
/// `dark` son encre, `upright` le redressement de ses lignes.
pub(crate) fn group(
    words: &[WordInput],
    image: &RgbImage,
    dark: &Mask,
    upright: Upright,
) -> Vec<BlockPlan> {
    if words.is_empty() {
        return Vec::new();
    }
    let ink = Ink {
        image,
        dark,
        upright,
    };
    let mut placed: Vec<Word> = words
        .iter()
        .map(|w| {
            let points = w.corners.map(|p| upright.apply(p));
            let (x0, y0, x1, y1) = super::bounds(&points);
            Word {
                x0,
                x1,
                top: y0,
                bottom: y1,
                ink0: x0,
                ink1: x1,
                cell: w.cell,
            }
        })
        .collect();
    tighten(&mut placed, &ink);
    for word in &mut placed {
        let (a, b) = ink_extent(word, &ink);
        word.ink0 = a;
        word.ink1 = b;
    }
    let mut lines: Vec<Line> = rows(&placed)
        .into_iter()
        .flat_map(|row| split_row(&placed, row))
        .map(|members| measure(&placed, members, &ink))
        .collect();
    lines.sort_by(|a, b| {
        a.baseline
            .total_cmp(&b.baseline)
            .then(a.left.total_cmp(&b.left))
    });
    let mut groups = paragraphs(lines);
    attach_fragments(&mut groups, &placed);
    groups
        .into_iter()
        .map(|g| finish(g, &placed, upright))
        .collect()
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// Une emprise deux fois trop haute couvre aussi la ligne du dessus (Vision) :
/// elle est ramenée à sa ligne, coupée au blanc qui les sépare.
fn tighten(words: &mut [Word], ink: &Ink) {
    let typical = median(words.iter().map(Word::height).collect());
    if typical <= 0.0 {
        return;
    }
    for word in words.iter_mut() {
        if word.height() <= 1.6 * typical {
            continue;
        }
        let blank = |y: f64| {
            let mut x = word.x0.floor();
            while x < word.x1 {
                if ink.is_ink(x, y) {
                    return false;
                }
                x += 1.0;
            }
            true
        };
        let target = word.bottom - 1.1 * typical;
        let mut best: Option<f64> = None;
        let mut y = (word.bottom - 1.5 * typical).floor();
        while y < word.bottom - 0.6 * typical {
            if blank(y) && best.is_none_or(|b| (y - target).abs() < (b - target).abs()) {
                best = Some(y);
            }
            y += 1.0;
        }
        word.top = best.unwrap_or(target);
    }
}

/// Étendue de l'encre d'un mot, au cœur de son emprise.
fn ink_extent(word: &Word, ink: &Ink) -> (f64, f64) {
    let h = word.height();
    let (y0, y1) = (word.top + 0.15 * h, word.bottom - 0.15 * h);
    let mut first: Option<f64> = None;
    let mut last = word.x0;
    let mut x = word.x0.floor();
    while x < word.x1 {
        let mut y = y0.floor();
        while y < y1 {
            if ink.is_ink(x, y) {
                first.get_or_insert(x);
                last = x + 1.0;
                break;
            }
            y += 1.0;
        }
        x += 1.0;
    }
    match first {
        Some(first) => (first, last),
        None => (word.x0, word.x1),
    }
}

/// Rangées : mots dont le centre est à la hauteur de la rangée.
fn rows(words: &[Word]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by(|&a, &b| words[a].center().total_cmp(&words[b].center()));
    // (mots, centre, hauteur) de chaque rangée.
    let mut rows: Vec<(Vec<usize>, f64, f64)> = Vec::new();
    for i in order {
        let (center, height) = (words[i].center(), words[i].height());
        let start = rows.len().saturating_sub(4);
        let found = rows[start..]
            .iter_mut()
            .find(|(_, c, h)| (center - *c).abs() <= 0.35 * h.min(height));
        match found {
            Some((members, _, _)) => members.push(i),
            None => rows.push((vec![i], center, height)),
        }
    }
    rows.into_iter()
        .map(|(mut members, _, _)| {
            members.sort_by(|&a, &b| words[a].ink0.total_cmp(&words[b].ink0));
            members
        })
        .collect()
}

/// Lignes d'une rangée, coupée aux grands blancs et aux changements de
/// cellule.
fn split_row(words: &[Word], row: Vec<usize>) -> Vec<Vec<usize>> {
    let h = median(row.iter().map(|&i| words[i].height()).collect());
    let gaps: Vec<f64> = row
        .windows(2)
        .map(|p| words[p[1]].ink0 - words[p[0]].ink1)
        .collect();
    // Ligne justifiée : plusieurs espaces élargis, et tous d'une largeur
    // voisine (une espace d'écart au plus en chasse fixe). Des écarts très
    // inégaux séparent des colonnes (ticket, tableau sans filets).
    let widened = gaps.iter().filter(|&&g| g > 0.75 * h).count();
    let spread = gaps.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - gaps.iter().copied().fold(f64::INFINITY, f64::min);
    let justified = widened >= 2 && spread <= JUSTIFIED_SPREAD * h;
    let limit = if justified { 2.5 * h } else { 1.5 * h };
    let mut out: Vec<Vec<usize>> = Vec::new();
    for (k, &i) in row.iter().enumerate() {
        let split = k == 0 || gaps[k - 1] > limit || words[row[k - 1]].cell != words[i].cell;
        if split {
            out.push(vec![i]);
        } else if let Some(last) = out.last_mut() {
            last.push(i);
        }
    }
    out
}

/// Mesure une ligne sur l'encre du scan : ligne de base, hauteur des
/// lettres, épaisseur des traits et couleur.
fn measure(words: &[Word], members: Vec<usize>, ink: &Ink) -> Line {
    let top = median(members.iter().map(|&i| words[i].top).collect());
    let bottom = median(members.iter().map(|&i| words[i].bottom).collect());
    let h = (bottom - top).max(1.0);
    let center = (top + bottom) / 2.0;
    let left = members
        .iter()
        .map(|&i| words[i].ink0)
        .fold(f64::INFINITY, f64::min);
    let right = members
        .iter()
        .map(|&i| words[i].ink1)
        .fold(f64::NEG_INFINITY, f64::max);
    let cell = words[members[0]].cell;
    // Encre de chaque rangée de pixels autour de la ligne.
    let y0 = (center - 0.9 * h).floor();
    let rows = (1.8 * h).ceil().max(1.0) as usize;
    let spans: Vec<(f64, f64)> = members
        .iter()
        .map(|&i| (words[i].ink0, words[i].ink1))
        .collect();
    let mut profile = vec![0usize; rows];
    for (r, count) in profile.iter_mut().enumerate() {
        let y = y0 + r as f64;
        for &(a, b) in &spans {
            let mut x = a;
            while x < b {
                if ink.is_ink(x, y) {
                    *count += 1;
                }
                x += 1.0;
            }
        }
    }
    // Maximum cherché au cœur de l'emprise, loin des lignes voisines.
    let core_rows = (0.3 * h).floor() as usize..((1.5 * h).ceil() as usize).min(rows);
    let peak_row = core_rows.max_by_key(|&r| profile[r]).unwrap_or(rows / 2);
    let peak = profile[peak_row].max(1);
    // Bande de la ligne : rangées d'encre autour du maximum, jusqu'au blanc
    // qui la sépare de ses voisines.
    let floor = (peak as f64 * 0.04).ceil() as usize;
    let mut a = peak_row;
    while a > 0 && profile[a - 1] > floor {
        a -= 1;
    }
    let mut b = peak_row;
    while b + 1 < rows && profile[b + 1] > floor {
        b += 1;
    }
    let total: usize = profile[a..=b].iter().sum();
    let quantile = |q: f64| {
        let mut seen = 0;
        for (r, &count) in profile.iter().enumerate().take(b + 1).skip(a) {
            seen += count;
            if seen as f64 >= q * total as f64 {
                return r;
            }
        }
        b
    };
    let (high, base) = (quantile(0.04), quantile(0.88));
    let baseline = y0 + base as f64 + 1.0;
    let size = (base as f64 - high as f64 + 1.0).max(0.3 * h);
    // Encre de la ligne, des accents aux jambages : rangées jusqu'au blanc
    // qui la sépare de ses voisines, bornées en hauteurs d'x (rangées les
    // plus denses) au-dessus et au-dessous de la ligne de base quand elle
    // les touche.
    let dense = (peak as f64 * 0.3).ceil() as usize;
    let x_top = (a..=base).find(|&r| profile[r] >= dense).unwrap_or(high);
    let x_height = (base as f64 - x_top as f64 + 1.0).max(1.0);
    let faint = (peak as f64 * 0.01).ceil() as usize;
    let mut top_row = a;
    while top_row > 0 && profile[top_row - 1] > faint {
        top_row -= 1;
    }
    let mut bottom_row = b;
    while bottom_row + 1 < rows && profile[bottom_row + 1] > faint {
        bottom_row += 1;
    }
    let ink_top = (y0 + top_row as f64).max(baseline - BAND_ABOVE * x_height);
    let ink_bottom = (y0 + bottom_row as f64 + 1.0).min(baseline + BAND_BELOW * x_height);
    // Épaisseur des traits et couleur de l'encre, dans la bande.
    let mut runs: Vec<f64> = Vec::new();
    let mut darkest: Vec<[f32; 3]> = Vec::new();
    for r in (a..=b).step_by(2) {
        let y = y0 + r as f64;
        for &(from, to) in &spans {
            let mut run = 0.0;
            let mut x = from;
            while x <= to {
                if ink.is_ink(x, y) {
                    run += 1.0;
                    let (px, py) = ink.pixel(x, y);
                    darkest.push(ink.image.get_f(px, py));
                } else if run > 0.0 {
                    runs.push(run);
                    run = 0.0;
                }
                x += 1.0;
            }
        }
    }
    darkest.sort_by(|p, q| scan::image::luma(*p).total_cmp(&scan::image::luma(*q)));
    darkest.truncate(darkest.len().div_ceil(5));
    let color = if darkest.is_empty() {
        [0.0; 3]
    } else {
        [0, 1, 2].map(|c| {
            let mut v: Vec<f32> = darkest.iter().map(|p| p[c]).collect();
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        })
    };
    Line {
        words: members,
        left,
        right,
        top,
        bottom,
        baseline,
        ink_top,
        ink_bottom,
        x_height,
        size,
        stem: median(runs),
        color,
        cell,
    }
}

/// Même style : corps, graisse et couleur voisins.
fn same_style(a: &Line, b: &Line) -> bool {
    let size = a.size.max(b.size) <= 1.3 * a.size.min(b.size);
    let stem = (a.stem - b.stem).abs() <= (0.35 * a.stem.max(b.stem)).max(1.5);
    let color = (0..3).all(|c| (a.color[c] - b.color[c]).abs() <= 45.0);
    size && stem && color
}

/// Lignes réunies en blocs (cf. `content::objects::blocks`).
fn paragraphs(lines: Vec<Line>) -> Vec<Vec<Line>> {
    let mut groups: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        let open = groups.len().saturating_sub(OPEN_BLOCKS);
        let mut best: Option<(usize, f64)> = None;
        for index in (open..groups.len()).rev() {
            if groups[index][0].cell != line.cell {
                continue;
            }
            if let Some(score) = joins(&groups[index], &line) {
                if best.is_none_or(|(_, top)| score < top) {
                    best = Some((index, score));
                }
            }
        }
        match best {
            Some((index, _)) => groups[index].push(line),
            None => groups.push(vec![line]),
        }
    }
    groups
}

/// Si `line` peut continuer le bloc `group`, écart entre leurs bords alignés
/// (plus il est faible, mieux elle s'y rattache).
fn joins(group: &[Line], line: &Line) -> Option<f64> {
    let last = &group[group.len() - 1];
    if !same_style(last, line) {
        return None;
    }
    let reference = median(group.iter().map(|l| l.size).collect());
    if line.size.max(reference) > 1.3 * line.size.min(reference) {
        return None;
    }
    let em = reference * EM_PER_HEIGHT;
    let step = line.baseline - last.baseline;
    if !(0.8 * em..=2.0 * em).contains(&step) {
        return None;
    }
    if group.len() >= 2 {
        let leading = group[1].baseline - group[0].baseline;
        if (step - leading).abs() > 0.25 * em {
            return None;
        }
    }
    let (b0, b1) = (last.left, last.right);
    let (l0, l1) = (line.left, line.right);
    let overlap = b1.min(l1) - b0.max(l0);
    if overlap <= 0.0 {
        return None;
    }
    let tolerance = 0.25 * em;
    let edges = [
        (b0 - l0).abs(),
        (b1 - l1).abs(),
        ((b0 + b1) - (l0 + l1)).abs() / 2.0,
    ];
    let aligned = edges.into_iter().fold(f64::INFINITY, f64::min);
    // À alignement égal, la ligne la plus proche l'emporte.
    let near = 0.05 * step;
    if aligned <= tolerance {
        return Some(aligned + near);
    }
    let indent = b0 - l0;
    let indented = group.len() == 1
        && (-2.0 * em..=5.0 * em).contains(&indent)
        && overlap >= 0.3 * (b1 - b0).min(l1 - l0);
    indented.then_some(tolerance + indent.abs() + near)
}

/// Un morceau seul sur la rangée d'une ligne d'un paragraphe, dans sa largeur
/// et de même style, rejoint cette ligne : Vision coupe une ligne justifiée
/// aux grands blancs.
fn attach_fragments(groups: &mut Vec<Vec<Line>>, words: &[Word]) {
    loop {
        let mut joined = None;
        'search: for (p, group) in groups.iter().enumerate() {
            if group.len() < 2 {
                continue;
            }
            let left = group.iter().map(|l| l.left).fold(f64::INFINITY, f64::min);
            let right = group
                .iter()
                .map(|l| l.right)
                .fold(f64::NEG_INFINITY, f64::max);
            let em = median(group.iter().map(Line::em).collect());
            for (f, fragment) in groups.iter().enumerate() {
                if f == p || fragment.len() != 1 {
                    continue;
                }
                let piece = &fragment[0];
                if piece.cell != group[0].cell
                    || piece.left < left - 0.4 * em
                    || piece.right > right + 0.4 * em
                {
                    continue;
                }
                let found = group.iter().position(|l| {
                    (l.center() - piece.center()).abs()
                        <= 0.35 * (l.bottom - l.top).min(piece.bottom - piece.top)
                        && same_style(l, piece)
                        && (piece.left - l.right).max(l.left - piece.right) <= 4.0 * em
                });
                if let Some(l) = found {
                    joined = Some((p, l, f));
                    break 'search;
                }
            }
        }
        let Some((p, l, f)) = joined else {
            return;
        };
        let piece = groups.remove(f).remove(0);
        let p = if f < p { p - 1 } else { p };
        let line = &mut groups[p][l];
        line.words.extend(piece.words);
        line.words
            .sort_by(|&a, &b| words[a].ink0.total_cmp(&words[b].ink0));
        line.left = line.left.min(piece.left);
        line.right = line.right.max(piece.right);
        line.ink_top = line.ink_top.min(piece.ink_top);
        line.ink_bottom = line.ink_bottom.max(piece.ink_bottom);
    }
}

/// Bloc achevé : alignement, nature (paragraphe ou liste) et retours à la
/// ligne voulus (cf. `content::objects::block_text`).
fn finish(lines: Vec<Line>, words: &[Word], upright: Upright) -> BlockPlan {
    let em = median(lines.iter().map(Line::em).collect());
    let leading = if lines.len() >= 2 {
        median(
            lines
                .windows(2)
                .map(|p| p[1].baseline - p[0].baseline)
                .collect(),
        )
    } else {
        0.0
    };
    let alignment = alignment(&lines, em);
    let widths: Vec<f64> = lines.iter().map(|l| l.right - l.left).collect();
    let max_width = widths.iter().copied().fold(0.0, f64::max);
    let full: Vec<bool> = widths.iter().map(|&w| w >= 0.8 * max_width).collect();
    let n = lines.len();
    let wrapped = full[..n - 1].iter().filter(|&&f| f).count();
    let paragraph = n >= 2 && 2 * wrapped >= n - 1 && max_width >= PARAGRAPH_EMS * em;
    let block_left = lines.iter().map(|l| l.left).fold(f64::INFINITY, f64::min);
    let block_right = lines
        .iter()
        .map(|l| l.right)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut hard = Vec::with_capacity(n);
    for k in 0..n {
        if k + 1 == n || !paragraph {
            hard.push(true);
            continue;
        }
        let soft = match alignment {
            Alignment::Justified => true,
            _ => {
                // Le premier mot de la suivante ne tenait pas au bout de la ligne.
                let first = lines[k + 1]
                    .words
                    .first()
                    .map_or(0.0, |&i| words[i].ink1 - words[i].ink0);
                let other_full = (0..n - 1).any(|j| j != k && full[j]);
                let free = (block_right - block_left) - (lines[k].right - block_left);
                full[k] && other_full && free < first + 0.25 * em
            }
        };
        hard.push(!soft);
    }
    BlockPlan {
        cell: lines[0].cell,
        alignment,
        paragraph,
        hard,
        em,
        leading,
        lines: lines
            .into_iter()
            .map(|l| {
                let x0 = l
                    .words
                    .iter()
                    .map(|&i| words[i].x0)
                    .fold(f64::INFINITY, f64::min);
                let x1 = l
                    .words
                    .iter()
                    .map(|&i| words[i].x1)
                    .fold(f64::NEG_INFINITY, f64::max);
                let corners = |top: f64, bottom: f64| {
                    [(x0, bottom), (x1, bottom), (x1, top), (x0, top)].map(|p| upright.invert(p))
                };
                LinePlan {
                    band: corners(l.ink_top, l.ink_bottom),
                    baseline: upright.invert((x0, l.baseline)),
                    length: x1 - x0,
                    x_height: l.x_height,
                    words: l.words,
                }
            })
            .collect(),
    }
}

fn alignment(lines: &[Line], em: f64) -> Alignment {
    if lines.len() < 2 {
        return Alignment::Left;
    }
    let tolerance = 0.25 * em;
    let close = |values: Vec<f64>| {
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        values.is_empty() || hi - lo <= tolerance
    };
    let n = lines.len();
    let left = close(lines[1..].iter().map(|l| l.left).collect())
        && lines[0].left >= lines[1].left - tolerance;
    let full_rights = close(lines[..n - 1].iter().map(|l| l.right).collect());
    let all_rights = close(lines.iter().map(|l| l.right).collect());
    let centers = close(lines.iter().map(|l| (l.left + l.right) / 2.0).collect());
    let all_lefts = close(lines.iter().map(|l| l.left).collect());
    let wide = lines.iter().any(|l| l.right - l.left >= PARAGRAPH_EMS * em);
    let wrapped_pair = n == 2 && lines[0].right > lines[1].right + tolerance;
    if wide && left && ((full_rights && n >= 3) || wrapped_pair) {
        Alignment::Justified
    } else if all_lefts || left {
        Alignment::Left
    } else if all_rights {
        Alignment::Right
    } else if centers {
        Alignment::Center
    } else {
        Alignment::Left
    }
}
