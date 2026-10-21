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
        .defaultSize(width: 920, height: 1040)
        .commands {
            FileCommands()
        }
    }
}

/// Reçoit les fichiers ouverts depuis le Finder ou déposés sur l'icône du Dock.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        DocumentOpener.shared.open(urls)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
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
