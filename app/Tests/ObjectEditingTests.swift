import AppKit
import XCTest
@testable import PDFStudio

/// Objets redimensionnés, copiés, coupés, collés, dupliqués et passés d'une
/// page à l'autre (tests/fixtures/edition.pdf, D71, D72) : récupération, et
/// commandes de la fenêtre sur un presse-papiers propre aux tests.
@MainActor
final class ObjectEditingTests: XCTestCase {
    private static let line = "Texte en Helvetica, police standard."
    private var directory: URL?
    private var previousPasteboard: NSPasteboard?

    override func setUp() {
        super.setUp()
        previousPasteboard = PageEditing.pasteboard
        PageEditing.pasteboard = NSPasteboard(name: NSPasteboard.Name("fr.sylvainrobert.pdfstudio.tests"))
        PageEditing.pasteboard.clearContents()
        PageEditing.copied = nil
    }

    override func tearDown() {
        PageEditing.pasteboard.releaseGlobally()
        if let previousPasteboard {
            PageEditing.pasteboard = previousPasteboard
        }
        PageEditing.copied = nil
        if let directory {
            try? FileManager.default.removeItem(at: directory)
        }
        super.tearDown()
    }

    /// Ouvre une copie du document de test, pour que la récupération ne vise pas le bundle.
    private func open() async throws -> DocumentViewModel {
        let source = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "edition", withExtension: "pdf"))
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("objets-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        directory = folder
        let url = folder.appendingPathComponent("edition.pdf")
        try FileManager.default.copyItem(at: source, to: url)
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready)
        model.workspace = .editing
        return model
    }

    /// Objets de la page `index` une fois chargés pour son état actuel.
    private func objects(_ model: DocumentViewModel, page index: Int) async throws -> [PageObjectItem] {
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            let page = try XCTUnwrap(model.pages[safe: index])
            let wanted = try XCTUnwrap(model.session).pageObjects(page: page.id).map(\.id)
            if let known = model.editing.objects(on: page), known.map(\.id) == wanted {
                return known
            }
            try? await Task.sleep(for: .milliseconds(25))
        }
        XCTFail("objets non chargés")
        throw CancellationError()
    }

    func testRedimensionneUnObjetDOrigineEtLeRetient() async throws {
        let model = try await open()
        defer {
            model.discardChanges()
            model.close()
            RecoveryWriter.shared.flush()
        }
        let session = try XCTUnwrap(model.session)
        let page = try XCTUnwrap(model.pages.first)
        let items = try await objects(model, page: 0)
        let image = try XCTUnwrap(items.first { $0.kind == .image && !$0.added })
        XCTAssertTrue(image.resizable)
        XCTAssertTrue(try XCTUnwrap(items.first { $0.kind == .path }).resizable)

        model.editing.select([image.id], on: page.id)
        XCTAssertEqual(model.editing.selectedResizable?.id, image.id)
        let bounds = image.bounds.cgRect
        model.editing.resizeObject(
            image.id, on: page.id,
            to: CGRect(x: bounds.minX, y: bounds.minY, width: bounds.width * 2, height: bounds.height * 2)
        )
        XCTAssertEqual(session.undoName(), "le redimensionnement")
        let resized = try XCTUnwrap(try session.pageObjects(page: page.id).first { $0.id == image.id })
        XCTAssertEqual(resized.bounds.width, image.bounds.width * 2, accuracy: 0.6)

        // Récupération : l'agrandissement survit au codage, et les
        // enregistrements antérieurs (sans agrandissement) se lisent encore.
        let edit = try XCTUnwrap(try session.contentRecord(page: page.id).first { $0.object == image.id })
        let stored = RecoveryRecord.ObjectEdit(edit)
        XCTAssertNotNil(stored.scaleX)
        let decoded = try JSONDecoder().decode(RecoveryRecord.ObjectEdit.self, from: JSONEncoder().encode(stored))
        XCTAssertEqual(decoded.item, edit)
        let older = try JSONSerialization.data(withJSONObject: [
            "object": 3, "deleted": false, "offsetX": 0, "offsetY": 0, "fingerprint": "x"
        ])
        let legacy = try JSONDecoder().decode(RecoveryRecord.ObjectEdit.self, from: older)
        XCTAssertEqual(legacy.item.scaleX, 1)
        XCTAssertEqual(legacy.item.scaleY, 1)
    }

    func testCopieColleDupliqueEtPasseSurUneAutrePage() async throws {
        let model = try await open()
        defer {
            model.discardChanges()
            model.close()
            RecoveryWriter.shared.flush()
        }
        let session = try XCTUnwrap(model.session)
        let editing = model.editing
        let first = try XCTUnwrap(model.pages.first)
        let second = try XCTUnwrap(model.pages[safe: 1])
        let firstItems = try await objects(model, page: 0)
        let line = try XCTUnwrap(firstItems.first { $0.text?.contains(Self.line) == true })

        // Copier : le presse-papiers reçoit un PDF et le texte.
        editing.select([line.id], on: first.id)
        XCTAssertTrue(editing.canCopy)
        editing.copySelection()
        XCTAssertNotNil(PageEditing.pasteboard.data(forType: .pdf))
        XCTAssertEqual(PageEditing.pasteboard.string(forType: .string), line.text)
        XCTAssertTrue(editing.canPaste)

        // Coller sur l'autre page : la copie y est sélectionnée.
        editing.paste(on: second)
        XCTAssertEqual(session.undoName(), "le collage")
        XCTAssertEqual(editing.page, second.id)
        XCTAssertEqual(editing.selected.count, 1)
        let pasted = try XCTUnwrap(try session.pageObjects(page: second.id).first { editing.selected.contains($0.id) })
        XCTAssertTrue(pasted.added)

        // Coller sur la page de l'original : décalé.
        editing.paste(on: first)
        let shifted = try XCTUnwrap(try session.pageObjects(page: first.id).first { editing.selected.contains($0.id) })
        XCTAssertEqual(shifted.bounds.x, line.bounds.x - 1 + PageEditing.pasteOffset, accuracy: 0.6)

        // Dupliquer, puis passer la copie sur l'autre page : une action chacun
        // (la sélection se lit dans les objets rechargés de la page).
        _ = try await objects(model, page: 0)
        editing.duplicateSelection()
        XCTAssertEqual(session.undoName(), "la duplication")
        let duplicate = try XCTUnwrap(editing.selected.first)
        _ = try await objects(model, page: 0)
        editing.transferSelection(to: second.id, center: CGPoint(x: 200, y: 200), keep: false)
        XCTAssertEqual(session.undoName(), "le déplacement")
        XCTAssertEqual(editing.page, second.id)
        XCTAssertFalse(try session.pageObjects(page: first.id).contains { $0.id == duplicate })

        // Couper l'original : il disparaît, et se recolle à sa place.
        _ = try await objects(model, page: 0)
        editing.select([line.id], on: first.id)
        editing.cutSelection()
        XCTAssertEqual(session.undoName(), "la coupe")
        XCTAssertNil(try session.pageObjects(page: first.id).first { $0.id == line.id })
        editing.paste(on: first)
        let back = try XCTUnwrap(try session.pageObjects(page: first.id).first { editing.selected.contains($0.id) })
        XCTAssertEqual(back.bounds.x, line.bounds.x - 1, accuracy: 0.6)

        // Un presse-papiers changé ailleurs n'est plus la copie de l'application.
        PageEditing.pasteboard.clearContents()
        PageEditing.pasteboard.setString("autre", forType: .string)
        XCTAssertFalse(editing.canPaste)

        while session.undo() {}
        XCTAssertNotNil(try session.pageObjects(page: first.id).first { $0.id == line.id })
    }
}
