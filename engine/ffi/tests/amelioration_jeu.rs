//! Jeu de test du mode Amélioration (`tests/fixtures/amelioration`).
//!
//! Chaque scan doit s'ouvrir, être reconnu comme une page scannée par le
//! moteur actuel, et correspondre à sa vérité terrain (fichier `.json` du même
//! nom, écrit par `tests/fixtures/generate.py`) : nombre et format des pages,
//! taille de l'image à sa résolution, et emplacements relevés. Les lignes de
//! texte, ramenées dans l'image par les coins de la page, doivent tomber sur
//! de l'encre, et les zones de fond sur du papier vierge : c'est ce qui permet
//! aux jalons suivants de mesurer leurs résultats sur ces emplacements.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfstudio_ffi::{EditSession, PdfDocument, RenderedPage};
use serde_json::Value;

/// Scans du jeu, sans le lot de 50 pages.
const SCANS: &[&str] = &[
    "01_lettre_inclinee",
    "02_rapport_incline",
    "03_hors_plafond",
    "04_page_droite",
    "05_fond_jauni",
    "06_photocopie_grise",
    "07_bandeaux_couleur",
    "08_tableau_cellules",
    "09_photo_et_degrade",
    "10_basse_qualite",
    "11_orientation",
    "12_photo_perspective",
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/amelioration")
        .join(name)
}

fn truth(name: &str) -> Value {
    let text = std::fs::read_to_string(fixture(&format!("{name}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn open(name: &str) -> Arc<PdfDocument> {
    PdfDocument::open(fixture(name).to_string_lossy().into_owned(), None).unwrap()
}

/// Homographie qui envoie les quatre points `from` sur `to`.
struct Homography([f64; 8]);

impl Homography {
    fn new(from: &[(f64, f64)], to: &[(f64, f64)]) -> Self {
        let mut m = [[0f64; 9]; 8];
        for (i, (&(x, y), &(u, v))) in from.iter().zip(to).enumerate() {
            m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        for col in 0..8 {
            let pivot = (col..8)
                .max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))
                .unwrap();
            m.swap(col, pivot);
            for row in 0..8 {
                if row != col {
                    let factor = m[row][col] / m[col][col];
                    let pivot_row = m[col];
                    for (value, p) in m[row].iter_mut().zip(pivot_row) {
                        *value -= factor * p;
                    }
                }
            }
        }
        let mut h = [0f64; 8];
        for (i, value) in h.iter_mut().enumerate() {
            *value = m[i][8] / m[i][i];
        }
        Homography(h)
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let h = &self.0;
        let d = h[6] * x + h[7] * y + 1.0;
        (
            (h[0] * x + h[1] * y + h[2]) / d,
            (h[3] * x + h[4] * y + h[5]) / d,
        )
    }
}

/// Luminance des pixels de l'image couverts par le rectangle `rect` de la page
/// droite (en points), ramené dans l'image par `to_image`.
fn luma_in(image: &RenderedPage, to_image: &Homography, rect: &[f64]) -> Vec<f64> {
    let corners = [
        to_image.apply(rect[0], rect[1]),
        to_image.apply(rect[2], rect[1]),
        to_image.apply(rect[2], rect[3]),
        to_image.apply(rect[0], rect[3]),
    ];
    // Étendue des coins sur un axe, bornée à l'image.
    let span = |values: [f64; 4], limit: u32| {
        let low = values.iter().copied().fold(f64::MAX, f64::min).max(0.0);
        let high = values.iter().copied().fold(f64::MIN, f64::max);
        (low as u32, high.min(f64::from(limit)) as u32)
    };
    let (x0, x1) = span(corners.map(|c| c.0), image.width);
    let (y0, y1) = span(corners.map(|c| c.1), image.height);
    let mut values = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * image.bytes_per_row + x * 4) as usize;
            let p = &image.pixels[i..i + 3];
            values
                .push(0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]));
        }
    }
    values
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * p) as usize]
}

