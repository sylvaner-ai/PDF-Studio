//! Objets redimensionnés, copiés, coupés, collés, dupliqués et déplacés d'une
//! page à une autre (D71, D72) : aperçu, enregistrement, annulation et
//! récupération.

use std::path::Path;
use std::sync::Arc;

use pdfstudio_ffi::{
    clipboard_from_file, pdf_clip_size, AddedObjectItem, AddedTextStyle, ClipContent, EditSession,
    PageObjectItem, PageObjectKind, PageRect, PastePlacement, PdfDocument, TextFont,
};

const LIGNE: &str = "Texte en Helvetica, police standard.";

fn open(name: &str) -> Arc<PdfDocument> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    PdfDocument::open(path.to_string_lossy().into_owned(), None).unwrap()
}

fn object(session: &EditSession, page: u64, id: u32) -> PageObjectItem {
    session
        .page_objects(page)
        .unwrap()
        .into_iter()
        .find(|item| item.id == id)
        .unwrap()
}

fn text_object(items: &[PageObjectItem], text: &str) -> Option<PageObjectItem> {
    items
        .iter()
        .find(|item| item.text.as_deref().is_some_and(|t| t.contains(text)))
        .cloned()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.6
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> PageRect {
    PageRect {
        x,
        y,
        width,
        height,
    }
}

/// Pixels d'une page rendue qui ne sont pas blancs.
fn inked(document: &PdfDocument, session: &EditSession, page: u64, index: u32) -> usize {
    let rendered = match session.content_edits(page) {
        Some(edits) => document.render_edited_page(index, edits, 0.5, 0).unwrap(),
        None => document.render_page(index, 0.5, 0).unwrap(),
    };
    rendered
        .pixels
        .chunks(4)
        .filter(|p| p[0] < 200 || p[1] < 200 || p[2] < 200)
        .count()
}

#[test]
fn redimensionne_les_objets_d_origine() {
    let document = open("edition.pdf");
    let session = EditSession::new(Arc::clone(&document));
    let page = session.pages()[0].id;
    let items = session.page_objects(page).unwrap();
    let image = items
        .iter()
        .find(|item| item.kind == PageObjectKind::Image)
        .unwrap()
        .clone();
    assert!(image.resizable && !image.added);
    let line = text_object(&items, LIGNE).unwrap();
    assert!(line.resizable);

    // L'image double de taille, son coin supérieur gauche en place.
    let b = image.bounds;
    let resized = session
        .set_object_bounds(
            page,
            image.id,
            rect(b.x, b.y, 2.0 * b.width, 2.0 * b.height),
        )
        .unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("le redimensionnement"));
    assert!(near(resized.bounds.x, b.x) && near(resized.bounds.y, b.y));
    assert!(near(resized.bounds.width, 2.0 * b.width));
    assert!(near(resized.bounds.height, 2.0 * b.height));
    // Déplacé ensuite, il garde sa nouvelle taille.
    session
        .move_objects(page, vec![image.id], 30.0, -10.0)
        .unwrap();
    let moved = object(&session, page, image.id);
    assert!(near(moved.bounds.x, b.x + 30.0) && near(moved.bounds.width, 2.0 * b.width));

    // Le texte grandit de moitié : son corps aussi.
    let l = line.bounds;
    let grown = session
        .set_object_bounds(page, line.id, rect(l.x, l.y, 1.5 * l.width, 1.5 * l.height))
        .unwrap();
    assert!(near(grown.bounds.width, 1.5 * l.width), "{grown:?}");
    assert!((grown.font_size - 1.5 * line.font_size).abs() < 0.05);
    assert!(inked(&document, &session, page, 0) > 0);

    // Enregistré : l'image et le texte agrandis se retrouvent.
    let exported = session.export_pdf().unwrap();
    let reopened = PdfDocument::from_data(exported.bytes, None).unwrap();
    let again = EditSession::new(Arc::clone(&reopened));
    let objects = again.page_objects(again.pages()[0].id).unwrap();
    let saved = objects
        .iter()
        .find(|item| item.kind == PageObjectKind::Image)
        .unwrap();
    assert!(near(saved.bounds.width, 2.0 * b.width), "{saved:?}");
    assert!(near(saved.bounds.x, b.x + 30.0));
    let text = text_object(&objects, LIGNE).unwrap();
    assert!(
        (text.font_size - 1.5 * line.font_size).abs() < 0.05,
        "{text:?}"
    );

    // Récupération : l'agrandissement est retenu.
    let record = session.content_record(page).unwrap();
    assert!(record
        .iter()
        .any(|item| item.object == image.id && (item.scale_x - 2.0).abs() < 1e-3));
    let restored = EditSession::new(open("edition.pdf"));
    let contents = session
        .pages()
        .iter()
        .map(|item| session.content_record(item.id).unwrap())
        .collect();
    restored
        .restore(session.pages(), contents, Vec::new())
        .unwrap();
    let back = object(&restored, restored.pages()[0].id, image.id);
    assert!(near(back.bounds.width, 2.0 * b.width));

    // Un tableau ne se redimensionne pas.
    let tables = EditSession::new(open("tableaux.pdf"));
    let tables_page = tables.pages()[0].id;
    let table = tables
        .page_objects(tables_page)
        .unwrap()
        .into_iter()
        .find(|item| item.kind == PageObjectKind::Table)
        .unwrap();
    assert!(!table.resizable);
    assert!(tables
        .set_object_bounds(tables_page, table.id, rect(0.0, 0.0, 50.0, 50.0))
        .is_err());

    // Tout s'annule.
    while session.undo() {}
    assert!(session.content_edits(page).is_none());
}

