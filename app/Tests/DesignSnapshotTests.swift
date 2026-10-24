import AppKit
import SwiftUI
import XCTest
@testable import PDFStudio

/// Captures d'écran de l'interface en mode clair et en mode sombre, sur des
/// documents du jeu de test, pour la revue du design (`make snapshots`, D49).
///
/// Ignoré par les tests ordinaires : il ne s'exécute que si la variable
/// d'environnement `PDFSTUDIO_SNAPSHOTS` est définie. Les images sont écrites
/// dans les caches du bac à sable de l'application, d'où `make snapshots` les
/// copie. Elles sont tirées de la fenêtre elle-même (`cacheDisplay`), sans
/// autorisation d'enregistrement de l'écran ; les matériaux translucides y
/// apparaissent sans le flou de ce qui est derrière la fenêtre.
@MainActor
final class DesignSnapshotTests: XCTestCase {
    private static let windowSize = CGSize(width: 1080, height: 760)

    func testCapturesLInterface() throws {
        guard ProcessInfo.processInfo.environment["PDFSTUDIO_SNAPSHOTS"] != nil else {
            throw XCTSkip("Captures d'écran : lancer `make snapshots`.")
        }
        let caches = try XCTUnwrap(FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first)
        let folder = caches.appendingPathComponent("Snapshots", isDirectory: true)
        try? FileManager.default.removeItem(at: folder)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { NSApp.appearance = nil }

        for (appearance, suffix) in [(NSAppearance.Name.aqua, "clair"), (.darkAqua, "sombre")] {
            NSApp.appearance = NSAppearance(named: appearance)
            try capture(WelcomeView().navigationTitle("PDF Studio"), as: "1-accueil-\(suffix)", in: folder)
            try captureDocument("edition", as: "2-lecture-\(suffix)", in: folder) { _ in }
            try captureDocument("multipage_100", as: "3-vignettes-\(suffix)", in: folder) { _ in }
            try captureDocument("organisation", as: "4-pages-\(suffix)", in: folder) { model in
                model.workspace = .organizing
            }
            try captureDocument("edition", as: "5-edition-\(suffix)", in: folder) { model in
                model.workspace = .editing
                self.selectFirstText(of: model)
            }
            try captureDocument("edition", as: "6-recherche-\(suffix)", in: folder) { model in
                model.search?.requestFocus()
                model.search?.update(query: "trimestriel")
            }
            try captureDocument("11_orientation", as: "7-amelioration-\(suffix)", in: folder) { model in
                model.workspace = .enhancing
            }
        }
        print("Captures d'écran : \(folder.path)")
    }

    // MARK: - Préparation

    private func fixture(_ name: String) throws -> URL {
        try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
    }

    /// Ouvre un document, le prépare, puis capture sa fenêtre.
    private func captureDocument(
        _ name: String, as file: String, in folder: URL, prepare: (DocumentViewModel) -> Void
    ) throws {
        // Un fichier du bundle de test ne peut pas recevoir de signet de sécurité.
        let reference = try DocumentReference(url: try fixture(name), securityScoped: false)
        let model = DocumentViewModel(reference: reference)
        Task { await model.open() }
        wait(upTo: 5) { model.phase == .ready }
        XCTAssertEqual(model.phase, .ready, name)
        try capture(DocumentView(model: model), as: file, in: folder) {
            prepare(model)
        }
        model.close()
    }

    /// Sélectionne le premier bloc de texte de la première page, une fois ses objets chargés.
    private func selectFirstText(of model: DocumentViewModel) {
        guard let page = model.pages.first else { return }
        wait(upTo: 5) { model.editing.objects(on: page) != nil }
        if let text = model.editing.objects(on: page)?.first(where: { $0.kind == .text }) {
            model.editing.select([text.id], on: page.id)
        }
    }

    // MARK: - Capture

    /// Affiche `view` dans une fenêtre à barre d'outils unifiée, laisse les pages
    /// se rendre, puis écrit l'image de toute la fenêtre, barre d'outils comprise.
    private func capture(
        _ view: some View, as file: String, in folder: URL, prepare: () -> Void = {}
    ) throws {
        let window = NSWindow(
            contentRect: CGRect(origin: .zero, size: Self.windowSize),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.toolbarStyle = .unified
        let host = NSHostingView(rootView: view)
        host.sceneBridgingOptions = [.toolbars, .title]
        window.contentView = host
        window.center()
        window.makeKeyAndOrderFront(nil)
        defer { window.close() }

        wait(seconds: 1)
        prepare()
        wait(seconds: 2.5)

        let frame = host.superview ?? host
        let bounds = frame.bounds
        let rep = try XCTUnwrap(frame.bitmapImageRepForCachingDisplay(in: bounds))
        frame.cacheDisplay(in: bounds, to: rep)
        let data = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        try data.write(to: folder.appendingPathComponent("\(file).png"))
    }

    /// Laisse tourner la boucle principale (rendu des pages, animations).
    private func wait(seconds: TimeInterval) {
        RunLoop.main.run(until: Date().addingTimeInterval(seconds))
    }

    private func wait(upTo seconds: TimeInterval, until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        }
    }
}
