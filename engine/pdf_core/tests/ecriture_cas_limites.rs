//! Écriture des documents réorganisés : cas limites, sur des variantes de
//! `organisation.pdf` (cinq pages « Feuille Alpha » à « Feuille Echo » ; Delta et
//! Echo pivotées de 90° ; champ « nom » sur Alpha, « ville » sur Delta).

use std::path::PathBuf;

use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Buffer;
use pdf_core::{write_pdf, Document, OutputPage, SourceData};

const NAMES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn turned(index: u32, quarter_turns: u8) -> OutputPage {
    OutputPage::Source {
        source: 0,
        index,
        quarter_turns,
    }
}

fn pages(indices: &[u32]) -> Vec<OutputPage> {
    indices.iter().map(|&index| turned(index, 0)).collect()
}

/// Modifie `organisation.pdf` en mémoire et renvoie le fichier obtenu.
fn variant(edit: impl FnOnce(&mut PdfDocument)) -> Vec<u8> {
    let mut doc = PdfDocument::from_bytes(&fixture("organisation.pdf")).unwrap();
    edit(&mut doc);
    let mut output = Vec::new();
    doc.write_to(&mut output).unwrap();
    output
}

fn write(base: Vec<u8>, pages: &[OutputPage]) -> Vec<u8> {
    write_pdf(&[SourceData::new(base, None)], pages).unwrap()
}

fn names(bytes: &[u8]) -> Vec<&'static str> {
    let doc = Document::from_bytes(bytes, None).unwrap();
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

fn rotations(bytes: &[u8]) -> Vec<i32> {
    let doc = PdfDocument::from_bytes(bytes).unwrap();
    (0..doc.page_count().unwrap())
        .map(|index| doc.load_pdf_page(index).unwrap().rotation().unwrap())
        .collect()
}

/// Nombre d'objets du fichier, accessibles ou non, qui vérifient `test`.
fn count_objects(bytes: &[u8], test: impl Fn(&PdfObject) -> bool) -> usize {
    let doc = PdfDocument::from_bytes(bytes).unwrap();
    (1..doc.count_objects().unwrap() as i32)
        .filter(|&number| {
            let object = doc.new_indirect(number, 0).unwrap();
            object.is_dict().unwrap_or(false) && test(&object)
        })
        .count()
}

fn name_of(object: &PdfObject, key: &str) -> Option<String> {
    let value = object.get_dict(key).ok().flatten()?;
    value
        .is_name()
        .ok()?
        .then(|| String::from_utf8_lossy(&value.as_name().unwrap()).into_owned())
}

fn page_objects(bytes: &[u8]) -> usize {
    count_objects(bytes, |object| {
        name_of(object, "Type").as_deref() == Some("Page")
    })
}

fn field_names(bytes: &[u8]) -> Vec<String> {
    let doc = PdfDocument::from_bytes(bytes).unwrap();
    let Some(form) = doc.catalog().unwrap().get_dict("AcroForm").unwrap() else {
        return Vec::new();
    };
    let fields = form.get_dict("Fields").unwrap().unwrap();
    (0..fields.len().unwrap() as i32)
        .map(|i| {
            let field = fields.get_array(i).unwrap().unwrap();
            field.get_dict("T").unwrap().unwrap().as_string().unwrap()
        })
        .collect()
}

fn pages_of(doc: &PdfDocument) -> Vec<PdfObject> {
    (0..5).map(|index| doc.find_page(index).unwrap()).collect()
}

fn array(doc: &PdfDocument, items: Vec<PdfObject>) -> PdfObject {
    let mut array = doc.new_array().unwrap();
    for item in items {
        array.array_push(item).unwrap();
    }
    array
}

fn name(value: &str) -> PdfObject {
    PdfObject::new_name(value).unwrap()
}

