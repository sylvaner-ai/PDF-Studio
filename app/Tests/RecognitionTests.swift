import CoreGraphics
import PDFKit
import XCTest
@testable import PDFStudio

/// Reconnaissance du texte par Vision, de bout en bout : détection des pages
/// scannées, lecture, couche de texte cherchable, annulation, enregistrement.
final class RecognitionTests: XCTestCase {
    private func fixture(_ name: String) throws -> String {
        try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"), "\(name).pdf absent").path
    }

    func testReconnaitUnePageScannee() throws {
        let session = EditSession(document: try PdfDocument.open(path: fixture("scan_simule"), password: nil))
        let pages = try session.pagesToRecognize()
        XCTAssertEqual(pages, session.pages().map(\.id), "page en image, sans texte")
        XCTAssertTrue(try session.searchPage(page: pages[0], query: "efficacite").isEmpty)

        let result = try session.recognizePage(
            page: pages[0], recognizer: VisionTextRecognizer(), settings: defaultRecognitionSettings()
        )
        XCTAssertEqual(result.engine, "Vision")
        XCTAssertGreaterThan(result.words.count, 100)
        XCTAssertEqual(result.quarterTurns, 0)
        XCTAssertGreaterThan(result.meanConfidence, 0.5)

        session.applyRecognitions(results: [result])
        XCTAssertEqual(session.undoName(), "la reconnaissance du texte")
        XCTAssertNotEqual(session.pages()[0].textLayerRevision, 0)
        XCTAssertTrue(try session.pagesToRecognize().isEmpty)
        // Recherche sans tenir compte des accents ni de la casse, avant l'enregistrement.
        XCTAssertFalse(try session.searchPage(page: pages[0], query: "EFFICACITE").isEmpty)
        XCTAssertFalse(try session.recognizedWords(page: pages[0]).isEmpty)

        // La couche de texte est enregistrée dans le PDF, invisible et cherchable.
        let exported = try session.exportPdf()
        let reopened = try PdfDocument.fromData(data: exported.bytes, password: nil)
        XCTAssertFalse(try reopened.searchPage(index: 0, query: "efficacité").isEmpty)
        XCTAssertTrue(try EditSession(document: reopened).pagesToRecognize().isEmpty)
        // PDFKit (Aperçu) lit la couche ligne par ligne, et non mot par mot en colonnes.
        let pdfKitText = try XCTUnwrap(PDFDocument(data: exported.bytes)?.page(at: 0)?.string)
        let lines = pdfKitText.split(whereSeparator: \.isNewline)
        let readLines = Set(result.words.map(\.line)).count
        XCTAssertLessThanOrEqual(lines.count, readLines + 2, "une ligne lue doit rester une ligne")
        XCTAssertTrue(lines.contains { $0.split(separator: " ").count >= 5 })

        XCTAssertTrue(session.undo())
        XCTAssertTrue(try session.recognizedWords(page: pages[0]).isEmpty)
        XCTAssertTrue(try session.searchPage(page: pages[0], query: "efficacite").isEmpty)
    }

    func testVisionLitUneLettreEnFrancais() throws {
        let document = try PdfDocument.open(path: fixture("lettre"), password: nil)
        let page = try document.renderPage(index: 0, scale: 300 / 72, rotation: 0)
        let image = try XCTUnwrap(page.makeCGImage())
        let words = try VisionTextReader.read(image, languages: ["fr-FR", "en-US"])
        let text = words.map(\.text).joined(separator: " ")
        for expected in ["Société", "commande", "Lyon,"] {
            XCTAssertTrue(text.contains(expected), "« \(expected) » absent")
        }
        XCTAssertTrue(words.allSatisfy { $0.quad.count == 8 && (0...1).contains($0.confidence) })
        // Les mots d'une ligne se suivent de gauche à droite, sans se chevaucher.
        let first = words.filter { $0.line == words[0].line }
        for (left, right) in zip(first, first.dropFirst()) {
            XCTAssertLessThanOrEqual(left.quad[2], right.quad[0] + 2)
        }
    }

    func testRetientLaCoucheDeTexte() throws {
        let word = LayerWordItem(text: "Évreux", quad: [10, 10, 60, 10, 60, 22, 10, 22], confidence: 0.4, line: 0)
        let page = RecoveryRecord.Page(
            DisplayPage(
                PageItem(id: 7, kind: .source(source: 0, index: 0), quarterTurns: 0, editRevision: 0,
                         textLayerRevision: 3),
                sizes: [[CGSize(width: 595, height: 842)]]
            ),
            layer: [word]
        )
        let decoded = try JSONDecoder().decode(RecoveryRecord.Page.self, from: JSONEncoder().encode(page))
        XCTAssertEqual(decoded.layerItems, [word])

        let session = EditSession(document: try PdfDocument.open(path: fixture("scan_simule"), password: nil))
        try session.restore(pages: [decoded.item], contents: [[]], layers: [decoded.layerItems])
        let restored = try XCTUnwrap(session.pages().first)
        XCTAssertNotEqual(restored.textLayerRevision, 0)
        XCTAssertEqual(try session.searchPage(page: restored.id, query: "evreux").count, 1)
        XCTAssertEqual(try session.recognizedWords(page: restored.id).first?.doubtful, true)
    }
}
