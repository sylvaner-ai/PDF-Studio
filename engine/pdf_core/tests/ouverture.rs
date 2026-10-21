//! Tests d'ouverture sur le jeu de PDF de référence (tests/fixtures).

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use pdf_core::{Document, DocumentHandle, PdfError, Source};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn open(name: &str) -> Document {
    Document::open_file(&fixture(name), None).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn ouvre_une_page_a4_et_lit_le_titre() {
    let doc = open("texte_simple.pdf");
    assert_eq!(doc.page_count(), 1);
    let size = doc.page_sizes()[0];
    assert!(
        approx(size.width, 595.28) && approx(size.height, 841.89),
        "{size:?}"
    );
    assert_eq!(doc.title(), Some("Texte simple"));
}

#[test]
fn compte_les_pages_des_gros_documents() {
    assert_eq!(open("multipage_100.pdf").page_count(), 100);
    assert_eq!(open("volumineux_600.pdf").page_count(), 600);
}

#[test]
fn respecte_formats_et_rotation() {
    let doc = open("formats_mixtes.pdf");
    let sizes = doc.page_sizes();
    assert_eq!(sizes.len(), 5);
    assert!(
        sizes[1].width > sizes[1].height,
        "A4 paysage : {:?}",
        sizes[1]
    );
    assert!(approx(sizes[2].width, 612.0) && approx(sizes[2].height, 792.0));
    // Page A4 portrait avec /Rotate 90 : affichée en paysage.
    assert!(
        approx(sizes[4].width, 841.89) && approx(sizes[4].height, 595.28),
        "{:?}",
        sizes[4]
    );
}

#[test]
fn ouvre_un_pdf_2_0() {
    assert_eq!(open("pdf20.pdf").page_count(), 1);
}

#[test]
fn demande_le_mot_de_passe() {
    let path = fixture("chiffre.pdf");
    assert!(matches!(
        Document::open_file(&path, None),
        Err(PdfError::PasswordRequired)
    ));
    assert!(matches!(
        Document::open_file(&path, Some("mauvais")),
        Err(PdfError::WrongPassword)
    ));
    let doc = Document::open_file(&path, Some("motdepasse")).expect("mot de passe correct");
    assert_eq!(doc.page_count(), 1);
}

#[test]
fn refuse_un_fichier_qui_n_est_pas_un_pdf() {
    let err = Document::open_file(&fixture("pas_un_pdf.pdf"), None).err();
    assert!(
        matches!(err, Some(PdfError::InvalidDocument { .. })),
        "{err:?}"
    );
}

#[test]
fn signale_un_fichier_absent_en_francais() {
    let err = Document::open_file(&fixture("absent.pdf"), None)
        .err()
        .expect("le fichier n'existe pas");
    assert!(matches!(err, PdfError::Unreadable { .. }));
    assert!(err.to_string().starts_with("Impossible de lire le fichier"));
}

#[test]
fn survit_a_un_fichier_tronque() {
    // MuPDF répare ce qu'il peut : soit des pages sont récupérées, soit une
    // erreur propre est renvoyée. Aucune panique n'est acceptable.
    match Document::open_file(&fixture("corrompu.pdf"), None) {
        Ok(doc) => {
            for index in 0..doc.page_count() {
                let _ = doc.page_content(index);
            }
        }
        Err(err) => assert!(matches!(err, PdfError::InvalidDocument { .. }), "{err:?}"),
    }
}

#[test]
fn refuse_une_page_inexistante() {
    let doc = open("texte_simple.pdf");
    let err = doc.page_content(3).err();
    assert!(matches!(
        err,
        Some(PdfError::PageOutOfRange { index: 3, count: 1 })
    ));
}

#[test]
fn ne_modifie_jamais_le_fichier_source() {
    let path = fixture("multipage_100.pdf");
    let before = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    {
        let doc = Document::open_file(&path, None).unwrap();
        doc.page_content(0).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn sert_les_pages_a_plusieurs_fils() {
    let handle =
        Arc::new(DocumentHandle::open(Source::File(fixture("multipage_100.pdf")), None).unwrap());
    assert_eq!(handle.page_count(), 100);
    let workers: Vec<_> = (0..4)
        .map(|t| {
            let handle = Arc::clone(&handle);
            thread::spawn(move || {
                for index in (t..100).step_by(4) {
                    let content = handle.page_content(index).unwrap();
                    assert!(content.size().width > 0.0);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn transmet_les_erreurs_d_ouverture_du_fil_dedie() {
    let bytes = std::fs::read(fixture("chiffre.pdf")).unwrap();
    let err = DocumentHandle::open(Source::Bytes(bytes), None).err();
    assert!(matches!(err, Some(PdfError::PasswordRequired)));
}
