//! Tableaux : détection, puis lignes, colonnes, tableau entier et cellules
//! modifiés, sur `tests/fixtures/tableau.pdf` et `tests/fixtures/tableaux.pdf`.
//!
//! Chaque modification est enregistrée, puis le fichier écrit est relu : le
//! tableau qu'on y détecte de nouveau vérifie que ses filets ont été réécrits
//! avec ses lignes et ses colonnes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use edit::{FontChoice, ObjectEdit, PageEdits, TableEdit, TextReplacement};
use pdf_core::content::objects::ObjectKind;
use pdf_core::{write_edited_pdf, Document, ObjectInfo, OutputPage, SourceData};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn objects(bytes: &[u8], edits: Option<&PageEdits>) -> Vec<ObjectInfo> {
    Document::from_bytes(bytes, None)
        .unwrap()
        .page_objects(0, edits, None)
        .unwrap()
}

fn tables(objects: &[ObjectInfo]) -> Vec<&ObjectInfo> {
    objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .collect()
}

fn edits(entries: Vec<(u32, ObjectEdit)>) -> Arc<PageEdits> {
    Arc::new(PageEdits::new(entries.into_iter().collect(), 1))
}

fn table_edit(rows: Option<Vec<u32>>, columns: Option<Vec<u32>>) -> ObjectEdit {
    ObjectEdit {
        table: Some(TableEdit {
            rows,
            columns,
            cells: BTreeMap::new(),
        }),
        ..ObjectEdit::default()
    }
}

/// Fichier enregistré avec `page_edits` sur sa première page.
fn write(bytes: &[u8], page_edits: Arc<PageEdits>) -> Vec<u8> {
    let pages = [OutputPage::Source {
        source: 0,
        index: 0,
        quarter_turns: 0,
    }];
    write_edited_pdf(
        &[SourceData::new(bytes.to_vec(), None)],
        &pages,
        &[Some(page_edits)],
    )
    .unwrap()
}

fn text(bytes: &[u8]) -> String {
    Document::from_bytes(bytes, None)
        .unwrap()
        .plain_text(0)
        .unwrap()
}

/// Lignes et colonnes de la grille d'un tableau.
fn size(table: &ObjectInfo) -> (u32, u32) {
    let grid = table.grid.as_ref().unwrap();
    (grid.rows, grid.columns)
}

const INVOICE: u32 = 42;

#[test]
fn detecte_le_tableau_d_une_facture() {
    let objects = objects(&fixture("tableau.pdf"), None);
    let found = tables(&objects);
    assert_eq!(found.len(), 1);
    let table = found[0];
    assert_eq!(table.id, INVOICE);
    assert_eq!(size(table), (9, 4));
    let grid = table.grid.as_ref().unwrap();
    assert_eq!(grid.cells.len(), 36);
    assert!(grid.cells.iter().all(|cell| cell.objects.len() == 1));
    // Chaque bloc d'une cellule appartient au tableau, comme ses filets.
    let members: Vec<&ObjectInfo> = objects
        .iter()
        .filter(|o| o.table == Some(INVOICE))
        .collect();
    assert_eq!(
        members
            .iter()
            .filter(|o| o.kind == ObjectKind::Text)
            .count(),
        36
    );
    assert!(members.iter().any(|o| o.kind == ObjectKind::Path));
    // Le total, sous la grille, n'en fait pas partie.
    let total = objects
        .iter()
        .find(|o| o.text.as_deref() == Some("Total HT"))
        .unwrap();
    assert_eq!(total.table, None);
}

#[test]
fn supprime_une_ligne() {
    let bytes = fixture("tableau.pdf");
    let before = objects(&bytes, None);
    let rows: Vec<u32> = (0..9).filter(|&r| r != 2).collect();
    let page_edits = edits(vec![(INVOICE, table_edit(Some(rows), None))]);

    let after = objects(&bytes, Some(&page_edits));
    let table = tables(&after)[0];
    assert_eq!(size(table), (8, 4));
    assert!(!after
        .iter()
        .any(|o| o.text.as_deref() == Some("Prestation n° 2")));
    // La ligne suivante remonte d'une hauteur de ligne, le haut ne bouge pas.
    let y = |objects: &[ObjectInfo], text: &str| {
        objects
            .iter()
            .find(|o| o.text.as_deref() == Some(text))
            .unwrap()
            .bounds
            .y
    };
    let step = y(&before, "Prestation n° 3") - y(&before, "Prestation n° 2");
    assert!((y(&after, "Prestation n° 3") - y(&before, "Prestation n° 2")).abs() < 0.01);
    assert!((y(&after, "Désignation") - y(&before, "Désignation")).abs() < 0.01);
    let old = tables(&before)[0].bounds;
    assert!((table.bounds.height - (old.height - step)).abs() < 0.01);

    let written = write(&bytes, page_edits);
    let content = text(&written);
    assert!(!content.contains("Prestation n° 2"));
    assert!(content.contains("Prestation n° 3"));
    let redetected = objects(&written, None);
    let table = tables(&redetected)[0];
    assert_eq!(size(table), (8, 4));
    assert!((table.bounds.height - (old.height - step)).abs() < 0.6);
}

