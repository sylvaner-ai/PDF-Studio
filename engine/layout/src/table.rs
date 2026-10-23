//! Tableaux d'une page : quadrillages (cellules fermées par des filets) et
//! tableaux à filets horizontaux (colonnes déduites du texte), lignes sans
//! filet déduites du texte, cellules fusionnées.

use std::ops::Range;

use crate::geometry::{Rect, Segment, TextBox};
use crate::lattice;
use crate::rulings::{rulings, Orientation, Ruling};

/// Origine de la structure d'un tableau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    /// Quadrillage : chaque cellule est fermée par des filets.
    Grid,
    /// Filets horizontaux seuls : les colonnes sont déduites du texte.
    Lined,
}

/// Cellule d'un tableau, éventuellement fusionnée avec ses voisines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Première ligne, à partir de 0 en haut.
    pub row: usize,
    /// Première colonne, à partir de 0 à gauche.
    pub column: usize,
    /// Nombre de lignes couvertes.
    pub row_span: usize,
    /// Nombre de colonnes couvertes.
    pub column_span: usize,
}

impl Cell {
    /// Lignes couvertes.
    pub fn rows(&self) -> Range<usize> {
        self.row..self.row + self.row_span
    }

    /// Colonnes couvertes.
    pub fn columns(&self) -> Range<usize> {
        self.column..self.column + self.column_span
    }
}

/// Tableau détecté.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// Origine de la structure.
    pub kind: TableKind,
    /// Limites des lignes, de haut en bas (ordonnées décroissantes) : une de
    /// plus que de lignes.
    pub rows: Vec<f64>,
    /// Limites des colonnes, de gauche à droite : une de plus que de colonnes.
    pub columns: Vec<f64>,
    /// Pour chaque limite de ligne : vrai si un filet la trace, faux si elle
    /// est déduite du texte.
    pub ruled_rows: Vec<bool>,
    /// Cellules, ligne par ligne puis de gauche à droite.
    pub cells: Vec<Cell>,
}

impl Table {
    /// Nombre de lignes.
    pub fn row_count(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }

    /// Nombre de colonnes.
    pub fn column_count(&self) -> usize {
        self.columns.len().saturating_sub(1)
    }

    /// Emprise du tableau.
    pub fn bbox(&self) -> Rect {
        Rect::new(
            self.columns[0],
            self.rows[self.rows.len() - 1],
            self.columns[self.columns.len() - 1],
            self.rows[0],
        )
    }

    /// Emprise d'une cellule.
    pub fn cell_rect(&self, cell: &Cell) -> Rect {
        Rect::new(
            self.columns[cell.column],
            self.rows[cell.row + cell.row_span],
            self.columns[cell.column + cell.column_span],
            self.rows[cell.row],
        )
    }

    /// Ligne dont la bande contient l'ordonnée `y`.
    pub fn row_at(&self, y: f64) -> Option<usize> {
        (0..self.row_count()).find(|&r| y <= self.rows[r] && y >= self.rows[r + 1])
    }

    /// Colonne dont la bande contient l'abscisse `x`.
    pub fn column_at(&self, x: f64) -> Option<usize> {
        (0..self.column_count()).find(|&c| x >= self.columns[c] && x <= self.columns[c + 1])
    }

    /// Indice de la cellule qui contient le point, s'il y en a une.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<usize> {
        let (row, column) = (self.row_at(y)?, self.column_at(x)?);
        self.cells
            .iter()
            .position(|cell| cell.rows().contains(&row) && cell.columns().contains(&column))
    }

    fn sort_cells(&mut self) {
        self.cells.sort_by_key(|cell| (cell.row, cell.column));
    }
}

/// Tableaux d'une page, de haut en bas, d'après ses segments (traits, bords de
/// rectangles) et les glyphes de son texte.
pub fn detect(segments: &[Segment], text: &[TextBox]) -> Vec<Table> {
    let rulings = rulings(segments);
    let mut grids: Vec<Table> = lattice::grids(&rulings)
        .iter()
        .filter_map(|cells| grid_table(cells))
        .collect();
    // Un quadrillage dans la cellule d'un autre en fait partie.
    let boxes: Vec<Rect> = grids.iter().map(Table::bbox).collect();
    let mut index = 0;
    grids.retain(|_| {
        let inner = boxes
            .iter()
            .enumerate()
            .any(|(other, b)| other != index && b.inflate(1.0).contains_rect(&boxes[index]));
        index += 1;
        !inner
    });
    let mut tables: Vec<Table> = grids
        .into_iter()
        .map(|table| split_rows(table, text, 3))
        .collect();
    let lined = lined_tables(&rulings, text, &tables);
    tables.extend(lined);
    tables.retain(|table| valid(table, text));
    tables.sort_by(|a, b| b.bbox().y1.total_cmp(&a.bbox().y1));
    tables
}

