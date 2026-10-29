import SwiftUI

/// Point d'entrée de PDF Studio.
///
/// Chaque fenêtre affiche un document, identifié par sa ``DocumentReference`` ;
/// les fenêtres se présentent comme les onglets d'une même fenêtre (D66). Une
/// fenêtre sans document affiche l'écran d'accueil.
@main
struct PDFStudioApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup(id: DocumentOpener.windowID, for: DocumentReference.self) { $reference in
            DocumentWindow(reference: $reference)
        }
        .defaultSize(width: 1080, height: 1040)
        .commands {
            FileCommands()
            SaveCommands()
            SidebarCommands()
            ViewerCommands()
            PageCommands()
            InsertionCommands()
            ObjectCommands()
            TableCommands()
        }
    }
}

/// Reçoit les fichiers ouverts depuis le Finder ou déposés sur l'icône du Dock.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Copies d'ouverture laissées par un arrêt imprévu.
        if let store = try? RecoveryStore.standard() {
            store.removeUnusedCopies()
        }
        BundledFonts.register()
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        DocumentOpener.shared.open(urls)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    /// Les modifications non enregistrées qui attendent d'être retenues sont
    /// écrites avant de quitter.
    func applicationWillTerminate(_ notification: Notification) {
        RecoveryWriter.shared.flush()
    }

    /// Prévient avant de quitter si des modifications ne sont pas enregistrées :
    /// elles sont retenues et seront proposées à la prochaine ouverture. Un
    /// enregistrement en cours est mené à son terme avant de quitter.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !DocumentOpener.shared.hasUnsavedChanges || confirmQuit() else { return .terminateCancel }
        guard DocumentOpener.shared.isSaving else { return .terminateNow }
        DocumentOpener.shared.whenSavesFinish { NSApp.reply(toApplicationShouldTerminate: true) }
        return .terminateLater
    }

    @MainActor
    private func confirmQuit() -> Bool {
        let alert = NSAlert()
        alert.messageText = String(localized: "Des documents ont des modifications non enregistrées.")
        alert.addButton(withTitle: String(localized: "Annuler"))
        if DocumentOpener.shared.hasUnrecoverableChanges {
            alert.informativeText = String(
                localized: "Certaines ne pourront pas être retrouvées : enregistrez-les avant de quitter."
            )
            alert.addButton(withTitle: String(localized: "Quitter sans enregistrer"))
        } else {
            alert.informativeText = String(
                localized: "Elles vous seront proposées à la prochaine ouverture de ces documents."
            )
            alert.addButton(withTitle: String(localized: "Quitter"))
        }
        return alert.runModal() == .alertSecondButtonReturn
    }
}

/// Menu Fichier : remplace « Nouveau » par « Nouvel onglet » et « Ouvrir… »,
/// qui ouvrent dans un nouvel onglet de la fenêtre au premier plan (D66).
struct FileCommands: Commands {
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Nouvel onglet") {
                DocumentOpener.shared.register(openWindow: openWindow)
                DocumentOpener.shared.openNewTab()
            }
            .keyboardShortcut("t")
            Button("Ouvrir…") {
                DocumentOpener.shared.register(openWindow: openWindow)
                DocumentOpener.shared.presentOpenPanel()
            }
            .keyboardShortcut("o")
        }
    }
}
