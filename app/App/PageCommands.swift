import SwiftUI

/// Menu Pages : organisation des pages sélectionnées dans les vignettes, ou de
/// la page lue s'il n'y a pas de sélection ; les insertions se font après elles.
/// Chaque action s'annule par ⌘Z.
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
            Divider()
            Button("Insérer une page vierge") { model?.insertBlankPage() }
                .keyboardShortcut("n", modifiers: [.command, .option])
                .disabled(!canEdit)
            Button("Insérer des pages d'un fichier…") { model?.chooseDocumentToInsert() }
                .keyboardShortcut("i", modifiers: [.command, .option])
                .disabled(!canEdit)
            Divider()
            Button("Extraire les pages…") { edit { $0.extractPages($1) } }
                .keyboardShortcut("e", modifiers: [.command, .option])
                .disabled(!canEdit)
            Button("Scinder le document…") { model?.split() }
                .disabled(!canEdit || (model?.pageCount ?? 0) < 2)
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

/// Menu Fichier : Enregistrer et Enregistrer sous, après « Fermer », puis les
/// exportations en images et en texte.
struct SaveCommands: Commands {
    @FocusedValue(\.documentModel) private var model

    var body: some Commands {
        CommandGroup(after: .saveItem) {
            Button("Enregistrer") { model?.save() }
                .keyboardShortcut("s")
                .disabled(model?.canSave != true)
            Button("Enregistrer sous…") { model?.saveAs() }
                .keyboardShortcut("s", modifiers: [.command, .shift])
                .disabled(model?.canSaveAs != true)
            Divider()
            Button("Exporter en images…") { model?.exportImages() }
                .disabled(model?.canEdit != true)
            Button("Exporter le texte…") { model?.exportText() }
                .disabled(model?.canEdit != true)
        }
    }
}
