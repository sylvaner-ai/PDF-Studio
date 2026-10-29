import AppKit

// MARK: - Tableaux

extension EditingOverlayView {
    /// Emprise d'une cellule dans la vue.
    func frame(of cell: TableCellItem) -> CGRect {
        guard let page else { return .zero }
        return rotation.apply(to: cell.bounds.cgRect, pageSize: page.baseSize)
    }

    /// Cellule du tableau `table` sous le point, ou la plus proche (clic sur un
    /// filet ou sur le bord).
    func cell(at point: CGPoint, in table: PageObjectItem) -> PageEditing.CellTarget? {
        guard let page, let grid = table.grid else { return nil }
        guard let cell = grid.cells.min(by: {
            distance(from: point, to: frame(of: $0)) < distance(from: point, to: frame(of: $1))
        }) else { return nil }
        return PageEditing.CellTarget(page: page.id, table: table.id, row: cell.row, column: cell.column)
    }

    /// Clic sur le tableau `table` : choisit sa cellule courante (double-clic :
    /// la saisit), ou étend la plage de cellules par ⇧-clic dans le tableau
    /// déjà sélectionné. Faux si le clic reste un clic de sélection d'objets.
    func clickTable(_ table: PageObjectItem, at point: CGPoint, count: Int, extend: Bool) -> Bool {
        guard let editing, let page, let cell = cell(at: point, in: table) else { return false }
        if extend {
            guard editing.page == page.id, editing.selectedTable?.id == table.id else { return false }
            editing.extendCells(to: cell)
            return true
        }
        let block = textBlock(at: point, in: cell, of: table)
        if count == 2 {
            editing.beginCellEditing(cell, block: block)
            return true
        }
        editing.selectCell(cell, block: block)
        drag = .move(start: point, offset: .zero, started: false)
        return true
    }

    /// Bloc de texte d'une cellule de tableau sous le point (le plus petit) : il
    /// se déplace seul, sans le tableau.
    func cellBlock(at point: CGPoint) -> PageObjectItem? {
        let tolerance = 2 * screenPoint
        return items.filter { $0.table != nil && $0.kind == .text && $0.movable && !$0.scanned }
            .filter { frame(of: $0).insetBy(dx: -tolerance, dy: -tolerance).contains(point) }
            .min { area(frame(of: $0)) < area(frame(of: $1)) }
    }

    /// Distance d'un point à un rectangle (0 s'il est dedans).
    private func distance(from point: CGPoint, to rect: CGRect) -> CGFloat {
        hypot(max(rect.minX - point.x, 0, point.x - rect.maxX), max(rect.minY - point.y, 0, point.y - rect.maxY))
    }

    /// Bloc de texte modifiable de la cellule `target` le plus proche du point :
    /// une cellule qui mêle des textes de corps ou de graisse différents en
    /// compte plusieurs, et le clic désigne celui que l'on veut modifier.
    func textBlock(at point: CGPoint, in target: PageEditing.CellTarget, of table: PageObjectItem) -> UInt32? {
        guard let cell = table.grid?.cells.first(where: { $0.row == target.row && $0.column == target.column }) else {
            return nil
        }
        let blocks = cell.objects.compactMap { id in items.first { $0.id == id && $0.editable } }
        return blocks.min { distance(from: point, to: frame(of: $0)) < distance(from: point, to: frame(of: $1)) }?.id
    }

    /// Cellules du tableau sélectionné, la plage choisie par ⇧-clic et sa
    /// cellule courante.
    func drawGrid(_ grid: TableGridItem, current: TableCellItem?, range: [TableCellItem], unit: CGFloat) {
        let accent = NSColor.controlAccentColor
        for cell in grid.cells {
            stroke(frame(of: cell), accent.withAlphaComponent(0.35), width: unit)
        }
        for cell in range {
            accent.withAlphaComponent(0.12).setFill()
            NSBezierPath(rect: frame(of: cell)).fill()
        }
        if let current {
            let box = frame(of: current)
            accent.withAlphaComponent(0.12).setFill()
            NSBezierPath(rect: box).fill()
            stroke(box, accent, width: 2 * unit)
        }
    }

