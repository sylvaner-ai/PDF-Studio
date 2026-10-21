import SwiftUI

/// Contenu d'une fenêtre : l'écran d'accueil tant qu'aucun document n'est
/// ouvert, puis le document.
struct DocumentWindow: View {
    @Binding var reference: DocumentReference?
    @Environment(\.openWindow) private var openWindow
    @State private var windowID = UUID()

    var body: some View {
        Group {
            if let reference {
                DocumentView(reference: reference)
                    .id(reference)
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
            DocumentOpener.shared.register(openWindow: openWindow)
            updateRegistration()
        }
        .onChange(of: reference) { updateRegistration() }
        .onDisappear {
            DocumentOpener.shared.unregisterEmptyWindow(id: windowID)
        }
    }

    /// Une fenêtre vide se propose pour accueillir le prochain document ouvert.
    private func updateRegistration() {
        if reference == nil {
            DocumentOpener.shared.registerEmptyWindow(id: windowID) { newReference in
                reference = newReference
            }
        } else {
            DocumentOpener.shared.unregisterEmptyWindow(id: windowID)
        }
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
