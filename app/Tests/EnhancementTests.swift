import AppKit
import CoreGraphics
import Foundation
import SwiftUI
import XCTest
@testable import PDFStudio

/// Mode Amélioration, jalon 2 (tests/fixtures/amelioration/11_orientation.pdf :
/// une page à l'envers, une page couchée et inclinée de 0,8°) : aperçu,
/// application en une étape d'annulation, comparaison avec l'original,
/// signets, récupération et retour aux pages d'origine.
@MainActor
final class EnhancementTests: XCTestCase {
    private var directory: URL?
    /// Réglages retenus avant le test, rendus à la fin.
    private var savedSettings: Any?

    override func setUp() {
        super.setUp()
        savedSettings = UserDefaults.standard.object(forKey: "enhancementSettings")
    }

    override func tearDown() {
        if let directory {
            try? FileManager.default.removeItem(at: directory)
        }
        UserDefaults.standard.set(savedSettings, forKey: "enhancementSettings")
        super.tearDown()
    }

    /// Ouvre une copie du document de test, pour que la récupération ne vise pas le bundle.
    private func open(_ name: String) async throws -> DocumentViewModel {
        let source = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("amélioration-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        directory = folder
        let url = folder.appendingPathComponent("\(name).pdf")
        try FileManager.default.copyItem(at: source, to: url)
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready, name)
        model.enhancement.settings = defaultEnhancementSettings()
        return model
    }

    private func close(_ model: DocumentViewModel) {
        model.discardChanges()
        model.close()
        RecoveryWriter.shared.flush()
    }

