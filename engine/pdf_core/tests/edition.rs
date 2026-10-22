//! Édition du contenu des pages : objets, choix de la police, aperçu et
//! enregistrement, sur `tests/fixtures/edition.pdf`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use edit::{FontChoice, ObjectEdit, PageEdits, TextReplacement};
use pdf_core::content::objects::ObjectKind;
use pdf_core::{
    write_edited_pdf, Document, DocumentHandle, ObjectInfo, OutputPage, Source, SourceData,
};

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/edition.pdf"))
        .unwrap()
}

fn objects(bytes: &[u8], page: u32) -> Vec<ObjectInfo> {
    Document::from_bytes(bytes, None)
        .unwrap()
        .page_objects(page, None)
        .unwrap()
}

fn text_of(objects: &[ObjectInfo]) -> Vec<&str> {
    objects.iter().filter_map(|o| o.text.as_deref()).collect()
}

fn edits(entries: Vec<(u32, ObjectEdit)>) -> Arc<PageEdits> {
    Arc::new(PageEdits::new(
        entries.into_iter().collect::<BTreeMap<_, _>>(),
        1,
    ))
}

fn replaced(text: &str, font: FontChoice) -> ObjectEdit {
    ObjectEdit {
        text: Some(TextReplacement {
            text: text.to_owned(),
            font,
        }),
        ..ObjectEdit::default()
    }
}

fn write(page_edits: Arc<PageEdits>) -> Vec<u8> {
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
    write_edited_pdf(
        &[SourceData::new(fixture(), None)],
        &pages,
        &[Some(page_edits), None],
    )
    .unwrap()
}

#[test]
fn reconnait_les_objets_de_la_page() {
    let objects = objects(&fixture(), 0);
    let kinds: Vec<ObjectKind> = objects.iter().map(|o| o.kind).collect();
    use ObjectKind::*;
    assert_eq!(kinds, [Text, Text, Text, Image, Path, Path]);
    assert_eq!(text_of(&objects)[0], "Rapport trimestriel");
    assert!(text_of(&objects)[1].starts_with("Le chiffre d'affaires du trimestre"));
    assert!(text_of(&objects)[1].ends_with("six mois d'activité."));
    assert_eq!(objects[2].font_name.as_deref(), Some("Helvetica"));
    assert!(objects.iter().all(|o| o.movable && !o.background));
    assert!(objects[..3].iter().all(|o| o.editable));
    // Espace d'affichage : origine en haut à gauche.
    assert!(objects[0].bounds.y < objects[1].bounds.y);
}

#[test]
fn enregistre_suppression_deplacement_et_texte() {
    let before = objects(&fixture(), 0);
    let bytes = write(edits(vec![
        (
            0,
            ObjectEdit {
                dx: 10.0,
                dy: -20.0,
                ..ObjectEdit::default()
            },
        ),
        (
            2,
            replaced("Nouveau texte, même police.", FontChoice::Original),
        ),
        (
            3,
            ObjectEdit {
                deleted: true,
                ..ObjectEdit::default()
            },
        ),
    ]));
    let after = objects(&bytes, 0);
    let kinds: Vec<ObjectKind> = after.iter().map(|o| o.kind).collect();
    assert!(!kinds.contains(&ObjectKind::Image));
    assert_eq!(kinds.iter().filter(|&&k| k == ObjectKind::Path).count(), 2);

    let title = after
        .iter()
        .find(|o| o.text.as_deref() == Some("Rapport trimestriel"))
        .unwrap();
    assert!((title.bounds.x - before[0].bounds.x - 10.0).abs() < 0.01);
    assert!((title.bounds.y - before[0].bounds.y - 20.0).abs() < 0.01);
    assert_eq!(title.font_name.as_deref(), Some("BitstreamVeraSans-Bold"));

    let line = after
        .iter()
        .find(|o| o.text.as_deref() == Some("Nouveau texte, même police."))
        .unwrap();
    assert_eq!(line.font_name.as_deref(), Some("Helvetica"));
    assert!((line.bounds.x - before[2].bounds.x).abs() < 0.01);
    // Le paragraphe, intact, garde son texte et sa place.
    let paragraph = after.iter().find(|o| o.text == before[1].text).unwrap();
    assert_eq!(paragraph.bounds, before[1].bounds);

    let text = Document::from_bytes(&bytes, None)
        .unwrap()
        .plain_text(0)
        .unwrap();
    assert!(text.contains("Nouveau texte, même police."));
    assert!(!text.contains("Helvetica, police standard"));
}

