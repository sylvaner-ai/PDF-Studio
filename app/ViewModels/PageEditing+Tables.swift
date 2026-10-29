import Foundation

/// Tableaux en mode Édition : cellule courante, lignes et colonnes
/// supprimées ou déplacées, saisie d'une cellule.
extension PageEditing {
    /// Tableau sélectionné seul.
    var selectedTable: PageObjectItem? {
        let items = selectedItems
        guard items.count == 1, let item = items.first, item.kind == .table, item.grid != nil else { return nil }
        return item
    }

    /// Cellule courante du tableau sélectionné, telle que connue.
    var currentCell: TableCellItem? {
        guard let table = selectedTable, let cell, cell.table == table.id else { return nil }
        return table.grid?.cells.first { $0.row == cell.row && $0.column == cell.column }
    }

    /// Cellule `cell` parmi les objets `items` d'une page.
    func cellItem(_ cell: CellTarget, in items: [PageObjectItem]) -> TableCellItem? {
        items.first { $0.id == cell.table }?.grid?.cells.first { $0.row == cell.row && $0.column == cell.column }
    }

    /// Vrai si l'action est possible sur la cellule courante (les lignes et
    /// les colonnes d'un tableau scanné ne se réorganisent pas).
    func canPerform(_ action: TableAction) -> Bool {
        guard let table = selectedTable, table.movable, !table.scanned, let grid = table.grid,
              let cell = currentCell else {
            return false
        }
        switch action {
        case .deleteRow, .deleteColumn: return true
        case .moveRowUp: return cell.row > 0
        case .moveRowDown: return cell.row + cell.rowSpan < grid.rows
        case .moveColumnLeft: return cell.column > 0
        case .moveColumnRight: return cell.column + cell.columnSpan < grid.columns
        }
    }

    /// Supprime ou déplace la ligne ou la colonne de la cellule courante ; une
    /// cellule fusionnée emporte toutes ses lignes (ou colonnes) à la
    /// suppression.
    func perform(_ action: TableAction) {
        guard canPerform(action), let model, let target = cell, let item = currentCell else { return }
        let (page, table) = (target.page, target.table)
        let rows = Array(item.row..<item.row + item.rowSpan)
        let columns = Array(item.column..<item.column + item.columnSpan)
        var next: CellTarget?
        let done: Bool
        switch action {
        case .deleteRow:
            done = model.perform { try $0.deleteTableBands(page: page, table: table, axis: .rows, bands: rows) }
        case .deleteColumn:
            done = model.perform { try $0.deleteTableBands(page: page, table: table, axis: .columns, bands: columns) }
        case .moveRowUp, .moveRowDown:
            // La ligne voisine passe de l'autre côté de la cellule (de toutes
            // ses lignes si elle est fusionnée).
            let (from, destination) = action == .moveRowUp
                ? (item.row - 1, item.row + item.rowSpan - 1)
                : (item.row + item.rowSpan, item.row)
            done = model.perform {
                try $0.moveTableBand(page: page, table: table, axis: .rows, from: from, to: destination)
            }
            let row = action == .moveRowUp ? item.row - 1 : item.row + 1
            next = CellTarget(page: page, table: table, row: row, column: item.column)
        case .moveColumnLeft, .moveColumnRight:
            let (from, destination) = action == .moveColumnLeft
                ? (item.column - 1, item.column + item.columnSpan - 1)
                : (item.column + item.columnSpan, item.column)
            done = model.perform {
                try $0.moveTableBand(page: page, table: table, axis: .columns, from: from, to: destination)
            }
            let column = action == .moveColumnLeft ? item.column - 1 : item.column + 1
            next = CellTarget(page: page, table: table, row: item.row, column: column)
        }
        guard done else { return }
        cell = next
        onChange?(page)
    }

    /// Ouvre la saisie d'une cellule : celle de son bloc de texte, ou celle
    /// d'une cellule vide à l'origine (texte dans le style des autres
    /// cellules). Une cellule qui mêle des textes de corps ou de graisse
    /// différents compte plusieurs blocs : la saisie porte sur `block` (le
    /// bloc visé par le clic), à défaut sur celui du dernier clic dans la
    /// cellule, sinon sur le premier bloc modifiable.
    func beginCellEditing(_ target: CellTarget, block: UInt32? = nil) {
        guard let items = knownItems(on: target.page), let item = cellItem(target, in: items) else { return }
        let blocks = item.objects.compactMap { id in items.first { $0.id == id } }
        let editable = blocks.filter(\.editable)
        let wanted = block ?? cellBlock
        let chosen: PageObjectItem?
        if blocks.count == 1 {
            chosen = editable.first
        } else {
            chosen = editable.first { $0.id == wanted } ?? editable.first
        }
        if let chosen {
            beginTextEditing(page: target.page, object: chosen.id)
        } else if item.fillable {
            selectCell(target)
            cellTarget = target
            onChange?(target.page)
        }
    }

    /// Texte actuel et police de saisie de la cellule en cours de saisie.
    func cellEditingContext() -> (text: String, font: PageObjectItem?)? {
        guard let target = cellTarget, let items = knownItems(on: target.page),
              let item = cellItem(target, in: items),
              let grid = items.first(where: { $0.id == target.table })?.grid else { return nil }
        // Police de saisie : celle d'un bloc de la même colonne, sinon du tableau.
        let blocks = grid.cells
            .sorted { ($0.column == item.column ? 0 : 1, $0.row) < ($1.column == item.column ? 0 : 1, $1.row) }
            .flatMap(\.objects)
        let font = blocks.lazy.compactMap { id in items.first { $0.id == id && $0.fontName != nil } }.first
        return (item.text ?? "", font)
    }
}

extension PageEditing.TableAction {
    /// Intitulé, dans le menu Tableau et le menu contextuel.
    var title: String {
        switch self {
        case .deleteRow: return String(localized: "Supprimer la ligne")
        case .deleteColumn: return String(localized: "Supprimer la colonne")
        case .moveRowUp: return String(localized: "Monter la ligne")
        case .moveRowDown: return String(localized: "Descendre la ligne")
        case .moveColumnLeft: return String(localized: "Déplacer la colonne à gauche")
        case .moveColumnRight: return String(localized: "Déplacer la colonne à droite")
        }
    }
}
