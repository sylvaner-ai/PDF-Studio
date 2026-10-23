//! Réorganisation d'un tableau : lignes ou colonnes supprimées, déplacées.
//!
//! Une réorganisation est décrite, pour chaque axe, par les bandes d'origine
//! conservées dans leur nouvel ordre. Elle se traduit en déplacements : chaque
//! élément situé dans une bande (texte d'une cellule, filet propre à une
//! cellule, fond d'une ligne) la suit ou disparaît avec elle ; un élément qui
//! traverse plusieurs bandes (filet vertical d'une colonne, cadre du tableau)
//! est déformé point par point, ses extrémités suivant les limites des
//! bandes. Le tableau se resserre : rien ne bouge en dehors de lui.
//!
//! Les limites gardent leur rang : le filet du haut reste en haut, celui sous
//! l'en-tête reste sous la première ligne. Quand des lignes disparaissent,
//! une seule des limites qui les bordent reste : la plus marquée (celle qui
//! borde le plus de cellules et porte un filet), la première à égalité. Le
//! cadre du tableau est toujours conservé, et la limite intérieure d'une
//! cellule fusionnée cède devant celle qui la borde.

use std::collections::BTreeSet;

use crate::geometry::Rect;
use crate::table::{Cell, Table};

/// Tolérance de position d'un élément sur une limite, en points.
const TOLERANCE: f64 = 1.5;

/// Axe d'un tableau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Lignes (ordonnées).
    Rows,
    /// Colonnes (abscisses).
    Columns,
}

/// Ce que devient un élément le long d'un axe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// Inchangé : l'élément est hors du tableau, ou l'axe n'est pas réorganisé.
    Unchanged,
    /// Supprimé avec sa bande.
    Removed,
    /// Déplacé d'un bloc, de la valeur donnée (en points de la page).
    Shifted(f64),
    /// Déformé point par point ([`Restructure::map`]).
    Stretched,
}

/// Réorganisation d'un axe, dans le sens de lecture : `t` croissant vers la
/// droite pour les colonnes, vers le bas pour les lignes.
#[derive(Clone, Debug)]
struct AxisMap {
    /// −1 pour les lignes (t = −y), 1 pour les colonnes.
    sign: f64,
    /// Limites d'origine, croissantes.
    bounds: Vec<f64>,
    /// Début de chaque bande d'origine après réorganisation (`None` : supprimée).
    starts: Vec<Option<f64>>,
    /// Nouvelle position de chaque limite d'origine (`None` : retirée).
    limits: Vec<Option<f64>>,
    /// Position où se referme chaque bande supprimée.
    collapse: Vec<f64>,
    /// Bandes d'origine, dans le nouvel ordre.
    order: Vec<usize>,
    identity: bool,
}

impl AxisMap {
    fn new(bounds: Vec<f64>, sign: f64, order: &[usize], weights: &[f64]) -> Self {
        let n = bounds.len() - 1;
        let order: Vec<usize> = {
            let mut seen = BTreeSet::new();
            order
                .iter()
                .copied()
                .filter(|&b| b < n && seen.insert(b))
                .collect()
        };
        let heights: Vec<f64> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
        let mut starts = vec![None; n];
        let mut position = bounds[0];
        for &band in &order {
            starts[band] = Some(position);
            position += heights[band];
        }
        let mut limits = vec![None; n + 1];
        if !order.is_empty() {
            let mut sorted = order.clone();
            sorted.sort_unstable();
            limits[0] = Some(bounds[0]);
            limits[n] = Some(position);
            let mut at = bounds[0];
            for (index, &band) in order.iter().enumerate() {
                at += heights[band];
                if index + 1 < order.len() {
                    // Entre deux bandes conservées, une seule limite reste :
                    // la plus marquée, sinon la première.
                    let kept = (sorted[index] + 1..=sorted[index + 1])
                        .fold(None, |best: Option<usize>, k| match best {
                            Some(b) if weights[b] >= weights[k] => Some(b),
                            _ => Some(k),
                        })
                        .unwrap_or(sorted[index] + 1);
                    limits[kept] = Some(at);
                }
            }
        }
        let mut collapse = vec![bounds[0]; n];
        let mut previous: Option<usize> = None;
        for band in 0..n {
            match starts[band] {
                Some(_) => previous = Some(band),
                None => {
                    collapse[band] = previous
                        .and_then(|p| starts[p].map(|s| s + heights[p]))
                        .unwrap_or(bounds[0]);
                }
            }
        }
        let identity = order.len() == n && order.iter().enumerate().all(|(i, &b)| i == b);
        AxisMap {
            sign,
            bounds,
            starts,
            limits,
            collapse,
            order,
            identity,
        }
    }