#[test]
fn chaque_scan_correspond_a_sa_verite_terrain() {
    for &name in SCANS {
        let truth = truth(name);
        assert_eq!(truth["fichier"], format!("{name}.pdf"));
        let pages = truth["pages"].as_array().unwrap();
        let document = open(&format!("{name}.pdf"));
        assert_eq!(document.page_count() as usize, pages.len(), "{name}");
        let sizes = document.page_sizes();

        // Toutes les pages sont des scans pour le moteur actuel (D58).
        let session = EditSession::new(Arc::clone(&document));
        let ids: Vec<u64> = session.pages().iter().map(|page| page.id).collect();
        assert_eq!(session.pages_to_recognize().unwrap(), ids, "{name}");

        for (index, page) in pages.iter().enumerate() {
            let label = format!("{name}, page {}", index + 1);
            let pdf = numbers(&page["page_pdf_pt"]);
            assert!(
                (f64::from(sizes[index].width) - pdf[0]).abs() < 0.1,
                "{label}"
            );
            assert!(
                (f64::from(sizes[index].height) - pdf[1]).abs() < 0.1,
                "{label}"
            );

            // L'image de la page, rendue à sa résolution, a la taille annoncée.
            let ppp = page["ppp"].as_f64().unwrap();
            let pixels = numbers(&page["image_px"]);
            let image = document
                .render_page(
                    index as u32,
                    (pixels[0] / f64::from(sizes[index].width)) as f32,
                    0,
                )
                .unwrap();
            assert!((f64::from(image.width) - pixels[0]).abs() <= 1.0, "{label}");
            assert!(
                (f64::from(image.height) - pixels[1]).abs() <= 1.0,
                "{label}"
            );

            let angle = page["inclinaison_deg"].as_f64();
            assert_eq!(
                page["a_verifier"].as_bool().unwrap(),
                angle.is_some_and(|a| a.abs() > 15.0),
                "{label}"
            );

            // Page droite (points) → image scannée (pixels), par ses coins.
            let format = numbers(&page["format_pt"]);
            let corners: Vec<(f64, f64)> = page["coins_page_px"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let c = numbers(c);
                    (c[0], c[1])
                })
                .collect();
            let page_corners = [
                (0.0, 0.0),
                (format[0], 0.0),
                (format[0], format[1]),
                (0.0, format[1]),
            ];
            let to_image = Homography::new(&page_corners, &corners);
            // Résolution de la page droite cohérente avec l'image.
            let side = (corners[1].0 - corners[0].0).hypot(corners[1].1 - corners[0].1);
            if page["perspective"] == false {
                assert!((side / format[0] * 72.0 - ppp).abs() < 1.0, "{label}");
            }

            let lines = page["lignes"].as_array().unwrap();
            assert!(lines.len() >= 5, "{label}");
            for line in lines {
                let rect = numbers(&line["rect"]);
                assert!(rect[0] >= 0.0 && rect[1] >= 0.0, "{label}");
                assert!(rect[2] <= format[0] && rect[3] <= format[1], "{label}");
                assert!(!line["texte"].as_str().unwrap().is_empty());
                // De l'encre (ou du texte clair sur un aplat) : du contraste.
                let mut values = luma_in(&image, &to_image, &rect);
                let spread = percentile(&mut values, 0.98) - percentile(&mut values, 0.02);
                assert!(
                    spread > 60.0,
                    "{label} : « {} » sans encre ({spread:.0})",
                    line["texte"]
                );
            }

            let zones = page["zones_fond"].as_array().unwrap();
            assert!(zones.len() >= 6, "{label}");
            for zone in zones {
                let mut values = luma_in(&image, &to_image, &numbers(zone));
                let spread = percentile(&mut values, 0.95) - percentile(&mut values, 0.05);
                let level = percentile(&mut values, 0.5);
                assert!(
                    level > 120.0 && spread < 50.0,
                    "{label} : zone {zone} ({level:.0}, {spread:.0})"
                );
            }

            for flat in page["aplats"].as_array().unwrap() {
                let color = numbers(&flat["couleur"]);
                let dominant = numbers(&flat["dominante_scan"]);
                // Couleur d'origine assombrie par le papier et l'éclairage.
                for (c, d) in color.iter().zip(&dominant) {
                    assert!(
                        d <= &(c + 3.0) && d >= &(c * 0.8 - 3.0),
                        "{label} : {}",
                        flat["id"]
                    );
                }
            }
        }
    }
}

#[test]
fn le_lot_compte_cinquante_pages_scannees() {
    let truth = truth("13_lot_50_pages");
    let pages = truth["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 50);
    let document = open("13_lot_50_pages.pdf");
    assert_eq!(document.page_count(), 50);
    let session = EditSession::new(document);
    assert_eq!(session.pages_to_recognize().unwrap().len(), 50);
    for page in pages {
        let source = page["fichier"].as_str().unwrap();
        assert!(fixture(source).exists(), "{source}");
    }
}
