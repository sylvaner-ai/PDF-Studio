//! Robustesse du moteur face aux fichiers endommagés.
//!
//! Chaque PDF du jeu de test est abîmé de façon déterministe (fichier
//! tronqué, octets remplacés, bloc effacé), puis parcouru comme le ferait
//! l'application : ouverture, rendu, texte, recherche, plan, objets des pages,
//! modifications, couche de texte d'un scan, enregistrement et relecture.
//! Chaque opération peut échouer avec une erreur, mais aucune ne doit
//! paniquer, bloquer le document ni produire un PDF illisible.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pdf_core::PANIC_DETAIL;
use pdfstudio_ffi::{
    EditSession, EngineError, LayerWordItem, PageKind, PageObjectKind, PageRecognition, PdfDocument,
};

/// Documents abîmés : les PDF natifs, scannés, protégés et volumineux du jeu
/// de test (les très gros documents ne sont que tronqués).
const DOCUMENTS: &[&str] = &[
    "texte_simple.pdf",
    "edition.pdf",
    "tableau.pdf",
    "tableaux.pdf",
    "encadre.pdf",
    "signets.pdf",
    "organisation.pdf",
    "signature_factice.pdf",
    "formats_mixtes.pdf",
    "pdf20.pdf",
    "police_cff.pdf",
    "vectoriel.pdf",
    "scan_simule.pdf",
    "scan_retouche.pdf",
    "chiffre.pdf",
    "corrompu.pdf",
];

/// Temps accordé au parcours d'un document abîmé.
const TIMEOUT: Duration = Duration::from_secs(60);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Générateur pseudo-aléatoire déterministe (xorshift64*).
struct Random(u64);

impl Random {
    fn new(seed: &str) -> Self {
        // Graine tirée du nom : chaque document a ses propres défauts, les
        // mêmes à chaque exécution.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for byte in seed.bytes() {
            state = (state ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3);
        }
        Random(state | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }
}