    fn n(&self) -> usize {
        self.bounds.len() - 1
    }

    /// Limite proche de `t`.
    fn limit_at(&self, t: f64) -> Option<usize> {
        (0..=self.n()).find(|&k| (t - self.bounds[k]).abs() <= TOLERANCE)
    }

    /// Bande qui contient `t` (hors des limites).
    fn band_at(&self, t: f64) -> Option<usize> {
        (0..self.n()).find(|&r| t >= self.bounds[r] && t <= self.bounds[r + 1])
    }

    fn inside(&self, t: f64) -> bool {
        t >= self.bounds[0] - TOLERANCE && t <= self.bounds[self.n()] + TOLERANCE
    }

    /// Nouvelle position de `t` (dans le sens de lecture).
    fn map_t(&self, t: f64) -> f64 {
        if self.identity || !self.inside(t) {
            return t;
        }
        if let Some(k) = self.limit_at(t) {
            if let Some(limit) = self.limits[k] {
                return limit + (t - self.bounds[k]);
            }
            let band = if k < self.n() && self.starts[k].is_none() {
                k
            } else {
                k.saturating_sub(1)
            };
            return self.collapse[band];
        }
        match self.band_at(t) {
            Some(band) => match self.starts[band] {
                Some(start) => t - self.bounds[band] + start,
                None => self.collapse[band],
            },
            None => t,
        }
    }

    /// Sort d'un élément qui s'étend de `lo` à `hi` (coordonnées de la page).
    fn place(&self, lo: f64, hi: f64) -> Placement {
        if self.identity {
            return Placement::Unchanged;
        }
        let (a, b) = (self.sign * lo, self.sign * hi);
        let (lo, hi) = (a.min(b), a.max(b));
        if !(self.inside(lo) && self.inside(hi)) {
            return Placement::Unchanged;
        }
        if self.order.is_empty() {
            return Placement::Removed;
        }
        // Filet sur une limite : il la suit, ou disparaît avec elle.
        if hi - lo <= 2.0 * TOLERANCE {
            if let Some(k) = self.limit_at((lo + hi) / 2.0) {
                return match self.limits[k] {
                    Some(limit) => Placement::Shifted(self.sign * (limit - self.bounds[k])),
                    None => Placement::Removed,
                };
            }
        }
        let (from, to) = (lo + TOLERANCE, hi - TOLERANCE);
        let bands: Vec<usize> = if from < to {
            (0..self.n())
                .filter(|&r| self.bounds[r + 1] > from && self.bounds[r] < to)
                .collect()
        } else {
            self.band_at((lo + hi) / 2.0).into_iter().collect()
        };
        if bands.is_empty() {
            return Placement::Unchanged;
        }
        if bands.iter().all(|&r| self.starts[r].is_none()) {
            return Placement::Removed;
        }
        if let [band] = bands[..] {
            let start = self.starts[band].unwrap_or(self.collapse[band]);
            return Placement::Shifted(self.sign * (start - self.bounds[band]));
        }
        Placement::Stretched
    }

    /// Nouvelle position d'une coordonnée de la page.
    fn map(&self, value: f64) -> f64 {
        self.sign * self.map_t(self.sign * value)
    }
}

/// Réorganisation d'un tableau : lignes et colonnes d'origine conservées, dans
/// leur nouvel ordre.
#[derive(Clone, Debug)]
pub struct Restructure {
    rows: AxisMap,
    columns: AxisMap,
    bbox: Rect,
    table: Table,
}