/// Valeurs voisines (à `tolerance` près) réunies, triées par ordre croissant.
fn cluster(mut values: Vec<f64>, tolerance: f64) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut groups: Vec<Vec<f64>> = Vec::new();
    for value in values {
        match groups.last_mut() {
            Some(group) if value - group[0] <= tolerance => group.push(value),
            _ => groups.push(vec![value]),
        }
    }
    groups
        .into_iter()
        .map(|group| group.iter().sum::<f64>() / group.len() as f64)
        .collect()
}

fn nearest(values: &[f64], value: f64) -> usize {
    let mut best = 0;
    for (index, &v) in values.iter().enumerate() {
        if (v - value).abs() < (values[best] - value).abs() {
            best = index;
        }
    }
    best
}

/// Tableau formé par les cellules d'un quadrillage.
fn grid_table(cells: &[Rect]) -> Option<Table> {
    let columns = cluster(cells.iter().flat_map(|c| [c.x0, c.x1]).collect(), 1.0);
    let mut rows = cluster(cells.iter().flat_map(|c| [c.y0, c.y1]).collect(), 1.0);
    rows.reverse();
    if columns.len() < 2 || rows.len() < 2 {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for rect in cells {
        let (column, last_column) = (nearest(&columns, rect.x0), nearest(&columns, rect.x1));
        let (row, last_row) = (nearest(&rows, rect.y1), nearest(&rows, rect.y0));
        if last_column <= column || last_row <= row {
            continue;
        }
        out.push(Cell {
            row,
            column,
            row_span: last_row - row,
            column_span: last_column - column,
        });
    }
    let ruled_rows = vec![true; rows.len()];
    let mut table = Table {
        kind: TableKind::Grid,
        rows,
        columns,
        ruled_rows,
        cells: out,
    };
    table.sort_cells();
    Some(table)
}

/// Au moins deux lignes, deux colonnes et deux cellules qui contiennent du
/// texte.
fn valid(table: &Table, text: &[TextBox]) -> bool {
    if table.row_count() < 2 || table.column_count() < 2 {
        return false;
    }
    let mut filled = vec![false; table.cells.len()];
    for glyph in text {
        let (x, y) = glyph.bbox.center();
        if let Some(cell) = table.cell_at(x, y) {
            filled[cell] = true;
        }
    }
    filled.iter().filter(|&&f| f).count() >= 2
}

// MARK: - Lignes de texte

/// Ligne de texte : glyphes de même ligne de base, en fragments séparés par
/// de grands écarts.
#[derive(Clone, Debug)]
struct Line {
    baseline: f64,
    /// Fragments, de gauche à droite : (début, fin).
    fragments: Vec<(f64, f64)>,
    /// Emprise verticale : bas, haut.
    bottom: f64,
    top: f64,
}

/// Écart entre deux fragments d'une ligne, en cadratins : au-delà d'une
/// espace ordinaire, les mots appartiennent à deux colonnes.
const FRAGMENT_GAP: f64 = 0.75;

fn text_lines(glyphs: &[&TextBox]) -> Vec<Line> {
    let mut sorted: Vec<&TextBox> = glyphs.to_vec();
    sorted.sort_by(|a, b| b.baseline.total_cmp(&a.baseline));
    let mut groups: Vec<Vec<&TextBox>> = Vec::new();
    for glyph in sorted {
        match groups.last_mut() {
            Some(group)
                if (group[0].baseline - glyph.baseline).abs()
                    <= 0.3 * group[0].size.max(glyph.size) =>
            {
                group.push(glyph)
            }
            _ => groups.push(vec![glyph]),
        }
    }
    groups
        .into_iter()
        .map(|mut group| {
            group.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            let size = group.iter().map(|g| g.size).fold(0.0, f64::max);
            let mut fragments: Vec<(f64, f64)> = Vec::new();
            for glyph in &group {
                match fragments.last_mut() {
                    Some(last) if glyph.bbox.x0 - last.1 <= FRAGMENT_GAP * size => {
                        last.1 = last.1.max(glyph.bbox.x1)
                    }
                    _ => fragments.push((glyph.bbox.x0, glyph.bbox.x1)),
                }
            }
            Line {
                baseline: group[0].baseline,
                fragments,
                bottom: group
                    .iter()
                    .map(|g| g.bbox.y0)
                    .fold(f64::INFINITY, f64::min),
                top: group
                    .iter()
                    .map(|g| g.bbox.y1)
                    .fold(f64::NEG_INFINITY, f64::max),
            }
        })
        .collect()
}

// MARK: - Lignes déduites du texte

/// Coupe en plusieurs lignes chaque bande dont le texte forme des rangées
/// distinctes : chaque rangée de glyphes de même ligne de base qui occupe au
/// moins deux cellules commence une ligne ; une rangée qui n'en occupe qu'une
/// (texte qui passe à la ligne dans sa cellule) continue la précédente. Une
/// bande n'est coupée que si elle donne au moins `minimum` lignes.
fn split_rows(mut table: Table, text: &[TextBox], minimum: usize) -> Table {
    let bbox = table.bbox();
    let inside: Vec<(usize, &TextBox)> = text
        .iter()
        .filter_map(|glyph| {
            let (x, y) = glyph.bbox.center();
            if !bbox.contains(x, y) {
                return None;
            }
            table.cell_at(x, y).map(|cell| (cell, glyph))
        })
        .collect();

    // Pour chaque ligne d'origine, ses nouvelles limites intérieures.
    let mut inserted: Vec<Vec<f64>> = vec![Vec::new(); table.row_count()];
    for (row, cuts) in inserted.iter_mut().enumerate() {
        let single: Vec<usize> = (0..table.cells.len())
            .filter(|&c| table.cells[c].row == row && table.cells[c].row_span == 1)
            .collect();
        if single.len() < 2 {
            continue;
        }
        let mut glyphs: Vec<(usize, &TextBox)> = inside
            .iter()
            .filter(|(cell, _)| single.contains(cell))
            .copied()
            .collect();
        glyphs.sort_by(|a, b| b.1.baseline.total_cmp(&a.1.baseline));
        // Rangées : (cellules occupées, bas, haut).
        let mut clusters: Vec<(Vec<usize>, f64, f64, f64, f64)> = Vec::new();
        for (cell, glyph) in glyphs {
            match clusters.last_mut() {
                Some((cells, baseline, size, bottom, top))
                    if (*baseline - glyph.baseline).abs() <= 0.3 * size.max(glyph.size) =>
                {
                    if !cells.contains(&cell) {
                        cells.push(cell);
                    }
                    *size = size.max(glyph.size);
                    *bottom = bottom.min(glyph.bbox.y0);
                    *top = top.max(glyph.bbox.y1);
                }
                _ => clusters.push((
                    vec![cell],
                    glyph.baseline,
                    glyph.size,
                    glyph.bbox.y0,
                    glyph.bbox.y1,
                )),
            }
        }
        // Rangées regroupées en lignes : (bas, haut).
        let mut groups: Vec<(f64, f64)> = Vec::new();
        for (index, (cells, _, _, bottom, top)) in clusters.iter().enumerate() {
            match groups.last_mut() {
                Some(group) if index > 0 && cells.len() < 2 => {
                    group.0 = group.0.min(*bottom);
                }
                _ => groups.push((*bottom, *top)),
            }
        }
        if groups.len() < minimum.max(2) {
            continue;
        }
        let (upper, lower) = (table.rows[row], table.rows[row + 1]);
        for pair in groups.windows(2) {
            let cut = (pair[0].0 + pair[1].1) / 2.0;
            let previous = cuts.last().copied().unwrap_or(upper);
            if cut < previous - 0.5 && cut > lower + 0.5 {
                cuts.push(cut);
            }
        }
    }
    if inserted.iter().all(Vec::is_empty) {
        return table;
    }

    // Nouvelles limites et correspondance des anciennes lignes.
    let mut rows = vec![table.rows[0]];
    let mut ruled = vec![table.ruled_rows[0]];
    let mut range: Vec<Range<usize>> = Vec::new();
    for (row, cuts) in inserted.iter().enumerate() {
        let first = rows.len() - 1;
        for &cut in cuts {
            rows.push(cut);
            ruled.push(false);
        }
        rows.push(table.rows[row + 1]);
        ruled.push(table.ruled_rows[row + 1]);
        range.push(first..rows.len() - 1);
    }
    let mut cells = Vec::new();
    for cell in &table.cells {
        let start = range[cell.row].start;
        let end = range[cell.row + cell.row_span - 1].end;
        if cell.row_span == 1 {
            for row in start..end {
                cells.push(Cell {
                    row,
                    row_span: 1,
                    ..*cell
                });
            }
        } else {
            cells.push(Cell {
                row: start,
                row_span: end - start,
                ..*cell
            });
        }
    }
    table.rows = rows;
    table.ruled_rows = ruled;
    table.cells = cells;
    table.sort_cells();
    table
}

// MARK: - Tableaux à filets horizontaux

/// Tableaux tracés par des filets horizontaux seuls (au moins trois, de même
/// longueur), hors des quadrillages ; les colonnes sont les bandes que le
/// texte occupe, séparées par des espaces vides sur toutes les lignes.
fn lined_tables(rulings: &[Ruling], text: &[TextBox], grids: &[Table]) -> Vec<Table> {
    let boxes: Vec<Rect> = grids.iter().map(|t| t.bbox().inflate(2.0)).collect();
    let horizontal: Vec<&Ruling> = rulings
        .iter()
        .filter(|r| r.orientation == Orientation::Horizontal)
        .filter(|r| {
            let middle = (r.start + r.end) / 2.0;
            !boxes.iter().any(|b| b.contains(middle, r.position))
        })
        .collect();
    let similar = |a: &Ruling, b: &Ruling| {
        let overlap = a.end.min(b.end) - a.start.max(b.start);
        overlap >= 0.9 * a.length().max(b.length())
    };
    let mut groups: Vec<Vec<&Ruling>> = Vec::new();
    for ruling in horizontal {
        match groups.last_mut() {
            Some(group) if similar(group[0], ruling) => group.push(ruling),
            _ => groups.push(vec![ruling]),
        }
    }

    let mut tables = Vec::new();
    for group in groups {
        if group.len() < 3 {
            continue;
        }
        // Bandes : chacune garde ses lignes de texte ; la plus longue suite de
        // bandes « tabulaires » (au moins la moitié de leurs lignes en
        // plusieurs fragments) forme le tableau.
        let x0 = group.iter().map(|r| r.start).fold(f64::INFINITY, f64::min);
        let x1 = group
            .iter()
            .map(|r| r.end)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut tabular: Vec<bool> = Vec::new();
        for pair in group.windows(2) {
            let band = Rect::new(x0, pair[1].position, x1, pair[0].position);
            let glyphs: Vec<&TextBox> = text
                .iter()
                .filter(|g| {
                    let (x, y) = g.bbox.center();
                    band.contains(x, y)
                })
                .collect();
            let lines = text_lines(&glyphs);
            let multi = lines.iter().filter(|l| l.fragments.len() >= 2).count();
            tabular.push(2 * multi >= lines.len());
        }
        let mut best: Range<usize> = 0..0;
        let mut start = 0;
        for index in 0..=tabular.len() {
            if index == tabular.len() || !tabular[index] {
                if index - start > best.len() {
                    best = start..index;
                }
                start = index + 1;
            }
        }
        if best.len() < 2 {
            continue;
        }
        let rules = &group[best.start..=best.end];
        if let Some(table) = lined_table(rules, x0, x1, text) {
            tables.push(table);
        }
    }
    tables
}

fn lined_table(rules: &[&Ruling], x0: f64, x1: f64, text: &[TextBox]) -> Option<Table> {
    let top = rules[0].position;
    let bottom = rules[rules.len() - 1].position;
    let region = Rect::new(x0, bottom, x1, top);
    let glyphs: Vec<&TextBox> = text
        .iter()
        .filter(|g| {
            let (x, y) = g.bbox.center();
            region.contains(x, y)
        })
        .collect();
    let lines = text_lines(&glyphs);
    let multi: Vec<&Line> = lines.iter().filter(|l| l.fragments.len() >= 2).collect();
    if 2 * multi.len() < lines.len() || multi.len() < 2 {
        return None;
    }
    // Colonnes : réunion des fragments des lignes du corps en plusieurs
    // fragments. L'en-tête (au-dessus du deuxième filet) n'y entre pas : ses
    // intitulés sont souvent alignés autrement que les valeurs.
    let body: Vec<&Line> = multi
        .iter()
        .copied()
        .filter(|l| l.baseline < rules[1].position)
        .collect();
    let reference = if body.len() >= 2 { body } else { multi.clone() };
    let mut spans: Vec<(f64, f64)> = reference.iter().flat_map(|l| l.fragments.clone()).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.0 <= last.1 => last.1 = last.1.max(span.1),
            _ => merged.push(span),
        }
    }
    if merged.len() < 2 {
        return None;
    }
    // Limite entre deux colonnes : au milieu du plus large intervalle de
    // l'écart qu'aucun fragment (en-tête compris) ne recouvre, sinon au milieu
    // de l'écart (le fragment qui le traverse fusionne ses cellules).
    let fragments: Vec<(f64, f64)> = lines.iter().flat_map(|l| l.fragments.clone()).collect();
    let limit = |from: f64, to: f64| -> f64 {
        let mut free = vec![(from, to)];
        for &(start, end) in &fragments {
            free = free
                .into_iter()
                .flat_map(|(a, b)| {
                    if end <= a || start >= b {
                        vec![(a, b)]
                    } else {
                        [(a, start), (end, b)]
                            .into_iter()
                            .filter(|(a, b)| b > a)
                            .collect()
                    }
                })
                .collect();
        }
        free.iter()
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
            .map_or((from + to) / 2.0, |(a, b)| (a + b) / 2.0)
    };
    let mut columns = vec![x0.min(merged[0].0)];
    for pair in merged.windows(2) {
        columns.push(limit(pair[0].1, pair[1].0));
    }
    columns.push(x1.max(merged[merged.len() - 1].1));
    let rows: Vec<f64> = rules.iter().map(|r| r.position).collect();
    let mut cells = Vec::new();
    for row in 0..rows.len() - 1 {
        for column in 0..columns.len() - 1 {
            cells.push(Cell {
                row,
                column,
                row_span: 1,
                column_span: 1,
            });
        }
    }
    let table = Table {
        kind: TableKind::Lined,
        ruled_rows: vec![true; rows.len()],
        rows,
        columns,
        cells,
    };
    let mut table = split_rows(table, text, 2);
    merge_spanning(&mut table, &lines);
    Some(table)
}