#[test]
fn une_page_supprimee_ne_reste_pas_dans_le_fichier() {
    // Destinations nommées, structure balisée, action d'ouverture et ordre de
    // calcul des champs renvoient chacun à toutes les pages.
    let base = variant(|doc| {
        let pages = pages_of(doc);
        let mut catalog = doc.catalog().unwrap();
        let mut names = doc.new_array().unwrap();
        let mut elements = doc.new_array().unwrap();
        for (index, page) in pages.iter().enumerate() {
            names
                .array_push(PdfObject::new_string(&format!("page.{index}")).unwrap())
                .unwrap();
            names
                .array_push(array(doc, vec![page.try_clone().unwrap(), name("Fit")]))
                .unwrap();
            let mut element = doc.new_dict().unwrap();
            element.dict_put("S", name("P")).unwrap();
            element.dict_put("Pg", page.try_clone().unwrap()).unwrap();
            elements
                .array_push(doc.add_object(&element).unwrap())
                .unwrap();
        }
        let mut dests = doc.new_dict().unwrap();
        dests.dict_put("Names", names).unwrap();
        let mut tree = doc.new_dict().unwrap();
        tree.dict_put("Dests", dests).unwrap();
        catalog.dict_put("Names", tree).unwrap();
        let mut root = doc.new_dict().unwrap();
        root.dict_put("Type", name("StructTreeRoot")).unwrap();
        root.dict_put("K", elements).unwrap();
        catalog
            .dict_put("StructTreeRoot", doc.add_object(&root).unwrap())
            .unwrap();
        let open = array(doc, vec![pages[4].try_clone().unwrap(), name("Fit")]);
        catalog.dict_put("OpenAction", open).unwrap();
        let mut form = catalog.get_dict("AcroForm").unwrap().unwrap();
        let fields = form.get_dict("Fields").unwrap().unwrap();
        let order = (0..2)
            .map(|i| fields.get_array(i).unwrap().unwrap())
            .collect();
        form.dict_put("CO", array(doc, order)).unwrap();
    });
    assert_eq!(page_objects(&base), 5);

    // L'extraction d'Alpha seule.
    let bytes = write(base.clone(), &pages(&[0]));
    assert_eq!(names(&bytes), ["Alpha"]);
    assert_eq!(page_objects(&bytes), 1);
    // Le champ « ville » de Delta, et sa valeur, ont disparu.
    let ville = |object: &PdfObject| {
        let title = object.get_dict("T").unwrap();
        title.is_some_and(|title| title.as_string().unwrap() == "ville")
    };
    assert_eq!(count_objects(&base, ville), 1);
    assert_eq!(count_objects(&bytes, ville), 0);
    assert_eq!(field_names(&bytes), ["nom"]);
    let doc = PdfDocument::from_bytes(&bytes).unwrap();
    let catalog = doc.catalog().unwrap();
    assert!(catalog.get_dict("OpenAction").unwrap().is_none());
    assert!(catalog.get_dict("StructTreeRoot").unwrap().is_none());

    // Supprimer Bravo et Charlie ; l'ouverture sur Echo reste.
    let bytes = write(base.clone(), &pages(&[0, 3, 4]));
    assert_eq!(page_objects(&bytes), 3);
    assert!(PdfDocument::from_bytes(&bytes)
        .unwrap()
        .catalog()
        .unwrap()
        .get_dict("OpenAction")
        .unwrap()
        .is_some());

    // Sans suppression, la structure balisée est gardée.
    let bytes = write(base, &pages(&[4, 3, 2, 1, 0]));
    let doc = PdfDocument::from_bytes(&bytes).unwrap();
    assert!(doc
        .catalog()
        .unwrap()
        .get_dict("StructTreeRoot")
        .unwrap()
        .is_some());
}

#[test]
fn une_page_repetee_garde_sa_propre_rotation() {
    let base = fixture("organisation.pdf");
    let bytes = write(base.clone(), &[turned(0, 1), turned(0, 0), turned(1, 0)]);
    assert_eq!(names(&bytes), ["Alpha", "Alpha", "Bravo"]);
    assert_eq!(rotations(&bytes), [90, 0, 0]);
    let bytes = write(
        base,
        &[turned(0, 1), turned(0, 1), turned(4, 1), turned(4, 0)],
    );
    assert_eq!(rotations(&bytes), [90, 90, 180, 90]);
}