#[test]
fn choisit_la_police_du_texte_remplace() {
    let mut doc = Document::from_bytes(&fixture(), None).unwrap();
    // Helvetica (police standard) contient tous les caractères.
    assert_eq!(
        doc.choose_font(0, 2, "Été 2026 : 15 € HT").unwrap(),
        FontChoice::Original
    );
    // Le titre n'intègre que les glyphes de « Rapport trimestriel ».
    assert_eq!(
        doc.choose_font(0, 0, "Rapport mensuel").unwrap(),
        FontChoice::Original
    );
    assert_ne!(
        doc.choose_font(0, 0, "Bilan zéro").unwrap(),
        FontChoice::Original
    );
}

#[test]
fn remplace_le_texte_avec_une_police_de_substitution() {
    let mut doc = Document::from_bytes(&fixture(), None).unwrap();
    let choice = doc.choose_font(0, 0, "Bilan zéro").unwrap();
    let bytes = write(edits(vec![(0, replaced("Bilan zéro", choice))]));
    let after = objects(&bytes, 0);
    let title = after
        .iter()
        .find(|o| o.text.as_deref() == Some("Bilan zéro"))
        .unwrap();
    assert_ne!(title.font_name.as_deref(), Some("BitstreamVeraSans-Bold"));
    let text = Document::from_bytes(&bytes, None)
        .unwrap()
        .plain_text(0)
        .unwrap();
    assert!(text.contains("Bilan zéro"));
}

#[test]
fn paragraphe_remplace_sur_plusieurs_lignes() {
    let new = "La croissance se poursuit au deuxième trimestre. Les contrats signés au printemps \
               assurent une visibilité de dix-huit mois sur le carnet de commandes.";
    let page_edits = edits(vec![(1, replaced(new, FontChoice::Original))]);
    let mut doc = Document::from_bytes(&fixture(), None).unwrap();
    let infos = doc.page_objects(0, Some(&page_edits)).unwrap();
    let paragraph = infos.iter().find(|o| o.id == 1).unwrap();
    assert_eq!(paragraph.text.as_deref(), Some(new));
    // Les glyphes manquent dans le sous-ensemble : le moteur le signale par
    // le choix de la police, pas ici ; la mise en page reste dans le bloc.
    let before = objects(&fixture(), 0);
    assert!((paragraph.bounds.x - before[1].bounds.x).abs() < 0.5);
}

#[test]
fn apercu_d_une_page_modifiee() {
    let handle = DocumentHandle::open(Source::Bytes(fixture()), None).unwrap();
    let original = handle.page_content(0).unwrap();
    let page_edits = edits(vec![(
        3,
        ObjectEdit {
            deleted: true,
            ..ObjectEdit::default()
        },
    )]);
    let edited = handle
        .edited_page_content(0, Arc::clone(&page_edits))
        .unwrap();
    let pixel = |content: &pdf_core::PageContent| {
        let pixmap = content
            .display_list()
            .to_pixmap(
                &mupdf::Matrix::IDENTITY,
                &mupdf::Colorspace::device_rgb(),
                false,
            )
            .unwrap();
        // Centre de l'image : (56.7 + 56.7, 321.3 + 37.8).
        let (x, y) = (113usize, 359usize);
        let n = pixmap.n() as usize;
        let stride = pixmap.stride() as usize;
        pixmap.samples()[y * stride + x * n..y * stride + x * n + 3].to_vec()
    };
    assert_ne!(pixel(&original), [255, 255, 255]);
    assert_eq!(pixel(&edited), [255, 255, 255]);
    // L'original reste intact pour la page non modifiée.
    assert_ne!(pixel(&handle.page_content(0).unwrap()), [255, 255, 255]);
    let objects = handle.page_objects(0, Some(page_edits)).unwrap();
    assert!(objects.iter().all(|o| o.kind != ObjectKind::Image));
}

#[test]
fn deplacement_sur_une_page_tournee() {
    let handle = DocumentHandle::open(Source::Bytes(fixture()), None).unwrap();
    // Page tournée de 90° : un déplacement vers la droite à l'écran est un
    // déplacement vers le haut de la page PDF.
    let (dx, dy) = handle.page_offset(1, 10.0, 0.0).unwrap();
    assert!(dx.abs() < 1e-4 && (dy - 10.0).abs() < 1e-4, "{dx} {dy}");
    let (dx, dy) = handle.page_offset(0, 10.0, 5.0).unwrap();
    assert!((dx - 10.0).abs() < 1e-4 && (dy + 5.0).abs() < 1e-4);
}

