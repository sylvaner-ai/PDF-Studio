import SwiftUI

/// Préparation du mode Édition, en bas de la fenêtre : recherche des pages
/// scannées, puis avancement de la reconnaissance de leur texte, avec le
/// bouton d'arrêt. La recherche, presque instantanée sur un document court,
/// ne s'affiche que si elle dure : le bandeau ne clignote pas à chaque entrée
/// en Édition.
struct RecognitionBadge: View {
    let progress: RecognitionProgress
    let stop: () -> Void

    /// Délai avant d'afficher la recherche des pages scannées.
    private static let searchDelay = Duration.milliseconds(400)

    @State private var showsSearch = false

    var body: some View {
        HStack(spacing: Design.Spacing.medium) {
            if progress.isSearching {
                ProgressView()
                    .controlSize(.small)
            } else {
                ProgressView(value: progress.fraction)
                    .frame(width: 120)
            }
            Text(status)
                .monospacedDigit()
            Button("Arrêter", action: stop)
                .disabled(progress.isStopping)
        }
        .padding(.horizontal, Design.Spacing.large)
        .padding(.vertical, Design.Spacing.small)
        .floatingBadge()
        .padding(.bottom, Design.Spacing.large)
        .accessibilityElement(children: .contain)
        .opacity(progress.isSearching && !showsSearch ? 0 : 1)
        .allowsHitTesting(!progress.isSearching || showsSearch)
        .task {
            try? await Task.sleep(for: Self.searchDelay)
            showsSearch = true
        }
    }

    private var status: String {
        if progress.isStopping {
            return progress.isSearching
                ? String(localized: "Arrêt de la recherche des pages scannées…")
                : String(localized: "Arrêt après la page en cours…")
        }
        return progress.isSearching
            ? String(localized: "Recherche des pages scannées…")
            : String(localized: "Reconnaissance du texte : page \(progress.done + 1) sur \(progress.total)")
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

/// Exportation en cours, en bas de la fenêtre.
struct ActivityBadge: View {
    let text: String

    var body: some View {
        HStack(spacing: Design.Spacing.small) {
            ProgressView()
                .controlSize(.small)
            Text(text)
        }
        .padding(.horizontal, Design.Spacing.large)
        .padding(.vertical, Design.Spacing.small)
        .floatingBadge()
        .padding(.bottom, Design.Spacing.large)
        .accessibilityElement(children: .combine)
    }
}