impl Restructure {
    /// Réorganisation de `table` : `rows` et `columns` donnent les lignes et
    /// les colonnes d'origine conservées, dans leur nouvel ordre.
    pub fn new(table: &Table, rows: &[usize], columns: &[usize]) -> Self {
        let row_bounds: Vec<f64> = table.rows.iter().map(|y| -y).collect();
        Restructure {
            rows: AxisMap::new(row_bounds, -1.0, rows, &weights(table, Axis::Rows)),
            columns: AxisMap::new(
                table.columns.clone(),
                1.0,
                columns,
                &weights(table, Axis::Columns),
            ),
            bbox: table.bbox().inflate(TOLERANCE),
            table: table.clone(),
        }
    }

    /// Vrai si rien ne change.
    pub fn is_identity(&self) -> bool {
        self.rows.identity && self.columns.identity
    }

    /// Vrai si l'élément d'emprise `bbox` appartient au tableau (il est
    /// entièrement dans son emprise).
    pub fn contains(&self, bbox: &Rect) -> bool {
        self.bbox.contains_rect(bbox)
    }

    /// Sort de l'élément d'emprise `bbox`, le long de chaque axe : lignes
    /// (ordonnées), puis colonnes (abscisses).
    pub fn place(&self, bbox: &Rect) -> (Placement, Placement) {
        if !self.contains(bbox) {
            return (Placement::Unchanged, Placement::Unchanged);
        }
        (
            self.rows.place(bbox.y0, bbox.y1),
            self.columns.place(bbox.x0, bbox.x1),
        )
    }

    /// Vrai si l'élément d'emprise `bbox` disparaît.
    pub fn removes(&self, bbox: &Rect) -> bool {
        let (rows, columns) = self.place(bbox);
        rows == Placement::Removed || columns == Placement::Removed
    }

    /// Nouvelle position d'un point d'un élément d'emprise `bbox`.
    pub fn map(&self, bbox: &Rect, x: f64, y: f64) -> (f64, f64) {
        let (rows, columns) = self.place(bbox);
        let along = |placement: Placement, axis: &AxisMap, value: f64| match placement {
            Placement::Shifted(d) => value + d,
            Placement::Stretched => axis.map(value),
            Placement::Unchanged | Placement::Removed => value,
        };
        (along(columns, &self.columns, x), along(rows, &self.rows, y))
    }

    /// Déplacement d'un élément qui se déplace d'un bloc (un texte, une
    /// image) : celui de son centre.
    pub fn offset(&self, bbox: &Rect) -> (f64, f64) {
        let (x, y) = bbox.center();
        let (rows, columns) = self.place(bbox);
        let along = |placement: Placement, axis: &AxisMap, value: f64| match placement {
            Placement::Shifted(d) => d,
            Placement::Stretched => axis.map(value) - value,
            Placement::Unchanged | Placement::Removed => 0.0,
        };
        (along(columns, &self.columns, x), along(rows, &self.rows, y))
    }

    /// Le tableau réorganisé ; `None` s'il ne reste ni ligne ni colonne.
    pub fn table(&self) -> Option<Table> {
        if self.rows.order.is_empty() || self.columns.order.is_empty() {
            return None;
        }
        let new_index = |axis: &AxisMap| -> Vec<Option<usize>> {
            let mut index = vec![None; axis.n()];
            for (i, &band) in axis.order.iter().enumerate() {
                index[band] = Some(i);
            }
            index
        };
        let (row_index, column_index) = (new_index(&self.rows), new_index(&self.columns));
        let bounds = |axis: &AxisMap| -> Vec<f64> {
            let mut out = vec![axis.bounds[0]];
            let mut at = axis.bounds[0];
            for &band in &axis.order {
                at += axis.bounds[band + 1] - axis.bounds[band];
                out.push(at);
            }
            out.iter().map(|t| axis.sign * t).collect()
        };
        let mut sorted = self.rows.order.clone();
        sorted.sort_unstable();
        let n = self.rows.n();
        let mut ruled_rows = vec![self.table.ruled_rows[0]];
        for i in 1..sorted.len() {
            ruled_rows.push(self.table.ruled_rows[sorted[i - 1] + 1]);
        }
        ruled_rows.push(self.table.ruled_rows[n]);
        let cells = self
            .cells_with(&row_index, &column_index)
            .into_iter()
            .map(|(_, cell)| cell)
            .collect();
        Some(Table {
            kind: self.table.kind,
            rows: bounds(&self.rows),
            columns: bounds(&self.columns),
            ruled_rows,
            cells,
        })
    }
}