/// Nombre de tirages de défauts par document : 1 par défaut, davantage pour
/// une recherche plus poussée (`PDFSTUDIO_ROBUSTESSE=20 cargo test …`).
fn rounds() -> usize {
    std::env::var("PDFSTUDIO_ROBUSTESSE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

/// Versions abîmées d'un document, nommées pour le diagnostic.
fn damaged(name: &str, round: usize, bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut random = Random::new(&format!("{name}#{round}"));
    let mut out = Vec::new();
    for percent in [5, 30, 60, 90, 99] {
        let cut = bytes.len() * percent / 100;
        out.push((format!("tronqué à {percent} %"), bytes[..cut].to_vec()));
    }
    for count in [1, 3, 10, 40, 150] {
        let mut copy = bytes.to_vec();
        for _ in 0..count {
            let at = random.below(copy.len());
            copy[at] = random.next() as u8;
        }
        out.push((format!("{count} octets remplacés"), copy));
    }
    // Chiffres changés : décalages de la table des objets, longueurs des flux,
    // numéros d'objets.
    let digits: Vec<usize> = (0..bytes.len())
        .filter(|&i| bytes[i].is_ascii_digit())
        .collect();
    if !digits.is_empty() {
        for count in [5, 50] {
            let mut copy = bytes.to_vec();
            for _ in 0..count {
                let at = digits[random.below(digits.len())];
                copy[at] = b'0' + random.below(10) as u8;
            }
            out.push((format!("{count} chiffres changés"), copy));
        }
    }
    for size in [64, 4096] {
        let mut copy = bytes.to_vec();
        let start = random.below(copy.len().saturating_sub(size));
        let end = (start + size).min(copy.len());
        copy[start..end].fill(0);
        out.push((format!("bloc de {size} octets effacé"), copy));
    }
    // Mots-clés de structure altérés.
    for (from, to) in [
        (&b"endobj"[..], &b"endobk"[..]),
        (b"stream", b"strean"),
        (b"/Length", b"/Lenght"),
        (b"/Kids", b"/Kidz"),
        (b"/Contents", b"/Contentz"),
    ] {
        let mut copy = bytes.to_vec();
        let mut changed = false;
        let mut at = 0;
        while let Some(pos) = find(&copy[at..], from) {
            let pos = at + pos;
            if random.below(2) == 0 {
                copy[pos..pos + to.len()].copy_from_slice(to);
                changed = true;
            }
            at = pos + from.len();
        }
        if changed {
            out.push((
                format!("« {} » altéré", String::from_utf8_lossy(from)),
                copy,
            ));
        }
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Une erreur est acceptable, une panique du moteur ne l'est pas.
fn check<T>(what: &str, result: Result<T, EngineError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(EngineError::Failed { detail, .. }) if detail.starts_with(PANIC_DETAIL) => {
            panic!("{what} : {detail}")
        }
        Err(_) => None,
    }
}

/// Mots d'une couche de texte factice, pour qu'une page scannée montre ses
/// objets comme après une reconnaissance.
fn layer_words() -> Vec<LayerWordItem> {
    let word = |text: &str, x: f32, y: f32, line: u32| LayerWordItem {
        text: text.into(),
        quad: vec![x, y, x + 60.0, y, x + 60.0, y + 14.0, x, y + 14.0],
        confidence: 0.9,
        line,
    };
    vec![
        word("Facture", 55.0, 742.0, 0),
        word("n°", 120.0, 742.0, 0),
        word("Montant", 55.0, 707.0, 1),
        word("total", 120.0, 707.0, 1),
    ]
}

/// Parcourt un document comme l'application, abîmé ou non.
fn exercise(bytes: Vec<u8>) {
    let Some(document) = check("ouverture", PdfDocument::from_data(bytes, None)) else {
        return;
    };
    let count = document.page_count();
    assert_eq!(document.page_sizes().len(), count as usize);
    let _ = document.title();
    check("plan", document.outline());
    for index in 0..count.min(3) {
        check("rendu", document.render_page(index, 0.25, 0));
        check("rendu tourné", document.render_page(index, 0.2, 90));
        check(
            "tuile",
            document.render_region(index, 1.0, 0, 10, 10, 64, 64),
        );
        check("texte", document.plain_text(index));
        check("recherche", document.search_page(index, "e".into()));
    }

    let session = EditSession::new(Arc::clone(&document));
    let pages = session.pages();
    check("pages à reconnaître", session.pages_to_recognize());
    if let Some(first) = pages.first() {
        // Couche de texte factice : les pages scannées montrent leurs objets.
        session.apply_recognitions(vec![PageRecognition {
            page: first.id,
            words: layer_words(),
            quarter_turns: 0,
            engine: "test".into(),
            mean_confidence: 0.9,
            doubtful_words: 0,
        }]);
        check(
            "recherche dans la couche",
            session.search_page(first.id, "facture".into()),
        );
        check("mots reconnus", session.recognized_words(first.id));
    }
    for page in pages.iter().take(3) {
        let Some(objects) = check("objets", session.page_objects(page.id)) else {
            continue;
        };
        if let Some(text) = objects.iter().find(|o| o.editable && !o.scanned) {
            check(
                "texte remplacé",
                session.replace_text(page.id, text.id, "Essai : œuvre — 12,50 €".into()),
            );
        }
        if let Some(movable) = objects
            .iter()
            .find(|o| o.movable && o.kind != PageObjectKind::Table)
        {
            check(
                "déplacement",
                session.move_objects(page.id, vec![movable.id], 12.0, -7.0),
            );
        }
        if let Some(last) = objects.iter().rev().find(|o| o.movable) {
            check(
                "suppression",
                session.delete_objects(page.id, vec![last.id]),
            );
        }
        check("objets modifiés", session.page_objects(page.id));
        check("récupération", session.content_record(page.id));
        if let (Some(edits), PageKind::Source { source: 0, index }) =
            (session.content_edits(page.id), page.kind)
        {
            check(
                "rendu modifié",
                document.render_edited_page(index, edits, 0.25, 0),
            );
        }
    }
    if count > 1 {
        check("rotation", session.rotate(vec![0], 1));
        check("déplacement de page", session.move_pages(vec![0], count));
    }
    if let Some(exported) = check("enregistrement", session.export_pdf()) {
        // Un PDF produit a été relu par le moteur : il doit se rouvrir.
        let reopened = PdfDocument::from_data(exported.bytes, None)
            .unwrap_or_else(|err| panic!("PDF enregistré illisible : {err}"));
        assert_eq!(reopened.page_count(), session.pages().len() as u32);
        check(
            "rendu après enregistrement",
            reopened.render_page(0, 0.25, 0),
        );
    }
    while session.undo() {}
    assert!(!session.is_modified());
}

/// Parcourt un document dans un fil à part, pour borner sa durée.
fn exercise_bounded(label: String, bytes: Vec<u8>) {
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::Builder::new()
        .name(label.clone())
        .spawn(move || {
            exercise(bytes);
            let _ = done_tx.send(());
        })
        .unwrap();
    match done_rx.recv_timeout(TIMEOUT) {
        Ok(()) => worker.join().unwrap(),
        // Le fil a paniqué : son message est remonté par `join`.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default();
                panic!("{label} : {message}");
            }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{label} : bloqué plus de {TIMEOUT:?}"),
    }
}

#[test]
fn les_documents_intacts_se_parcourent() {
    for name in DOCUMENTS {
        let bytes = std::fs::read(fixture(name)).unwrap();
        exercise_bounded((*name).to_owned(), bytes);
    }
}

#[test]
fn les_documents_abimes_ne_font_pas_planter_le_moteur() {
    let mut cases = 0;
    for name in DOCUMENTS {
        let bytes = std::fs::read(fixture(name)).unwrap();
        for round in 0..rounds() {
            for (damage, copy) in damaged(name, round, &bytes) {
                exercise_bounded(format!("{name}, tirage {round}, {damage}"), copy);
                cases += 1;
            }
        }
    }
    assert!(cases > 200, "{cases} cas seulement");
}

#[test]
fn les_gros_documents_tronques_s_ouvrent_ou_sont_refuses() {
    for name in ["multipage_100.pdf", "volumineux_600.pdf"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        for percent in [10, 50, 95] {
            let cut = bytes[..bytes.len() * percent / 100].to_vec();
            exercise_bounded(format!("{name}, tronqué à {percent} %"), cut);
        }
    }
}