#[test]
fn copie_et_colle_sur_une_autre_page() {
    let document = open("edition.pdf");
    let session = EditSession::new(Arc::clone(&document));
    let pages = session.pages();
    let (first, second) = (pages[0].id, pages[1].id);
    let items = session.page_objects(first).unwrap();
    let line = text_object(&items, LIGNE).unwrap();
    let image = items
        .iter()
        .find(|item| item.kind == PageObjectKind::Image)
        .unwrap()
        .clone();

    let clipboard = session
        .copy_objects(first, vec![line.id, image.id])
        .unwrap();
    // Rien ne change dans le document.
    assert!(session.undo_name().is_none());
    assert_eq!(clipboard.pieces.len(), 1);
    assert!(
        matches!(&clipboard.pieces[0].content, ClipContent::Image { data } if data.starts_with(b"%PDF"))
    );
    assert!(clipboard.text.contains(LIGNE));
    assert!(!clipboard.pdf.is_empty());
    // La copie couvre les deux objets, à leur place.
    let left = line.bounds.x.min(image.bounds.x);
    assert!((clipboard.x - left).abs() < 1.5, "{} {left}", clipboard.x);
    let size = pdf_clip_size(clipboard.pdf.clone()).unwrap();
    assert!((size.width - clipboard.width).abs() < 1.5);

    // Collée sur la page tournée (page droite en paysage), à la même place
    // à l'écran.
    let pasted = session
        .paste_objects(
            second,
            clipboard.clone(),
            PastePlacement::InPlace { dx: 0.0, dy: 0.0 },
            None,
        )
        .unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("le collage"));
    assert_eq!(pasted.len(), 1);
    let piece = &pasted[0];
    assert!(piece.added && piece.resizable && piece.kind == PageObjectKind::Image);
    assert!(near(piece.bounds.width, clipboard.width), "{piece:?}");
    assert!(near(piece.bounds.x, clipboard.x) && near(piece.bounds.y, clipboard.y));
    assert!(inked(&document, &session, second, 1) > 0);

    // Enregistré, le texte copié se lit sur la seconde page, et redevient un
    // bloc de texte modifiable à la réouverture (formulaire déplié, D69).
    let exported = session.export_pdf().unwrap();
    let reopened = PdfDocument::from_data(exported.bytes, None).unwrap();
    assert!(reopened.plain_text(1).unwrap().contains(LIGNE));
    let again = EditSession::new(Arc::clone(&reopened));
    let objects = again.page_objects(again.pages()[1].id).unwrap();
    let copied = text_object(&objects, LIGNE).unwrap();
    assert!(copied.editable, "{copied:?}");
    assert!(objects
        .iter()
        .any(|item| item.kind == PageObjectKind::Image));

    // Récupération : la copie collée revient telle quelle.
    let record = session.content_record(second).unwrap();
    assert!(matches!(
        record.iter().find_map(|item| item.added.clone()),
        Some(AddedObjectItem::Image { data, .. }) if data.starts_with(b"%PDF")
    ));
    let restored = EditSession::new(open("edition.pdf"));
    let contents = session
        .pages()
        .iter()
        .map(|item| session.content_record(item.id).unwrap())
        .collect();
    restored
        .restore(session.pages(), contents, Vec::new())
        .unwrap();
    let back = restored
        .page_objects(restored.pages()[1].id)
        .unwrap()
        .into_iter()
        .find(|item| item.added)
        .unwrap();
    assert!(near(back.bounds.width, piece.bounds.width));

    // Dans un autre document.
    let other = EditSession::new(open("texte_simple.pdf"));
    let other_page = other.pages()[0].id;
    other
        .paste_objects(
            other_page,
            clipboard,
            PastePlacement::Centered { x: 200.0, y: 300.0 },
            None,
        )
        .unwrap();
    let exported = other.export_pdf().unwrap();
    let reopened = PdfDocument::from_data(exported.bytes, None).unwrap();
    assert!(reopened.plain_text(0).unwrap().contains(LIGNE));

    assert!(session.undo());
    assert!(session.content_edits(second).is_none());
}