impl Restructure {
    /// Cellules du tableau réorganisé, chacune avec l'indice de la cellule
    /// d'origine dont elle provient, ligne par ligne.
    pub fn cells(&self) -> Vec<(usize, Cell)> {
        let new_index = |axis: &AxisMap| -> Vec<Option<usize>> {
            let mut index = vec![None; axis.n()];
            for (i, &band) in axis.order.iter().enumerate() {
                index[band] = Some(i);
            }
            index
        };
        self.cells_with(&new_index(&self.rows), &new_index(&self.columns))
    }

    fn cells_with(
        &self,
        row_index: &[Option<usize>],
        column_index: &[Option<usize>],
    ) -> Vec<(usize, Cell)> {
        let mut cells = Vec::new();
        for (source, cell) in self.table.cells.iter().enumerate() {
            let rows: Vec<usize> = cell.rows().filter_map(|r| row_index[r]).collect();
            let columns: Vec<usize> = cell.columns().filter_map(|c| column_index[c]).collect();
            let (Some(&r0), Some(&r1)) = (rows.iter().min(), rows.iter().max()) else {
                continue;
            };
            let (Some(&c0), Some(&c1)) = (columns.iter().min(), columns.iter().max()) else {
                continue;
            };
            cells.push((
                source,
                Cell {
                    row: r0,
                    column: c0,
                    row_span: r1 - r0 + 1,
                    column_span: c1 - c0 + 1,
                },
            ));
        }
        cells.sort_by_key(|(_, cell)| (cell.row, cell.column));
        cells
    }
}

/// Poids de chaque limite d'un axe : la largeur (en bandes de l'autre axe)
/// sur laquelle elle borde des cellules, plus encore si elle porte un filet.
/// Une limite intérieure à une cellule fusionnée, ou une limite de ligne sans
/// filet, pèse moins.
fn weights(table: &Table, axis: Axis) -> Vec<f64> {
    let (count, across) = match axis {
        Axis::Rows => (table.row_count(), table.column_count()),
        Axis::Columns => (table.column_count(), table.row_count()),
    };
    let mut weights = vec![0.0; count + 1];
    for cell in &table.cells {
        let (start, span, width) = match axis {
            Axis::Rows => (cell.row, cell.row_span, cell.column_span),
            Axis::Columns => (cell.column, cell.column_span, cell.row_span),
        };
        weights[start] += width as f64;
        weights[start + span] += width as f64;
    }
    if axis == Axis::Rows {
        for (weight, ruled) in weights.iter_mut().zip(&table.ruled_rows) {
            if *ruled {
                *weight += 2.0 * across as f64;
            }
        }
    }
    weights
}

