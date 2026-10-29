import SwiftUI

/// Outils d'ajout du mode Édition, en haut des pages : « Texte » (le
/// prochain clic sur une page y commence un texte) et « Image » (une image
/// du Mac, posée au centre de la page lue). La barre d'outils de la fenêtre
/// garde ses six commandes (D44).
struct AddingPalette: View {
    let editing: PageEditing

    var body: some View {
        HStack(spacing: Design.Spacing.small) {
            Toggle(isOn: placing) {
                Label("Texte", systemImage: "textformat")
            }
            .toggleStyle(.button)
            .help(Text("Ajouter un texte : cliquez ensuite sur la page, à l'endroit voulu (⌃⌘T)"))
            Button { editing.chooseImage() } label: {
                Label("Image", systemImage: "photo")
            }
            .help(Text("Ajouter une image au centre de la page (⌃⌘I) ; vous pouvez aussi la déposer sur la page"))
        }
        .padding(.horizontal, Design.Spacing.small)
        .padding(.vertical, Design.Spacing.tight)
        .floatingPanel()
        .padding(Design.Spacing.medium)
    }

    private var placing: Binding<Bool> {
        Binding(
            get: { editing.isPlacingText },
            set: { $0 ? editing.startPlacingText() : editing.cancelPlacingText() }
        )
    }
}

/// Style des textes ajoutés, en mode Édition : police (les polices standard
/// du PDF, puis celles du Mac), gras, italique, corps et couleur. Il règle le
/// texte ajouté sélectionné (⌘Z annule chaque changement) et les textes
/// ajoutés ensuite.
struct AddedTextPanel: View {
    let editing: PageEditing

    /// Polices du Mac, lues à l'ouverture du panneau.
    @State private var families: [String] = []
    /// Couleur choisie dans le nuancier, pas encore appliquée.
    @State private var draftColor: CGColor?
    @State private var colorTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: Design.Spacing.medium) {
            Text(editing.selectedAddedText == nil
                 ? LocalizedStringKey("Nouveau texte")
                 : LocalizedStringKey("Texte ajouté"))
                .font(.headline)
            Picker("Police", selection: fontBinding(\.family)) {
                ForEach(AddedTextFont.standardFamilies, id: \.self) { Text($0).tag($0) }
                Divider()
                ForEach(families, id: \.self) { Text($0).tag($0) }
                if !AddedTextFont.standardFamilies.contains(currentFont.family),
                   !families.contains(currentFont.family) {
                    Text(currentFont.family).tag(currentFont.family)
                }
            }
            .help(Text("Helvetica, Times et Courier sont lus partout sans être intégrés au document"))
            HStack(spacing: Design.Spacing.small) {
                Toggle(isOn: fontBinding(\.bold)) {
                    Image(systemName: "bold")
                }
                .toggleStyle(.button)
                .help(Text("Gras"))
                .accessibilityLabel(Text("Gras"))
                Toggle(isOn: fontBinding(\.italic)) {
                    Image(systemName: "italic")
                }
                .toggleStyle(.button)
                .help(Text("Italique"))
                .accessibilityLabel(Text("Italique"))
                Spacer()
                TextField("Corps", value: size, format: .number.precision(.fractionLength(0...1)))
                    .frame(width: 48)
                    .multilineTextAlignment(.trailing)
                    .labelsHidden()
                Stepper("Corps", value: size, in: 1...500)
                    .labelsHidden()
                Text("pt")
                    .foregroundStyle(.secondary)
            }
            ColorPicker("Couleur", selection: color, supportsOpacity: false)
        }
        .padding(Design.Spacing.medium)
        .frame(width: 260)
        .floatingPanel()
        .padding(Design.Spacing.medium)
        .onAppear { families = AddedTextFont.installedFamilies }
        .onDisappear { colorTask?.cancel() }
    }

    private var currentFont: AddedTextFont {
        AddedTextFont(editing.shownTextStyle.font)
    }

    /// Une propriété de la police : la changer applique le style.
    private func fontBinding<Value>(_ property: WritableKeyPath<AddedTextFont, Value>) -> Binding<Value> {
        Binding(
            get: { currentFont[keyPath: property] },
            set: { value in
                var chosen = currentFont
                chosen[keyPath: property] = value
                var style = editing.shownTextStyle
                style.font = chosen.engine
                editing.applyTextStyle(style)
            }
        )
    }

    private var size: Binding<Double> {
        Binding(
            get: { Double(editing.shownTextStyle.size) },
            set: { value in
                var style = editing.shownTextStyle
                style.size = Float(min(max(value, 1), 500))
                editing.applyTextStyle(style)
            }
        )
    }

    /// Couleur du texte ; le nuancier change la couleur en continu : elle
    /// s'applique quand elle cesse de changer.
    private var color: Binding<CGColor> {
        Binding(
            get: { draftColor ?? editing.shownTextStyle.nsColor.cgColor },
            set: { color in
                draftColor = color
                colorTask?.cancel()
                let editing = editing
                colorTask = Task { @MainActor in
                    try? await Task.sleep(for: .milliseconds(400))
                    guard !Task.isCancelled else { return }
                    draftColor = nil
                    guard let srgb = CGColorSpace(name: CGColorSpace.sRGB),
                          let converted = color.converted(to: srgb, intent: .defaultIntent, options: nil),
                          let components = converted.components, components.count >= 3 else { return }
                    var style = editing.shownTextStyle
                    style.red = Float(components[0])
                    style.green = Float(components[1])
                    style.blue = Float(components[2])
                    editing.applyTextStyle(style)
                }
            }
        )
    }
}