    /// Saisie d'une cellule vide, dans la police d'un bloc voisin.
    func openCellEditor(for target: PageEditing.CellTarget) {
        guard let editing, let context = editing.cellEditingContext(),
              let table = items.first(where: { $0.id == target.table }),
              let cell = table.grid?.cells.first(where: { $0.row == target.row && $0.column == target.column })
        else { return }
        let font = context.font.map(Self.font(for:)) ?? .systemFont(ofSize: 10)
        openEditor(bounds: frame(of: cell), text: context.text, font: font)
        editorCell = target
    }

    /// Menu contextuel d'un tableau : lignes, colonnes, tableau entier.
    func tableMenu(at point: CGPoint) -> NSMenu? {
        guard let editing, let page else { return nil }
        guard let hit = object(at: point), hit.kind == .table, let cell = cell(at: point, in: hit) else {
            return nil
        }
        // Un clic droit dans la plage choisie la garde.
        let inRange = editing.selectedTable?.id == hit.id && editing.selectedCells.contains {
            $0.row == cell.row && $0.column == cell.column
        }
        if !inRange {
            editing.selectCell(cell)
        }
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (index, action) in PageEditing.TableAction.allCases.enumerated() {
            if action == .moveRowUp || action == .moveColumnLeft {
                menu.addItem(.separator())
            }
            let item = NSMenuItem(
                title: editing.title(for: action), action: #selector(tableAction(_:)), keyEquivalent: ""
            )
            item.target = self
            item.tag = index
            item.isEnabled = editing.canPerform(action)
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let delete = NSMenuItem(
            title: String(localized: "Supprimer le tableau"), action: #selector(deleteTable(_:)), keyEquivalent: ""
        )
        delete.target = self
        delete.isEnabled = hit.movable && editing.page == page.id
        menu.addItem(delete)
        return menu
    }

    @objc private func tableAction(_ sender: NSMenuItem) {
        let actions = PageEditing.TableAction.allCases
        guard actions.indices.contains(sender.tag) else { return }
        editing?.perform(actions[sender.tag])
    }

    @objc private func deleteTable(_ sender: NSMenuItem) {
        editing?.deleteSelection()
    }

    /// Mots reconnus douteux, à vérifier.
    func drawDoubtfulWords(_ words: [RecognizedWordItem], page: DisplayPage, unit: CGFloat) {
        for word in words {
            let box = rotation.apply(to: word.bounds.cgRect, pageSize: page.baseSize)
            NSColor.systemOrange.withAlphaComponent(0.18).setFill()
            NSBezierPath(rect: box).fill()
            stroke(box, .systemOrange, width: unit)
        }
    }

    /// Trace un contour ; il s'épaissit si « Augmenter le contraste » est activé.
    func stroke(_ rect: CGRect, _ color: NSColor, width: CGFloat, dashed: Bool = false) {
        let path = NSBezierPath(rect: rect)
        path.lineWidth = width * (NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast ? 1.5 : 1)
        if dashed {
            path.setLineDash([4 * width, 3 * width], count: 2, phase: 0)
        }
        color.setStroke()
        path.stroke()
    }

    /// Objets qu'un clic peut atteindre (cf. `Array.selectable`).
    var hittableItems: [PageObjectItem] {
        items.selectable
    }

    /// Police de saisie : celle du bloc si elle est installée sur le Mac.
    static func font(for item: PageObjectItem) -> NSFont {
        let size = CGFloat(max(item.fontSize, 4))
        return item.displayFontName.flatMap { NSFont(name: $0, size: size) } ?? .systemFont(ofSize: size)
    }

    /// Objet sous le pointeur : un objet ajouté, posé sur tout le reste (le
    /// dernier ajouté d'abord), sinon le plus petit, à égalité le plus haut
    /// placé.
    func object(at point: CGPoint) -> PageObjectItem? {
        let tolerance = 3 * screenPoint
        let touched = hittableItems.reversed()
            .filter { frame(of: $0).insetBy(dx: -tolerance, dy: -tolerance).contains(point) }
        if let added = touched.first(where: \.added) {
            return added
        }
        return touched.min { area(frame(of: $0)) < area(frame(of: $1)) }
    }

    func area(_ rect: CGRect) -> CGFloat { rect.width * rect.height }
}
