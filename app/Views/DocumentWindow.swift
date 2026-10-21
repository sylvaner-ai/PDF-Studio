import SwiftUI

/// Contenu d'une fenêtre : l'écran d'accueil tant qu'aucun document n'est
/// ouvert, puis le document.
struct DocumentWindow: View {
    @Binding var reference: DocumentReference?
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var windowID = UUID()
    /// Change quand la fenêtre accueille un autre document, et pas quand son
    /// document change seulement de fichier (« Enregistrer sous »).
    @State private var documentID = UUID()

    var body: some View {
        Group {
            if let reference {
                DocumentView(reference: reference, onRelocate: { self.reference = $0 })
                    .id(documentID)
            } else {
                WelcomeView()
                    .navigationTitle("PDF Studio")
            }
        }
        .frame(minWidth: 480, minHeight: 360)
        .dropDestination(for: URL.self) { urls, _ in
            let pdfs = urls.filter(DocumentOpener.isPDF)
            DocumentOpener.shared.open(pdfs)
            return !pdfs.isEmpty
        }
        .onAppear {
            updateRegistration()
            DocumentOpener.shared.register(openWindow: openWindow)
        }
        .onChange(of: reference) { updateRegistration() }
        .onDisappear {
            DocumentOpener.shared.unregisterWindow(id: windowID)
        }
    }

    /// Déclare la fenêtre et son document ; vide, elle se propose pour accueillir
    /// le prochain document ouvert.
    private func updateRegistration() {
        DocumentOpener.shared.registerWindow(
            id: windowID,
            reference: reference,
            adopt: {
                documentID = UUID()
                reference = $0
            },
            close: { dismissWindow() }
        )
    }
}

/// Écran d'accueil : un seul message discret.
struct WelcomeView: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "doc")
                .font(.system(size: 40, weight: .ultraLight))
                .foregroundStyle(.tertiary)
            Text("Déposez un PDF ici")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .underPageBackgroundColor))
        .accessibilityElement(children: .combine)
        .accessibilityHint("Ou choisissez Fichier, Ouvrir.")
    }
}