    /// Attend sans bloquer le fil principal, où se termine le traitement.
    private func wait(upTo seconds: TimeInterval = 60, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(25))
        }
    }

    func testLAperçuMontreAvantEtApresDansLeSensFinal() async throws {
        let model = try await open("11_orientation")
        defer { close(model) }
        model.workspace = .enhancing
        let page = try XCTUnwrap(model.pages[safe: 1])
        await model.enhancement.loadPreview(of: page)
        let preview = try XCTUnwrap(model.enhancement.preview)
        XCTAssertEqual(preview.report.status, .corrected)
        XCTAssertEqual(preview.report.quarterTurns, 3)
        XCTAssertEqual(Double(preview.report.correctedAngle), 0.8, accuracy: 0.2)
        // Même format avant et après, en portrait une fois la page remise à l'endroit.
        XCTAssertEqual(preview.before.cgImage.width, preview.after.cgImage.width)
        XCTAssertEqual(preview.before.cgImage.height, preview.after.cgImage.height)
        XCTAssertGreaterThan(preview.after.cgImage.height, preview.after.cgImage.width)
        // L'aperçu ne modifie pas le document.
        XCTAssertNil(model.session?.undoName())
        XCTAssertFalse(model.isModified)
    }

    /// Le mode dans sa fenêtre, comme l'ouvre l'utilisateur : l'aperçu de la
    /// page affichée arrive, et la navigation change de page. Avec
    /// `PDFSTUDIO_ENHANCE_DOCUMENT` (chemin lisible depuis le bac à sable de
    /// l'application), sur ce document plutôt que sur le jeu de test.
    func testLeModeDansSaFenetreMontreLAperçuEtChangeDePage() async throws {
        let model: DocumentViewModel
        if let path = ProcessInfo.processInfo.environment["PDFSTUDIO_ENHANCE_DOCUMENT"] {
            let reference = try DocumentReference(url: URL(fileURLWithPath: path), securityScoped: false)
            model = DocumentViewModel(reference: reference)
            await model.open()
            XCTAssertEqual(model.phase, .ready)
            model.enhancement.settings = defaultEnhancementSettings()
        } else {
            model = try await open("11_orientation")
        }
        defer { close(model) }
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 1080, height: 760),
            styleMask: [.titled, .closable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        let host = NSHostingView(rootView: DocumentView(model: model))
        host.sceneBridgingOptions = [.toolbars, .title]
        window.contentView = host
        window.makeKeyAndOrderFront(nil)
        defer { window.close() }
        await wait(upTo: 2) { false }

        model.workspace = .enhancing
        let start = Date()
        await wait(upTo: 30) { model.enhancement.preview?.key.page == model.pages.first?.id }
        let first = try XCTUnwrap(model.enhancement.preview, "aucun aperçu après 30 s")
        print("Aperçu de la page 1 : \(Int(Date().timeIntervalSince(start) * 1000)) ms")
        XCTAssertEqual(first.key.page, model.pages.first?.id)
        XCTAssertFalse(model.enhancement.isPreviewing)
        guard model.pages.count > 1 else { return }

        // Page suivante : le mode suit la page affichée sans le canevas.
        model.canvas.go(to: 1)
        XCTAssertEqual(model.canvas.currentPage, 1, "la navigation ne change pas de page")
        let next = Date()
        await wait(upTo: 30) { model.enhancement.preview?.key.page == model.pages[1].id }
        XCTAssertEqual(model.enhancement.preview?.key.page, model.pages[1].id, "aucun aperçu de la page 2")
        print("Aperçu de la page 2 : \(Int(Date().timeIntervalSince(next) * 1000)) ms")
        XCTAssertFalse(model.enhancement.isPreviewing)
    }

    func testAppliqueAnnuleEtRendLesPagesDOrigine() async throws {
        let model = try await open("11_orientation")
        defer { close(model) }
        model.workspace = .enhancing
        let originals = model.pages
        model.enhancement.range = .all
        model.enhancement.apply()
        XCTAssertFalse(model.canEdit, "les pages restent figées pendant le traitement")
        await wait { model.enhancement.task == nil }

        let session = try XCTUnwrap(model.session)
        XCTAssertEqual(session.undoName(), "l'amélioration des pages")
        XCTAssertTrue(model.isModified)
        XCTAssertEqual(model.pages.map(\.quarterTurns), [2, 3])
        // La page couchée est redressée : elle vient du document des pages améliorées.
        XCTAssertEqual(model.pages[1].source?.document, 1)
        XCTAssertEqual(model.pages[1].replaced?.source, originals[1].source)
        XCTAssertTrue(model.canCompare)
        XCTAssertEqual(model.pages[1].original.source, originals[1].source)
        XCTAssertEqual(model.pages[1].original.quarterTurns, 0)
        // Un signet vers la page d'origine mène à la page améliorée.
        XCTAssertEqual(model.position(ofOriginalPage: 1), 1)
        XCTAssertTrue(model.notice?.contains("1 page redressée") == true, model.notice ?? "")

        // Les modifications retenues pour la récupération gardent la page d'origine.
        let record = RecoveryRecord.Page(model.pages[1])
        let decoded = try JSONDecoder().decode(RecoveryRecord.Page.self, from: JSONEncoder().encode(record))
        XCTAssertEqual(decoded.item.replaces?.quarterTurns, 0)
        XCTAssertEqual(decoded.item.replaces?.kind, .source(source: 0, index: 1))

        // Retour aux pages d'origine, puis annulation de ce retour.
        model.enhancement.range = .all
        XCTAssertTrue(model.enhancement.canRestore)
        model.enhancement.restoreOriginals()
        XCTAssertEqual(session.undoName(), "le retour aux pages d'origine")
        XCTAssertEqual(model.pages.map(\.source), originals.map(\.source))
        XCTAssertEqual(model.pages.map(\.quarterTurns), [0, 0])
        XCTAssertTrue(model.pages.allSatisfy { $0.replaced == nil })
        XCTAssertTrue(session.undo())
        XCTAssertTrue(session.undo())
        XCTAssertEqual(session.pages().map(\.replaces), [nil, nil])
    }

    func testUnePageNativeNEstPasModifiee() async throws {
        let model = try await open("texte_simple")
        defer { close(model) }
        model.workspace = .enhancing
        model.enhancement.apply()
        await wait { model.enhancement.task == nil }
        XCTAssertNil(model.session?.undoName())
        XCTAssertFalse(model.isModified)
        XCTAssertEqual(model.notice, "Aucune page scannée à redresser.")
    }
}