#[test]
fn supprime_une_colonne() {
    let bytes = fixture("tableau.pdf");
    let page_edits = edits(vec![(INVOICE, table_edit(None, Some(vec![0, 2, 3])))]);
    let after = objects(&bytes, Some(&page_edits));
    assert_eq!(size(tables(&after)[0]), (9, 3));
    assert!(!after.iter().any(|o| o.text.as_deref() == Some("Quantité")));

    let written = write(&bytes, page_edits);
    let content = text(&written);
    assert!(!content.contains("Quantité"));
    assert!(content.contains("Prix unitaire"));
    let redetected = objects(&written, None);
    let table = tables(&redetected)[0];
    assert_eq!(size(table), (9, 3));
    // Le total, hors du tableau, ne bouge pas.
    let total = |objects: &[ObjectInfo]| {
        objects
            .iter()
            .find(|o| o.text.as_deref() == Some("Total HT"))
            .unwrap()
            .bounds
    };
    assert_eq!(total(&redetected).x, total(&objects(&bytes, None)).x);
}

#[test]
fn deplace_une_ligne() {
    let bytes = fixture("tableau.pdf");
    let mut rows: Vec<u32> = (0..9).collect();
    rows.swap(1, 2);
    let page_edits = edits(vec![(INVOICE, table_edit(Some(rows), None))]);
    let before = objects(&bytes, None);
    let written = write(&bytes, page_edits);
    let redetected = objects(&written, None);
    assert_eq!(size(tables(&redetected)[0]), (9, 4));
    // Les deux lignes ont échangé leur place.
    let y = |objects: &[ObjectInfo], text: &str| {
        objects
            .iter()
            .find(|o| o.text.as_deref() == Some(text))
            .unwrap()
            .bounds
            .y
    };
    for (a, b) in [
        ("Prestation n° 1", "Prestation n° 2"),
        ("12.50 €", "25.00 €"),
    ] {
        assert!((y(&redetected, a) - y(&before, b)).abs() < 0.6);
        assert!((y(&redetected, b) - y(&before, a)).abs() < 0.6);
    }
}

#[test]
fn deplace_et_supprime_le_tableau() {
    let bytes = fixture("tableau.pdf");
    let before = tables(&objects(&bytes, None))[0].bounds;
    let moved = ObjectEdit {
        dx: 20.0,
        dy: -30.0,
        ..ObjectEdit::default()
    };
    let written = write(&bytes, edits(vec![(INVOICE, moved)]));
    let redetected = objects(&written, None);
    let after = tables(&redetected)[0].bounds;
    assert!(
        (after.x - before.x - 20.0).abs() < 0.6,
        "{before:?} {after:?}"
    );
    assert!((after.y - before.y - 30.0).abs() < 0.6);

    let deleted = ObjectEdit {
        deleted: true,
        ..ObjectEdit::default()
    };
    let page_edits = edits(vec![(INVOICE, deleted)]);
    let after = objects(&bytes, Some(&page_edits));
    assert!(tables(&after).is_empty());
    assert!(!after.iter().any(|o| o.table == Some(INVOICE)));
    let written = write(&bytes, page_edits);
    let content = text(&written);
    assert!(!content.contains("Prestation"));
    assert!(content.contains("Total HT"));
    let redetected = objects(&written, None);
    assert!(tables(&redetected).is_empty());
    assert!(!redetected.iter().any(|o| o.kind == ObjectKind::Path));
}

/// Tableau de `tableaux.pdf` : 6 lignes × 5 colonnes, en-tête sur deux lignes
/// avec cellules fusionnées, cellules vides.
fn quarterly(objects: &[ObjectInfo]) -> &ObjectInfo {
    tables(objects)
        .into_iter()
        .find(|t| t.grid.as_ref().unwrap().columns == 5)
        .unwrap()
}

#[test]
fn detecte_les_cellules_fusionnees_et_le_tableau_a_filets() {
    let objects = objects(&fixture("tableaux.pdf"), None);
    let found = tables(&objects);
    assert_eq!(found.len(), 2);
    let grid = quarterly(&objects).grid.as_ref().unwrap();
    assert_eq!((grid.rows, grid.columns), (6, 5));
    assert_eq!(grid.cells.len(), 27);
    let spans: Vec<(u32, u32, u32, u32)> = grid
        .cells
        .iter()
        .filter(|c| c.row_span > 1 || c.column_span > 1)
        .map(|c| (c.row, c.column, c.row_span, c.column_span))
        .collect();
    assert_eq!(spans, [(0, 0, 2, 1), (0, 1, 1, 2), (0, 3, 1, 2)]);
    let empty = grid.cells.iter().filter(|c| c.objects.is_empty()).count();
    assert_eq!(empty, 3);
    assert!(grid
        .cells
        .iter()
        .filter(|c| c.objects.is_empty())
        .all(|c| c.reference.is_some()));

    let lined = found
        .iter()
        .find(|t| t.grid.as_ref().unwrap().columns == 3)
        .unwrap();
    assert_eq!(size(lined), (4, 3));
}

