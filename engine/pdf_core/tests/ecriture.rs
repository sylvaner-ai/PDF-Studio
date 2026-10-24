//! Tests d'écriture des documents réorganisés, sur `organisation.pdf` : cinq
//! pages « Feuille Alpha » à « Feuille Echo » avec plan, liens, note, champs de
//! formulaire et attributs hérités de l'arbre des pages.

use std::path::PathBuf;

use mupdf::pdf::{PdfDocument, PdfObject};
use pdf_core::{is_signed, write_pdf, Document, OutputPage, PdfError, SourceData};

const NAMES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];

fn source(name: &str, password: Option<&str>) -> SourceData {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name} : {e}"));
    SourceData::new(bytes, password.map(str::to_owned))
}

fn page(index: u32) -> OutputPage {
    OutputPage::Source {
        source: 0,
        index,
        quarter_turns: 0,
    }
}

fn pages(indices: &[u32]) -> Vec<OutputPage> {
    indices.iter().copied().map(page).collect()
}

fn write(pages: &[OutputPage]) -> Vec<u8> {
    write_pdf(&[source("organisation.pdf", None)], pages).unwrap()
}

/// Nom de chaque page d'après son texte, ce qui vérifie aussi que ses
/// ressources (polices) ont suivi.
fn names(bytes: &[u8], password: Option<&str>) -> Vec<&'static str> {
    let doc = Document::from_bytes(bytes, password).unwrap();
    (0..doc.page_count())
        .map(|index| {
            let text = doc.page_text(index).unwrap();
            NAMES
                .into_iter()
                .find(|name| !text.search(&format!("Feuille {name}")).is_empty())
                .unwrap_or("-")
        })
        .collect()
}

fn reopen(bytes: &[u8]) -> PdfDocument {
    PdfDocument::from_bytes(bytes).unwrap()
}

fn rotations(doc: &PdfDocument) -> Vec<i32> {
    (0..doc.page_count().unwrap())
        .map(|index| doc.load_pdf_page(index).unwrap().rotation().unwrap())
        .collect()
}

fn outline(bytes: &[u8]) -> Vec<(String, u32, Option<u32>)> {
    Document::from_bytes(bytes, None)
        .unwrap()
        .outline()
        .into_iter()
        .map(|entry| (entry.title, entry.level, entry.page))
        .collect()
}

/// Pages visées par les liens internes de la page `index`.
fn link_targets(doc: &PdfDocument, index: i32) -> Vec<u32> {
    doc.load_page(index)
        .unwrap()
        .links()
        .unwrap()
        .filter_map(|link| link.dest.map(|dest| dest.loc.page_number))
        .collect()
}

fn subtypes(doc: &PdfDocument, index: i32) -> Vec<String> {
    let Some(annots) = doc.find_page(index).unwrap().get_dict("Annots").unwrap() else {
        return Vec::new();
    };
    (0..annots.len().unwrap() as i32)
        .map(|i| {
            let annot = annots.get_array(i).unwrap().unwrap();
            let subtype = annot.get_dict("Subtype").unwrap().unwrap();
            String::from_utf8(subtype.as_name().unwrap().to_vec()).unwrap()
        })
        .collect()
}

fn catalog_entry(doc: &PdfDocument, key: &str) -> Option<PdfObject> {
    doc.catalog().unwrap().get_dict(key).unwrap()
}

