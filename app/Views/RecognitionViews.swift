import SwiftUI

/// Bouton « Reconnaître le texte » de la barre d'outils : la page lue, les
/// pages sélectionnées, les pages scannées ou tout le document.
struct RecognitionMenu: View {
    let model: DocumentViewModel

    var body: some View {
        Menu {
            Button("Pages scannées du document") { model.recognizeText(.scannedPages) }
            Button("Page courante") { model.recognizeText(.currentPage) }
            Button("Pages sélectionnées") { model.recognizeText(.selection) }
                .disabled(model.selectionCount == 0)
            Divider()
            Button("Tout le document") { model.recognizeText(.allPages) }
        } label: {
            Label("Reconnaître le texte", systemImage: "text.viewfinder")
        } primaryAction: {
            model.recognizeText(.scannedPages)
        }
        .help(Text("Reconnaître le texte des pages scannées (OCR), pour le rendre cherchable"))
        .disabled(!model.canEdit)
    }
}

/// Avancement de la reconnaissance du texte, en bas de la fenêtre, avec le
/// bouton d'arrêt.
struct RecognitionBadge: View {
    let progress: RecognitionProgress
    let stop: () -> Void

    var body: some View {
        HStack(spacing: Design.Spacing.medium) {
            ProgressView(value: progress.fraction)
                .frame(width: 120)
            Text(progress.isStopping
                 ? String(localized: "Arrêt après la page en cours…")
                 : String(localized: "Reconnaissance du texte : page \(progress.done + 1) sur \(progress.total)"))
                .monospacedDigit()
            Button("Arrêter", action: stop)
                .disabled(progress.isStopping)
        }
        .padding(.horizontal, Design.Spacing.large)
        .padding(.vertical, Design.Spacing.small)
        .floatingBadge()
        .padding(.bottom, Design.Spacing.large)
        .accessibilityElement(children: .contain)
    }
}

/// Information passagère, en bas de la fenêtre.
struct NoticeBadge: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "text.viewfinder")
            .padding(.horizontal, Design.Spacing.large)
            .padding(.vertical, Design.Spacing.small)
            .floatingBadge()
            .padding(.bottom, Design.Spacing.large)
            .allowsHitTesting(false)
    }
}
