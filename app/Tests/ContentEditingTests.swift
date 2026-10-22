import XCTest
@testable import PDFStudio

/// Édition du contenu des pages (tests/fixtures/edition.pdf) : objets, rendu des
/// modifications, récupération.
final class ContentEditingTests: XCTestCase {
    private func open() throws -> PdfDocument {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "edition", withExtension: "pdf"))
        return try PdfDocument.open(path: url.path, password: nil)
    }

    /// Composante rouge, verte et bleue d'un pixel rendu.
    private func pixel(_ page: RenderedPage, x: Int, y: Int) -> [UInt8] {
        let offset = y * Int(page.bytesPerRow) + x * 4
        return Array(page.pixels[offset..<offset + 3])
    }

    func testModifieEtRendLeContenuDUnePage() throws {
        let document = try open()
        let session = EditSession(document: document)
        let page = try XCTUnwrap(session.pages().first)
        XCTAssertEqual(page.editRevision, 0)
        let objects = try session.pageObjects(page: page.id)
        XCTAssertEqual(objects.map(\.kind), [.text, .text, .text, .image, .path, .path])
        XCTAssertEqual(objects[0].text, "Rapport trimestriel")
        XCTAssertTrue(objects.allSatisfy(\.movable))

        // L'image disparaît du rendu.
        let image = objects[3].bounds
        let (x, y) = (Int(image.x + image.width / 2), Int(image.y + image.height / 2))
        let before = try document.renderPage(index: 0, scale: 1, rotation: 0)
        XCTAssertNotEqual(pixel(before, x: x, y: y), [255, 255, 255])
        try session.deleteObjects(page: page.id, objects: [3])
        XCTAssertEqual(session.undoName(), "la suppression")
        let edited = try XCTUnwrap(session.pages().first)
        XCTAssertNotEqual(edited.editRevision, 0)
        let edits = try XCTUnwrap(session.contentEdits(page: page.id))
        XCTAssertEqual(edits.revision(), edited.editRevision)
        let after = try document.renderEditedPage(index: 0, edits: edits, scale: 1, rotation: 0)
        XCTAssertEqual(pixel(after, x: x, y: y), [255, 255, 255])

        let line = try session.replaceText(page: page.id, object: 2, text: "Ligne modifiée")
        XCTAssertEqual(line.text, "Ligne modifiée")
        XCTAssertFalse(line.substituted)
        let exported = try session.exportPdf()
        let written = try PdfDocument.fromData(data: exported.bytes, password: nil)
        XCTAssertTrue(try written.plainText(index: 0).contains("Ligne modifiée"))

        // L'annulation rend la page d'origine.
        XCTAssertTrue(session.undo())
        XCTAssertTrue(session.undo())
        XCTAssertEqual(session.pages().first?.editRevision, 0)
        XCTAssertNil(session.contentEdits(page: page.id))
    }

    func testRetientEtRestaureLeContenu() throws {
        let session = EditSession(document: try open())
        let pages = session.pages()
        let first = try XCTUnwrap(pages.first).id
        try session.moveObjects(page: first, objects: [0], dx: 10, dy: 20)
        _ = try session.replaceText(page: first, object: 2, text: "Texte retrouvé")

        let sizes = [try open().pageSizes().map(\.cgSize)]
        let records = try session.pages().map { item in
            RecoveryRecord.Page(DisplayPage(item, sizes: sizes), edits: try session.contentRecord(page: item.id))
        }
        XCTAssertEqual(records[0].edits?.count, 2)
        XCTAssertNil(records[1].edits)
        let decoded = try JSONDecoder().decode(
            [RecoveryRecord.Page].self, from: JSONEncoder().encode(records)
        )
        XCTAssertEqual(decoded, records)

        let restored = EditSession(document: try open())
        try restored.restore(
            pages: decoded.map(\.item), contents: decoded.map(\.contentItems), layers: decoded.map(\.layerItems)
        )
        let page = try XCTUnwrap(restored.pages().first)
        XCTAssertNotEqual(page.editRevision, 0)
        let objects = try restored.pageObjects(page: page.id)
        XCTAssertEqual(objects[2].text, "Texte retrouvé")
        let original = try session.pageObjects(page: first)
        XCTAssertEqual(objects[0].bounds.x, original[0].bounds.x, accuracy: 0.01)
        XCTAssertEqual(objects[0].bounds.y, original[0].bounds.y, accuracy: 0.01)
    }

    func testRameneLesDeplacementsSurPageDroite() {
        let move = CGVector(dx: 3, dy: 5)
        XCTAssertEqual(DisplayRotation().unapply(to: move), move)
        XCTAssertEqual(DisplayRotation(degrees: 90).unapply(to: move), CGVector(dx: 5, dy: -3))
        XCTAssertEqual(DisplayRotation(degrees: 180).unapply(to: move), CGVector(dx: -3, dy: -5))
        XCTAssertEqual(DisplayRotation(degrees: 270).unapply(to: move), CGVector(dx: -5, dy: 3))
        // Cohérent avec la rotation des rectangles.
        let size = CGSize(width: 200, height: 300)
        let rect = CGRect(x: 10, y: 20, width: 30, height: 40)
        for degrees in [90, 180, 270] {
            let rotation = DisplayRotation(degrees: degrees)
            let shown = rotation.apply(to: rect, pageSize: size)
            let moved = rotation.apply(to: rect.offsetBy(dx: 3, dy: 5), pageSize: size)
            let back = rotation.unapply(to: CGVector(dx: moved.minX - shown.minX, dy: moved.minY - shown.minY))
            XCTAssertEqual(back, move, "\(degrees)°")
        }
    }
}
