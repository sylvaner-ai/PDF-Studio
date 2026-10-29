//! Texte logé dans des formulaires PDF (objets externes `/Form`), comme dans
//! les PDF exportés ou imprimés par macOS, sur `tests/fixtures/formulaire.pdf`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use edit::{FontChoice, ObjectEdit, PageEdits, TextReplacement};
use pdf_core::content::objects::ObjectKind;
use pdf_core::{write_edited_pdf, Document, ObjectInfo, OutputPage, SourceData};

const TITRE: &str = "Devis fictif n° 0042";
const IMBRIQUE: &str = "Bloc dans un formulaire imbriqué";
const PIED: &str = "Pied de page hors formulaire";

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/formulaire.pdf"))
        .unwrap()
}

fn objects(bytes: &[u8], page: u32) -> Vec<ObjectInfo> {
    Document::from_bytes(bytes, None)
        .unwrap()
        .page_objects(page, None, None)
        .unwrap()
}

fn block<'a>(objects: &'a [ObjectInfo], start: &str) -> &'a ObjectInfo {
    objects
        .iter()
        .find(|o| o.text.as_deref().is_some_and(|t| t.starts_with(start)))
        .unwrap_or_else(|| panic!("bloc « {start} » introuvable"))
}

fn write(edits: Vec<(u32, ObjectEdit)>) -> Vec<u8> {
    let pages = [0, 1].map(|index| OutputPage::Source {
        source: 0,
        index,
        quarter_turns: 0,
    });
    let edits = Arc::new(PageEdits::new(
        edits.into_iter().collect::<BTreeMap<_, _>>(),
        1,
    ));
    write_edited_pdf(
        &[SourceData::new(fixture(), None)],
        &pages,
        &[Some(edits), None],
    )
    .unwrap()
}

#[test]
fn le_texte_des_formulaires_devient_des_blocs_modifiables() {
    let objects = objects(&fixture(), 0);
    assert!(objects.iter().all(|o| o.kind != ObjectKind::Form));
    let texts: Vec<&ObjectInfo> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Text)
        .collect();
    assert_eq!(texts.len(), 4);
    assert!(texts.iter().all(|o| o.editable && o.movable));
    // Chaque bloc garde sa police, bien que la page et les deux formulaires
    // nomment tous G1 une police différente.
    let font = |start: &str| block(&objects, start).font_name.clone();
    assert_eq!(font(TITRE).as_deref(), Some("Helvetica-Bold"));
    assert_eq!(font("Ce paragraphe").as_deref(), Some("Helvetica"));
    assert_eq!(font(IMBRIQUE).as_deref(), Some("Courier"));
    assert_eq!(font(PIED).as_deref(), Some("Times-Roman"));
    let paragraph = block(&objects, "Ce paragraphe");
    assert!(paragraph
        .text
        .as_deref()
        .unwrap()
        .ends_with("l'opérateur Do."));
    // Le formulaire imbriqué est placé par sa matrice : 56 + 4 points du
    // bord gauche.
    assert!((block(&objects, IMBRIQUE).bounds.x - 60.0).abs() < 0.5);
    // Le filet sous le titre est un objet à part.
    assert!(objects.iter().any(|o| o.kind == ObjectKind::Path));
}

#[test]
fn un_formulaire_sans_texte_reste_entier() {
    let objects = objects(&fixture(), 1);
    let kinds: Vec<ObjectKind> = objects.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, [ObjectKind::Text, ObjectKind::Form]);
}

#[test]
fn le_texte_d_un_formulaire_garde_sa_police_d_origine() {
    let mut document = Document::from_bytes(&fixture(), None).unwrap();
    let objects = document.page_objects(0, None, None).unwrap();
    let paragraph = block(&objects, "Ce paragraphe");
    assert_eq!(
        document
            .choose_font(0, paragraph.id, "Texte modifié, accentué : été")
            .unwrap(),
        FontChoice::Original
    );
}

#[test]
fn modifie_et_enregistre_le_texte_d_un_formulaire() {
    let original = objects(&fixture(), 0);
    let paragraph = block(&original, "Ce paragraphe").id;
    let nested = block(&original, IMBRIQUE).id;
    let title = block(&original, TITRE).id;
    let written = write(vec![
        (
            paragraph,
            ObjectEdit {
                text: Some(TextReplacement {
                    text: "Paragraphe remplacé dans le formulaire.".to_owned(),
                    font: FontChoice::Original,
                }),
                ..ObjectEdit::default()
            },
        ),
        (
            nested,
            ObjectEdit {
                deleted: true,
                ..ObjectEdit::default()
            },
        ),
        (
            title,
            ObjectEdit {
                dx: 100.0,
                ..ObjectEdit::default()
            },
        ),
    ]);

    let mut document = Document::from_bytes(&written, None).unwrap();
    let text = document.plain_text(0).unwrap();
    assert!(text.contains("Paragraphe remplacé dans le formulaire."));
    assert!(!text.contains("Ce paragraphe"));
    assert!(!text.contains(IMBRIQUE));
    assert!(text.contains(TITRE));
    assert!(text.contains(PIED));

    // Le fichier enregistré se relit avec les mêmes polices.
    let saved = document.page_objects(0, None, None).unwrap();
    let font = |start: &str| block(&saved, start).font_name.clone();
    assert_eq!(font("Paragraphe remplacé").as_deref(), Some("Helvetica"));
    assert_eq!(font(TITRE).as_deref(), Some("Helvetica-Bold"));
    assert_eq!(font(PIED).as_deref(), Some("Times-Roman"));
    let moved = block(&saved, TITRE).bounds.x - block(&original, TITRE).bounds.x;
    assert!((moved - 100.0).abs() < 0.5, "titre déplacé de {moved}");

    // La page sans modification est recopiée telle quelle.
    let second = document.page_objects(1, None, None).unwrap();
    assert_eq!(second.len(), 2);
}