#[test]
fn une_page_presente_deux_fois_dans_l_arbre_est_dedoublee() {
    let base = variant(|doc| {
        let pages = pages_of(doc);
        let mut kids: Vec<PdfObject> = pages.iter().map(|page| page.try_clone().unwrap()).collect();
        kids.insert(1, pages[0].try_clone().unwrap());
        let mut tree = doc.catalog().unwrap().get_dict("Pages").unwrap().unwrap();
        tree.dict_put("Kids", array(doc, kids)).unwrap();
        tree.dict_put("Count", PdfObject::new_int(6).unwrap())
            .unwrap();
    });
    assert_eq!(
        names(&base),
        ["Alpha", "Alpha", "Bravo", "Charlie", "Delta", "Echo"]
    );

    // Seule la seconde occurrence pivote.
    let mut order = pages(&[0, 1, 2, 3, 4, 5]);
    order[1] = turned(1, 1);
    let bytes = write(base.clone(), &order);
    assert_eq!(rotations(&bytes), [0, 90, 0, 0, 90, 90]);

    let bytes = write(base, &[turned(2, 0), turned(1, 1), turned(0, 0)]);
    assert_eq!(names(&bytes), ["Bravo", "Alpha", "Alpha"]);
    assert_eq!(rotations(&bytes), [0, 90, 0]);
    let doc = PdfDocument::from_bytes(&bytes).unwrap();
    assert_ne!(
        doc.find_page(1).unwrap().as_indirect().unwrap(),
        doc.find_page(2).unwrap().as_indirect().unwrap()
    );
}

#[test]
fn les_pages_inserees_n_heritent_pas_de_la_racine() {
    let base = variant(|doc| {
        // Alpha hérite de la racine son cadrage et sa rotation.
        doc.find_page(0).unwrap().dict_delete("Rotate").unwrap();
        let mut tree = doc.catalog().unwrap().get_dict("Pages").unwrap().unwrap();
        let crop = (0..4)
            .map(|v| PdfObject::new_int([0, 0, 200, 200][v]).unwrap())
            .collect();
        tree.dict_put("CropBox", array(doc, crop)).unwrap();
        tree.dict_put("Rotate", PdfObject::new_int(180).unwrap())
            .unwrap();
    });
    let sources = [
        SourceData::new(base, None),
        SourceData::new(fixture("texte_simple.pdf"), None),
    ];
    let order = [
        turned(0, 0),
        OutputPage::Source {
            source: 1,
            index: 0,
            quarter_turns: 0,
        },
        OutputPage::Blank {
            width: 595.0,
            height: 842.0,
            quarter_turns: 0,
        },
    ];
    let bytes = write_pdf(&sources, &order).unwrap();
    // Alpha garde ce qu'elle héritait ; les autres pages, non.
    assert_eq!(rotations(&bytes), [180, 0, 0]);
    let doc = Document::from_bytes(&bytes, None).unwrap();
    let sizes = doc.page_sizes();
    assert_eq!((sizes[0].width, sizes[0].height), (200.0, 200.0));
    assert!(sizes[1].width > 500.0);
    assert_eq!((sizes[2].width, sizes[2].height), (595.0, 842.0));
}

#[test]
fn ignore_les_annotations_nulles_ou_disparues() {
    let base = variant(|doc| {
        let mut annots = doc
            .find_page(1)
            .unwrap()
            .get_dict("Annots")
            .unwrap()
            .unwrap();
        annots
            .array_push(doc.new_indirect(9999, 0).unwrap())
            .unwrap();
        annots.array_push(PdfObject::new_null()).unwrap();
    });
    let bytes = write(base.clone(), &pages(&[1, 1]));
    assert_eq!(names(&bytes), ["Bravo", "Bravo"]);

    let sources = [
        SourceData::new(fixture("texte_simple.pdf"), None),
        SourceData::new(base, None),
    ];
    let other = OutputPage::Source {
        source: 1,
        index: 1,
        quarter_turns: 0,
    };
    let bytes = write_pdf(&sources, &[turned(0, 0), other]).unwrap();
    assert_eq!(names(&bytes)[1], "Bravo");
}

#[test]
fn garde_les_champs_sans_annotation() {
    let base = variant(|doc| {
        let mut field = doc.new_dict().unwrap();
        field.dict_put("FT", name("Tx")).unwrap();
        field
            .dict_put("T", PdfObject::new_string("calcul").unwrap())
            .unwrap();
        field
            .dict_put("V", PdfObject::new_string("42").unwrap())
            .unwrap();
        let field = doc.add_object(&field).unwrap();
        let form = doc
            .catalog()
            .unwrap()
            .get_dict("AcroForm")
            .unwrap()
            .unwrap();
        form.get_dict("Fields")
            .unwrap()
            .unwrap()
            .array_push(field)
            .unwrap();
    });
    let bytes = write(base.clone(), &pages(&[1, 0, 2, 3, 4]));
    assert_eq!(field_names(&bytes), ["nom", "ville", "calcul"]);
    // Delta supprimée : son champ part, celui sans annotation reste.
    let bytes = write(base, &pages(&[0, 1, 2, 4]));
    assert_eq!(field_names(&bytes), ["nom", "calcul"]);
}

