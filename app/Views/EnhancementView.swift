import SwiftUI

/// Mode Amélioration : aperçu avant/après de la page affichée, à gauche, et
/// panneau des réglages, à droite (cf. ``PageEnhancement``).
struct EnhancementView: View {
    let model: DocumentViewModel
    @Bindable var enhancement: PageEnhancement

    var body: some View {
        HStack(spacing: 0) {
            EnhancementPreviewArea(model: model, enhancement: enhancement)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            EnhancementPanel(model: model, enhancement: enhancement)
                .frame(width: 300)
        }
    }
}

/// Aperçu de la page affichée, avec ses boutons de navigation.
private struct EnhancementPreviewArea: View {
    let model: DocumentViewModel
    let enhancement: PageEnhancement

    var body: some View {
        VStack(spacing: Design.Spacing.medium) {
            ZStack {
                if let preview = shownPreview {
                    BeforeAfterView(before: preview.before.cgImage, after: preview.after.cgImage)
                } else if let failure = enhancement.previewFailure {
                    Text(failure)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                } else {
                    ProgressView()
                        .controlSize(.small)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .overlay(alignment: .topTrailing) {
                if enhancement.isPreviewing, shownPreview != nil {
                    ProgressView()
                        .controlSize(.small)
                        .padding(Design.Spacing.medium)
                }
            }
            navigation
        }
        .padding(Design.Spacing.large)
        .task(id: page.map(enhancement.key(for:))) {
            if let page {
                await enhancement.loadPreview(of: page)
            }
        }
    }

    private var page: DisplayPage? {
        model.pages[safe: model.canvas.currentPage]
    }

    /// Aperçu de la page affichée ; celui d'une autre page n'est pas montré.
    private var shownPreview: PageEnhancement.Preview? {
        guard let preview = enhancement.preview, preview.key.page == page?.id else { return nil }
        return preview
    }

    private var navigation: some View {
        HStack(spacing: Design.Spacing.medium) {
            Button { model.canvas.go(to: model.canvas.currentPage - 1) } label: {
                Image(systemName: "chevron.left")
            }
            .help(Text("Page précédente"))
            .disabled(model.canvas.currentPage == 0)
            Text("Page \(model.canvas.currentPage + 1) sur \(model.pageCount)")
                .monospacedDigit()
                .foregroundStyle(.secondary)
            Button { model.canvas.go(to: model.canvas.currentPage + 1) } label: {
                Image(systemName: "chevron.right")
            }
            .help(Text("Page suivante"))
            .disabled(model.canvas.currentPage + 1 >= model.pageCount)
        }
        .buttonStyle(.borderless)
    }
}

/// Deux images de même format superposées : l'image « après » à gauche du
/// curseur, l'image « avant » à droite ; le curseur se fait glisser.
struct BeforeAfterView: View {
    let before: CGImage
    let after: CGImage

    /// Position du curseur, de 0 (tout « avant ») à 1 (tout « après »).
    @State private var split: CGFloat = 0.5

    var body: some View {
        GeometryReader { proxy in
            let frame = fitted(in: proxy.size)
            ZStack(alignment: .topLeading) {
                image(before)
                image(after)
                    .mask(alignment: .leading) {
                        Rectangle().frame(width: frame.width * split)
                    }
                Rectangle()
                    .fill(Color.accentColor)
                    .frame(width: 2, height: frame.height)
                    .offset(x: frame.width * split - 1)
                    .allowsHitTesting(false)
                labels(width: frame.width)
            }
            .frame(width: frame.width, height: frame.height)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0).onChanged { value in
                    split = min(max(value.location.x / max(frame.width, 1), 0), 1)
                }
            )
            .shadow(color: .black.opacity(0.15), radius: 3, y: 1)
            .position(x: proxy.size.width / 2, y: proxy.size.height / 2)
            .accessibilityElement()
            .accessibilityLabel(Text("Aperçu avant et après"))
            .accessibilityValue(Text("\(Int((split * 100).rounded())) % de la page améliorée"))
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: split = min(split + 0.1, 1)
                case .decrement: split = max(split - 0.1, 0)
                @unknown default: break
                }
            }
        }
    }

    private func image(_ image: CGImage) -> some View {
        Image(decorative: image, scale: 1)
            .resizable()
            .interpolation(.high)
    }

    private func labels(width: CGFloat) -> some View {
        HStack {
            tag("Après")
            Spacer()
            tag("Avant")
        }
        .frame(width: width)
        .padding(.top, Design.Spacing.small)
        .allowsHitTesting(false)
    }

    private func tag(_ text: LocalizedStringKey) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, Design.Spacing.small)
            .padding(.vertical, 2)
            .floatingBadge()
            .padding(.horizontal, Design.Spacing.small)
    }

    /// Format de l'image ajusté à la place disponible.
    private func fitted(in size: CGSize) -> CGSize {
        let width = CGFloat(after.width), height = CGFloat(after.height)
        guard width > 0, height > 0 else { return .zero }
        let scale = min(size.width / width, size.height / height)
        return CGSize(width: width * scale, height: height * scale)
    }
}

