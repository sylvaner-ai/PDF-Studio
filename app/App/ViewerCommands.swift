import SwiftUI

/// Document de la fenêtre active, pour les menus.
struct DocumentModelKey: FocusedValueKey {
    typealias Value = DocumentViewModel
}

extension FocusedValues {
    var documentModel: DocumentViewModel? {
        get { self[DocumentModelKey.self] }
        set { self[DocumentModelKey.self] = newValue }
    }
}

/// Menus de lecture : recherche, présentation et navigation, avec les raccourcis
/// habituels de macOS.
struct ViewerCommands: Commands {
    @FocusedValue(\.documentModel) private var model

    var body: some Commands {
        CommandGroup(replacing: .textEditing) {
            Button("Rechercher…") { model?.search?.requestFocus() }
                .keyboardShortcut("f")
                .disabled(model?.search == nil)
            Button("Occurrence suivante") { model?.search?.next() }
                .keyboardShortcut("g")
                .disabled(model?.search?.hasHits != true)
            Button("Occurrence précédente") { model?.search?.previous() }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(model?.search?.hasHits != true)
        }

        CommandGroup(before: .sidebar) {
            Button("Taille réelle") { canvas?.actualSize() }
                .keyboardShortcut("0")
                .disabled(canvas == nil)
            Button("Agrandir") { canvas?.zoomIn() }
                .keyboardShortcut("+")
                .disabled(canvas == nil)
            Button("Réduire") { canvas?.zoomOut() }
                .keyboardShortcut("-")
                .disabled(canvas == nil)
            Button("Ajuster à la page") { canvas?.fit(.page) }
                .keyboardShortcut("9")
                .disabled(canvas == nil)
            Button("Ajuster à la largeur") { canvas?.fit(.width) }
                .keyboardShortcut("9", modifiers: [.command, .option])
                .disabled(canvas == nil)
            Divider()
            // Rotation de l'affichage seul ; ⌘L et ⌘R font pivoter les pages (menu Pages).
            Button("Faire pivoter l'affichage à gauche") { canvas?.rotateDisplay(clockwise: false) }
                .keyboardShortcut("l", modifiers: [.command, .option])
                .disabled(canvas == nil)
            Button("Faire pivoter l'affichage à droite") { canvas?.rotateDisplay(clockwise: true) }
                .keyboardShortcut("r", modifiers: [.command, .option])
                .disabled(canvas == nil)
            Divider()
            Picker("Affichage des pages", selection: displayMode) {
                Text("Défilement continu").tag(DisplayMode.continuous)
                Text("Page par page").tag(DisplayMode.singlePage)
            }
            .pickerStyle(.inline)
            .disabled(canvas == nil)
            Divider()
        }

        CommandMenu("Aller") {
            Button("Page précédente") { canvas?.goToPreviousPage() }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
                .disabled(canvas?.canGoBack != true)
            Button("Page suivante") { canvas?.goToNextPage() }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])
                .disabled(canvas?.canGoForward != true)
            Divider()
            Button("Première page") { canvas?.goToFirstPage() }
                .disabled(canvas?.canGoBack != true)
            Button("Dernière page") { canvas?.goToLastPage() }
                .disabled(canvas?.canGoForward != true)
        }
    }

    /// Canevas du document actif, une fois les pages affichées.
    private var canvas: CanvasController? {
        guard let model, model.phase == .ready else { return nil }
        return model.canvas
    }

    private var displayMode: Binding<DisplayMode> {
        let canvas = self.canvas
        return Binding(
            get: { canvas?.displayMode ?? .continuous },
            set: { canvas?.setDisplayMode($0) }
        )
    }
}