#[test]
fn coupe_duplique_et_deplace() {
    let session = EditSession::new(open("edition.pdf"));
    let pages = session.pages();
    let (first, second) = (pages[0].id, pages[1].id);
    let items = session.page_objects(first).unwrap();
    let count = items.len();
    let image = items
        .iter()
        .find(|item| item.kind == PageObjectKind::Image)
        .unwrap()
        .clone();
    let line = text_object(&items, LIGNE).unwrap();

    // Dupliquée, l'image se pose un peu plus bas à droite.
    let copies = session.duplicate_objects(first, vec![image.id]).unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("la duplication"));
    assert_eq!(copies.len(), 1);
    assert!(near(copies[0].bounds.x, image.bounds.x + 12.0 - 1.0));
    assert!(near(copies[0].bounds.width, image.bounds.width + 2.0));
    assert_eq!(session.page_objects(first).unwrap().len(), count + 1);

    // Coupée, la ligne disparaît ; ⌘Z la rend.
    let clipboard = session.cut_objects(first, vec![line.id]).unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("la coupe"));
    assert!(text_object(&session.page_objects(first).unwrap(), LIGNE).is_none());
    assert!(clipboard.text.contains(LIGNE));
    assert!(session.undo());
    assert!(text_object(&session.page_objects(first).unwrap(), LIGNE).is_some());

    // Un texte ajouté passe sur l'autre page, centré sur le point visé, et
    // reste un texte modifiable ; la ligne d'origine l'accompagne.
    let style = AddedTextStyle {
        font: TextFont::Standard {
            name: "Helvetica".to_owned(),
        },
        size: 14.0,
        red: 0.0,
        green: 0.0,
        blue: 0.0,
    };
    let note = session
        .add_text(first, 300.0, 700.0, "Note".to_owned(), style.clone(), None)
        .unwrap();
    let moved = session
        .transfer_objects(
            first,
            vec![note.id, line.id],
            second,
            400.0,
            200.0,
            false,
            None,
        )
        .unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("le déplacement"));
    assert_eq!(moved.len(), 2);
    let text = moved.iter().find(|item| item.added_text.is_some()).unwrap();
    assert_eq!(text.text.as_deref(), Some("Note"));
    assert_eq!(text.added_text.as_ref().map(|s| s.size), Some(14.0));
    let left = moved.iter().map(|i| i.bounds.x).fold(f32::MAX, f32::min);
    let right = moved
        .iter()
        .map(|i| i.bounds.x + i.bounds.width)
        .fold(f32::MIN, f32::max);
    assert!(((left + right) / 2.0 - 400.0).abs() < 1.0, "{left} {right}");
    let remaining = session.page_objects(first).unwrap();
    assert!(text_object(&remaining, LIGNE).is_none());
    assert!(!remaining.iter().any(|item| item.id == note.id));
    // Une seule étape d'annulation rend les deux pages.
    assert!(session.undo());
    let remaining = session.page_objects(first).unwrap();
    assert!(text_object(&remaining, LIGNE).is_some());
    assert!(remaining.iter().any(|item| item.id == note.id));
    assert!(session.content_edits(second).is_none());

    // Copié (⌥ glisser) : l'original reste.
    session
        .transfer_objects(first, vec![line.id], second, 300.0, 300.0, true, None)
        .unwrap();
    assert_eq!(session.undo_name().as_deref(), Some("la copie"));
    assert!(text_object(&session.page_objects(first).unwrap(), LIGNE).is_some());

    // Un texte ajouté se redimensionne : son corps suit la hauteur.
    let b = note.bounds;
    let bigger = session
        .set_object_bounds(
            first,
            note.id,
            rect(b.x, b.y, 2.0 * b.width, 2.0 * b.height),
        )
        .unwrap();
    assert_eq!(bigger.added_text.map(|s| s.size), Some(28.0));
    assert!(near(bigger.bounds.x, b.x) && near(bigger.bounds.y, b.y));
}

