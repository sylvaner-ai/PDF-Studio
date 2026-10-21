//! Tests de rendu sur le jeu de PDF de référence.

use std::path::PathBuf;

use pdf_core::Document;
use render::{Region, Rotation};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn pixel(bitmap: &render::Bitmap, x: u32, y: u32) -> [u8; 4] {
    let i = (y * bitmap.bytes_per_row + x * 4) as usize;
    bitmap.pixels[i..i + 4].try_into().unwrap()
}

fn dark_ratio(bitmap: &render::Bitmap) -> f64 {
    let dark = bitmap
        .pixels
        .chunks_exact(4)
        .filter(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300)
        .count();
    dark as f64 / (bitmap.width * bitmap.height) as f64
}

#[test]
fn rend_une_page_a_72_et_144_ppp() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();

    let small = render::render(&content, 1.0, Rotation::None).unwrap();
    assert_eq!((small.width, small.height), (596, 842));
    assert_eq!(small.bytes_per_row, small.width * 4);
    assert_eq!(
        small.pixels.len(),
        (small.bytes_per_row * small.height) as usize
    );
    assert_eq!(
        pixel(&small, 0, 0),
        [255, 255, 255, 255],
        "fond blanc opaque"
    );

    let large = render::render(&content, 2.0, Rotation::None).unwrap();
    assert_eq!((large.width, large.height), (1191, 1684));
    let ratio = dark_ratio(&large);
    assert!(
        ratio > 0.001 && ratio < 0.2,
        "le texte doit apparaître : {ratio}"
    );
}

#[test]
fn rend_les_couleurs() {
    let doc = Document::open_file(&fixture("vectoriel.pdf"), None).unwrap();
    let bitmap = render::render_at_dpi(&doc.page_content(0).unwrap(), 72.0).unwrap();
    let reddish = bitmap
        .pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 150 && p[1] < 100 && p[2] < 100)
        .count();
    assert!(
        reddish > 1000,
        "disque rouge attendu, {reddish} pixels rouges"
    );
}

#[test]
fn rend_une_page_scannee() {
    let doc = Document::open_file(&fixture("scan_simule.pdf"), None).unwrap();
    let bitmap = render::render_at_dpi(&doc.page_content(0).unwrap(), 150.0).unwrap();
    assert!(dark_ratio(&bitmap) > 0.005);
}

#[test]
fn rend_la_page_tournee_en_paysage() {
    let doc = Document::open_file(&fixture("formats_mixtes.pdf"), None).unwrap();
    let bitmap = render::render(&doc.page_content(4).unwrap(), 1.0, Rotation::None).unwrap();
    assert!(bitmap.width > bitmap.height);
}

#[test]
fn refuse_les_echelles_invalides() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 100.0] {
        assert!(
            render::render(&content, scale, Rotation::None).is_err(),
            "échelle {scale}"
        );
    }
}

/// Écart moyen entre deux images de même taille, sur 255.
fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let total: u64 = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    total as f64 / a.len() as f64
}

/// Tourne une image d'un quart de tour dans le sens horaire.
fn rotate_quarter(bitmap: &render::Bitmap) -> Vec<u8> {
    let (w, h) = (bitmap.width, bitmap.height);
    let mut out = vec![0; bitmap.pixels.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = (h - 1 - y, x);
            let dst = ((ny * h + nx) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&pixel(bitmap, x, y));
        }
    }
    out
}

#[test]
fn tourne_l_affichage_par_quarts_de_tour() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    let straight = render::render(&content, 1.0, Rotation::None).unwrap();

    let quarter = render::render(&content, 1.0, Rotation::Quarter).unwrap();
    assert_eq!((quarter.width, quarter.height), (842, 596));
    let diff = mean_diff(&quarter.pixels, &rotate_quarter(&straight));
    assert!(diff < 2.0, "quart de tour : écart {diff}");

    let half = render::render(&content, 1.0, Rotation::Half).unwrap();
    let expected = rotate_quarter(&render::Bitmap {
        pixels: rotate_quarter(&straight),
        width: 842,
        height: 596,
        bytes_per_row: 842 * 4,
    });
    let diff = mean_diff(&half.pixels, &expected);
    assert!(diff < 2.0, "demi-tour : écart {diff}");

    let three = render::render(&content, 1.0, Rotation::ThreeQuarters).unwrap();
    assert_eq!((three.width, three.height), (842, 596));
    let diff = mean_diff(&rotate_quarter(&three), &straight.pixels);
    assert!(diff < 2.0, "trois quarts de tour : écart {diff}");
}

