import AppKit
import XCTest
@testable import PDFStudio

/// Onglets (D66) : les documents ouverts et les nouveaux onglets rejoignent la
/// fenêtre d'où vient la demande, et un onglet passé en arrière-plan garde son
/// document.
///
/// Le test passe par les vraies fenêtres SwiftUI de l'application qui héberge
/// les tests, à partir de son écran d'accueil. Avec `PDFSTUDIO_SNAPSHOTS`
/// (`make snapshots`), il capture aussi la fenêtre et sa barre d'onglets.
@MainActor
final class WindowTabsTests: XCTestCase {
    private var directory: URL?
    private var group: NSWindowTabGroup?

    override func tearDown() async throws {
        for window in group?.windows ?? [] {
            window.close()
        }
        RecoveryWriter.shared.flush()
        if let directory {
            try? FileManager.default.removeItem(at: directory)
        }
        try await super.tearDown()
    }

    func testLesDocumentsEtLesNouveauxOngletsRejoignentLaFenetre() async throws {
        let front = try await applicationWindow()
        let first = try copy("texte_simple")
        let second = try copy("tableau")
        let third = try copy("organisation")
        DocumentOpener.shared.open([first, second], besides: front)
        await wait {
            DocumentOpener.shared.window(showing: first.path) != nil
                && DocumentOpener.shared.window(showing: second.path) != nil
        }
        let firstWindow = try XCTUnwrap(DocumentOpener.shared.window(showing: first.path), "premier document")
        let secondWindow = try XCTUnwrap(DocumentOpener.shared.window(showing: second.path), "second document")
        group = front.tabGroup

        // L'écran d'accueil accueille le premier document, le second s'ouvre dans un onglet.
        XCTAssertTrue(firstWindow === front)
        XCTAssertTrue(secondWindow.tabGroup === front.tabGroup)
        XCTAssertEqual(front.tabbedWindows?.count, 2)
        XCTAssertTrue(front.tabGroup?.selectedWindow === secondWindow)

        // En arrière-plan, le premier onglet reste ouvert sur son document.
        await pause(seconds: 1)
        XCTAssertTrue(DocumentOpener.shared.isShown(path: first.path))

        // Un document ouvert depuis la fenêtre au premier plan (⌘O, Finder) la
        // rejoint, sans écran d'accueil en plus, ni dans un onglet ni à part.
        DocumentOpener.shared.open([third])
        await wait { DocumentOpener.shared.window(showing: third.path) != nil }
        await pause(seconds: 1)
        XCTAssertTrue(DocumentOpener.shared.window(showing: third.path)?.tabGroup === front.tabGroup)
        XCTAssertEqual(front.tabbedWindows?.count, 3)
        XCTAssertEqual(documentWindows().count, 3, "fenêtre ou onglet en trop")

        // Fichier confié par le Finder, comme à un double-clic : SwiftUI ouvre de
        // lui-même une fenêtre vide, qui doit être refermée. (Un fichier texte, que
        // PDF Studio ignore : un PDF hors du bac à sable demanderait un signet.)
        let text = try XCTUnwrap(directory).appendingPathComponent("note.txt")
        try Data("PDF Studio".utf8).write(to: text)
        NSApp.delegate?.application?(NSApp, open: [text])
        var most = documentWindows().count
        let deadline = Date().addingTimeInterval(3)
        while Date() < deadline {
            most = max(most, documentWindows().count)
            await pause(seconds: 0.05)
        }
        print("Fenêtres pendant l'ouverture depuis le Finder : \(most) au plus, \(documentWindows().count) ensuite.")
        XCTAssertEqual(documentWindows().count, 3, "écran d'accueil vide resté ouvert")
        XCTAssertEqual(front.tabbedWindows?.count, 3)

        // Nouvel onglet (⌘T) : un écran d'accueil, qui n'est pas refermé.
        DocumentOpener.shared.openNewTab(besides: secondWindow)
        await wait { front.tabbedWindows?.count == 4 }
        await pause(seconds: 1)
        XCTAssertEqual(front.tabbedWindows?.count, 4)
        XCTAssertEqual(documentWindows().count, 4)

        // Bouton + de la barre d'onglets : traité par le délégué de la fenêtre,
        // que la fenêtre de SwiftUI ne devance pas.
        let newTab = #selector(NSResponder.newWindowForTab(_:))
        XCTAssertTrue(secondWindow.delegate?.responds(to: newTab) == true)
        XCTAssertFalse(secondWindow.responds(to: newTab), "la fenêtre de SwiftUI traite elle-même le bouton +")

        if ProcessInfo.processInfo.environment["PDFSTUDIO_SNAPSHOTS"] != nil {
            secondWindow.makeKeyAndOrderFront(nil)
            await pause(seconds: 2)
            try capture(secondWindow, as: "8-onglets")
        }
    }

    // MARK: - Préparation

    /// Écran d'accueil ouvert par l'application au lancement.
    private func applicationWindow() async throws -> NSWindow {
        var window: NSWindow?
        await wait {
            window = NSApp.windows.first { $0.tabbingIdentifier == DocumentOpener.tabbingIdentifier && $0.isVisible }
            return window != nil
        }
        guard let window else {
            throw XCTSkip("L'application hôte des tests n'a pas ouvert de fenêtre.")
        }
        return window
    }

    /// Fenêtres de l'application encore ouvertes : visibles, ou onglets cachés.
    private func documentWindows() -> [NSWindow] {
        NSApp.windows.filter { window in
            window.tabbingIdentifier == DocumentOpener.tabbingIdentifier
                && (window.isVisible || (window.tabbedWindows?.count ?? 0) > 1)
        }
    }

    /// Copie d'un document du jeu de test, ouvrable sans signet de sécurité.
    private func copy(_ name: String) throws -> DocumentReference {
        let source = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
        let folder = directory ?? FileManager.default.temporaryDirectory.appendingPathComponent("onglets-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        directory = folder
        let url = folder.appendingPathComponent("\(name).pdf")
        try FileManager.default.copyItem(at: source, to: url)
        return try DocumentReference(url: url, securityScoped: false)
    }

    /// Image de la fenêtre entière, barre de titre et onglets compris.
    private func capture(_ window: NSWindow, as file: String) throws {
        let caches = try XCTUnwrap(FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first)
        let folder = caches.appendingPathComponent("Snapshots", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let frame = try XCTUnwrap(window.contentView?.superview)
        let rep = try XCTUnwrap(frame.bitmapImageRepForCachingDisplay(in: frame.bounds))
        frame.cacheDisplay(in: frame.bounds, to: rep)
        let data = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        try data.write(to: folder.appendingPathComponent("\(file).png"))
    }

    /// Attend sans bloquer le fil principal, où s'ouvrent les fenêtres.
    private func wait(upTo seconds: TimeInterval = 10, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(25))
        }
    }

    private func pause(seconds: TimeInterval) async {
        try? await Task.sleep(for: .seconds(seconds))
    }
}
