import AppKit
import SwiftUI
import UniformTypeIdentifiers
import os

/// Centralise toutes les façons d'ouvrir un PDF : menu, glisser-déposer sur une
/// fenêtre, Finder et icône du Dock.
///
/// Une fenêtre d'accueil encore vide accueille le premier document ouvert ; les
/// suivants ouvrent une nouvelle fenêtre. Un même fichier n'est jamais ouvert
/// deux fois : SwiftUI ramène sa fenêtre au premier plan.
@MainActor
final class DocumentOpener {
    static let shared = DocumentOpener()
    static let windowID = "document"

    private let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "ouverture")
    private var openWindow: OpenWindowAction?
    private var emptyWindows: [(id: UUID, adopt: (DocumentReference) -> Void)] = []
    private var pending: [URL] = []

    private init() {}

    /// Mémorise l'action SwiftUI qui ouvre une fenêtre.
    func register(openWindow: OpenWindowAction) {
        self.openWindow = openWindow
        flush()
    }

    /// Déclare une fenêtre d'accueil capable d'afficher le prochain document.
    func registerEmptyWindow(id: UUID, adopt: @escaping (DocumentReference) -> Void) {
        emptyWindows.removeAll { $0.id == id }
        emptyWindows.append((id, adopt))
        flush()
    }

    func unregisterEmptyWindow(id: UUID) {
        emptyWindows.removeAll { $0.id == id }
    }

    /// Ouvre les PDF parmi les fichiers donnés ; les autres sont ignorés.
    func open(_ urls: [URL]) {
        pending.append(contentsOf: urls.filter(Self.isPDF))
        flush()
    }

    func presentOpenPanel() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.message = String(localized: "Choisissez un ou plusieurs documents PDF.")
        panel.begin { [weak self] response in
            guard response == .OK else { return }
            let urls = panel.urls
            Task { @MainActor in self?.open(urls) }
        }
    }

    static func isPDF(_ url: URL) -> Bool {
        UTType(filenameExtension: url.pathExtension)?.conforms(to: .pdf) ?? false
    }

    private func flush() {
        while let url = pending.first {
            let reference: DocumentReference
            do {
                reference = try DocumentReference(url: url)
            } catch {
                pending.removeFirst()
                let reason = error.localizedDescription
                logger.error("Signet impossible pour \(url.path, privacy: .private) : \(reason, privacy: .public)")
                presentAccessError(for: url)
                continue
            }
            if !emptyWindows.isEmpty {
                emptyWindows.removeFirst().adopt(reference)
            } else if let openWindow {
                openWindow(id: Self.windowID, value: reference)
            } else {
                return // Aucune fenêtre encore prête : on réessaiera à la prochaine.
            }
            pending.removeFirst()
        }
    }

    private func presentAccessError(for url: URL) {
        let alert = NSAlert()
        alert.messageText = String(localized: "Impossible d'ouvrir « \(url.lastPathComponent) ».")
        alert.informativeText = String(localized: "PDF Studio n'a pas obtenu l'accès à ce fichier.")
        alert.runModal()
    }
}