#[test]
fn angles_quelconques_ramenes_au_quart_de_tour() {
    assert_eq!(Rotation::from_degrees(-90), Rotation::ThreeQuarters);
    assert_eq!(Rotation::from_degrees(450), Rotation::Quarter);
    assert_eq!(Rotation::from_degrees(180), Rotation::Half);
    assert_eq!(Rotation::from_degrees(10), Rotation::None);
    assert_eq!(Rotation::Half.degrees(), 180);
}

#[test]
fn une_zone_est_identique_a_la_page_entiere() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    // Deux zones qui recouvrent les premières lignes de texte, page droite puis tournée.
    let regions = [
        (Rotation::None, (101, 203)),
        (Rotation::Quarter, (1380, 117)),
    ];
    for (rotation, (x, y)) in regions {
        let full = render::render(&content, 2.0, rotation).unwrap();
        let region = Region {
            x,
            y,
            width: 300,
            height: 150,
        };
        let tile = render::render_region(&content, 2.0, rotation, region).unwrap();
        assert_eq!((tile.width, tile.height), (300, 150));
        let mut crop = Vec::new();
        for y in region.y..region.y + region.height {
            let start = (y * full.bytes_per_row + region.x * 4) as usize;
            crop.extend_from_slice(&full.pixels[start..start + region.width as usize * 4]);
        }
        let diff = mean_diff(&tile.pixels, &crop);
        assert!(diff < 0.5, "{rotation:?} : écart {diff}");
        assert!(dark_ratio(&tile) > 0.001, "la zone contient du texte");
    }
}

#[test]
fn tuiles_au_dela_de_la_taille_maximale() {
    let doc = Document::open_file(&fixture("scan_simule.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    let scale = 40.0; // A4 à 2 880 ppp : 23 811 pixels de large
    assert!(render::render(&content, scale, Rotation::None).is_err());
    let (width, height) = render::image_size(content.size(), scale, Rotation::None);
    assert!(width > render::MAX_DIMENSION);
    let tile = render::render_region(
        &content,
        scale,
        Rotation::None,
        Region {
            x: width / 2,
            y: height / 2,
            width: 512,
            height: 512,
        },
    )
    .unwrap();
    assert_eq!((tile.width, tile.height), (512, 512));
}

#[test]
fn zones_hors_page() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    let edge = render::render_region(
        &content,
        1.0,
        Rotation::None,
        Region {
            x: 500,
            y: 800,
            width: 512,
            height: 512,
        },
    )
    .unwrap();
    assert_eq!((edge.width, edge.height), (96, 42), "zone rognée au bord");
    let outside = Region {
        x: 596,
        y: 0,
        width: 10,
        height: 10,
    };
    assert!(render::render_region(&content, 1.0, Rotation::None, outside).is_err());
}

/// Les polices standard non intégrées doivent dessiner un vrai tiret cadratin
/// (« — »), et non un demi-cadratin : régression observée sur macOS.
#[test]
fn tiret_cadratin_des_polices_standard() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    let text = content
        .display_list()
        .to_text_page(mupdf::TextPageFlags::empty())
        .unwrap();
    let mut dashes = Vec::new();
    for block in text.blocks() {
        for line in block.lines() {
            for quad in line
                .chars()
                .filter(|c| c.char() == Some('—'))
                .map(|c| c.quad())
            {
                dashes.push(pdf_core::PageRect {
                    x: quad.ul.x,
                    y: quad.ul.y,
                    width: quad.ur.x - quad.ul.x,
                    height: quad.ll.y - quad.ul.y,
                });
            }
        }
    }
    assert_eq!(dashes.len(), 5, "un tiret par ligne de police");
    let scale = 4.0;
    let bitmap = render::render(&content, scale, Rotation::None).unwrap();
    for rect in dashes {
        let (x0, x1) = (
            (rect.x * scale) as u32,
            ((rect.x + rect.width) * scale) as u32,
        );
        let (y0, y1) = (
            (rect.y * scale) as u32,
            ((rect.y + rect.height) * scale) as u32,
        );
        let longest_run = (y0..y1)
            .map(|y| {
                let mut best = 0;
                let mut run = 0;
                for x in x0..x1 {
                    let p = pixel(&bitmap, x, y);
                    if (p[0] as u32 + p[1] as u32 + p[2] as u32) < 384 {
                        run += 1;
                        best = best.max(run);
                    } else {
                        run = 0;
                    }
                }
                best
            })
            .max()
            .unwrap_or(0);
        let ratio = longest_run as f32 / (x1 - x0) as f32;
        assert!(
            ratio > 0.8,
            "tiret trop court ({ratio:.2} de la chasse) : {rect:?}"
        );
    }
}
