import XCTest
@testable import PDFStudio

/// Tableaux (tests/fixtures/tableaux.pdf) : grille, lignes et colonnes
/// supprimées ou déplacées, saisie d'une cellule vide, récupération.
final class TableEditingTests: XCTestCase {
    private func open() throws -> PdfDocument {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "tableaux", withExtension: "pdf"))
        return try PdfDocument.open(path: url.path, password: nil)
    }

    /// Tableau trimestriel : 6 lignes × 5 colonnes à l'origine.
    private func quarterly(_ session: EditSession, page: UInt64) throws -> PageObjectItem {
        try XCTUnwrap(session.pageObjects(page: page).first { $0.kind == .table && ($0.grid?.columns ?? 0) >= 4 })
    }

    func testDetecteLesTableauxEtLeursCellules() throws {
        let session = EditSession(document: try open())
        let page = try XCTUnwrap(session.pages().first).id
        let objects = try session.pageObjects(page: page)
        XCTAssertEqual(objects.filter { $0.kind == .table }.count, 2)
        let table = try quarterly(session, page: page)
        let grid = try XCTUnwrap(table.grid)
        XCTAssertEqual(grid.rows, 6)
        XCTAssertEqual(grid.columns, 5)
        XCTAssertEqual(grid.cells.filter { $0.rowSpan > 1 || $0.columnSpan > 1 }.count, 3)
        XCTAssertEqual(grid.cells.filter(\.fillable).count, 3)
        // Les blocs des cellules appartiennent au tableau.
        let members = objects.filter { $0.table == table.id }
        XCTAssertTrue(members.contains { $0.text == "France" })
    }

    func testModifieLignesColonnesEtCellules() throws {
        let session = EditSession(document: try open())
        let page = try XCTUnwrap(session.pages().first).id
        let table = try quarterly(session, page: page).id

        try session.deleteTableBands(page: page, table: table, axis: .rows, bands: [3])
        XCTAssertEqual(session.undoName(), "la suppression de la ligne")
        XCTAssertEqual(try quarterly(session, page: page).grid?.rows, 5)
        try session.moveTableBand(page: page, table: table, axis: .columns, from: 4, to: 3)
        XCTAssertEqual(session.undoName(), "le déplacement de la colonne")
        // « Export » ne peut pas quitter « Ventes ».
        XCTAssertThrowsError(try session.moveTableBand(page: page, table: table, axis: .columns, from: 2, to: 4))

        // T3 / France, vide à l'origine.
        let item = try session.setCellText(page: page, table: table, row: 3, column: 1, text: "128")
        let cell = try XCTUnwrap(item.grid?.cells.first { $0.row == 3 && $0.column == 1 })
        XCTAssertEqual(cell.text, "128")
        XCTAssertFalse(cell.overflow)

        // Récupération : les modifications du tableau survivent au codage.
        let sizes = [try open().pageSizes().map(\.cgSize)]
        let records = try session.pages().map { item in
            RecoveryRecord.Page(DisplayPage(item, sizes: sizes), edits: try session.contentRecord(page: item.id))
        }
        let decoded = try JSONDecoder().decode([RecoveryRecord.Page].self, from: JSONEncoder().encode(records))
        XCTAssertEqual(decoded, records)
        XCTAssertNotNil(decoded[0].edits?.first?.table)
        let restored = EditSession(document: try open())
        try restored.restore(
            pages: decoded.map(\.item), contents: decoded.map(\.contentItems), layers: decoded.map(\.layerItems)
        )
        let restoredPage = try XCTUnwrap(restored.pages().first).id
        XCTAssertEqual(try restored.contentRecord(page: restoredPage), try session.contentRecord(page: page))

        let exported = try session.exportPdf()
        let written = try PdfDocument.fromData(data: exported.bytes, password: nil)
        let text = try written.plainText(index: 0)
        XCTAssertTrue(text.contains("128"))
        XCTAssertFalse(text.contains("135"))

        while session.undo() {}
        XCTAssertEqual(try quarterly(session, page: page).grid?.rows, 6)
    }

    func testIntitulesDuMenuTableau() {
        let titles = PageEditing.TableAction.allCases.map(\.title)
        XCTAssertEqual(Set(titles).count, titles.count)
        XCTAssertEqual(PageEditing.TableAction.deleteRow.title, "Supprimer la ligne")
    }
}
