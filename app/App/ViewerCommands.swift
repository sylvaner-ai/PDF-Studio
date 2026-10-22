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
            // La recherche s'ouvre dans un panneau sur les pages (lecture ou édition).
            Button("Rechercher…") { search?.requestFocus() }
                .keyboardShortcut("f")
                .disabled(search == nil)
            Button("Occurrence suivante") { search?.next() }
                .keyboardShortcut("g")
                .disabled(search?.hasHits != true)
            Button("Occurrence précédente") { search?.previous() }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(search?.hasHits != true)
        }

        CommandGroup(before: .sidebar) {
            Toggle("Lecture", isOn: workspace(.reading))
                .keyboardShortcut("1")
                .disabled(canvas == nil)
            Toggle("Organisation des pages", isOn: workspace(.organizing))
                .keyboardShortcut("2")
                .disabled(canvas == nil)
            Toggle("Édition", isOn: workspace(.editing))
                .keyboardShortcut("3")
                .disabled(canvas == nil)
            Divider()
            Button("Taille réelle") { reading?.actualSize() }
                .keyboardShortcut("0")
                .disabled(reading == nil)
            // Dans la grille d'organisation, ⌘+ et ⌘- changent la taille des vignettes.
            Button("Agrandir") { zoom(larger: true) }
                .keyboardShortcut("+")
                .disabled(canvas == nil)
            Button("Réduire") { zoom(larger: false) }
                .keyboardShortcut("-")
                .disabled(canvas == nil)
            Button("Ajuster à la page") { reading?.fit(.page) }
                .keyboardShortcut("9")
                .disabled(reading == nil)
            Button("Ajuster à la largeur") { reading?.fit(.width) }
                .keyboardShortcut("9", modifiers: [.command, .option])
                .disabled(reading == nil)
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
            .disabled(reading == nil)
            Divider()
        }

        CommandMenu("Aller") {
            Button("Page précédente") { reading?.goToPreviousPage() }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
                .disabled(reading?.canGoBack != true)
            Button("Page suivante") { reading?.goToNextPage() }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])
                .disabled(reading?.canGoForward != true)
            Divider()
            Button("Première page") { reading?.goToFirstPage() }
                .disabled(reading?.canGoBack != true)
            Button("Dernière page") { reading?.goToLastPage() }
                .disabled(reading?.canGoForward != true)
        }
    }

    /// Canevas du document actif, une fois les pages affichées.
    private var canvas: CanvasController? {
        guard let model, model.phase == .ready else { return nil }
        return model.canvas
    }

    /// Canevas du document actif s'il est affiché (et non la grille d'organisation).
    private var reading: CanvasController? {
        model?.workspace == .organizing ? nil : canvas
    }

    private var search: SearchModel? {
        model?.workspace == .organizing ? nil : model?.search
    }

    private func workspace(_ workspace: Workspace) -> Binding<Bool> {
        let model = self.model
        return Binding(
            get: { model?.workspace == workspace },
            set: { if $0 { model?.workspace = workspace } }
        )
    }

    private func zoom(larger: Bool) {
        if let model, model.workspace == .organizing {
            model.zoomGrid(in: larger)
        } else if larger {
            canvas?.zoomIn()
        } else {
            canvas?.zoomOut()
        }
    }

    private var displayMode: Binding<DisplayMode> {
        let canvas = self.canvas
        return Binding(
            get: { canvas?.displayMode ?? .continuous },
            set: { canvas?.setDisplayMode($0) }
        )
    }
}
