import AppKit

// MARK: - Objets : glisser vers une autre page, menu contextuel

extension EditingOverlayView {
    /// Point de l'événement dans la vue.
    func location(of event: NSEvent) -> CGPoint {
        convert(event.locationInWindow, from: nil)
    }

    /// Fin du glisser de la sélection, lâchée au point `pointer` de la vue :
    /// sur une autre page, les objets y passent, leur groupe centré là où on
    /// l'a amené ; `copy` (⌥) en fait une copie, les originaux restant à leur
    /// place. Sinon, la sélection se déplace sur sa page.
    func finishMove(by offset: CGVector, at pointer: CGPoint, copy: Bool) {
        guard let editing, let page else { return }
        let canvas = enclosingScrollView as? PageCanvasView
        let window = convert(pointer, to: nil)
        let target = canvas?.page(atWindow: window) ?? page
        guard target.id != page.id || copy, let canvas, let center = editing.selectionCenter else {
            editing.moveSelection(by: rotation.unapply(to: offset))
            return
        }
        // Centre du groupe à l'écran, déplacé comme le pointeur.
        let shown = rotation.apply(to: CGRect(origin: center, size: .zero), pageSize: page.baseSize).origin
        let moved = CGPoint(x: shown.x + offset.dx, y: shown.y + offset.dy)
        guard let point = canvas.point(atWindow: convert(moved, to: nil), onPage: target) else { return }
        editing.transferSelection(to: target.id, center: point, keep: copy)
    }

    /// Menu contextuel d'un objet (couper, copier, dupliquer, supprimer) ou
    /// de la page (coller là où l'on a cliqué).
    func objectMenu(at point: CGPoint) -> NSMenu? {
        guard let editing, let page, !editing.isPlacingText else { return nil }
        editing.clickedPage = page.id
        if let hit = object(at: point), !(editing.page == page.id && editing.selected.contains(hit.id)) {
            editing.select([hit.id], on: page.id)
        } else if object(at: point) == nil {
            editing.clearSelection()
        }
        menuPoint = rotation.unapply(to: point, pageSize: page.baseSize)
        let menu = NSMenu()
        menu.autoenablesItems = false
        let copies = editing.canCopy
        menu.addItem(entry(String(localized: "Couper"), #selector(cutObjects(_:)), enabled: copies))
        menu.addItem(entry(String(localized: "Copier"), #selector(copyObjects(_:)), enabled: copies))
        menu.addItem(entry(String(localized: "Coller"), #selector(pasteObjects(_:)), enabled: editing.canPaste))
        menu.addItem(entry(String(localized: "Dupliquer"), #selector(duplicateObjects(_:)), enabled: copies))
        menu.addItem(.separator())
        let deletes = editing.hasSelection && editing.selectedItems.contains(where: \.movable)
        menu.addItem(entry(String(localized: "Supprimer"), #selector(deleteObjects(_:)), enabled: deletes))
        return menu
    }

    private func entry(_ title: String, _ action: Selector, enabled: Bool) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        item.isEnabled = enabled
        return item
    }

    @objc private func cutObjects(_ sender: NSMenuItem) {
        editing?.cutSelection()
    }

    @objc private func copyObjects(_ sender: NSMenuItem) {
        editing?.copySelection()
    }

    /// Colle au point du clic droit, sans sélection ; sinon comme ⌘V.
    @objc private func pasteObjects(_ sender: NSMenuItem) {
        guard let editing, let page else { return }
        editing.paste(on: page, at: editing.hasSelection ? nil : menuPoint)
    }

    @objc private func duplicateObjects(_ sender: NSMenuItem) {
        editing?.duplicateSelection()
    }

    @objc private func deleteObjects(_ sender: NSMenuItem) {
        editing?.deleteSelection()
    }
}
