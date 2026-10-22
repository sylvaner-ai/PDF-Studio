import AppKit
import XCTest
@testable import PDFStudio

/// Tests du pont Swift ↔ Rust sur le jeu de PDF de référence (tests/fixtures).
final class EngineBridgeTests: XCTestCase {
    private func fixture(_ name: String) throws -> String {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"),
            "fixture \(name).pdf absente du bundle de test"
        )
        return url.path
    }

    func testOuvreEtRendLaPremierePage() throws {
        let document = try PdfDocument.open(path: fixture("texte_simple"), password: nil)
        XCTAssertEqual(document.pageCount(), 1)
        XCTAssertEqual(document.title(), "Texte simple")

        let page = try document.renderPage(index: 0, scale: 2, rotation: 0)
        XCTAssertEqual(page.width, 1191)
        XCTAssertEqual(page.height, 1684)
        let image = try XCTUnwrap(page.makeCGImage())
        XCTAssertEqual(image.width, 1191)
        XCTAssertEqual(image.height, 1684)
    }

    func testDemandeLeMotDePasse() throws {
        let path = try fixture("chiffre")
        XCTAssertThrowsError(try PdfDocument.open(path: path, password: nil)) { error in
            guard case .PasswordRequired = error as? EngineError else {
                return XCTFail("PasswordRequired attendu, obtenu \(error)")
            }
        }
        XCTAssertThrowsError(try PdfDocument.open(path: path, password: "mauvais")) { error in
            guard case .WrongPassword = error as? EngineError else {
                return XCTFail("WrongPassword attendu, obtenu \(error)")
            }
        }
        XCTAssertEqual(try PdfDocument.open(path: path, password: "motdepasse").pageCount(), 1)
    }

    func testMessageDErreurEnFrancais() throws {
        XCTAssertThrowsError(try PdfDocument.open(path: fixture("pas_un_pdf"), password: nil)) { error in
            let message = (error as? EngineError)?.userMessage ?? ""
            XCTAssertTrue(message.hasPrefix("Ce fichier n'est pas un PDF valide"), message)
        }
    }

    /// Cahier des charges §6 : ouverture d'un PDF de 100 pages en moins de 2 s.
    func testOuvreCentPagesEnMoinsDeDeuxSecondes() throws {
        let path = try fixture("multipage_100")
        let start = Date()
        let document = try PdfDocument.open(path: path, password: nil)
        let elapsed = Date().timeIntervalSince(start)
        XCTAssertEqual(document.pageCount(), 100)
        XCTAssertLessThan(elapsed, 2.0)
    }

    func testLeRenduMetEnCache() async throws {
        let document = try PdfDocument.open(path: fixture("multipage_100"), password: nil)
        let renderer = PageRenderer(sources: PageSources(document))
        let size = try XCTUnwrap(document.pageSizes().first).cgSize
        let source = DisplayPage.Source(document: 0, index: 3)
        let page = DisplayPage(id: 1, source: source, quarterTurns: 0, baseSize: size)
        let first = try await renderer.image(of: page, pixelWidth: 640)
        let second = try await renderer.image(of: page, pixelWidth: 640)
        XCTAssertLessThanOrEqual(abs(first.cgImage.width - 640), 1)
        XCTAssertTrue(first.cgImage === second.cgImage, "la deuxième demande doit venir du cache")

        // Déplacée ou dupliquée, la page garde son image ; tournée, elle passe en paysage.
        let copy = DisplayPage(id: 2, source: page.source, quarterTurns: 0, baseSize: size)
        let copied = try await renderer.image(of: copy, pixelWidth: 640)
        XCTAssertTrue(first.cgImage === copied.cgImage)
        let turned = DisplayPage(id: 2, source: page.source, quarterTurns: 1, baseSize: size)
        let landscape = try await renderer.image(of: turned, pixelWidth: 640)
        XCTAssertGreaterThan(landscape.cgImage.width, landscape.cgImage.height)

        let blank = DisplayPage(id: 3, source: nil, quarterTurns: 0, baseSize: size)
        do {
            _ = try await renderer.image(of: blank, pixelWidth: 640)
            XCTFail("une page vierge ne se rend pas")
        } catch {
            XCTAssertTrue(error is BlankPageError)
        }
    }

    func testRendUnePageTourneeEtUneZone() throws {
        let document = try PdfDocument.open(path: fixture("texte_simple"), password: nil)
        let turned = try document.renderPage(index: 0, scale: 2, rotation: 90)
        XCTAssertEqual(turned.width, 1684)
        XCTAssertEqual(turned.height, 1191)
        let tile = try document.renderRegion(
            index: 0, scale: 2, rotation: 0, x: 100, y: 200, width: 256, height: 128
        )
        XCTAssertEqual(tile.width, 256)
        XCTAssertEqual(tile.height, 128)
        XCTAssertEqual(tile.pixels.count, 256 * 128 * 4)
    }

    func testLitLePlanDuDocument() throws {
        let document = try PdfDocument.open(path: fixture("signets"), password: nil)
        let items = try document.outline()
        XCTAssertEqual(items.count, 9)
        let tree = OutlineTree(items: items)
        XCTAssertEqual(tree.roots.map(\.title), ["1. Introduction", "2. Méthode", "3. Résultats"])
        XCTAssertEqual(tree.roots[1].children?.map(\.page), [2, 3])
        XCTAssertNil(tree.roots[0].children?.first?.children, "une section est une feuille")
    }

    func testChercheSansTenirCompteDesAccents() throws {
        let simple = try PdfDocument.open(path: fixture("texte_simple"), password: nil)
        XCTAssertEqual(try simple.searchPage(index: 0, query: "EFFICACITE").count, 5)
        XCTAssertEqual(try simple.searchPage(index: 0, query: "oeuvre").count, 5)
        XCTAssertTrue(try simple.searchPage(index: 0, query: "introuvable").isEmpty)
    }

    @MainActor
    func testRechercheDansToutLeDocument() async throws {
        let document = try PdfDocument.open(path: fixture("multipage_100"), password: nil)
        let sizes = [document.pageSizes().map(\.cgSize)]
        let pages = EditSession(document: document).pages().map { DisplayPage($0, sizes: sizes) }
        let canvas = CanvasController(pages: pages)
        let search = SearchModel(sources: PageSources(document), pages: pages, canvas: canvas)
        search.update(query: "Page 50 sur")
        let deadline = Date().addingTimeInterval(10)
        while search.isSearching, Date() < deadline {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertFalse(search.isSearching)
        XCTAssertEqual(search.hits, [SearchHit(page: 49, index: 0)])
        XCTAssertEqual(search.status, "1 sur 1")

        // Après une réorganisation, la recherche reprend sur les nouvelles positions.
        let reversed = Array(pages.reversed())
        canvas.setPages(reversed)
        search.setPages(reversed)
        let again = Date().addingTimeInterval(10)
        repeat {
            try await Task.sleep(for: .milliseconds(20))
        } while search.isSearching && Date() < again
        XCTAssertEqual(search.hits, [SearchHit(page: 50, index: 0)])
        // L'occurrence montrée avant la réorganisation reste la courante.
        XCTAssertEqual(search.status, "1 sur 1")
    }

    func testOrganiseLesPagesEtExporte() throws {
        let document = try PdfDocument.open(path: fixture("organisation"), password: nil)
        let session = EditSession(document: document)
        try session.rotate(positions: [0], quarterTurns: 1)
        XCTAssertEqual(try session.duplicate(positions: [1]), [2])
        XCTAssertEqual(try session.movePages(positions: [5], before: 0), [0])
        try session.delete(positions: [1])
        XCTAssertEqual(session.undoName(), "la suppression")
        XCTAssertTrue(session.isModified())
        XCTAssertThrowsError(try session.delete(positions: [0, 1, 2, 3, 4])) { error in
            XCTAssertEqual((error as? EngineError)?.userMessage, "Un document doit garder au moins une page.")
        }

        let sources = session.pages().map { page -> UInt32? in
            guard case let .source(_, index) = page.kind else { return nil }
            return index
        }
        XCTAssertEqual(sources, [4, 1, 1, 2, 3])

        let url = FileManager.default.temporaryDirectory.appendingPathComponent("organise-\(UUID()).pdf")
        defer { try? FileManager.default.removeItem(at: url) }
        let exported = try session.exportPdf()
        try exported.bytes.write(to: url)
        let written = try PdfDocument.open(path: url.path, password: nil)
        XCTAssertEqual(written.pageCount(), 5)
        XCTAssertFalse(try written.plainText(index: 0).isEmpty)

        XCTAssertTrue(session.undo())
        XCTAssertEqual(session.redoName(), "la suppression")
        XCTAssertFalse(try session.hasSignature())

        // Seule la révision écrite est enregistrée ; annuler y revient.
        session.markSaved(revision: exported.revision)
        XCTAssertTrue(session.isModified())
        XCTAssertTrue(session.redo())
        XCTAssertFalse(session.isModified())
    }
}

