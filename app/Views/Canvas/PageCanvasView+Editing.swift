import AppKit
import UniformTypeIdentifiers

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
        // Le texte ne se sélectionne qu'en lecture.
        pagesView.text.clear()
        refreshText()
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

    /// Tab et ⇧Tab : passer d'un objet à l'autre.
    func handleTabKey(_ event: NSEvent, editing: PageEditing) -> Bool {
        let flags = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard event.keyCode == 48, flags.subtracting(.shift).isEmpty else { return false }
        return selectAdjacentObject(forward: !flags.contains(.shift), editing: editing)
    }

    /// Échap abandonne l'outil « Texte » avant le clic sur la page.
    func handlePlacingKey(_ event: NSEvent, editing: PageEditing) -> Bool {
        guard event.keyCode == 53, editing.isPlacingText else { return false }
        editing.cancelPlacingText()
        return true
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

    /// Sélectionne l'objet suivant ou précédent, dans l'ordre de lecture de la
    /// page, puis sur les pages voisines. Au bout du document, rien n'est
    /// sélectionné : Tab passe alors à la vue suivante de la fenêtre.
    private func selectAdjacentObject(forward: Bool, editing: PageEditing) -> Bool {
        let step = forward ? 1 : -1
        var index = editing.page.flatMap { id in pages.firstIndex { $0.id == id } } ?? currentPage
        var current = editing.page == pages[safe: index]?.id ? editing.selected : []
        while pages.indices.contains(index) {
            let page = pages[index]
            guard let items = editing.objects(on: page) else {
                // Objets en cours de chargement : la page est montrée, et le
                // prochain Tab y entrera.
                go(to: index)
                return true
            }
            let screen = page.rotation(rotation)
            let frame = { (item: PageObjectItem) in screen.apply(to: item.bounds.cgRect, pageSize: page.baseSize) }
            let objects = items.selectable.inReadingOrder(frame: frame)
            let position = forward
                ? objects.lastIndex { current.contains($0.id) }
                : objects.firstIndex { current.contains($0.id) }
            let next = position.map { $0 + step } ?? (forward ? 0 : objects.count - 1)
            if let item = objects[safe: next] {
                editing.select([item.id], on: page.id)
                reveal(frame(item), onPage: index)
                NSAccessibility.post(
                    element: pagesView, notification: .announcementRequested,
                    userInfo: [
                        .announcement: item.accessibilityDescription,
                        .priority: NSAccessibilityPriorityLevel.high.rawValue
                    ]
                )
                return true
            }
            index += step
            current = []
        }
        return false
    }

    /// Images déposées sur une page, à y ajouter (mode Édition).
    func droppedImages(_ sender: NSDraggingInfo) -> [URL] {
        guard editing != nil else { return [] }
        let objects = sender.draggingPasteboard.readObjects(
            forClasses: [NSURL.self],
            options: [
                .urlReadingFileURLsOnly: true,
                .urlReadingContentsConformToTypes: [UTType.image.identifier]
            ]
        )
        return (objects as? [URL] ?? []).filter { !DocumentOpener.isPDF($0) }
    }

    /// Ajoute les images déposées à la page sous le pointeur, centrées sur
    /// lui ; renvoie `false` hors des pages.
    func addDroppedImages(_ urls: [URL], at location: NSPoint) -> Bool {
        let point = pagesView.convert(location, from: nil)
        guard let editing, let view = pageViews.values.first(where: { $0.frame.contains(point) }),
              let page = pages.first(where: { $0.id == view.pageID }),
              let rotation = screenRotation(ofPage: page.id)
        else { return false }
        let local = CGPoint(x: point.x - view.frame.minX, y: point.y - view.frame.minY)
        let center = rotation.unapply(to: local, pageSize: page.baseSize)
        for url in urls {
            editing.addImage(from: url, on: page.id, at: center)
        }
        return true
    }

    /// Fait défiler juste assez pour montrer `rect`, dans les coordonnées de la page `index`.
    private func reveal(_ rect: CGRect, onPage index: Int) {
        if pageViews[index] == nil || (mode == .singlePage && index != singlePage) {
            go(to: index)
        }
        navigationTarget = index
        pageViews[index]?.scrollToVisible(rect.insetBy(dx: -24, dy: -24))
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