/// Fusionne, dans chaque ligne, les cellules qu'un même fragment de texte
/// recouvre (un intertitre, un total qui déborde sur la colonne voisine).
fn merge_spanning(table: &mut Table, lines: &[Line]) {
    for line in lines {
        let Some(row) = table.row_at(line.baseline.max(line.bottom).min(line.top)) else {
            continue;
        };
        for &(start, end) in &line.fragments {
            let covered: Vec<usize> = (0..table.column_count())
                .filter(|&c| start < table.columns[c + 1] - 1.0 && end > table.columns[c] + 1.0)
                .collect();
            let (Some(&first), Some(&last)) = (covered.first(), covered.last()) else {
                continue;
            };
            if first == last {
                continue;
            }
            // Cellules de la ligne touchées, fusionnées en une seule.
            let touched: Vec<Cell> = table
                .cells
                .iter()
                .filter(|cell| {
                    cell.rows().contains(&row)
                        && cell.column <= last
                        && cell.column + cell.column_span > first
                })
                .copied()
                .collect();
            if touched.iter().any(|cell| cell.row_span != 1) {
                continue;
            }
            let column = touched.iter().map(|c| c.column).min().unwrap_or(first);
            let end_column = touched
                .iter()
                .map(|c| c.column + c.column_span)
                .max()
                .unwrap_or(last + 1);
            table.cells.retain(|cell| !touched.contains(cell));
            table.cells.push(Cell {
                row,
                column,
                row_span: 1,
                column_span: end_column - column,
            });
        }
    }
    table.sort_cells();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(y: f64, x0: f64, x1: f64) -> Segment {
        Segment::new((x0, y), (x1, y))
    }

    fn v(x: f64, y0: f64, y1: f64) -> Segment {
        Segment::new((x, y0), (x, y1))
    }

    /// Glyphes d'un mot de corps 10, chasse 5, ligne de base `baseline`.
    fn word(text: &mut Vec<TextBox>, x: f64, baseline: f64, letters: usize) {
        for i in 0..letters {
            let x0 = x + 5.0 * i as f64;
            text.push(TextBox {
                bbox: Rect::new(x0, baseline - 2.0, x0 + 5.0, baseline + 8.0),
                baseline,
                size: 10.0,
            });
        }
    }

    fn grid(rows: &[f64], columns: &[f64]) -> Vec<Segment> {
        let (left, right) = (columns[0], columns[columns.len() - 1]);
        let (top, bottom) = (rows[0], rows[rows.len() - 1]);
        let mut segments: Vec<Segment> = rows.iter().map(|&y| h(y, left, right)).collect();
        segments.extend(columns.iter().map(|&x| v(x, bottom, top)));
        segments
    }

    #[test]
    fn quadrillage_et_texte() {
        let segments = grid(&[300.0, 280.0, 260.0, 240.0], &[50.0, 150.0, 250.0]);
        let mut text = Vec::new();
        for (row, y) in [285.0, 265.0, 245.0].into_iter().enumerate() {
            word(&mut text, 55.0, y, 4 + row);
            word(&mut text, 200.0, y, 3);
        }
        let tables = detect(&segments, &text);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.kind, TableKind::Grid);
        assert_eq!((table.row_count(), table.column_count()), (3, 2));
        assert_eq!(table.cells.len(), 6);
        assert_eq!(table.cell_at(60.0, 270.0), Some(2));
        assert_eq!(
            table.cell_rect(&table.cells[5]),
            Rect::new(150.0, 240.0, 250.0, 260.0)
        );
    }

    #[test]
    fn un_cadre_seul_n_est_pas_un_tableau() {
        // Un encadré d'une seule cellule, ou une grille vide.
        let mut text = Vec::new();
        word(&mut text, 55.0, 285.0, 4);
        assert!(detect(&grid(&[300.0, 240.0], &[50.0, 250.0]), &text).is_empty());
        let empty = grid(&[300.0, 280.0, 260.0], &[50.0, 150.0, 250.0]);
        assert!(detect(&empty, &text).is_empty());
    }

    #[test]
    fn colonnes_tracees_lignes_deduites_du_texte() {
        // En-tête séparé par un filet, puis quatre rangées sans filet.
        let segments = grid(&[300.0, 280.0, 180.0], &[50.0, 150.0, 250.0]);
        let mut text = Vec::new();
        word(&mut text, 55.0, 285.0, 5);
        word(&mut text, 155.0, 285.0, 5);
        for (index, y) in [265.0, 250.0, 235.0, 220.0].into_iter().enumerate() {
            word(&mut text, 55.0, y, 6);
            word(&mut text, 200.0, y, 4);
            if index == 1 {
                // Texte qui passe à la ligne dans sa cellule.
                word(&mut text, 55.0, y - 7.5, 3);
            }
        }
        let tables = detect(&segments, &text);
        let table = &tables[0];
        assert_eq!(table.row_count(), 5, "{:?}", table.rows);
        assert_eq!(table.ruled_rows, [true, true, false, false, false, true]);
        // La deuxième rangée garde sa suite.
        let second = table.cell_at(60.0, 245.0).unwrap();
        assert_eq!(table.cell_at(60.0, 243.0), Some(second));
        assert_eq!(table.cells[second].row, 2);
    }

    #[test]
    fn filets_horizontaux_seuls() {
        // Trois filets : en-tête, puis trois rangées ; colonnes séparées par
        // de larges espaces ; un intertitre déborde sur deux colonnes.
        let segments = [
            h(300.0, 50.0, 350.0),
            h(280.0, 50.0, 350.0),
            h(200.0, 50.0, 350.0),
        ];
        let mut text = Vec::new();
        for x in [55.0, 155.0, 255.0] {
            word(&mut text, x, 285.0, 6);
        }
        for y in [265.0, 250.0, 235.0] {
            for x in [55.0, 155.0, 255.0] {
                word(&mut text, x, y, 4);
            }
        }
        word(&mut text, 55.0, 210.0, 25);
        let tables = detect(&segments, &text);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.kind, TableKind::Lined);
        assert_eq!((table.row_count(), table.column_count()), (5, 3));
        let title = table.cell_at(60.0, 212.0).unwrap();
        assert_eq!(table.cells[title].column_span, 2);
    }

    #[test]
    fn des_paragraphes_entre_filets_ne_sont_pas_un_tableau() {
        let segments = [
            h(300.0, 50.0, 350.0),
            h(250.0, 50.0, 350.0),
            h(200.0, 50.0, 350.0),
        ];
        let mut text = Vec::new();
        for y in [285.0, 270.0, 235.0, 220.0] {
            word(&mut text, 55.0, y, 50);
        }
        assert!(detect(&segments, &text).is_empty());
    }
}