fn field_names(doc: &PdfDocument) -> Vec<String> {
    let form = catalog_entry(doc, "AcroForm").expect("formulaire");
    let fields = form.get_dict("Fields").unwrap().unwrap();
    (0..fields.len().unwrap() as i32)
        .map(|i| {
            let field = fields.get_array(i).unwrap().unwrap();
            field
                .get_dict("T")
                .unwrap()
                .unwrap()
                .as_string()
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn s(value: &str) -> String {
    value.to_owned()
}

#[test]
fn la_rotation_seule_garde_toute_la_structure() {
    let mut order = pages(&[0, 1, 2, 3, 4]);
    for (position, turns) in [(0, 1), (3, 3)] {
        if let OutputPage::Source { quarter_turns, .. } = &mut order[position] {
            *quarter_turns = turns;
        }
    }
    let bytes = write(&order);
    let doc = reopen(&bytes);

    assert_eq!(names(&bytes, None), NAMES);
    // Delta et Echo héritaient d'une rotation de 90° : 90 + 270 = 360 pour Delta.
    assert_eq!(rotations(&doc), [90, 0, 0, 0, 90]);
    assert!(catalog_entry(&doc, "PageLabels").is_some());
    assert!(catalog_entry(&doc, "Metadata").is_some());
    assert!(catalog_entry(&doc, "ViewerPreferences").is_some());
    assert_eq!(field_names(&doc), ["nom", "ville"]);
    assert_eq!(
        Document::from_bytes(&bytes, None).unwrap().title(),
        Some("Organisation des pages")
    );
    assert_eq!(
        outline(&bytes),
        outline(source("organisation.pdf", None).bytes())
    );
}

#[test]
fn le_reordonnancement_garde_signets_liens_et_champs() {
    let bytes = write(&pages(&[4, 3, 2, 1, 0]));
    let doc = reopen(&bytes);

    assert_eq!(
        names(&bytes, None),
        ["Echo", "Delta", "Charlie", "Bravo", "Alpha"]
    );
    assert_eq!(rotations(&doc), [90, 90, 0, 0, 0]);
    assert_eq!(
        outline(&bytes),
        [
            (s("Alpha"), 0, Some(4)),
            (s("Bravo"), 0, Some(3)),
            (s("Charlie"), 0, Some(2)),
            (s("Annexes"), 0, None),
            (s("Delta"), 1, Some(1)),
            (s("Echo"), 1, Some(0)),
        ]
    );
    // Alpha menait à Charlie et à Echo, désormais pages 3 et 1.
    assert_eq!(link_targets(&doc, 4), [2, 0]);
    assert_eq!(subtypes(&doc, 3), ["Link", "Text", "Popup"]);
    assert_eq!(field_names(&doc), ["nom", "ville"]);
    // La numérotation logique ne suit pas un nouvel ordre ; le reste du catalogue, si.
    assert!(catalog_entry(&doc, "PageLabels").is_none());
    assert!(catalog_entry(&doc, "Metadata").is_some());
    assert!(catalog_entry(&doc, "ViewerPreferences").is_some());
}

#[test]
fn la_suppression_retire_les_references_aux_pages_supprimees() {
    // Sans Charlie : le lien d'Alpha vers Charlie disparaît, celui vers Echo reste.
    let bytes = write(&pages(&[0, 1, 3, 4]));
    let doc = reopen(&bytes);
    assert_eq!(names(&bytes, None), ["Alpha", "Bravo", "Delta", "Echo"]);
    assert_eq!(link_targets(&doc, 0), [3]);
    assert_eq!(subtypes(&doc, 0), ["Link", "Widget"]);
    assert!(!outline(&bytes).iter().any(|(title, ..)| title == "Charlie"));

    // Sans Alpha ni Delta : leurs signets et leurs champs disparaissent ; le
    // groupe « Annexes » reste pour Echo.
    let bytes = write(&pages(&[1, 2, 4]));
    let doc = reopen(&bytes);
    assert_eq!(names(&bytes, None), ["Bravo", "Charlie", "Echo"]);
    assert_eq!(
        outline(&bytes),
        [
            (s("Bravo"), 0, Some(0)),
            (s("Charlie"), 0, Some(1)),
            (s("Annexes"), 0, None),
            (s("Echo"), 1, Some(2)),
        ]
    );
    assert!(field_names(&doc).is_empty());
}

#[test]
fn la_duplication_copie_les_annotations_mais_pas_les_champs() {
    let bytes = write(&pages(&[0, 0, 2, 4, 1, 1]));
    let doc = reopen(&bytes);

    assert_eq!(
        names(&bytes, None),
        ["Alpha", "Alpha", "Charlie", "Echo", "Bravo", "Bravo"]
    );
    assert_eq!(subtypes(&doc, 0), ["Link", "Link", "Widget"]);
    assert_eq!(subtypes(&doc, 1), ["Link", "Link"]);
    assert_eq!(link_targets(&doc, 1), [2, 3]);
    // La copie de la note perd sa fenêtre, recréée au besoin par le lecteur.
    assert_eq!(subtypes(&doc, 5), ["Link", "Text"]);
    // Le champ « ville » était sur Delta, supprimée.
    assert_eq!(field_names(&doc), ["nom"]);
    assert_ne!(
        doc.find_page(0).unwrap().as_indirect().unwrap(),
        doc.find_page(1).unwrap().as_indirect().unwrap()
    );
}

#[test]
fn insere_des_pages_d_un_autre_document_et_une_page_vierge() {
    let sources = [
        source("organisation.pdf", None),
        source("organisation.pdf", None),
    ];
    let order = [
        page(2),
        OutputPage::Source {
            source: 1,
            index: 0,
            quarter_turns: 0,
        },
        OutputPage::Source {
            source: 1,
            index: 1,
            quarter_turns: 2,
        },
        OutputPage::Blank {
            width: 420.0,
            height: 595.0,
            quarter_turns: 1,
        },
    ];
    let bytes = write_pdf(&sources, &order).unwrap();
    let doc = reopen(&bytes);

    assert_eq!(names(&bytes, None), ["Charlie", "Alpha", "Bravo", "-"]);
    assert_eq!(rotations(&doc), [0, 0, 180, 90]);
    // Venues d'ailleurs, les pages perdent leurs liens internes et leurs champs,
    // mais gardent leurs liens Web et leurs notes.
    assert!(subtypes(&doc, 1).is_empty());
    assert_eq!(subtypes(&doc, 2), ["Link", "Text"]);
    let blank = doc
        .find_page(3)
        .unwrap()
        .get_dict("MediaBox")
        .unwrap()
        .unwrap();
    let width = blank.get_array(2).unwrap().unwrap().as_float().unwrap();
    let height = blank.get_array(3).unwrap().unwrap().as_float().unwrap();
    assert_eq!((width, height), (420.0, 595.0));
    // Seul le plan du document de base est repris.
    assert_eq!(outline(&bytes), [(s("Charlie"), 0, Some(0))]);
    assert!(field_names(&doc).is_empty());
}

/// Regroupe les pages sous deux nœuds intermédiaires (Alpha à Charlie, puis Delta
/// et Echo) qui portent leurs ressources, leur format et, pour le second, la
/// rotation ; les ressources du second sont un objet direct.
fn nested_tree(bytes: &[u8]) -> Vec<u8> {
    let mut doc = PdfDocument::from_bytes(bytes).unwrap();
    let mut root = catalog_entry(&doc, "Pages").unwrap();
    let pages: Vec<_> = (0..5).map(|index| doc.find_page(index).unwrap()).collect();
    let mut nodes = doc.new_array().unwrap();
    for (range, rotate) in [(0..3, None), (3..5, Some(90))] {
        let first = &pages[range.start];
        let mut node = doc.new_dict().unwrap();
        node.dict_put("Type", PdfObject::new_name("Pages").unwrap())
            .unwrap();
        node.dict_put("Parent", root.try_clone().unwrap()).unwrap();
        node.dict_put("Count", PdfObject::new_int(range.len() as i32).unwrap())
            .unwrap();
        let resources = first.get_dict("Resources").unwrap().unwrap();
        let resources = match rotate {
            Some(_) => resources.copy_dict().unwrap().try_clone().unwrap(),
            None => doc.add_object(&resources.copy_dict().unwrap()).unwrap(),
        };
        node.dict_put("Resources", resources).unwrap();
        let media_box = first.get_dict("MediaBox").unwrap().unwrap();
        node.dict_put("MediaBox", media_box.try_clone().unwrap())
            .unwrap();
        if let Some(rotate) = rotate {
            node.dict_put("Rotate", PdfObject::new_int(rotate).unwrap())
                .unwrap();
        }
        let node = doc.add_object(&node).unwrap();
        let mut kids = doc.new_array().unwrap();
        for page in &pages[range] {
            let mut page = page.try_clone().unwrap();
            for key in ["Resources", "MediaBox", "Rotate"] {
                page.dict_delete(key).unwrap();
            }
            page.dict_put("Parent", node.try_clone().unwrap()).unwrap();
            kids.array_push(page).unwrap();
        }
        let mut node_dict = node.try_clone().unwrap();
        node_dict.dict_put("Kids", kids).unwrap();
        nodes.array_push(node).unwrap();
    }
    root.dict_put("Kids", nodes).unwrap();
    let mut output = Vec::new();
    doc.write_to(&mut output).unwrap();
    output
}

#[test]
fn les_attributs_herites_suivent_les_pages() {
    let nested = nested_tree(source("organisation.pdf", None).bytes());
    let doc = reopen(&nested);
    assert!(doc
        .find_page(0)
        .unwrap()
        .get_dict("Resources")
        .unwrap()
        .is_none());
    assert_eq!(rotations(&doc), [0, 0, 0, 90, 90]);
    assert_eq!(names(&nested, None), NAMES);

    let base = [SourceData::new(nested, None)];
    let bytes = write_pdf(&base, &pages(&[4, 0, 3, 3, 2])).unwrap();
    let doc = reopen(&bytes);
    assert_eq!(
        names(&bytes, None),
        ["Echo", "Alpha", "Delta", "Delta", "Charlie"]
    );
    assert_eq!(rotations(&doc), [90, 0, 90, 90, 0]);
    for index in 0..5 {
        let page = doc.find_page(index).unwrap();
        assert!(page.get_dict("MediaBox").unwrap().is_some());
        assert!(page.get_dict("Resources").unwrap().is_some());
    }
}

#[test]
fn un_document_chiffre_reste_chiffre() {
    let base = source("chiffre.pdf", Some("motdepasse"));
    let rotated = OutputPage::Source {
        source: 0,
        index: 0,
        quarter_turns: 1,
    };
    let blank = OutputPage::Blank {
        width: 595.0,
        height: 842.0,
        quarter_turns: 0,
    };
    for order in [vec![rotated], vec![rotated, blank]] {
        let bytes = write_pdf(std::slice::from_ref(&base), &order).unwrap();
        assert!(matches!(
            Document::from_bytes(&bytes, None),
            Err(PdfError::PasswordRequired)
        ));
        let doc = Document::from_bytes(&bytes, Some("motdepasse")).unwrap();
        assert_eq!(doc.page_count() as usize, order.len());
        assert!(!doc
            .page_text(0)
            .unwrap()
            .search("Titre en bleu profond")
            .is_empty());
    }
}

#[test]
fn refuse_une_liste_de_pages_impossible() {
    let base = [source("organisation.pdf", None)];
    assert!(matches!(
        write_pdf(&base, &[]),
        Err(PdfError::WriteFailed { .. })
    ));
    assert!(matches!(
        write_pdf(&base, &pages(&[0, 9])),
        Err(PdfError::WriteFailed { .. })
    ));
    let unknown = OutputPage::Source {
        source: 3,
        index: 0,
        quarter_turns: 0,
    };
    assert!(matches!(
        write_pdf(&base, &[page(1), unknown]),
        Err(PdfError::WriteFailed { .. })
    ));
    assert!(matches!(
        write_pdf(&[], &pages(&[0])),
        Err(PdfError::WriteFailed { .. })
    ));
}

#[test]
fn detecte_les_documents_signes() {
    assert!(is_signed(&source("signature_factice.pdf", None)).unwrap());
    assert!(!is_signed(&source("organisation.pdf", None)).unwrap());
    assert!(!is_signed(&source("chiffre.pdf", Some("motdepasse"))).unwrap());
}

fn replaced(index: u32, content_index: u32) -> OutputPage {
    OutputPage::Replaced {
        source: 0,
        index,
        content_source: 1,
        content_index,
        quarter_turns: 0,
    }
}

#[test]
fn une_page_remplacee_garde_signets_liens_et_champs() {
    // Contenu d'Alpha et de Delta remplacé (comme par une page améliorée),
    // dans l'ordre d'origine.
    let sources = [
        source("organisation.pdf", None),
        source("texte_simple.pdf", None),
    ];
    let order = [replaced(0, 0), page(1), page(2), replaced(3, 0), page(4)];
    let bytes = write_pdf(&sources, &order).unwrap();
    let doc = reopen(&bytes);

    assert_eq!(names(&bytes, None), ["-", "Bravo", "Charlie", "-", "Echo"]);
    let replacement = Document::from_bytes(sources[1].bytes(), None)
        .unwrap()
        .plain_text(0)
        .unwrap();
    let text = Document::from_bytes(&bytes, None)
        .unwrap()
        .plain_text(0)
        .unwrap();
    assert_eq!(text, replacement);
    // Delta héritait d'une rotation de 90° : la page remplacée prend celle de
    // son nouveau contenu.
    assert_eq!(rotations(&doc), [0, 0, 0, 0, 90]);
    assert_eq!(
        outline(&bytes),
        outline(source("organisation.pdf", None).bytes())
    );
    assert_eq!(link_targets(&doc, 0), [2, 4]);
    assert_eq!(field_names(&doc), ["nom", "ville"]);
    assert!(catalog_entry(&doc, "PageLabels").is_some());

    // Dans un nouvel ordre, avec la page d'origine répétée : la copie garde
    // son contenu d'origine.
    let order = [page(4), replaced(0, 0), page(0)];
    let bytes = write_pdf(&sources, &order).unwrap();
    let doc = reopen(&bytes);
    assert_eq!(names(&bytes, None), ["Echo", "-", "Alpha"]);
    assert_eq!(rotations(&doc), [90, 0, 0]);
    assert!(outline(&bytes).contains(&(s("Alpha"), 0, Some(1))));
    assert_eq!(link_targets(&doc, 1), [0]);
    assert_eq!(
        field_names(&doc),
        field_names(&reopen(&write(&pages(&[4, 0, 0]))))
    );
}
