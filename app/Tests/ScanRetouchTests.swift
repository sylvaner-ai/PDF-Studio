import XCTest
@testable import PDFStudio

/// Retouche d'une page scannée (tests/fixtures/scan_retouche.pdf), lue par
/// Vision : lignes retrouvées dans l'image, texte réécrit avec la police
/// retrouvée, réglages fins, couche de texte à jour, récupération.
final class ScanRetouchTests: XCTestCase {
    private func open() throws -> PdfDocument {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "scan_retouche", withExtension: "pdf"))
        return try PdfDocument.open(path: url.path, password: nil)
    }

    /// Session dont la page scannée a été lue par Vision.
    private func recognizedSession() throws -> (EditSession, UInt64) {
        let session = EditSession(document: try open())
        let page = try XCTUnwrap(session.pagesToRecognize().first)
        let result = try session.recognizePage(
            page: page, recognizer: VisionTextRecognizer(), settings: defaultRecognitionSettings()
        )
        session.applyRecognitions(results: [result])
        return (session, page)
    }

    func testRetoucheUneLigneScannee() throws {
        BundledFonts.register()
        let (session, page) = try recognizedSession()
        let objects = try session.pageObjects(page: page)
        XCTAssertTrue(objects.allSatisfy(\.scanned))
        XCTAssertTrue(objects.contains { $0.kind == .table && $0.grid != nil })
        let line = try XCTUnwrap(objects.first { $0.kind == .text && $0.text?.contains("avril") == true })
        XCTAssertTrue(line.editable)
        XCTAssertNil(line.scan)

        let text = try XCTUnwrap(line.text).replacingOccurrences(of: "avril", with: "mai")
        let edited = try session.replaceText(page: page, object: line.id, text: text)
        XCTAssertEqual(edited.text, text)
        let scan = try XCTUnwrap(edited.scan)
        XCTAssertFalse(scan.candidates.isEmpty)
        XCTAssertLessThanOrEqual(scan.candidates.count, 3)
        XCTAssertEqual(scan.candidates.map(\.score), scan.candidates.map(\.score).sorted(by: >))
        XCTAssertTrue(scan.candidates.allSatisfy { (0...1).contains($0.score) })
        XCTAssertEqual(scan.style, scan.measured)
        XCTAssertEqual(scan.chosenCandidate, 0)

        // Réglages fins, puis retour aux mesures ; chacun s'annule.
        var bigger = scan.style
        bigger.size += 1
        bigger.weight = 0.3
        let adjusted = try session.setScanStyle(page: page, object: line.id, style: bigger)
        XCTAssertEqual(adjusted.fontSize, bigger.size, accuracy: 0.001)
        XCTAssertEqual(adjusted.scan?.style.weight ?? 0, 0.3, accuracy: 0.001)
        let reset = try session.resetScanStyle(page: page, object: line.id)
        XCTAssertEqual(reset.scan?.style, scan.measured)
        XCTAssertTrue(session.undo())
        XCTAssertEqual(try session.pageObjects(page: page).first { $0.id == line.id }?.scan?.style, bigger)

        // La couche de texte suit le texte réécrit, avant et après l'enregistrement.
        XCTAssertFalse(try session.searchPage(page: page, query: "mai").isEmpty)
        let exported = try session.exportPdf()
        let written = try PdfDocument.fromData(data: exported.bytes, password: nil)
        XCTAssertFalse(try written.searchPage(index: 0, query: "mai").isEmpty)
        XCTAssertTrue(try written.searchPage(index: 0, query: "avril").isEmpty)
    }

    func testLesLignesDUnTableauScanneNeSeReorganisentPas() throws {
        let (session, page) = try recognizedSession()
        let table = try XCTUnwrap(try session.pageObjects(page: page).first { $0.kind == .table })
        XCTAssertThrowsError(try session.deleteTableBands(page: page, table: table.id, axis: .rows, bands: [0]))
    }

    func testRetientLaRetouche() throws {
        let edit = ObjectEditItem(
            object: 0x4000_0001, deleted: false, dx: 0, dy: 0, text: nil, font: nil,
            fingerprint: "line", table: nil, scan: "texte\tÉchéance"
        )
        let stored = RecoveryRecord.ObjectEdit(edit)
        let decoded = try JSONDecoder().decode(RecoveryRecord.ObjectEdit.self, from: JSONEncoder().encode(stored))
        XCTAssertEqual(decoded.item, edit)
    }

    func testLaComparaisonMontreLesPagesDOrigine() {
        let item = PageItem(
            id: 3, kind: .source(source: 0, index: 0), quarterTurns: 1, editRevision: 9, textLayerRevision: 2
        )
        let page = DisplayPage(item, sizes: [[CGSize(width: 595, height: 842)]])
        XCTAssertEqual(page.original.editRevision, 0)
        XCTAssertEqual(page.original.textRevision, 2)
        XCTAssertTrue(page.original.hasSameLayout(as: page))
    }
}
