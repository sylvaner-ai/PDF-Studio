//! Tests de rendu sur le jeu de PDF de référence.

use std::path::PathBuf;

use pdf_core::Document;

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

    let small = render::render(&content, 1.0).unwrap();
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

    let large = render::render(&content, 2.0).unwrap();
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
    let bitmap = render::render(&doc.page_content(4).unwrap(), 1.0).unwrap();
    assert!(bitmap.width > bitmap.height);
}

#[test]
fn refuse_les_echelles_invalides() {
    let doc = Document::open_file(&fixture("texte_simple.pdf"), None).unwrap();
    let content = doc.page_content(0).unwrap();
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 100.0] {
        assert!(render::render(&content, scale).is_err(), "échelle {scale}");
    }
}
