//! Redressement des pages du mode Amélioration (jalon 2), sur le jeu de test
//! `tests/fixtures/amelioration` : inclinaison mesurée à moins de 0,2° de la
//! vérité terrain, inclinaison résiduelle de la page redressée sous 0,2°,
//! pages droites et pages au-delà du plafond laissées telles quelles, pages
//! couchées ou retournées remises à l'endroit, coins découverts remplis de la
//! couleur du papier ; puis application à la session, annulation, export et
//! retour aux pages d'origine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ocr::{preprocess, GrayImage};
use pdfstudio_ffi::{
    default_enhancement_settings, EditSession, EnhancedPage, PageKind, PdfDocument, RenderedPage,
    StraighteningStatus,
};
use serde_json::Value;

/// Scans dont l'inclinaison est connue (la photo en perspective n'en a pas).
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
];

/// Écart toléré, en degrés (critère d'acceptation).
const TOLERANCE: f64 = 0.2;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/amelioration")
        .join(name)
}

fn truth(name: &str) -> Value {
    let text = std::fs::read_to_string(fixture(&format!("{name}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn open(name: &str) -> Arc<PdfDocument> {
    PdfDocument::open(
        fixture(&format!("{name}.pdf"))
            .to_string_lossy()
            .into_owned(),
        None,
    )
    .unwrap()
}

fn gray(page: &RenderedPage) -> GrayImage {
    GrayImage::from_rgba(page.width, page.height, page.bytes_per_row, &page.pixels)
}

/// Inclinaison des lignes de texte de la page rendue, en degrés.
fn skew(page: &RenderedPage, dpi: f32) -> f64 {
    let image = preprocess::flatten_background(&gray(page), dpi);
    preprocess::measure_skew(&image, dpi, 45.0).degrees
}

fn pixel(page: &RenderedPage, x: u32, y: u32) -> [u8; 3] {
    let k = (y * page.bytes_per_row + x * 4) as usize;
    [page.pixels[k], page.pixels[k + 1], page.pixels[k + 2]]
}

/// Couleur du papier de la marge, entre `near` et `far` pixels du coin
/// `(x, y)` dans la direction `(dx, dy)` : médiane de la moitié la plus
/// claire des pixels.
fn margin_color(
    page: &RenderedPage,
    x: u32,
    y: u32,
    dx: i32,
    dy: i32,
    near: u32,
    far: u32,
) -> [u8; 3] {
    let mut pixels = Vec::new();
    for i in near..far {
        for j in near..far {
            let px = (x as i32 + dx * i as i32) as u32;
            let py = (y as i32 + dy * j as i32) as u32;
            pixels.push(pixel(page, px, py));
        }
    }
    pixels.sort_by_key(|c| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]));
    let bright = &pixels[pixels.len() / 2..];
    let mut out = [0u8; 3];
    for (c, value) in out.iter_mut().enumerate() {
        let mut values: Vec<u8> = bright.iter().map(|p| p[c]).collect();
        values.sort_unstable();
        *value = values[values.len() / 2];
    }
    out
}

fn enhance_all(session: &EditSession) -> Vec<Arc<EnhancedPage>> {
    session
        .pages()
        .iter()
        .map(|page| {
            session
                .enhance_page(page.id, default_enhancement_settings(), None)
                .unwrap()
        })
        .collect()
}

#[test]
fn redresse_le_jeu_de_test() {
    let mut failures = Vec::new();
    for &name in SCANS {
        let truth = truth(name);
        let session = EditSession::new(open(name));
        for (position, enhanced) in enhance_all(&session).iter().enumerate() {
            let expected = &truth["pages"][position];
            let angle = expected["inclinaison_deg"].as_f64().unwrap();
            let turns = (4 - expected["quart_de_tour"].as_u64().unwrap() as u8) % 4;
            let dpi = expected["ppp"].as_f64().unwrap() as f32;
            let report = enhanced.report();
            let label = format!("{name} p{}", position + 1);
            assert_eq!(report.quarter_turns, turns, "{label} : sens");

            if expected["a_verifier"].as_bool().unwrap() {
                // Au-delà du plafond : mesurée, mais laissée telle quelle.
                assert_eq!(report.status, StraighteningStatus::ToCheck, "{label}");
                assert!(!report.changes_pixels, "{label}");
                let measured = f64::from(report.measured_angle.unwrap());
                assert!((measured - angle).abs() < TOLERANCE, "{label} : {measured}");
                continue;
            }
            if angle.abs() < 0.1 {
                // Déjà droite : aucune correction, l'image n'est pas réécrite.
                assert!(
                    matches!(
                        report.status,
                        StraighteningStatus::Straight | StraighteningStatus::Unmeasurable
                    ),
                    "{label} : {:?}",
                    report.status
                );
                assert_eq!(report.corrected_angle, 0.0, "{label}");
                assert!(!report.changes_pixels, "{label}");
                continue;
            }
            assert_eq!(report.status, StraighteningStatus::Corrected, "{label}");
            assert!(report.changes_pixels, "{label}");
            let corrected = f64::from(report.corrected_angle);
            // Écart à la vérité, puis inclinaison mesurée sur la page
            // redressée, mise à l'endroit.
            let error = corrected - angle;
            let rendered = enhanced.render(dpi / 72.0, i32::from(turns) * 90).unwrap();
            let residual = skew(&rendered, dpi);
            println!("{label} : vrai {angle:+.2}°, corrigé {corrected:+.3}°, écart {error:+.3}°, résidu {residual:+.3}°");
            if error.abs() >= TOLERANCE || residual.abs() >= TOLERANCE {
                failures.push(format!(
                    "{label} : écart {error:+.3}°, résidu {residual:+.3}°"
                ));
            }

            // Coins découverts : couleur du papier voisin (marge), sans bord
            // noir ni blanc.
            let (w, h) = (rendered.width, rendered.height);
            let (near, far) = ((dpi * 0.1) as u32, (dpi * 0.25) as u32);
            for (x, y, dx, dy) in [
                (1, 1, 1, 1),
                (w - 2, 1, -1, 1),
                (w - 2, h - 2, -1, -1),
                (1, h - 2, 1, -1),
            ] {
                let color = pixel(&rendered, x, y);
                let margin = margin_color(&rendered, x, y, dx, dy, near, far);
                for c in 0..3 {
                    assert!(
                        (i32::from(color[c]) - i32::from(margin[c])).abs() <= 16,
                        "{label} : coin ({x}, {y}) {color:?}, marge {margin:?}"
                    );
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn applique_annule_exporte_et_rend_les_pages_d_origine() {
    let session = EditSession::new(open("11_orientation"));
    let originals = session.pages();
    let enhanced = enhance_all(&session);
    // Page 1 : retournée seulement (rotation de l'organisation, image
    // intacte) ; page 2 : couchée et inclinée (image redressée).
    assert!(!enhanced[0].report().changes_pixels);
    assert!(enhanced[1].report().changes_pixels);

    let document = session
        .enhanced_document(enhanced.clone())
        .unwrap()
        .expect("une page améliorée");
    assert_eq!(document.page_count(), 1);
    // Sans le document des pages améliorées, rien n'est appliqué.
    assert!(session.apply_enhancements(enhanced.clone(), None).is_err());
    assert_eq!(session.undo_name(), None);

    let source = session.add_source(document);
    session
        .apply_enhancements(enhanced.clone(), Some(source))
        .unwrap();
    assert_eq!(
        session.undo_name().as_deref(),
        Some("l'amélioration des pages")
    );
    let pages = session.pages();
    assert_eq!(pages[0].kind, originals[0].kind);
    assert_eq!(pages[0].quarter_turns, 2);
    assert_eq!(pages[1].kind, PageKind::Source { source, index: 0 });
    assert_eq!(pages[1].quarter_turns, 3);
    for (page, original) in pages.iter().zip(&originals) {
        let replaced = page.replaces.expect("page d'origine retenue");
        assert_eq!(replaced.kind, original.kind);
        assert_eq!(replaced.quarter_turns, 0);
    }

    // Le fichier exporté : deux pages, à l'endroit et droites.
    let bytes = session.export_pdf().unwrap().bytes;
    let original_size = std::fs::metadata(fixture("11_orientation.pdf"))
        .unwrap()
        .len();
    assert!(
        (bytes.len() as u64) < original_size * 2,
        "{} octets pour {original_size}",
        bytes.len()
    );
    let exported = PdfDocument::from_data(bytes, None).unwrap();
    assert_eq!(exported.page_count(), 2);
    for index in 0..2 {
        let rendered = exported.render_page(index, 200.0 / 72.0, 0).unwrap();
        assert!(rendered.height > rendered.width, "page {index} en portrait");
        let image = preprocess::flatten_background(&gray(&rendered), 200.0);
        assert!(!preprocess::is_sideways(&image, 200.0), "page {index}");
        let residual = skew(&rendered, 200.0);
        assert!(residual.abs() < TOLERANCE, "page {index} : {residual}");
    }

    // Annuler, rétablir.
    assert!(session.undo());
    let pages = session.pages();
    assert_eq!(pages[1].kind, originals[1].kind);
    assert!(pages.iter().all(|page| page.replaces.is_none()));
    assert!(session.redo());
    assert_eq!(
        session.pages()[1].kind,
        PageKind::Source { source, index: 0 }
    );

    // Retour aux pages d'origine, lui aussi annulable.
    let ids: Vec<u64> = pages.iter().map(|page| page.id).collect();
    session.restore_original_pages(ids).unwrap();
    assert_eq!(
        session.undo_name().as_deref(),
        Some("le retour aux pages d'origine")
    );
    let pages = session.pages();
    for (page, original) in pages.iter().zip(&originals) {
        assert_eq!(page.kind, original.kind);
        assert_eq!(page.quarter_turns, original.quarter_turns);
        assert!(page.replaces.is_none());
    }
}

#[test]
fn une_page_droite_n_est_pas_modifiee() {
    let session = EditSession::new(open("04_page_droite"));
    let enhanced = enhance_all(&session);
    assert!(session
        .enhanced_document(enhanced.clone())
        .unwrap()
        .is_none());
    session.apply_enhancements(enhanced, None).unwrap();
    // Aucune étape d'annulation : le document n'a pas changé.
    assert_eq!(session.undo_name(), None);
    assert!(session.pages()[0].replaces.is_none());
}

#[test]
fn reglages_desactives() {
    let mut settings = default_enhancement_settings();
    settings.deskew = false;
    settings.orientation = false;
    let session = EditSession::new(open("11_orientation"));
    for page in session.pages() {
        let report = session
            .enhance_page(page.id, settings.clone(), None)
            .unwrap()
            .report();
        assert_eq!(report.status, StraighteningStatus::Disabled);
        assert_eq!(report.quarter_turns, 0);
        assert!(!report.changes_pixels);
    }
    let json = pdfstudio_ffi::enhancement_settings_to_json(settings.clone());
    assert_eq!(
        pdfstudio_ffi::enhancement_settings_from_json(json),
        settings
    );
}

#[test]
fn une_page_native_n_est_pas_un_scan() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texte_simple.pdf");
    let session =
        EditSession::new(PdfDocument::open(path.to_string_lossy().into_owned(), None).unwrap());
    let report = session
        .enhance_page(session.pages()[0].id, default_enhancement_settings(), None)
        .unwrap()
        .report();
    assert_eq!(report.status, StraighteningStatus::NotScanned);
    assert!(!report.changes_pixels);
}