#[test]
fn accepte_un_arbre_au_compte_incoherent() {
    // Un nœud intermédiaire annonce deux pages mais en contient trois.
    let base = variant(|doc| {
        let pages = pages_of(doc);
        let mut tree = doc.catalog().unwrap().get_dict("Pages").unwrap().unwrap();
        let mut node = doc.new_dict().unwrap();
        node.dict_put("Type", name("Pages")).unwrap();
        node.dict_put("Parent", tree.try_clone().unwrap()).unwrap();
        node.dict_put("Count", PdfObject::new_int(2).unwrap())
            .unwrap();
        let node = doc.add_object(&node).unwrap();
        let mut kids = doc.new_array().unwrap();
        for page in &pages[..3] {
            let mut page = page.try_clone().unwrap();
            page.dict_put("Parent", node.try_clone().unwrap()).unwrap();
            kids.array_push(page).unwrap();
        }
        node.try_clone().unwrap().dict_put("Kids", kids).unwrap();
        let top = vec![
            node,
            pages[3].try_clone().unwrap(),
            pages[4].try_clone().unwrap(),
        ];
        tree.dict_put("Kids", array(doc, top)).unwrap();
    });
    let bytes = write(base, &pages(&[4, 3, 2, 1, 0]));
    assert_eq!(
        names(&bytes),
        ["Echo", "Delta", "Charlie", "Bravo", "Alpha"]
    );
}

#[test]
fn arrondit_une_rotation_qui_n_est_pas_un_multiple_de_90() {
    let base = variant(|doc| {
        doc.find_page(0)
            .unwrap()
            .dict_put("Rotate", PdfObject::new_int(45).unwrap())
            .unwrap();
    });
    // MuPDF affiche 45° comme 90° ; un quart de tour de plus donne 180°.
    let bytes = write(base.clone(), &[turned(0, 1), turned(1, 0)]);
    assert_eq!(rotations(&bytes), [180, 0]);
    let mut order = pages(&[0, 1, 2, 3, 4]);
    order[0] = turned(0, 3);
    assert_eq!(rotations(&write(base, &order))[0], 0);
}

#[test]
fn une_ressource_partagee_n_est_copiee_qu_une_fois() {
    const MARK: &str = "% ressource partagee par toutes les pages";
    let other = variant(|doc| {
        let mut dict = doc.new_dict().unwrap();
        dict.dict_put("Type", name("XObject")).unwrap();
        dict.dict_put("Subtype", name("Form")).unwrap();
        let bbox = (0..4).map(|_| PdfObject::new_int(0).unwrap()).collect();
        dict.dict_put("BBox", array(doc, bbox)).unwrap();
        let stream = doc
            .add_stream(
                &Buffer::from_bytes(MARK.as_bytes()).unwrap(),
                Some(&dict),
                false,
            )
            .unwrap();
        for page in pages_of(doc) {
            let resources = page.get_dict_inheritable("Resources").unwrap().unwrap();
            let mut xobjects = doc.new_dict().unwrap();
            xobjects
                .dict_put("Partage", stream.try_clone().unwrap())
                .unwrap();
            let mut copy = resources.copy_dict().unwrap().try_clone().unwrap();
            copy.dict_put("XObject", xobjects).unwrap();
            page.try_clone()
                .unwrap()
                .dict_put("Resources", copy)
                .unwrap();
        }
    });
    let sources = [
        SourceData::new(fixture("texte_simple.pdf"), None),
        SourceData::new(other, None),
    ];
    let order: Vec<_> = (0..5)
        .map(|index| OutputPage::Source {
            source: 1,
            index,
            quarter_turns: 0,
        })
        .collect();
    let bytes = write_pdf(&sources, &order).unwrap();
    assert_eq!(names(&bytes), NAMES);
    let copies = count_objects(&bytes, |object| {
        object.is_stream().unwrap() && object.read_stream().unwrap() == MARK.as_bytes()
    });
    assert_eq!(copies, 1);
}
