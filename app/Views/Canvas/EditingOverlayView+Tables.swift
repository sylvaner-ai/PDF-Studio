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
        let distance = { (rect: CGRect) -> CGFloat in
            hypot(max(rect.minX - point.x, 0, point.x - rect.maxX), max(rect.minY - point.y, 0, point.y - rect.maxY))
        }
        guard let cell = grid.cells.min(by: { distance(frame(of: $0)) < distance(frame(of: $1)) }) else { return nil }
        return PageEditing.CellTarget(page: page.id, table: table.id, row: cell.row, column: cell.column)
    }

    /// Cellules du tableau sélectionné, et sa cellule courante.
    func drawGrid(_ grid: TableGridItem, current: TableCellItem?, unit: CGFloat) {
        let accent = NSColor.controlAccentColor
        for cell in grid.cells {
            stroke(frame(of: cell), accent.withAlphaComponent(0.35), width: unit)
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
        editing.selectCell(cell)
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (index, action) in PageEditing.TableAction.allCases.enumerated() {
            if action == .moveRowUp || action == .moveColumnLeft {
                menu.addItem(.separator())
            }
            let item = NSMenuItem(title: action.title, action: #selector(tableAction(_:)), keyEquivalent: "")
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
}
