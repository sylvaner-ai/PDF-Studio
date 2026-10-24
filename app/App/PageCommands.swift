import AppKit
import SwiftUI

/// Menu Pages : organisation des pages visées (``DocumentViewModel/targetPositions`` :
/// la sélection de la grille, ou en lecture la page lue) ; les insertions se font
/// après elles. Chaque action s'annule par ⌘Z, reconnaissance du texte comprise.
struct PageCommands: Commands {
    @FocusedValue(\.documentModel) private var model

    var body: some Commands {
        CommandMenu("Pages") {
            Button("Faire pivoter à gauche") { edit { $0.rotate($1, clockwise: false) } }
                .keyboardShortcut("l")
                .disabled(!hasTargets)
            Button("Faire pivoter à droite") { edit { $0.rotate($1, clockwise: true) } }
                .keyboardShortcut("r")
                .disabled(!hasTargets)
            Divider()
            Button("Dupliquer") { edit { $0.duplicate($1) } }
                .keyboardShortcut("d")
                .disabled(!hasTargets)
            Button("Supprimer", action: deletePages)
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(model?.canDeleteTargets != true)
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
                .disabled(!hasTargets)
            Button("Scinder le document…") { model?.split() }
                .disabled(!canEdit || (model?.pageCount ?? 0) < 2)
            Divider()
            // La reconnaissance du texte se lance à l'entrée en mode Édition (D58).
            Button("Arrêter la reconnaissance du texte") { model?.stopRecognition() }
                .keyboardShortcut(".", modifiers: .command)
                .disabled(model?.recognition == nil)
        }
    }

    private var canEdit: Bool {
        model?.canEdit == true
    }

    /// Des pages sont visées (dans la grille, rien n'est visé sans sélection).
    private var hasTargets: Bool {
        canEdit && (model?.targetCount ?? 0) > 0
    }

    /// ⌘⌫ efface le début de la ligne dans un champ de texte (recherche) ; il ne
    /// supprime les pages que hors d'un champ.
    private func deletePages() {
        if let text = NSApp.keyWindow?.firstResponder as? NSTextView {
            text.deleteToBeginningOfLine(nil)
            return
        }
        edit { $0.delete($1) }
    }

    /// Applique une action aux pages visées du document actif.
    private func edit(_ action: (DocumentViewModel, [Int]) -> Void) {
        guard let model, model.canEdit else { return }
        action(model, model.targetPositions)
    }
}

/// Menu Tableau, en mode Édition : ligne et colonne de la cellule courante du
/// tableau sélectionné, et le tableau entier. Chaque action s'annule par ⌘Z.
struct TableCommands: Commands {
    @FocusedValue(\.documentModel) private var model

    var body: some Commands {
        CommandMenu("Tableau") {
            ForEach(PageEditing.TableAction.allCases, id: \.self) { action in
                if action == .moveRowUp || action == .moveColumnLeft {
                    Divider()
                }
                Button(action.title) { editing?.perform(action) }
                    .disabled(editing?.canPerform(action) != true)
            }
            Divider()
            Button("Supprimer le tableau") { editing?.deleteSelection() }
                .disabled(editing?.selectedTable?.movable != true)
        }
    }

    /// Édition du document actif, en mode Édition seulement.
    private var editing: PageEditing? {
        guard let model, model.canEdit, model.workspace == .editing else { return nil }
        return model.editing
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
