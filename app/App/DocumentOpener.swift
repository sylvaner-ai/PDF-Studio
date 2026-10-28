import AppKit
import SwiftUI
import UniformTypeIdentifiers
import os

/// Centralise toutes les façons d'ouvrir un PDF : menu, glisser-déposer sur une
/// fenêtre, Finder et icône du Dock.
///
/// Chaque document a sa fenêtre AppKit, présentée comme un onglet (D66) : un
/// document ouvert rejoint, en dernier onglet, la fenêtre d'où vient la demande
/// (celle qui a reçu le dépôt, sinon la fenêtre au premier plan). Un écran
/// d'accueil encore vide de cette fenêtre accueille le premier document ouvert.
/// Un même fichier n'est jamais ouvert deux fois : son onglet est ramené au
/// premier plan. Un écran d'accueil ne reste ouvert à côté de documents que
/// s'il a été demandé (⌘T).
@MainActor
final class DocumentOpener {
    static let shared = DocumentOpener()
    static let windowID = "document"
    /// Identifiant commun des fenêtres de documents : AppKit ne réunit en onglets
    /// que des fenêtres qui le partagent.
    static let tabbingIdentifier = "fr.sylvainrobert.pdfstudio.document"

    private let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "ouverture")
    private var openWindow: OpenWindowAction?
    private var windows: [UUID: WindowEntry] = [:]
    private var pending: [DocumentReference] = []
    /// Vrai pendant `flush()`, qui peut être rappelée par la fenêtre qu'elle ouvre.
    private var isFlushing = false
    /// Fenêtres AppKit des fenêtres connues, repérées par ``WindowTabbing``.
    private var appKitWindows: [UUID: WeakWindow] = [:]
    /// Écrans d'accueil demandés (⌘T) ou déjà présents à cette demande : ils
    /// restent ouverts même si des documents sont affichés.
    private var keptWelcomes: Set<UUID> = []
    /// Fenêtres déclarées et reliées à leur fenêtre AppKit, dont la place a été
    /// décidée.
    private var settled: Set<UUID> = []
    /// Fenêtre dont la barre d'onglets accueille les prochaines fenêtres ouvertes.
    private weak var tabTarget: NSWindow?
    /// Fenêtres demandées qui ne se sont pas encore placées dans `tabTarget` :
    /// documents ouverts dans une nouvelle fenêtre, et onglets d'accueil (⌘T).
    /// Une autre fenêtre (celle que SwiftUI ouvre de lui-même quand le Finder
    /// lui confie un fichier, par exemple) n'est pas placée, et un écran
    /// d'accueil ainsi ouvert est refermé dès qu'un document est affiché.
    private var awaitingDocuments: [DocumentReference] = []
    private var awaitingWelcomes = 0
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

    private struct WeakWindow {
        weak var window: NSWindow?
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
        settle(id)
        flush()
        closeSurplusWelcomeWindows()
    }

    func unregisterWindow(id: UUID) {
        windows[id] = nil
        appKitWindows[id] = nil
        keptWelcomes.remove(id)
        settled.remove(id)
    }

    /// Relie une fenêtre déclarée à sa fenêtre AppKit, et la place dans la barre
    /// d'onglets de la fenêtre d'où vient la demande qui l'a ouverte.
    func attach(window: NSWindow, id: UUID) {
        let isNew = appKitWindows[id]?.window !== window
        appKitWindows[id] = WeakWindow(window: window)
        window.tabbingMode = .preferred
        window.tabbingIdentifier = Self.tabbingIdentifier
        WindowCloseGuard.install(on: window, model: nil)
        guard isNew else { return }
        settle(id)
        closeSurplusWelcomeWindows()
    }

    /// Une fois la fenêtre déclarée et reliée à sa fenêtre AppKit, la place en
    /// onglet si elle a été demandée.
    private func settle(_ id: UUID) {
        guard !settled.contains(id), let entry = windows[id], let window = appKitWindows[id]?.window else { return }
        settled.insert(id)
        if let reference = entry.reference, let index = awaitingDocuments.firstIndex(of: reference) {
            awaitingDocuments.remove(at: index)
            placeAsTab(window)
        } else if entry.reference == nil, awaitingWelcomes > 0 {
            awaitingWelcomes -= 1
            keptWelcomes.insert(id)
            placeAsTab(window)
        }
    }

    /// Vrai si une fenêtre affiche ce fichier.
    func isShown(path: String) -> Bool {
        windows.values.contains { $0.reference?.path == path }
    }

    /// Fenêtre AppKit qui affiche ce fichier.
    func window(showing path: String) -> NSWindow? {
        windows.first { $0.value.reference?.path == path }.flatMap { appKitWindows[$0.key]?.window }
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

    /// Ouvre les PDF parmi les fichiers donnés, en onglets de `window` (par
    /// défaut, la fenêtre au premier plan) ; les autres fichiers sont ignorés.
    func open(_ urls: [URL], besides window: NSWindow? = nil) {
        var references: [DocumentReference] = []
        for url in urls where Self.isPDF(url) {
            do {
                references.append(try DocumentReference(url: url))
            } catch {
                let reason = error.localizedDescription
                logger.error("Signet impossible pour \(url.path, privacy: .private) : \(reason, privacy: .public)")
                presentAccessError(for: url)
            }
        }
        open(references, besides: window)
    }

    func open(_ references: [DocumentReference], besides window: NSWindow? = nil) {
        guard !references.isEmpty else { return }
        tabTarget = window ?? frontWindow
        pending.append(contentsOf: references)
        flush()
    }

    /// Ouvre un écran d'accueil dans un nouvel onglet de `window` (par défaut, la
    /// fenêtre au premier plan) ; sans fenêtre, dans une nouvelle fenêtre.
    func openNewTab(besides window: NSWindow? = nil) {
        guard let openWindow else { return }
        tabTarget = window ?? frontWindow
        awaitingWelcomes += 1
        // Les onglets d'accueil déjà ouverts sont voulus eux aussi.
        keptWelcomes.formUnion(windows.filter { $0.value.reference == nil }.keys)
        openWindow(id: Self.windowID)
    }

    func presentOpenPanel() {
        let target = frontWindow
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.message = String(localized: "Choisissez un ou plusieurs documents PDF.")
        panel.begin { [weak self] response in
            guard response == .OK else { return }
            let urls = panel.urls
            Task { @MainActor in self?.open(urls, besides: target) }
        }
    }

    /// Fenêtre de document au premier plan : la fenêtre active (ou celle d'une
    /// alerte affichée en feuille), sinon la plus haute à l'écran.
    private var frontWindow: NSWindow? {
        let known = appKitWindows.values.compactMap(\.window)
        let candidates = [NSApp.keyWindow, NSApp.mainWindow].compactMap { $0 } + NSApp.orderedWindows
        return candidates.first { candidate in known.contains { $0 === candidate } }
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

        while let reference = pending.first {
            let welcome = availableWelcome
            guard welcome != nil || openWindow != nil else {
                return // Aucune fenêtre encore prête : on réessaiera à la prochaine.
            }
            pending.removeFirst()
            if let shown = windows.first(where: { $0.value.reference == reference }) {
                // Onglet qui affiche déjà le document, ramené devant.
                if let window = appKitWindows[shown.key]?.window {
                    window.makeKeyAndOrderFront(nil)
                } else {
                    openWindow?(id: Self.windowID, value: reference)
                }
            } else if let welcome, let entry = windows[welcome] {
                windows[welcome]?.reference = reference
                entry.adopt(reference)
                appKitWindows[welcome]?.window?.makeKeyAndOrderFront(nil)
            } else if let openWindow {
                awaitingDocuments.append(reference)
                openWindow(id: Self.windowID, value: reference)
            }
        }
    }

    /// Écran d'accueil qui accueille le prochain document : celui de la fenêtre
    /// d'où vient la demande, ou un autre onglet de cette fenêtre ; sans fenêtre
    /// d'origine, n'importe lequel.
    private var availableWelcome: UUID? {
        let welcomes = windows.filter { $0.value.reference == nil }.map(\.key)
        guard let target = tabTarget else { return welcomes.first }
        let tabs = target.tabbedWindows ?? [target]
        return welcomes.first { appKitWindows[$0]?.window === target }
            ?? welcomes.first { id in tabs.contains { $0 === appKitWindows[id]?.window } }
    }

    /// Place une fenêtre en dernier onglet de la fenêtre d'où vient la demande,
    /// si AppKit ne l'y a pas déjà mise.
    private func placeAsTab(_ window: NSWindow) {
        guard let target = tabTarget, target !== window,
              appKitWindows.values.contains(where: { $0.window === target }) else { return }
        let tabs = target.tabbedWindows ?? [target]
        if !tabs.contains(where: { $0 === window }), let last = tabs.last {
            last.addTabbedWindow(window, ordered: .above)
        }
        window.makeKeyAndOrderFront(nil)
    }

    /// Ferme les écrans d'accueil vides que personne n'a demandés alors qu'un
    /// document est affiché : la fenêtre que SwiftUI crée au lancement en plus
    /// des fenêtres restaurées, ou celle qu'il ouvre quand le Finder lui confie
    /// un fichier. Un écran d'accueil n'est jugé qu'une fois sa place décidée.
    private func closeSurplusWelcomeWindows() {
        guard windows.values.contains(where: { $0.reference != nil }) else { return }
        let surplus = windows.filter { id, entry in
            entry.reference == nil && settled.contains(id) && !keptWelcomes.contains(id)
        }
        for (id, entry) in surplus {
            let window = appKitWindows[id]?.window
            unregisterWindow(id: id)
            if let window {
                closeWhenShown(window)
            } else {
                entry.close()
            }
        }
    }

    /// Ferme une fenêtre que SwiftUI est peut-être encore en train de montrer :
    /// fermée pendant sa déclaration, elle serait remise à l'écran juste après,
    /// vide et inconnue. La fermeture attend donc la fin de l'affichage, et elle
    /// est refaite tant que la fenêtre reste ouverte.
    private func closeWhenShown(_ window: NSWindow) {
        let logger = logger
        Task { @MainActor in
            for delay in [50, 250, 1000] {
                try? await Task.sleep(for: .milliseconds(delay))
                guard window.isVisible || window.tabbedWindows != nil else {
                    logger.notice("Écran d'accueil vide fermé : un document est affiché.")
                    return
                }
                window.close()
            }
            if window.isVisible || window.tabbedWindows != nil {
                logger.error("Un écran d'accueil vide est resté ouvert.")
            } else {
                logger.notice("Écran d'accueil vide fermé : un document est affiché.")
            }
        }
    }

    private func presentAccessError(for url: URL) {
        let alert = NSAlert()
        alert.messageText = String(localized: "Impossible d'ouvrir « \(url.lastPathComponent) ».")
        alert.informativeText = String(localized: "PDF Studio n'a pas obtenu l'accès à ce fichier.")
        alert.runModal()
    }
}

/// Reconnaissance des PDF, et fichiers reçus par glisser-déposer.
extension DocumentOpener {
    nonisolated static func isPDF(_ url: URL) -> Bool {
        UTType(filenameExtension: url.pathExtension)?.conforms(to: .pdf) ?? false
    }

    /// Fichiers PDF d'un glisser-déposer.
    static func pdfs(on pasteboard: NSPasteboard) -> [URL] {
        let objects = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
        return (objects as? [URL] ?? []).filter(isPDF)
    }

    /// Ouvre en onglets les PDF déposés sur une vue SwiftUI (liste des vignettes).
    func open(_ providers: [NSItemProvider]) {
        for provider in providers where provider.canLoadObject(ofClass: URL.self) {
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url, Self.isPDF(url) else { return }
                Task { @MainActor in DocumentOpener.shared.open([url]) }
            }
        }
    }
}
