import SwiftUI

/// Point d'entrée de PDF Studio.
///
/// Chaque fenêtre affiche un document, identifié par sa ``DocumentReference``.
/// Une fenêtre sans document affiche l'écran d'accueil.
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
        }
    }
}

/// Reçoit les fichiers ouverts depuis le Finder ou déposés sur l'icône du Dock.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Copies d'ouverture laissées par un arrêt imprévu.
        if let store = try? RecoveryStore.standard() {
            store.removeUnusedBases()
        }
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        DocumentOpener.shared.open(urls)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    /// Prévient avant de quitter si des modifications ne sont pas enregistrées :
    /// elles sont retenues et seront proposées à la prochaine ouverture.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard DocumentOpener.shared.hasUnsavedChanges else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = String(localized: "Des documents ont des modifications non enregistrées.")
        alert.informativeText = String(
            localized: "Elles vous seront proposées à la prochaine ouverture de ces documents."
        )
        alert.addButton(withTitle: String(localized: "Annuler"))
        alert.addButton(withTitle: String(localized: "Quitter"))
        return alert.runModal() == .alertSecondButtonReturn ? .terminateNow : .terminateCancel
    }
}

/// Menu Fichier : remplace « Nouveau » par « Ouvrir… ».
struct FileCommands: Commands {
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Ouvrir…") {
                DocumentOpener.shared.register(openWindow: openWindow)
                DocumentOpener.shared.presentOpenPanel()
            }
            .keyboardShortcut("o")
        }
    }
}