#[test]
fn saisit_le_texte_d_une_cellule_vide() {
    let bytes = fixture("tableaux.pdf");
    let table = quarterly(&objects(&bytes, None)).clone();
    let grid = table.grid.as_ref().unwrap();
    // T2, Export : vide.
    let cell = grid
        .cells
        .iter()
        .find(|c| c.row == 3 && c.column == 2)
        .unwrap();
    assert!(cell.objects.is_empty());
    let mut cells = BTreeMap::new();
    cells.insert(
        (3, 2),
        TextReplacement {
            text: "48".to_owned(),
            font: FontChoice::Original,
        },
    );
    let edit = ObjectEdit {
        table: Some(TableEdit {
            rows: None,
            columns: None,
            cells,
        }),
        ..ObjectEdit::default()
    };
    let page_edits = edits(vec![(table.id, edit)]);
    let after = objects(&bytes, Some(&page_edits));
    let filled = quarterly(&after)
        .grid
        .as_ref()
        .unwrap()
        .cells
        .iter()
        .find(|c| c.row == 3 && c.column == 2)
        .unwrap()
        .clone();
    assert_eq!(filled.text.as_deref(), Some("48"));
    assert!(!filled.overflow);

    let written = write(&bytes, page_edits);
    let redetected = objects(&written, None);
    let grid = quarterly(&redetected).grid.as_ref().unwrap();
    let cell = grid
        .cells
        .iter()
        .find(|c| c.row == 3 && c.column == 2)
        .unwrap();
    assert_eq!(cell.objects.len(), 1, "{grid:?}");
    let block = redetected.iter().find(|o| o.id == cell.objects[0]).unwrap();
    assert_eq!(block.text.as_deref(), Some("48"));
    // Aligné à droite comme les autres valeurs de la colonne, au même corps.
    let reference = redetected
        .iter()
        .find(|o| o.text.as_deref() == Some("45"))
        .unwrap();
    let right = |o: &ObjectInfo| o.bounds.x + o.bounds.width;
    assert!((right(block) - right(reference)).abs() < 0.6);
    assert_eq!(block.font_size, reference.font_size);
}

#[test]
fn supprime_une_colonne_d_un_en_tete_fusionne() {
    let bytes = fixture("tableaux.pdf");
    let table = quarterly(&objects(&bytes, None)).clone();
    // Sans « Export » : « Ventes » ne couvre plus qu'une colonne.
    let page_edits = edits(vec![(table.id, table_edit(None, Some(vec![0, 1, 3, 4])))]);
    let after = objects(&bytes, Some(&page_edits));
    let grid = quarterly_with(&after, 4);
    let sales = grid
        .cells
        .iter()
        .find(|c| c.row == 0 && c.column == 1)
        .unwrap();
    assert_eq!(sales.column_span, 1);

    let written = write(&bytes, page_edits);
    let content = text(&written);
    assert!(!content.contains("Export"));
    assert!(content.contains("Ventes") && content.contains("Variables"));
    let redetected = objects(&written, None);
    let grid = quarterly_with(&redetected, 4);
    assert_eq!((grid.rows, grid.columns), (6, 4));
    // Le filet entre « Ventes » et « Charges » reste ; celui qui séparait
    // « France » d'« Export » sous « Ventes » disparaît.
    let merged: Vec<(u32, u32, u32, u32)> = grid
        .cells
        .iter()
        .filter(|c| c.row_span > 1 || c.column_span > 1)
        .map(|c| (c.row, c.column, c.row_span, c.column_span))
        .collect();
    assert_eq!(merged, [(0, 0, 2, 1), (0, 2, 1, 2)], "{grid:?}");
}

fn quarterly_with(objects: &[ObjectInfo], columns: u32) -> &pdf_core::TableInfo {
    tables(objects)
        .into_iter()
        .filter_map(|t| t.grid.as_ref())
        .find(|g| g.columns == columns && g.rows == 6)
        .unwrap()
}

#[test]
fn supprime_une_ligne_d_un_tableau_a_filets() {
    let bytes = fixture("tableaux.pdf");
    let lined = tables(&objects(&bytes, None))
        .into_iter()
        .find(|t| t.grid.as_ref().unwrap().columns == 3)
        .unwrap()
        .clone();
    let page_edits = edits(vec![(lined.id, table_edit(Some(vec![0, 1, 3]), None))]);
    let written = write(&bytes, page_edits);
    let content = text(&written);
    assert!(!content.contains("Nantes"));
    assert!(content.contains("Rennes") && content.contains("Lyon"));
    let redetected = objects(&written, None);
    let table = tables(&redetected)
        .into_iter()
        .find(|t| t.grid.as_ref().unwrap().columns == 3)
        .unwrap();
    assert_eq!(size(table), (3, 3));
}
