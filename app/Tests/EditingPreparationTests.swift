import Foundation
import XCTest
@testable import PDFStudio

/// Entrée en mode Édition (D58) : un PDF natif passe aussitôt en Édition, un
/// scan voit d'abord le texte de ses pages reconnu par Vision.
@MainActor
final class EditingPreparationTests: XCTestCase {
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
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("édition-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        directory = folder
        let url = folder.appendingPathComponent("\(name).pdf")
        try FileManager.default.copyItem(at: source, to: url)
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready, name)
        return model
    }

    /// Fermée sans enregistrer, la fenêtre ne laisse rien à récupérer.
    private func close(_ model: DocumentViewModel) {
        model.discardChanges()
        model.close()
        RecoveryWriter.shared.flush()
    }

    /// Attend sans bloquer le fil principal, où se termine la préparation.
    private func wait(upTo seconds: TimeInterval = 60, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(25))
        }
    }

    func testUnPDFNatifPasseDirectementEnEdition() async throws {
        let model = try await open("texte_simple")
        defer { close(model) }
        model.workspace = .editing
        XCTAssertTrue(model.isEditingCanvas, "le canevas passe en Édition pendant la recherche des scans")
        await wait { model.recognitionTask == nil }
        XCTAssertNil(model.recognition)
        XCTAssertTrue(model.canEdit)
        XCTAssertTrue(model.isEditingCanvas)
        XCTAssertTrue(model.pages.allSatisfy { $0.textRevision == 0 }, "aucune page reconnue")
        XCTAssertNil(model.session?.undoName())
        XCTAssertNil(model.notice)
    }

    func testUnScanEstReconnuPuisPasseEnEdition() async throws {
        let model = try await open("scan_simule")
        defer { close(model) }
        model.workspace = .editing
        XCTAssertNotNil(model.recognition)
        XCTAssertFalse(model.canEdit, "les pages restent figées pendant la préparation")
        await wait { model.recognition?.isSearching != true }
        if model.recognition != nil {
            XCTAssertFalse(model.isEditingCanvas, "les pages sont montrées comme en lecture pendant la lecture")
        }
        await wait { model.recognitionTask == nil }

        XCTAssertEqual(model.workspace, .editing)
        XCTAssertTrue(model.isEditingCanvas)
        XCTAssertTrue(model.canEdit)
        let page = try XCTUnwrap(model.pages.first)
        XCTAssertNotEqual(page.textRevision, 0, "la page scannée a sa couche de texte")
        XCTAssertEqual(model.session?.undoName(), "la reconnaissance du texte")
        XCTAssertTrue(model.notice?.hasPrefix("Texte reconnu") == true, model.notice ?? "")
        XCTAssertTrue(try XCTUnwrap(model.session).pagesToRecognize().isEmpty)

        // Une nouvelle entrée en Édition ne relit pas les pages déjà reconnues.
        model.workspace = .reading
        model.workspace = .editing
        await wait { model.recognitionTask == nil }
        XCTAssertEqual(model.pages.first?.textRevision, page.textRevision)
    }

    func testQuitterLEditionPendantLaRechercheNeLitRien() async throws {
        let model = try await open("scan_simule")
        defer { close(model) }
        model.workspace = .editing
        model.workspace = .reading
        await wait { model.recognitionTask == nil }
        XCTAssertNil(model.recognition)
        XCTAssertTrue(model.canEdit)
        XCTAssertTrue(model.pages.allSatisfy { $0.textRevision == 0 })
        XCTAssertNil(model.session?.undoName())
    }
}
