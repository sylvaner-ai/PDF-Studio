import SwiftUI

/// Réglages du texte réécrit d'une ligne scannée, en mode Édition : police
/// (parmi les trois retrouvées sur le scan, avec leur ressemblance), corps,
/// graisse, interlettrage, flou, grain et couleur.
///
/// Un réglage s'applique quand le curseur est relâché ; chacun s'annule par ⌘Z.
struct ScanRetouchPanel: View {
    let editing: PageEditing
    let scan: ScanTextItem

    /// Réglages affichés, appliqués au relâchement d'un curseur.
    @State private var draft: ScanStyleItem
    /// Application différée d'une couleur choisie dans le nuancier.
    @State private var colorTask: Task<Void, Never>?

    init(editing: PageEditing, scan: ScanTextItem) {
        self.editing = editing
        self.scan = scan
        _draft = State(initialValue: scan.style)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Retouche du texte scanné")
                .font(.headline)
            fontPicker
            slider(
                "Corps", value: \.size,
                range: scan.measured.size * 0.6...scan.measured.size * 1.6, step: 0.1, unit: "pt"
            )
            slider("Graisse", value: \.weight, range: -1...1, step: 0.05)
            slider(
                "Interlettrage", value: \.spacing,
                range: scan.measured.spacing - 1...scan.measured.spacing + 3, step: 0.05, unit: "pt"
            )
            slider("Flou", value: \.blur, range: 0...max(2, scan.measured.blur * 3), step: 0.05, unit: "pt")
            slider("Grain", value: \.noise, range: 0...40, step: 1)
            ColorPicker("Couleur", selection: color, supportsOpacity: false)
            HStack {
                Button("Réglages mesurés") { editing.resetScanStyle() }
                    .disabled(scan.style == scan.measured)
                    .help(Text("Revenir à la police et aux réglages retrouvés sur le scan"))
                Spacer()
            }
        }
        .controlSize(.small)
        .padding(12)
        .frame(width: 270)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
        .padding(12)
        .onChange(of: scan.style) { _, style in
            draft = style
        }
        .onDisappear { colorTask?.cancel() }
    }

    // MARK: - Police

    private var fontPicker: some View {
        Picker("Police", selection: candidate) {
            ForEach(Array(scan.candidates.enumerated()), id: \.offset) { index, candidate in
                Text(Self.title(of: candidate)).tag(index)
            }
            if scan.chosenCandidate == nil {
                Text(scan.style.family).tag(-1)
            }
        }
        .help(Text("Polices les plus proches du scan, avec leur ressemblance"))
    }

    private var candidate: Binding<Int> {
        Binding(
            get: { scan.chosenCandidate ?? -1 },
            set: { index in
                guard index >= 0 else { return }
                editing.chooseScanCandidate(index)
            }
        )
    }

    private static func title(of candidate: ScanCandidateItem) -> String {
        let score = Double(candidate.score).formatted(.percent.precision(.fractionLength(0)))
        return String(localized: "\(candidate.family) — \(score)")
    }

    // MARK: - Réglages

    private func slider(
        _ title: LocalizedStringKey, value: WritableKeyPath<ScanStyleItem, Float>,
        range: ClosedRange<Float>, step: Float, unit: String? = nil
    ) -> some View {
        let binding = Binding(
            get: { min(max(draft[keyPath: value], range.lowerBound), range.upperBound) },
            set: { draft[keyPath: value] = $0 }
        )
        let number = Double(draft[keyPath: value]).formatted(.number.precision(.fractionLength(0...2)))
        return VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title)
                Spacer()
                Text(unit.map { "\(number) \($0)" } ?? number)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .font(.caption)
            Slider(value: binding, in: range, step: step) { Text(title) } onEditingChanged: { active in
                if !active {
                    editing.setScanStyle(draft)
                }
            }
            .labelsHidden()
        }
    }

    /// Couleur du texte ; le nuancier change la couleur en continu : elle
    /// s'applique quand elle cesse de changer.
    private var color: Binding<CGColor> {
        Binding(
            get: {
                CGColor(srgbRed: CGFloat(draft.red), green: CGFloat(draft.green), blue: CGFloat(draft.blue), alpha: 1)
            },
            set: { color in
                guard let srgb = CGColorSpace(name: CGColorSpace.sRGB),
                      let converted = color.converted(to: srgb, intent: .defaultIntent, options: nil),
                      let components = converted.components, components.count >= 3 else { return }
                draft.red = Float(components[0])
                draft.green = Float(components[1])
                draft.blue = Float(components[2])
                colorTask?.cancel()
                let (editing, style) = (editing, draft)
                colorTask = Task { @MainActor in
                    try? await Task.sleep(for: .milliseconds(400))
                    guard !Task.isCancelled else { return }
                    editing.setScanStyle(style)
                }
            }
        )
    }
}