/// Écriture des fichiers enregistrés.
final class FileSaverTests: XCTestCase {
    private var directory: URL!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("enregistrement-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: directory)
    }

    func testRemplaceUnFichierEnGardantUneCopie() throws {
        let backups = directory.appendingPathComponent("sauvegardes")
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("document.pdf")
        try Data("ancien".utf8).write(to: url)

        try FileSaver.replace(url, with: Data("nouveau".utf8), backups: backups)
        XCTAssertEqual(try Data(contentsOf: url), Data("nouveau".utf8))
        let copies = try FileManager.default.contentsOfDirectory(at: backups, includingPropertiesForKeys: nil)
        XCTAssertEqual(copies.count, 1)
        XCTAssertTrue(copies[0].lastPathComponent.hasSuffix(" document.pdf"))
        XCTAssertEqual(try Data(contentsOf: copies[0]), Data("ancien".utf8))

        // Un nouveau fichier n'a rien à sauvegarder ; un fichier remplacé, si.
        let other = directory.appendingPathComponent("copie.pdf")
        try FileSaver.write(Data("copie".utf8), to: other, backups: backups)
        try FileSaver.write(Data("copie 2".utf8), to: other, backups: backups)
        XCTAssertEqual(try Data(contentsOf: other), Data("copie 2".utf8))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: backups.path).count, 2)
    }

    func testNeGardeQueLesSauvegardesRecentes() throws {
        let backups = directory.appendingPathComponent("sauvegardes")
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("document.pdf")
        try Data("0".utf8).write(to: url)
        // Un dossier rangé parmi les sauvegardes n'est jamais supprimé.
        let exports = backups.appendingPathComponent("Exportations")
        try FileManager.default.createDirectory(at: exports, withIntermediateDirectories: true)
        for version in 1...(FileSaver.maxBackups + 3) {
            try FileSaver.replace(url, with: Data("\(version)".utf8), backups: backups)
        }
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(atPath: backups.path).count, FileSaver.maxBackups + 1
        )
        XCTAssertTrue(FileManager.default.fileExists(atPath: exports.path))
    }

    func testRangeChaqueExportationDansSonDossier() throws {
        let exports = directory.appendingPathComponent("exportations")
        try FileManager.default.createDirectory(at: exports, withIntermediateDirectories: true)
        let images = (1...(FileSaver.maxBackups + 5)).map { directory.appendingPathComponent("page \($0).png") }
        for url in images {
            try Data("ancienne".utf8).write(to: url)
        }
        // Tous les fichiers remplacés par une exportation sont gardés, ensemble.
        let backups = try FileSaver.exportBackupDirectory(named: "document", in: exports)
        for url in images {
            try FileSaver.export(Data("nouvelle".utf8), to: url, backups: backups)
        }
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: backups.path).count, images.count)
        XCTAssertEqual(try Data(contentsOf: images[0]), Data("nouvelle".utf8))
        // Seuls les dossiers des dernières exportations restent.
        for _ in 1...FileSaver.maxExportBackups {
            _ = try FileSaver.exportBackupDirectory(named: "document", in: exports)
        }
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(atPath: exports.path).count, FileSaver.maxExportBackups
        )
    }
}

