import XCTest
@testable import PDFStudio

/// Tests du rangement des modifications non enregistrées, dans un dossier temporaire.
final class RecoveryStoreTests: XCTestCase {
    private var directory: URL!
    private var store: RecoveryStore!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("récupération-\(UUID())")
        store = try RecoveryStore(directory: directory)
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: directory)
    }

    private func record(
        path: String, basePath: String? = nil, sources: [String] = [], pages: [RecoveryRecord.Page]
    ) throws -> RecoveryRecord {
        let file = directory.appendingPathComponent("empreinte")
        try Data("contenu".utf8).write(to: file)
        return RecoveryRecord(
            path: path, stamp: try FileStamp(of: file), basePath: basePath, sources: sources, pages: pages,
            date: Date()
        )
    }

    func testRetientEtRetrouveLesModifications() throws {
        let page = RecoveryRecord.Page(source: 0, index: 2, width: 595, height: 842, quarterTurns: 1)
        let saved = try record(path: "/Documents/contrat.pdf", pages: [page])
        store.save(saved)
        XCTAssertEqual(store.record(for: "/Documents/contrat.pdf"), saved)
        XCTAssertNil(store.record(for: "/Documents/autre.pdf"))

        store.removeRecord(for: "/Documents/contrat.pdf")
        XCTAssertNil(store.record(for: "/Documents/contrat.pdf"))
    }

    func testRestaureLesPagesDansLeMoteur() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "organisation", withExtension: "pdf"))
        let document = try PdfDocument.open(path: url.path, password: nil)
        let session = EditSession(document: document)
        let sizes = [document.pageSizes().map(\.cgSize)]
        var pages = session.pages().reversed().map { RecoveryRecord.Page(DisplayPage($0, sizes: sizes)) }
        pages.append(RecoveryRecord.Page(source: nil, index: 0, width: 300, height: 400, quarterTurns: 5))

        try session.restore(pages: pages.map(\.item), contents: pages.map(\.contentItems))
        let restored = session.pages().map { DisplayPage($0, sizes: sizes) }
        XCTAssertEqual(restored.compactMap(\.source?.index), [4, 3, 2, 1, 0])
        XCTAssertEqual(restored.last?.baseSize, CGSize(width: 300, height: 400))
        XCTAssertEqual(restored.last?.quarterTurns, 1)
        XCTAssertTrue(session.isModified())
        XCTAssertEqual(session.undoName(), "la restauration")
        XCTAssertEqual(try session.sourceBytes(source: 0), try Data(contentsOf: url))
    }

    func testNeGardeQueLesCopiesUtilisees() throws {
        let manager = FileManager.default
        let used = try store.makeCopy(Data("version d'ouverture".utf8), of: .base)
        let unused = try store.makeCopy(Data("autre version".utf8), of: .base)
        let inserted = try store.makeCopy(Data("pages insérées".utf8), of: .source)
        let dropped = try store.makeCopy(Data("insertion annulée".utf8), of: .source)
        let saved = try record(path: "/Documents/contrat.pdf", basePath: used, sources: [inserted], pages: [])
        store.save(saved)

        store.removeUnusedCopies()
        XCTAssertTrue(manager.fileExists(atPath: used))
        XCTAssertTrue(manager.fileExists(atPath: inserted))
        XCTAssertFalse(manager.fileExists(atPath: unused))
        XCTAssertFalse(manager.fileExists(atPath: dropped))
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: used)), Data("version d'ouverture".utf8))

        store.discard(saved)
        XCTAssertNil(store.record(for: saved.path))
        XCTAssertFalse(manager.fileExists(atPath: used))
        XCTAssertFalse(manager.fileExists(atPath: inserted))

        // Seules les copies de ce rangement peuvent être supprimées.
        let outside = directory.deletingLastPathComponent().appendingPathComponent("base-\(UUID()).pdf")
        try Data("ailleurs".utf8).write(to: outside)
        defer { try? FileManager.default.removeItem(at: outside) }
        store.removeCopy(outside.path)
        XCTAssertTrue(manager.fileExists(atPath: outside.path))
    }

    func testLEmpreinteChangeAvecLeFichier() throws {
        let file = directory.appendingPathComponent("document.pdf")
        try Data("première version".utf8).write(to: file)
        let first = try FileStamp(of: file)
        XCTAssertEqual(try FileStamp(of: file), first)
        try Data("deuxième version, plus longue".utf8).write(to: file)
        XCTAssertNotEqual(try FileStamp(of: file), first)
    }
}
