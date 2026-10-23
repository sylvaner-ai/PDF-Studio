//! Couche de texte des pages reconnues : détection des pages scannées,
//! recherche avant l'enregistrement, écriture et relecture.

use std::path::Path;
use std::sync::Arc;

use edit::{LayerWord, TextLayer};
use pdf_core::{write_document, Document, DocumentHandle, OutputPage, Source, SourceData};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Mot droit, en points PDF (ordonnées vers le haut).
fn word(text: &str, x: f32, y: f32, width: f32, line: u32) -> LayerWord {
    LayerWord {
        text: text.into(),
        quad: [x, y, x + width, y, x + width, y + 12.0, x, y + 12.0],
        confidence: 0.9,
        line,
    }
}

fn layer() -> Arc<TextLayer> {
    Arc::new(TextLayer {
        words: vec![
            word("L'œuvre", 72.0, 700.0, 50.0, 0),
            word("coûte", 130.0, 700.0, 40.0, 0),
            word("12,50 €", 176.0, 700.0, 45.0, 0),
            word("Évreux", 72.0, 680.0, 45.0, 1),
        ],
        engine: "essai".into(),
    })
}

#[test]
fn detecte_les_pages_scannees() {
    let mut scan = Document::from_bytes(&fixture("scan_simule.pdf"), None).unwrap();
    assert!(scan.needs_recognition(0).unwrap());
    let mut native = Document::from_bytes(&fixture("texte_simple.pdf"), None).unwrap();
    assert!(!native.needs_recognition(0).unwrap());
    assert!(!native.text_lines(0).unwrap().is_empty());
}