/// Géométrie de l'affichage : disposition des pages et rotation.
final class PageLayoutTests: XCTestCase {
    private let portrait = CGSize(width: 600, height: 800)
    private let landscape = CGSize(width: 800, height: 600)

    func testDisposeLesPagesEnColonneCentree() {
        let layout = PageLayout(sizes: [portrait, landscape], mode: .continuous)
        let margin = PageLayout.margin
        XCTAssertEqual(layout.documentSize.width, 800 + 2 * margin)
        XCTAssertEqual(layout.frames[0], CGRect(x: 100 + margin, y: margin, width: 600, height: 800))
        XCTAssertEqual(layout.frames[1].minY, margin + 800 + PageLayout.spacing)
        XCTAssertEqual(layout.documentSize.height, layout.frames[1].maxY + margin)
        XCTAssertEqual(layout.currentPage(in: CGRect(x: 0, y: 700, width: 800, height: 500)), 1)
    }

    func testPageParPage() {
        let layout = PageLayout(sizes: [portrait, landscape], mode: .singlePage, visiblePage: 1)
        XCTAssertEqual(layout.frames[0], .zero)
        XCTAssertEqual(layout.frames[1].size, landscape)
        XCTAssertEqual(layout.pages(intersecting: CGRect(x: 0, y: 0, width: 2_000, height: 2_000)), [1])
    }

