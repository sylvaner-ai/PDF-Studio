import AppKit
import SwiftUI
import UniformTypeIdentifiers
import os

/// Centralise toutes les façons d'ouvrir un PDF : menu, glisser-déposer sur une
/// fenêtre, Finder et icône du Dock.
///
/// Une fenêtre d'accueil encore vide accueille le premier document ouvert ; les
/// suivants ouvrent une nouvelle fenêtre. Un même fichier n'est jamais ouvert
/// deux fois : sa fenêtre est ramenée au premier plan. Une fenêtre d'accueil ne
/// reste ouverte que si aucun document n'est affiché.
@MainActor
final class DocumentOpener {
    static let shared = DocumentOpener()
    static let windowID = "document"

    private let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "ouverture")
    private var openWindow: OpenWindowAction?
    private var windows: [UUID: WindowEntry] = [:]
    private var pending: [URL] = []
    /// Vrai pendant `flush()`, qui peut être rappelée par la fenêtre qu'elle ouvre.
    private var isFlushing = false
    /// Documents dont des modifications ne sont pas enregistrées, et si elles
    /// pourront être proposées à leur prochaine ouverture.
    private var unsaved: [ObjectIdentifier: Bool] = [:]
    /// Enregistrements en cours, et ce qui attend leur fin (quitter l'application).
    private var savesInProgress = 0
    private var afterSaves: [() -> Void] = []

    /// Fenêtre connue : son document (`nil` pour l'écran d'accueil), et de quoi
    /// lui en confier un ou la fermer.
    private struct WindowEntry {
        var reference: DocumentReference?
        let adopt: (DocumentReference) -> Void
        let close: () -> Void
    }

    private init() {}

    /// Mémorise l'action SwiftUI qui ouvre une fenêtre.
    func register(openWindow: OpenWindowAction) {
        self.openWindow = openWindow
        flush()
    }

    /// Déclare une fenêtre et le document qu'elle affiche ; sans document, elle
    /// accueillera le prochain ouvert.
    func registerWindow(
        id: UUID,
        reference: DocumentReference?,
        adopt: @escaping (DocumentReference) -> Void,
        close: @escaping () -> Void
    ) {
        if let reference, windows.contains(where: { $0.key != id && $0.value.reference == reference }) {
            // Document déjà affiché dans une autre fenêtre (restauration au lancement).
            windows[id] = nil
            close()
            return
        }
        windows[id] = WindowEntry(reference: reference, adopt: adopt, close: close)
        flush()
        closeSurplusWelcomeWindows()
    }

    func unregisterWindow(id: UUID) {
        windows[id] = nil
    }

    /// Vrai si une fenêtre affiche ce fichier.
    func isShown(path: String) -> Bool {
        windows.values.contains { $0.reference?.path == path }
    }

    /// Retient qu'un document a, ou n'a plus, des modifications non enregistrées.
    func setUnsaved(_ isUnsaved: Bool, recoverable: Bool = true, for document: AnyObject) {
        unsaved[ObjectIdentifier(document)] = isUnsaved ? recoverable : nil
    }

    var hasUnsavedChanges: Bool { !unsaved.isEmpty }

    /// Vrai si des modifications non enregistrées ne pourront pas être retrouvées.
    var hasUnrecoverableChanges: Bool { unsaved.values.contains(false) }

    /// Un enregistrement commence : interrompu, il laisserait le fichier à moitié écrit.
    func beginSave() {
        savesInProgress += 1
    }

    func endSave() {
        savesInProgress = max(savesInProgress - 1, 0)
        guard savesInProgress == 0 else { return }
        let actions = afterSaves
        afterSaves = []
        actions.forEach { $0() }
    }

    var isSaving: Bool { savesInProgress > 0 }

    /// Exécute `action` une fois tous les enregistrements terminés.
    func whenSavesFinish(_ action: @escaping () -> Void) {
        if isSaving {
            afterSaves.append(action)
        } else {
            action()
        }
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

    /// Confie les fichiers en attente aux fenêtres disponibles.
    ///
    /// Ouvrir ou remplir une fenêtre peut rappeler aussitôt `register` ou
    /// `registerWindow`, donc cette méthode : l'appel imbriqué ne fait rien,
    /// et la boucle en cours tient compte de la nouvelle fenêtre. Chaque fichier est
    /// retiré de la file avant d'être confié.
    private func flush() {
        guard !isFlushing else { return }
        isFlushing = true
        defer { isFlushing = false }

        while let url = pending.first {
            let hasWelcome = windows.values.contains { $0.reference == nil }
            guard hasWelcome || openWindow != nil else {
                return // Aucune fenêtre encore prête : on réessaiera à la prochaine.
            }
            pending.removeFirst()
            let reference: DocumentReference
            do {
                reference = try DocumentReference(url: url)
            } catch {
                let reason = error.localizedDescription
                logger.error("Signet impossible pour \(url.path, privacy: .private) : \(reason, privacy: .public)")
                presentAccessError(for: url)
                continue
            }
            let isShown = windows.values.contains { $0.reference == reference }
            if !isShown, let welcome = windows.first(where: { $0.value.reference == nil }) {
                windows[welcome.key]?.reference = reference
                welcome.value.adopt(reference)
            } else if let openWindow {
                // Nouvelle fenêtre, ou celle qui affiche déjà le document, ramenée devant.
                openWindow(id: Self.windowID, value: reference)
            }
        }
    }

    /// Ferme les écrans d'accueil restés vides alors qu'un document est affiché,
    /// comme la fenêtre que SwiftUI crée au lancement en plus des fenêtres restaurées.
    private func closeSurplusWelcomeWindows() {
        guard windows.values.contains(where: { $0.reference != nil }) else { return }
        let surplus = windows.filter { $0.value.reference == nil }
        for (id, window) in surplus {
            windows[id] = nil
            window.close()
        }
    }

    private func presentAccessError(for url: URL) {
        let alert = NSAlert()
        alert.messageText = String(localized: "Impossible d'ouvrir « \(url.lastPathComponent) ».")
        alert.informativeText = String(localized: "PDF Studio n'a pas obtenu l'accès à ce fichier.")
        alert.runModal()
    }
}
