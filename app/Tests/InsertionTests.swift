import XCTest
@testable import PDFStudio

/// Tests de l'insertion de pages, à travers le pont Swift ↔ Rust.
final class InsertionTests: XCTestCase {
    private func open(_ name: String) throws -> PdfDocument {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
        return try PdfDocument.open(path: url.path, password: nil)
    }

    func testInsereDesPagesVenuesDUneAutreFenetreEtUnePageVierge() throws {
        let session = EditSession(document: try open("organisation"))

        // Pages glissées depuis une autre fenêtre : exportées, puis rouvertes en mémoire.
        let other = EditSession(document: try open("signets"))
        try other.rotate(positions: [0], quarterTurns: 1)
        let copied = try PdfDocument.fromData(data: try other.exportPages(positions: [0, 2]), password: nil)
        XCTAssertEqual(copied.pageCount(), 2)

        let source = session.addSource(document: copied)
        XCTAssertEqual(source, 1)
        XCTAssertEqual(try session.insertPages(at: 1, source: source, indices: [0, 1]), [1, 2])
        XCTAssertEqual(try session.insertBlank(at: 0, size: PageSize(width: 300, height: 400)), 0)
        XCTAssertEqual(session.pages().count, 8)
        XCTAssertEqual(session.undoName(), "l'insertion")
        // La copie de récupération d'une source reprend ses octets.
        XCTAssertFalse(try session.sourceBytes(source: source).isEmpty)

        let saved = try PdfDocument.fromData(data: try session.exportPdf(), password: nil)
        XCTAssertEqual(saved.pageCount(), 8)
        XCTAssertTrue(session.undo())
        XCTAssertEqual(session.pages().count, 7)
    }
}