/// Réglages du mode Amélioration, bilan de la page affichée et actions.
private struct EnhancementPanel: View {
    let model: DocumentViewModel
    @Bindable var enhancement: PageEnhancement

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Design.Spacing.large) {
                Text("Amélioration des pages scannées")
                    .font(.headline)
                VStack(alignment: .leading, spacing: Design.Spacing.small) {
                    Toggle("Redresser les pages inclinées", isOn: $enhancement.settings.deskew)
                    Toggle("Remettre à l'endroit les pages couchées ou retournées",
                           isOn: $enhancement.settings.orientation)
                }
                Picker("Pages", selection: $enhancement.range) {
                    Text("Toutes").tag(PageEnhancement.Range.all)
                    Text("Sélection").tag(PageEnhancement.Range.selection)
                    Text("Page affichée").tag(PageEnhancement.Range.current)
                }
                .pickerStyle(.segmented)
                advanced
                if let report = currentReport {
                    Label(report.summary, systemImage: report.status == .toCheck
                          ? "exclamationmark.triangle" : "info.circle")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack {
                    Button("Appliquer") { enhancement.apply() }
                        .buttonStyle(.borderedProminent)
                        .disabled(!enhancement.canApply)
                        .help(Text("Redresser les pages choisies ; ⌘Z pour annuler"))
                    Button("Rétablir l'original") { enhancement.restoreOriginals() }
                        .disabled(!enhancement.canRestore)
                        .help(Text("Rendre aux pages choisies leur version d'origine"))
                }
                if !pagesToCheck.isEmpty {
                    toCheck
                }
            }
            .padding(Design.Spacing.large)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var advanced: some View {
        DisclosureGroup("Réglages avancés") {
            VStack(alignment: .leading, spacing: Design.Spacing.small) {
                angleStepper("Seuil", value: $enhancement.settings.minAngle, range: 0...5, step: 0.05,
                             help: "Inclinaison sous laquelle une page est laissée telle quelle")
                angleStepper("Plafond", value: $enhancement.settings.maxAngle,
                             range: enhancement.settings.minAngle...45, step: 1,
                             help: "Au-delà, la page n'est pas redressée et est marquée « à vérifier »")
                Stepper(value: $enhancement.settings.jpegQuality, in: 50...100, step: 5) {
                    Text("Qualité de l'image : \(enhancement.settings.jpegQuality)")
                        .monospacedDigit()
                }
                .help(Text("Qualité JPEG de l'image améliorée"))
                Button("Réglages par défaut") { enhancement.resetSettings() }
                    .disabled(enhancement.settings == defaultEnhancementSettings())
            }
            .padding(.top, Design.Spacing.small)
        }
    }

    private func angleStepper(
        _ title: LocalizedStringKey, value: Binding<Float>, range: ClosedRange<Float>, step: Float, help: String
    ) -> some View {
        Stepper(value: value, in: range, step: step) {
            HStack {
                Text(title)
                Spacer()
                Text("\(value.wrappedValue.formatted(.number.precision(.fractionLength(2))))°")
                    .monospacedDigit()
            }
        }
        .help(Text(help))
    }

    /// Bilan de la page affichée, d'après son aperçu.
    private var currentReport: EnhancementReport? {
        guard let preview = enhancement.preview,
              preview.key.page == model.pages[safe: model.canvas.currentPage]?.id else { return nil }
        return preview.report
    }

    /// Page « à vérifier » encore présente, avec sa position.
    private struct CheckedPage: Identifiable {
        let id: UInt64
        let position: Int
    }

    private var pagesToCheck: [CheckedPage] {
        enhancement.pagesToCheck.compactMap { id in
            model.pages.firstIndex { $0.id == id }.map { CheckedPage(id: id, position: $0) }
        }
    }

    private var toCheck: some View {
        VStack(alignment: .leading, spacing: Design.Spacing.small) {
            Label("Pages à vérifier", systemImage: "exclamationmark.triangle")
                .font(.subheadline.weight(.semibold))
            Text("Inclinaison au-delà du plafond : ces pages n'ont pas été redressées.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(pagesToCheck) { page in
                Button("Page \(page.position + 1)") { model.canvas.go(to: page.position) }
                    .buttonStyle(.link)
            }
        }
    }
}

/// Avancement du traitement du mode Amélioration, en bas de la fenêtre, avec
/// le bouton d'arrêt.
struct EnhancementBadge: View {
    let progress: PageEnhancement.Progress
    let stop: () -> Void

    var body: some View {
        HStack(spacing: Design.Spacing.medium) {
            ProgressView(value: progress.fraction)
                .frame(width: 120)
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
    }

    private var status: String {
        if progress.isStopping {
            return String(localized: "Arrêt après la page en cours…")
        }
        let page = min(progress.done + 1, progress.total)
        return String(localized: "Amélioration : page \(page) sur \(progress.total)")
    }
}
