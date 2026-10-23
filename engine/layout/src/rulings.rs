//! Filets : traits horizontaux et verticaux d'une page, rassemblés.
//!
//! Les segments viennent des tracés (traits, bords de rectangles, rectangles
//! pleins très fins) ou, pour un scan, de l'image. Seuls les segments
//! horizontaux ou verticaux comptent ; ceux qui sont presque à la même
//! hauteur (ou abscisse) sont alignés, puis ceux qui se prolongent sont
//! réunis en un seul filet.

use crate::geometry::Segment;

/// Sens d'un filet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Horizontal.
    Horizontal,
    /// Vertical.
    Vertical,
}

/// Filet horizontal ou vertical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruling {
    /// Sens.
    pub orientation: Orientation,
    /// Ordonnée d'un filet horizontal, abscisse d'un filet vertical.
    pub position: f64,
    /// Début, le long du filet (abscisse ou ordonnée la plus petite).
    pub start: f64,
    /// Fin, le long du filet.
    pub end: f64,
}

impl Ruling {
    /// Longueur.
    pub fn length(&self) -> f64 {
        self.end - self.start
    }

    /// Vrai si le filet passe par la coordonnée `along` (à `tolerance` près).
    pub fn covers(&self, along: f64, tolerance: f64) -> bool {
        along >= self.start - tolerance && along <= self.end + tolerance
    }
}

/// Écart en deçà duquel deux filets parallèles sont alignés, en points.
pub const SNAP: f64 = 2.0;
/// Écart en deçà duquel deux filets alignés se prolongent, en points.
pub const JOIN: f64 = 2.0;
/// Longueur minimale d'un filet, en points.
const MIN_LENGTH: f64 = 3.0;

/// Filets tirés des segments : horizontaux d'abord (de haut en bas), puis
/// verticaux (de gauche à droite).
pub fn rulings(segments: &[Segment]) -> Vec<Ruling> {
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    for segment in segments {
        let (dx, dy) = (
            (segment.b.0 - segment.a.0).abs(),
            (segment.b.1 - segment.a.1).abs(),
        );
        if !(dx.is_finite() && dy.is_finite()) {
            continue;
        }
        if dx >= MIN_LENGTH && dy <= (0.02 * dx).max(0.5) {
            horizontal.push(Ruling {
                orientation: Orientation::Horizontal,
                position: (segment.a.1 + segment.b.1) / 2.0,
                start: segment.a.0.min(segment.b.0),
                end: segment.a.0.max(segment.b.0),
            });
        } else if dy >= MIN_LENGTH && dx <= (0.02 * dy).max(0.5) {
            vertical.push(Ruling {
                orientation: Orientation::Vertical,
                position: (segment.a.0 + segment.b.0) / 2.0,
                start: segment.a.1.min(segment.b.1),
                end: segment.a.1.max(segment.b.1),
            });
        }
    }
    let mut horizontal = merge(horizontal);
    // De haut en bas, puis de gauche à droite.
    horizontal.sort_by(|a, b| {
        b.position
            .total_cmp(&a.position)
            .then(a.start.total_cmp(&b.start))
    });
    let mut out = horizontal;
    out.extend(merge(vertical));
    out
}

/// Aligne puis réunit des filets de même sens ; résultat trié par position
/// croissante, puis par début.
fn merge(mut rulings: Vec<Ruling>) -> Vec<Ruling> {
    rulings.sort_by(|a, b| a.position.total_cmp(&b.position));
    // Groupes de positions voisines : chacun prend la position moyenne,
    // pondérée par la longueur.
    let mut groups: Vec<Vec<Ruling>> = Vec::new();
    for ruling in rulings {
        match groups.last_mut() {
            Some(group) if ruling.position - group[0].position <= SNAP => group.push(ruling),
            _ => groups.push(vec![ruling]),
        }
    }
    let mut out = Vec::new();
    for mut group in groups {
        let weight: f64 = group.iter().map(|r| r.length().max(1e-6)).sum();
        let position = group
            .iter()
            .map(|r| r.position * r.length().max(1e-6))
            .sum::<f64>()
            / weight;
        group.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut current: Option<Ruling> = None;
        for ruling in group {
            match &mut current {
                Some(c) if ruling.start <= c.end + JOIN => c.end = c.end.max(ruling.end),
                _ => {
                    out.extend(current.take());
                    current = Some(Ruling { position, ..ruling });
                }
            }
        }
        out.extend(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filets_alignes_et_reunis() {
        let segments = [
            Segment::new((0.0, 100.0), (50.0, 100.0)),
            Segment::new((51.5, 100.8), (120.0, 100.8)),
            Segment::new((0.0, 50.0), (120.0, 50.3)),
            Segment::new((0.0, 50.0), (0.0, 100.0)),
            // Trait oblique et trait trop court : ignorés.
            Segment::new((0.0, 0.0), (30.0, 30.0)),
            Segment::new((10.0, 10.0), (11.0, 10.0)),
        ];
        let rulings = rulings(&segments);
        assert_eq!(rulings.len(), 3);
        let top = rulings[0];
        assert_eq!(top.orientation, Orientation::Horizontal);
        assert_eq!((top.start, top.end), (0.0, 120.0));
        assert!((top.position - 100.4).abs() < 0.1, "{top:?}");
        assert!((rulings[1].position - 50.15).abs() < 1e-9);
        assert_eq!(rulings[2].orientation, Orientation::Vertical);
        assert_eq!((rulings[2].start, rulings[2].end), (50.0, 100.0));
    }

    #[test]
    fn filets_separes() {
        // Deux traits sur la même ligne, séparés de 10 pt : deux filets.
        let rulings = rulings(&[
            Segment::new((0.0, 10.0), (40.0, 10.0)),
            Segment::new((50.0, 10.0), (90.0, 10.0)),
        ]);
        assert_eq!(rulings.len(), 2);
        assert!(rulings[0].covers(41.0, 2.0) && !rulings[0].covers(45.0, 2.0));
    }
}
