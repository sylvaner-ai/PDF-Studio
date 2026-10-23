//! Cellules d'un quadrillage : rectangles fermés par quatre filets.
//!
//! Chaque croisement d'un filet horizontal et d'un filet vertical est un coin
//! possible. À partir d'un coin supérieur gauche, la cellule est fermée par le
//! croisement le plus proche en bas à droite qui soit relié aux deux autres
//! coins par les mêmes filets (méthode de pdfplumber). Une cellule fusionnée,
//! dont un filet intérieur manque, est donc trouvée d'un seul tenant.

use std::collections::{HashMap, HashSet};

use crate::geometry::Rect;
use crate::rulings::{Orientation, Ruling};

/// Tolérance d'un croisement : un filet qui s'arrête juste avant l'autre le
/// croise encore, en points.
pub const INTERSECTION: f64 = 2.5;

/// Une cellule fermée et les filets qui la bordent : (haut, bas, gauche,
/// droite), en indices dans les filets horizontaux et verticaux.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RawCell {
    pub bbox: Rect,
    pub corners: [(usize, usize); 4],
}

/// Cellules fermées par les filets, regroupées par quadrillage : les cellules
/// qui partagent un coin sont du même quadrillage.
pub(crate) fn grids(rulings: &[Ruling]) -> Vec<Vec<Rect>> {
    let horizontal: Vec<&Ruling> = rulings
        .iter()
        .filter(|r| r.orientation == Orientation::Horizontal)
        .collect();
    let vertical: Vec<&Ruling> = rulings
        .iter()
        .filter(|r| r.orientation == Orientation::Vertical)
        .collect();

    let mut crosses: HashSet<(usize, usize)> = HashSet::new();
    let mut on_h: Vec<Vec<usize>> = vec![Vec::new(); horizontal.len()];
    let mut on_v: Vec<Vec<usize>> = vec![Vec::new(); vertical.len()];
    for (i, h) in horizontal.iter().enumerate() {
        for (j, v) in vertical.iter().enumerate() {
            if h.covers(v.position, INTERSECTION) && v.covers(h.position, INTERSECTION) {
                crosses.insert((i, j));
                on_h[i].push(j);
                on_v[j].push(i);
            }
        }
    }
    for list in &mut on_h {
        list.sort_by(|&a, &b| vertical[a].position.total_cmp(&vertical[b].position));
    }
    for list in &mut on_v {
        list.sort_by(|&a, &b| horizontal[b].position.total_cmp(&horizontal[a].position));
    }

    let mut corners: Vec<(usize, usize)> = crosses.iter().copied().collect();
    corners.sort_unstable();
    let mut cells: Vec<RawCell> = Vec::new();
    for &(i, j) in &corners {
        let (x, y) = (vertical[j].position, horizontal[i].position);
        'search: for &below in &on_v[j] {
            if horizontal[below].position >= y - 1.0 {
                continue;
            }
            for &right in &on_h[i] {
                if vertical[right].position <= x + 1.0 {
                    continue;
                }
                if crosses.contains(&(below, right)) {
                    cells.push(RawCell {
                        bbox: Rect::new(x, horizontal[below].position, vertical[right].position, y),
                        corners: [(i, j), (i, right), (below, j), (below, right)],
                    });
                    break 'search;
                }
            }
        }
    }

    // Regroupement par coins communs.
    let mut parent: Vec<usize> = (0..cells.len()).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        let mut root = x;
        while parent[root] != root {
            root = parent[root];
        }
        let mut node = x;
        while parent[node] != root {
            let next = parent[node];
            parent[node] = root;
            node = next;
        }
        root
    }
    let mut owner: HashMap<(usize, usize), usize> = HashMap::new();
    for (index, cell) in cells.iter().enumerate() {
        for corner in cell.corners {
            match owner.get(&corner) {
                Some(&other) => {
                    let (a, b) = (find(&mut parent, index), find(&mut parent, other));
                    parent[a] = b;
                }
                None => {
                    owner.insert(corner, index);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<Rect>> = HashMap::new();
    for (index, cell) in cells.iter().enumerate() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(cell.bbox);
    }
    let mut out: Vec<Vec<Rect>> = groups.into_values().collect();
    // De haut en bas, puis de gauche à droite.
    out.sort_by(|a, b| {
        let top = |cells: &[Rect]| cells.iter().map(|c| c.y1).fold(f64::NEG_INFINITY, f64::max);
        let left = |cells: &[Rect]| cells.iter().map(|c| c.x0).fold(f64::INFINITY, f64::min);
        top(b).total_cmp(&top(a)).then(left(a).total_cmp(&left(b)))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Segment;
    use crate::rulings::rulings;

    fn h(y: f64, x0: f64, x1: f64) -> Segment {
        Segment::new((x0, y), (x1, y))
    }

    fn v(x: f64, y0: f64, y1: f64) -> Segment {
        Segment::new((x, y0), (x, y1))
    }

    #[test]
    fn quadrillage_simple() {
        // 2 lignes × 3 colonnes.
        let mut segments = Vec::new();
        for y in [100.0, 80.0, 60.0] {
            segments.push(h(y, 0.0, 300.0));
        }
        for x in [0.0, 100.0, 200.0, 300.0] {
            segments.push(v(x, 60.0, 100.0));
        }
        let grids = grids(&rulings(&segments));
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].len(), 6);
    }

    #[test]
    fn cellule_fusionnee() {
        // Ligne du bas : les deux premières colonnes fusionnées (le filet
        // vertical intérieur s'arrête à la ligne du haut).
        let segments = [
            h(100.0, 0.0, 300.0),
            h(80.0, 0.0, 300.0),
            h(60.0, 0.0, 300.0),
            v(0.0, 60.0, 100.0),
            v(100.0, 80.0, 100.0),
            v(200.0, 60.0, 100.0),
            v(300.0, 60.0, 100.0),
        ];
        let grids = grids(&rulings(&segments));
        assert_eq!(grids.len(), 1);
        let cells = &grids[0];
        assert_eq!(cells.len(), 5);
        assert!(cells.contains(&Rect::new(0.0, 60.0, 200.0, 80.0)));
    }

    #[test]
    fn deux_tableaux_distincts() {
        let mut segments = Vec::new();
        for (dx, dy) in [(0.0, 0.0), (0.0, -200.0)] {
            for y in [100.0, 80.0, 60.0] {
                segments.push(h(y + dy, dx, 200.0 + dx));
            }
            for x in [0.0, 100.0, 200.0] {
                segments.push(v(x + dx, 60.0 + dy, 100.0 + dy));
            }
        }
        let grids = grids(&rulings(&segments));
        assert_eq!(grids.len(), 2);
        assert!(grids[0][0].y1 > grids[1][0].y1);
    }
}
