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

/// Cellule qui mêle des textes de corps et de graisse différents
/// (tests/fixtures/tableau_styles.pdf) : chaque texte est un bloc, et la
/// saisie de la cellule porte sur celui que l'on vise.
@MainActor
final class CellBlocksEditingTests: XCTestCase {
    private var directory: URL?

    override func tearDown() {
        if let directory {
            try? FileManager.default.removeItem(at: directory)
        }
        super.tearDown()
    }

    /// Ouvre une copie du document de test, pour que la récupération ne vise pas le bundle.
    private func open(_ name: String) async throws -> DocumentViewModel {
        let source = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("cellules-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        directory = folder
        let url = folder.appendingPathComponent("\(name).pdf")
        try FileManager.default.copyItem(at: source, to: url)
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready, name)
        return model
    }

    /// Attend sans bloquer le fil principal.
    private func wait(upTo seconds: TimeInterval = 60, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(25))
        }
    }

    func testUneCelluleAuxStylesMixtesSeModifieBlocParBloc() async throws {
        let model = try await open("tableau_styles")
        defer {
            model.discardChanges()
            model.close()
            RecoveryWriter.shared.flush()
        }
        model.workspace = .editing
        await wait { model.recognitionTask == nil }
        let page = try XCTUnwrap(model.pages.first)
        var items: [PageObjectItem] = []
        await wait(upTo: 30) {
            items = model.editing.objects(on: page) ?? []
            return !items.isEmpty
        }
        let table = try XCTUnwrap(items.first { $0.kind == .table })
        let cell = try XCTUnwrap(table.grid?.cells.first { $0.objects.count == 2 })
        let (title, detail) = (cell.objects[0], cell.objects[1])
        XCTAssertTrue(items.filter { [title, detail].contains($0.id) }.allSatisfy(\.editable))
        let target = PageEditing.CellTarget(page: page.id, table: table.id, row: cell.row, column: cell.column)
        let editing = model.editing

        // Sans indication, la saisie porte sur le premier bloc.
        editing.beginCellEditing(target)
        XCTAssertEqual(editing.textTarget?.object, title)
        editing.endTextEditing(text: nil)

        // Un double-clic sur le second bloc ouvre celui-ci.
        editing.beginCellEditing(target, block: detail)
        XCTAssertEqual(editing.textTarget?.object, detail)
        editing.endTextEditing(text: nil)

        // Un clic simple retient le bloc visé, que reprend la touche Retour.
        editing.selectCell(target, block: detail)
        editing.beginCellEditing(target)
        XCTAssertEqual(editing.textTarget?.object, detail)
        editing.endTextEditing(text: nil)

        // Un bloc qui n'est pas de la cellule est ignoré.
        editing.beginCellEditing(target, block: .max)
        XCTAssertEqual(editing.textTarget?.object, title)
        editing.endTextEditing(text: nil)
    }
}