/// Police CFF intégrée sans `/Encoding` : ses codes suivent l'encodage de son
/// programme, où 0xE9 est « é » (et non « Ø » comme en StandardEncoding).
#[test]
fn lit_l_encodage_propre_d_une_police_cff() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/police_cff.pdf"),
    )
    .unwrap();
    let found = objects(&bytes, 0);
    let texts = text_of(&found);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Montant prélevé : 42,50 €")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t.contains("Échéance réglée")));
    assert!(found.iter().all(|o| o.editable));
    let line = found
        .iter()
        .find(|o| o.text.as_deref().is_some_and(|t| t.contains("Montant")))
        .unwrap();

    // Les accents retapés restent dans la police d'origine.
    let new = "Montant prélevé : 25,50 €";
    let mut doc = Document::from_bytes(&bytes, None).unwrap();
    assert_eq!(
        doc.choose_font(0, line.id, new).unwrap(),
        FontChoice::Original
    );
    let written = write_edited_pdf(
        &[SourceData::new(bytes.clone(), None)],
        &[OutputPage::Source {
            source: 0,
            index: 0,
            quarter_turns: 0,
        }],
        &[Some(edits(vec![(
            line.id,
            replaced(new, FontChoice::Original),
        )]))],
    )
    .unwrap();
    let after = objects(&written, 0);
    let replaced_line = after
        .iter()
        .find(|o| o.text.as_deref().is_some_and(|t| t.contains("25,50")))
        .unwrap();
    assert!(!replaced_line.substituted);
    assert_eq!(replaced_line.font_name.as_deref(), Some("VeraCFF"));
    let text = Document::from_bytes(&written, None)
        .unwrap()
        .plain_text(0)
        .unwrap();
    assert!(text.contains(new), "{text}");
}

/// Encadré d'étiquettes et de valeurs (en-tête de facture) : les étiquettes
/// forment un bloc, les valeurs, d'un autre style, un second bloc, même quand
/// une étiquette et sa valeur se suivent dans la même ligne de texte ; modifier
/// les valeurs laisse chaque ligne à sa place.
#[test]
fn encadre_etiquettes_et_valeurs() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/encadre.pdf"),
    )
    .unwrap();
    let found = objects(&bytes, 0);
    let block = |objects: &[ObjectInfo], needle: &str| {
        objects
            .iter()
            .find(|o| o.text.as_deref().is_some_and(|t| t.contains(needle)))
            .cloned()
            .unwrap()
    };
    // Pied de page, étiquettes, valeurs : aucune ligne faite d'espaces.
    assert_eq!(found.len(), 3, "{:?}", text_of(&found));
    let labels = block(&found, "Date de facture");
    assert_eq!(
        labels.text.as_deref(),
        Some(
            "Date de facture\nN° de facture\nRemise à zéro\nFin d'engagement\n\
             N° de compte client\nN° de ligne\nRéférence"
        )
    );
    let values = block(&found, "02/04/2026");
    let old = "02/04/2026\n12345678901234\n01/01/2026\nSans engagement\n87654321\n\
               01 23 45 67 89\nXX-1234-5678";
    assert_eq!(values.text.as_deref(), Some(old));
    assert!(values.editable);
    assert_eq!(values.font_name.as_deref(), Some("Helvetica-Bold"));

    let new = old.replace("12345678901234", "98765432109876");
    let written = write_edited_pdf(
        &[SourceData::new(bytes.clone(), None)],
        &[OutputPage::Source {
            source: 0,
            index: 0,
            quarter_turns: 0,
        }],
        &[Some(edits(vec![(
            values.id,
            replaced(&new, FontChoice::Original),
        )]))],
    )
    .unwrap();
    let after = objects(&written, 0);
    let edited = block(&after, "98765432109876");
    assert_eq!(edited.text.as_deref(), Some(new.as_str()));
    // Mêmes lignes de base, au même bord gauche (127,4 pt), sans débordement.
    assert!(!edited.overflow);
    assert!(
        (edited.bounds.y - values.bounds.y).abs() < 1.0,
        "{edited:?}"
    );
    assert!((edited.bounds.height - values.bounds.height).abs() < 1.0);
    assert!((edited.bounds.x - 127.4).abs() < 0.5, "{edited:?}");
    let unchanged = block(&after, "Date de facture");
    assert_eq!(unchanged.text, labels.text);
    assert!((unchanged.bounds.x - labels.bounds.x).abs() < 0.01);
    assert!((unchanged.bounds.y - labels.bounds.y).abs() < 0.01);
}