#[test]
fn colle_depuis_une_autre_application() {
    // Un PDF collé (copie d'une autre application) se pose au point visé.
    let source = EditSession::new(open("edition.pdf"));
    let first = source.pages()[0].id;
    let line = text_object(&source.page_objects(first).unwrap(), LIGNE).unwrap();
    let pdf = source.copy_objects(first, vec![line.id]).unwrap().pdf;
    let size = pdf_clip_size(pdf.clone()).unwrap();
    let clipboard = clipboard_from_file(pdf, size.width, size.height).unwrap();
    assert!(clipboard_from_file(b"rien".to_vec(), 10.0, 10.0).is_err());

    let session = EditSession::new(open("texte_simple.pdf"));
    let page = session.pages()[0].id;
    let pasted = session
        .paste_objects(
            page,
            clipboard,
            PastePlacement::Centered { x: 150.0, y: 100.0 },
            None,
        )
        .unwrap();
    let b = pasted[0].bounds;
    assert!(
        (b.x + b.width / 2.0 - 150.0).abs() < 0.6 && (b.y + b.height / 2.0 - 100.0).abs() < 0.6
    );

    // Sur une page tournée d'un quart de tour par l'organisation, la copie
    // reste droite à l'écran : ses côtés s'échangent sur la page droite.
    session.rotate(vec![0], 1).unwrap();
    let pasted_again = session
        .paste_objects(
            page,
            clipboard_from_file(
                source.copy_objects(first, vec![line.id]).unwrap().pdf,
                size.width,
                size.height,
            )
            .unwrap(),
            PastePlacement::Centered { x: 150.0, y: 300.0 },
            None,
        )
        .unwrap();
    let turned = pasted_again[0].bounds;
    assert!(
        near(turned.width, b.height) && near(turned.height, b.width),
        "{turned:?} {b:?}"
    );

    // La copie d'un objet que l'écran montre tourné garde cet aspect (avec
    // la marge de l'extrait).
    let copied = session
        .paste_objects(
            page,
            session.copy_objects(page, vec![pasted[0].id]).unwrap(),
            PastePlacement::Centered { x: 150.0, y: 300.0 },
            None,
        )
        .unwrap();
    let same = copied[0].bounds;
    assert!(
        (same.width - b.width - 2.0).abs() < 0.6 && (same.height - b.height - 2.0).abs() < 0.6,
        "{same:?} {b:?}"
    );
}