    func testTourneLesPagesEtLesRectangles() {
        let quarter = DisplayRotation(degrees: 90)
        XCTAssertEqual(quarter.apply(to: portrait), landscape)
        XCTAssertEqual(DisplayRotation(degrees: -90).degrees, 270)
        XCTAssertEqual(DisplayRotation(degrees: 450).degrees, 90)

        // Rectangle en haut à gauche de la page droite.
        let rect = CGRect(x: 10, y: 20, width: 100, height: 30)
        XCTAssertEqual(quarter.apply(to: rect, pageSize: portrait), CGRect(x: 750, y: 10, width: 30, height: 100))
        XCTAssertEqual(
            DisplayRotation(degrees: 180).apply(to: rect, pageSize: portrait),
            CGRect(x: 490, y: 750, width: 100, height: 30)
        )
        XCTAssertEqual(
            DisplayRotation(degrees: 270).apply(to: rect, pageSize: portrait),
            CGRect(x: 20, y: 490, width: 30, height: 100)
        )
    }

    func testAjusteLeZoomEtSuitLesPaliers() {
        let layout = PageLayout(sizes: [portrait], mode: .continuous)
        // Document de 664 × 864 pt : la page et ses marges de 32 pt.
        let window = CGSize(width: 800, height: 600)
        XCTAssertEqual(layout.fitMagnification(.width, viewport: window, page: 0), 1.204, accuracy: 0.000_1)
        XCTAssertEqual(layout.fitMagnification(.page, viewport: window, page: 0), 0.694, accuracy: 0.000_1)
        XCTAssertEqual(layout.fitMagnification(.automatic, viewport: window, page: 0), 1.204, accuracy: 0.000_1)
        // Sur un grand écran, la page ne dépasse pas 1 100 pt de large.
        let wide = CGSize(width: 2_400, height: 1_400)
        XCTAssertEqual(layout.fitMagnification(.automatic, viewport: wide, page: 0), 1.833, accuracy: 0.000_1)

        XCTAssertEqual(Zoom.step(from: 1, direction: 1), 1.1)
        XCTAssertEqual(Zoom.step(from: 1, direction: -1), 0.9)
        XCTAssertEqual(Zoom.step(from: 1.2, direction: 1), 1.25)
        XCTAssertEqual(Zoom.step(from: 16, direction: 1), 16)
        XCTAssertEqual(Zoom.step(from: 0.1, direction: -1), 0.1)
    }

    @MainActor
    func testReconnaitLesTouchesDeLecture() throws {
        func key(_ code: UInt16, _ flags: NSEvent.ModifierFlags = [], _ characters: String = "") throws -> NSEvent {
            try XCTUnwrap(NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: 0,
                context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code
            ))
        }
        XCTAssertEqual(ReadingKey(try key(49)), .screen(down: true))
        XCTAssertEqual(ReadingKey(try key(49, .shift)), .screen(down: false))
        XCTAssertEqual(ReadingKey(try key(125)), .line(down: true))
        XCTAssertEqual(ReadingKey(try key(125, .command)), .documentEdge(end: true))
        XCTAssertEqual(ReadingKey(try key(115)), .documentEdge(end: false))
        XCTAssertEqual(ReadingKey(try key(123)), .side(right: false))
        XCTAssertNil(ReadingKey(try key(0, [], "a")))

        XCTAssertEqual(Zoom.direction(for: try key(24, .command, "=")), 1)
        XCTAssertEqual(Zoom.direction(for: try key(24, [.command, .shift], "+")), 1)
        XCTAssertEqual(Zoom.direction(for: try key(27, .command, "-")), -1)
        XCTAssertNil(Zoom.direction(for: try key(24, [], "=")))
    }
}
