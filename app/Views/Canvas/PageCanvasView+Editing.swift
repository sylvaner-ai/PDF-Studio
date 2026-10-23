import AppKit

// MARK: - Mode Édition

extension PageCanvasView {
    /// Passe en mode Édition (`editing` non nul) ou revient à la lecture.
    func setEditing(_ editing: PageEditing?) {
        guard editing !== self.editing else { return }
        self.editing?.onChange = nil
        self.editing = editing
        editing?.onChange = { [weak self] page in
            self?.editingDidChange(on: page)
        }
        pageViews.values.forEach { $0.setEditing(editing) }
    }

    /// Les objets ou la sélection de la page `id` ont changé.
    private func editingDidChange(on id: UInt64) {
        for view in pageViews.values where view.pageID == id {
            view.editingDidChange()
        }
    }

    /// Rotation à l'écran de la page `id`, la sienne et celle de l'affichage.
    func screenRotation(ofPage id: UInt64) -> DisplayRotation? {
        pages.first { $0.id == id }?.rotation(rotation)
    }

    /// Touches de la sélection : flèches (1 pt, 10 pt avec ⇧), ⌫ et ⌦,
    /// Retour pour modifier un texte (ou la cellule courante d'un tableau),
    /// Échap pour tout désélectionner.
    func handleEditingKey(_ event: NSEvent, editing: PageEditing) -> Bool {
        let flags = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard flags.subtracting(.shift).isEmpty, editing.hasSelection, let page = editing.page else { return false }
        switch event.keyCode {
        case 53:
            editing.clearSelection()
        case 51, 117:
            editing.deleteSelection()
        case 36, 76:
            if editing.selectedTable != nil, let cell = editing.cell {
                editing.beginCellEditing(cell)
                return true
            }
            let items = editing.selectedItems
            guard items.count == 1, let item = items.first, item.editable else { return false }
            editing.beginTextEditing(page: page, object: item.id)
        case 123...126:
            guard let rotation = screenRotation(ofPage: page) else { return false }
            let vector = Self.arrowVector(event.keyCode, step: flags.contains(.shift) ? 10 : 1)
            editing.moveSelection(by: rotation.unapply(to: vector))
        default:
            return false
        }
        return true
    }

    /// Déplacement à l'écran demandé par une flèche.
    private static func arrowVector(_ keyCode: UInt16, step: CGFloat) -> CGVector {
        switch keyCode {
        case 123: return CGVector(dx: -step, dy: 0)
        case 124: return CGVector(dx: step, dy: 0)
        case 125: return CGVector(dx: 0, dy: step)
        default: return CGVector(dx: 0, dy: -step)
        }
    }
}