/// Vrai si `order` (bandes d'origine de l'axe, dans leur nouvel ordre) garde
/// chaque cellule fusionnée d'un seul tenant ; ses bandes peuvent échanger
/// leur place (deux sous-colonnes d'un même intitulé).
pub fn keeps_merged_cells(table: &Table, axis: Axis, order: &[usize]) -> bool {
    let count = match axis {
        Axis::Rows => table.row_count(),
        Axis::Columns => table.column_count(),
    };
    let mut seen = BTreeSet::new();
    if order.iter().any(|&b| b >= count || !seen.insert(b)) {
        return false;
    }
    table.cells.iter().all(|cell| {
        let bands = match axis {
            Axis::Rows => cell.rows(),
            Axis::Columns => cell.columns(),
        };
        let positions: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, band)| bands.contains(band))
            .map(|(position, _)| position)
            .collect();
        positions.windows(2).all(|w| w[1] == w[0] + 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::TableKind;

    /// 3 lignes de 20 pt (de 300 à 240) × 2 colonnes (50-150-250) ; la
    /// première colonne de la dernière ligne est fusionnée avec la seconde.
    fn table() -> Table {
        let mut cells = Vec::new();
        for row in 0..2 {
            for column in 0..2 {
                cells.push(Cell {
                    row,
                    column,
                    row_span: 1,
                    column_span: 1,
                });
            }
        }
        cells.push(Cell {
            row: 2,
            column: 0,
            row_span: 1,
            column_span: 2,
        });
        Table {
            kind: TableKind::Grid,
            rows: vec![300.0, 280.0, 260.0, 240.0],
            columns: vec![50.0, 150.0, 250.0],
            ruled_rows: vec![true; 4],
            cells,
        }
    }

    #[test]
    fn suppression_d_une_ligne() {
        let t = table();
        let r = Restructure::new(&t, &[0, 2], &[0, 1]);
        // Texte de la ligne supprimée, et filet sous elle (limite 2, retirée).
        assert!(r.removes(&Rect::new(55.0, 262.0, 80.0, 272.0)));
        assert!(r.removes(&Rect::new(50.0, 259.8, 250.0, 260.2)));
        // Le filet sous la première ligne reste ; la dernière ligne remonte.
        assert_eq!(
            r.place(&Rect::new(50.0, 279.8, 250.0, 280.2)).0,
            Placement::Shifted(0.0)
        );
        assert_eq!(r.offset(&Rect::new(55.0, 242.0, 80.0, 252.0)), (0.0, 20.0));
        // Filet vertical de toute la hauteur : son bas remonte.
        let left = Rect::new(49.8, 240.0, 50.2, 300.0);
        assert_eq!(r.place(&left).0, Placement::Stretched);
        assert_eq!(r.map(&left, 50.0, 240.0), (50.0, 260.0));
        assert_eq!(r.map(&left, 50.0, 300.0), (50.0, 300.0));
        // Un élément hors du tableau ne bouge pas.
        assert_eq!(r.offset(&Rect::new(300.0, 250.0, 320.0, 255.0)), (0.0, 0.0));
        let new = r.table().unwrap();
        assert_eq!(new.rows, [300.0, 280.0, 260.0]);
        assert_eq!(new.cells.len(), 3);
        // La cellule fusionnée vient de la cinquième cellule d'origine.
        assert_eq!(r.cells()[2].0, 4);
        assert_eq!(new.cells[2].column_span, 2);
    }

    #[test]
    fn suppression_de_la_derniere_ligne_et_d_une_colonne() {
        let t = table();
        let r = Restructure::new(&t, &[0, 1], &[0]);
        // Le cadre du bas (limite 3) remonte à la place du filet retiré.
        assert_eq!(
            r.place(&Rect::new(50.0, 239.8, 250.0, 240.2)).0,
            Placement::Shifted(20.0)
        );
        assert!(r.removes(&Rect::new(50.0, 259.8, 250.0, 260.2)));
        // La seconde colonne disparaît, le bord droit vient à 150.
        assert!(r.removes(&Rect::new(160.0, 282.0, 200.0, 292.0)));
        let right = Rect::new(249.8, 240.0, 250.2, 300.0);
        assert_eq!(r.place(&right).1, Placement::Shifted(-100.0));
        // La cellule fusionnée perd une colonne.
        let new = r.table().unwrap();
        assert_eq!(new.columns, [50.0, 150.0]);
        assert!(new.cells.iter().all(|c| c.column_span == 1));
    }

    #[test]
    fn deplacement_de_lignes() {
        let t = table();
        let r = Restructure::new(&t, &[1, 0, 2], &[0, 1]);
        assert_eq!(r.offset(&Rect::new(55.0, 282.0, 80.0, 292.0)), (0.0, -20.0));
        assert_eq!(r.offset(&Rect::new(55.0, 262.0, 80.0, 272.0)), (0.0, 20.0));
        // Les filets gardent leur place.
        assert_eq!(
            r.place(&Rect::new(50.0, 279.8, 250.0, 280.2)).0,
            Placement::Shifted(0.0)
        );
        assert!(keeps_merged_cells(&t, Axis::Rows, &[1, 0, 2]));
        assert!(keeps_merged_cells(&t, Axis::Columns, &[1, 0]));
        assert!(keeps_merged_cells(&t, Axis::Columns, &[1]));
        assert!(!keeps_merged_cells(&t, Axis::Rows, &[0, 0]));
        assert!(Restructure::new(&t, &[0, 1, 2], &[0, 1]).is_identity());
    }

    #[test]
    fn tout_supprimer() {
        let t = table();
        let r = Restructure::new(&t, &[], &[0, 1]);
        assert!(r.removes(&Rect::new(50.0, 240.0, 250.0, 300.0)));
        assert!(r.table().is_none());
    }
}