#[test]
fn recherche_dans_la_couche_avant_l_enregistrement() {
    let handle = DocumentHandle::open(Source::Bytes(fixture("scan_simule.pdf")), None).unwrap();
    assert!(handle.search_page(0, "oeuvre").unwrap().is_empty());
    let found = handle
        .search_page_with_layer(0, "oeuvre coute", layer())
        .unwrap();
    assert_eq!(found.len(), 1);
    let rect = found[0].rects[0];
    // Espace d'affichage : origine en haut à gauche de la page A4 ; « œuvre »
    // commence au troisième des sept caractères de « L'œuvre ».
    assert!((rect.x - (72.0 + 2.0 * 50.0 / 7.0)).abs() < 0.5, "{rect:?}");
    assert!((rect.y - (842.0 - 712.0)).abs() < 1.5, "{rect:?}");
    assert_eq!(
        handle
            .search_page_with_layer(0, "evreux", layer())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn ecrit_une_couche_invisible_cherchable() {
    let source = SourceData::new(fixture("scan_simule.pdf"), None);
    let page = OutputPage::Source {
        source: 0,
        index: 0,
        quarter_turns: 0,
    };
    let bytes = write_document(&[source], &[page], &[], &[Some(layer())]).unwrap();

    let mut doc = Document::from_bytes(&bytes, None).unwrap();
    assert!(
        !doc.needs_recognition(0).unwrap(),
        "la page a désormais du texte"
    );
    let lines = doc.text_lines(0).unwrap();
    let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["L'œuvre coûte 12,50 €", "Évreux"], "{lines:?}");
    // Le mot occupe l'emprise reconnue : de 72 à 221 points, ligne de pied
    // vers 700 points depuis le bas.
    let first = lines[0].rect;
    assert!((first.x - 72.0).abs() < 0.5, "{first:?}");
    assert!((first.x + first.width - 221.0).abs() < 0.5, "{first:?}");
    assert!((first.y + first.height - 142.0).abs() < 1.0, "{first:?}");
    assert_eq!(doc.page_text(0).unwrap().search("evreux").len(), 1);

    // Le texte est invisible : le rendu de la page ne change pas.
    let original = Document::from_bytes(&fixture("scan_simule.pdf"), None).unwrap();
    let before = original.page_content(0).unwrap();
    let after = doc.page_content(0).unwrap();
    let pixmap = |content: &pdf_core::PageContent| {
        content
            .display_list()
            .to_pixmap(
                &mupdf::Matrix::new_scale(0.5, 0.5),
                &mupdf::Colorspace::device_rgb(),
                false,
            )
            .unwrap()
            .samples()
            .to_vec()
    };
    assert_eq!(pixmap(&before), pixmap(&after));
}

#[test]
fn couche_sur_une_page_tournee() {
    // Page 2 d'edition.pdf : /Rotate 90. Un mot écrit horizontalement à
    // l'écran est vertical dans l'espace de la page.
    let bytes = fixture("edition.pdf");
    let mut doc = Document::from_bytes(&bytes, None).unwrap();
    let to_display = doc.display_matrix(1).unwrap();
    let to_page = to_display.inverse().unwrap();
    let corner = |x: f64, y: f64| {
        let p = to_page.apply(pdf_core::content::geometry::Point::new(x, y));
        [p.x as f32, p.y as f32]
    };
    // Mot de 100 × 20 points à l'écran, en (50, 60).
    let (bl, br, tr, tl) = (
        corner(50.0, 80.0),
        corner(150.0, 80.0),
        corner(150.0, 60.0),
        corner(50.0, 60.0),
    );
    let layer = TextLayer {
        words: vec![LayerWord {
            text: "Tourné".into(),
            quad: [bl[0], bl[1], br[0], br[1], tr[0], tr[1], tl[0], tl[1]],
            confidence: 1.0,
            line: 0,
        }],
        engine: "essai".into(),
    };
    let pages = [
        OutputPage::Source {
            source: 0,
            index: 0,
            quarter_turns: 0,
        },
        OutputPage::Source {
            source: 0,
            index: 1,
            quarter_turns: 0,
        },
    ];
    let written = write_document(
        &[SourceData::new(bytes, None)],
        &pages,
        &[],
        &[None, Some(Arc::new(layer))],
    )
    .unwrap();
    let reread = Document::from_bytes(&written, None).unwrap();
    let line = reread
        .text_lines(1)
        .unwrap()
        .into_iter()
        .find(|line| line.text.contains("Tourné"))
        .unwrap();
    assert!((line.rect.x - 50.0).abs() < 1.0, "{line:?}");
    assert!((line.rect.width - 100.0).abs() < 1.0, "{line:?}");
    assert!(
        line.rect.height < 25.0,
        "texte horizontal à l'écran : {line:?}"
    );
}

#[test]
fn texte_selectionnable_avant_et_apres_l_enregistrement() {
    let handle = DocumentHandle::open(Source::Bytes(fixture("scan_simule.pdf")), None).unwrap();
    assert!(
        handle.glyph_lines(0).unwrap().is_empty(),
        "un scan n'a pas de texte avant la reconnaissance"
    );
    let before = handle.glyph_lines_with_layer(0, layer()).unwrap();
    let texts: Vec<&str> = before.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["L'œuvre coûte 12,50 €", "Évreux"], "{before:?}");
    for line in &before {
        assert_eq!(line.rects.len(), line.text.chars().count(), "{line:?}");
    }

    // Une fois écrite, la couche invisible se retrouve dans le texte de la page.
    let source = SourceData::new(fixture("scan_simule.pdf"), None);
    let page = OutputPage::Source {
        source: 0,
        index: 0,
        quarter_turns: 0,
    };
    let bytes = write_document(&[source], &[page], &[], &[Some(layer())]).unwrap();
    let reopened = DocumentHandle::open(Source::Bytes(bytes), None).unwrap();
    let after = reopened.glyph_lines(0).unwrap();
    let texts: Vec<&str> = after.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["L'œuvre coûte 12,50 €", "Évreux"], "{after:?}");
    let first = after[0].rects.first().unwrap();
    let last = after[0].rects.last().unwrap();
    assert!((first.x - 72.0).abs() < 0.5, "{first:?}");
    assert!((last.x + last.width - 221.0).abs() < 0.5, "{last:?}");
    assert!(last.y > 100.0 && last.y < 142.0, "{last:?}");
}
