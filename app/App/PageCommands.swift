import SwiftUI

/// Menu Pages : organisation des pages sélectionnées dans les vignettes, ou de
/// la page lue s'il n'y a pas de sélection. Chaque action s'annule par ⌘Z.
struct PageCommands: Commands {
    @FocusedValue(\.documentModel) private var model

    var body: some Commands {
        CommandMenu("Pages") {
            Button("Faire pivoter à gauche") { edit { $0.rotate($1, clockwise: false) } }
                .keyboardShortcut("l")
                .disabled(!canEdit)
            Button("Faire pivoter à droite") { edit { $0.rotate($1, clockwise: true) } }
                .keyboardShortcut("r")
                .disabled(!canEdit)
            Divider()
            Button("Dupliquer") { edit { $0.duplicate($1) } }
                .keyboardShortcut("d")
                .disabled(!canEdit)
            Button("Supprimer") { edit { $0.delete($1) } }
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(model.map { $0.canDelete($0.targetPositions) } != true)
        }
    }

    private var canEdit: Bool {
        model?.canEdit == true
    }

    /// Applique une action aux pages visées du document actif.
    private func edit(_ action: (DocumentViewModel, [Int]) -> Void) {
        guard let model, model.canEdit else { return }
        action(model, model.targetPositions)
    }
}
