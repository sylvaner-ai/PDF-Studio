//! Tests de lecture : plan du document et recherche plein texte.

use std::path::PathBuf;

use pdf_core::{Document, DocumentHandle, PageRect, Source};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn open(name: &str) -> Document {
    Document::open_file(&fixture(name), None).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn inside(rect: PageRect, width: f32, height: f32) -> bool {
    rect.x >= 0.0 && rect.y >= 0.0 && rect.x + rect.width <= width && rect.y + rect.height <= height
}

#[test]
fn plan_hierarchique_aplati() {
    let outline = open("signets.pdf").outline();
    let summary: Vec<_> = outline
        .iter()
        .map(|e| (e.title.as_str(), e.level, e.page))
        .collect();
    assert_eq!(
        summary,
        [
            ("1. Introduction", 0, Some(0)),
            ("1.1 Section 1", 1, Some(0)),
            ("1.2 Section 2", 1, Some(1)),
            ("2. Méthode", 0, Some(2)),
            ("2.1 Section 1", 1, Some(2)),
            ("2.2 Section 2", 1, Some(3)),
            ("3. Résultats", 0, Some(4)),
            ("3.1 Section 1", 1, Some(4)),
            ("3.2 Section 2", 1, Some(5)),
        ]
    );
    // Les chapitres visent la page entière ; les sections, la première ligne,
    // à 4 cm (113,4 pt) du haut de la page.
    assert_eq!(outline[0].top, None);
    let top = outline[1].top.expect("position de la section");
    assert!((top - 113.4).abs() < 1.0, "{top}");
}

#[test]
fn document_sans_plan() {
    assert!(open("texte_simple.pdf").outline().is_empty());
}

#[test]
fn recherche_insensible_aux_accents_et_a_la_casse() {
    let doc = open("texte_simple.pdf");
    let size = doc.page_sizes()[0];
    let text = doc.page_text(0).unwrap();

    // Cinq lignes reprennent le début du texte de référence, une par police.
    let found = text.search("EFFICACITE");
    assert_eq!(found.len(), 5, "{found:?}");
    for m in &found {
        assert_eq!(m.rects.len(), 1);
        let rect = m.rects[0];
        assert!(inside(rect, size.width, size.height), "{rect:?}");
        assert!(rect.width > 30.0 && rect.height > 5.0, "{rect:?}");
    }
    // Les occurrences sont dans l'ordre de lecture, de haut en bas.
    assert!(found.windows(2).all(|w| w[0].rects[0].y < w[1].rects[0].y));

    assert_eq!(text.search("oeuvre").len(), 5, "œ s'écrit aussi oe");
    assert_eq!(text.search("l'œuvre").len(), 5);
    assert_eq!(text.search("bleu profond").len(), 1);
    assert_eq!(text.search("ligatures fi fl ffi").len(), 1);
    assert!(text.search("introuvable").is_empty());
}

#[test]
fn page_scannee_sans_texte() {
    let text = open("scan_simule.pdf").page_text(0).unwrap();
    assert!(text.is_empty());
    assert!(text.search("a").is_empty());
}

#[test]
fn recherche_sur_une_page_tournee() {
    let doc = open("formats_mixtes.pdf");
    let size = doc.page_sizes()[4];
    let found = doc.page_text(4).unwrap().search("rotation");
    assert_eq!(found.len(), 1);
    let rect = found[0].rects[0];
    // Page portrait tournée d'un quart de tour : le texte se lit de haut en bas,
    // près du bord droit de la page affichée.
    assert!(inside(rect, size.width, size.height), "{rect:?}");
    assert!(rect.height > rect.width, "{rect:?}");
    assert!(rect.x > size.width - 120.0, "{rect:?}");
}

#[test]
fn recherche_page_par_page_depuis_un_autre_fil() {
    let handle = DocumentHandle::open(Source::File(fixture("multipage_100.pdf")), None).unwrap();
    let pages: Vec<u32> = (0..handle.page_count())
        .filter(|&i| !handle.search_page(i, "page 50 sur").unwrap().is_empty())
        .collect();
    assert_eq!(pages, [49]);
    assert!(handle.search_page(100, "page").is_err());
    assert_eq!(handle.outline().unwrap(), []);
}
