import SwiftUI

/// Bandeau du mode Édition, au bas du canevas : ce qui est sélectionné (nature,
/// police et corps d'un texte), la remarque sur le dernier texte modifié, ou
/// la marche à suivre.
struct EditingBadge: View {
    let editing: PageEditing

    var body: some View {
        VStack(spacing: 4) {
            Text(summary)
                .font(.callout)
            if let detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let notice = editing.notice {
                Label(notice, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
        .padding(12)
        .frame(maxWidth: 560)
        .accessibilityElement(children: .combine)
        .allowsHitTesting(false)
    }

    private var summary: String {
        if editing.textTarget != nil || editing.cellTarget != nil {
            return String(localized: "Retour pour valider, ⇧Retour pour passer à la ligne, Échap pour annuler.")
        }
        let items = editing.selectedItems
        switch items.count {
        case 0:
            return String(localized: "Cliquez sur un texte, une image, un tracé ou un tableau pour le sélectionner.")
        case 1:
            return Self.name(of: items[0])
        default:
            return String(localized: "\(items.count) objets sélectionnés")
        }
    }

    private var detail: String? {
        guard editing.textTarget == nil, editing.cellTarget == nil else { return nil }
        let items = editing.selectedItems
        guard let item = items.first, items.count == 1 else {
            return items.isEmpty
                ? String(localized: "Double-cliquez sur un texte pour le modifier.")
                : String(localized: "Flèches pour déplacer (⇧ : 10 pt), ⌫ pour supprimer.")
        }
        guard item.movable else {
            return String(localized: "Cet objet ne peut pas être modifié.")
        }
        if let grid = item.grid {
            return Self.tableDetail(grid, cell: editing.currentCell)
        }
        var parts: [String] = []
        if item.kind == .text {
            if let font = item.displayFontName {
                let size = item.fontSize.formatted(.number.precision(.fractionLength(0...1)))
                parts.append(String(localized: "\(font), \(size) pt"))
            }
            parts.append(item.editable
                ? String(localized: "double-clic ou Retour pour modifier le texte")
                : String(localized: "texte non modifiable, déplaçable seulement"))
        } else {
            parts.append(String(localized: "Flèches pour déplacer (⇧ : 10 pt), ⌫ pour supprimer"))
        }
        return parts.joined(separator: " · ")
    }

    private static func name(of item: PageObjectItem) -> String {
        switch item.kind {
        case .text: return String(localized: "Bloc de texte")
        case .image: return String(localized: "Image")
        case .path: return String(localized: "Tracé")
        case .form: return String(localized: "Groupe d'objets")
        case .shading: return String(localized: "Dégradé")
        case .table: return String(localized: "Tableau")
        }
    }

    /// Taille du tableau, cellule courante et marche à suivre.
    private static func tableDetail(_ grid: TableGridItem, cell: TableCellItem?) -> String {
        var parts = [String(localized: "\(grid.rows) lignes × \(grid.columns) colonnes")]
        if let cell {
            parts.append(String(localized: "cellule ligne \(cell.row + 1), colonne \(cell.column + 1)"))
            if cell.overflow {
                parts.append(String(localized: "le texte saisi dépasse de la cellule"))
            }
        }
        let help = String(localized: "double-clic pour modifier une cellule, menu Tableau pour ses lignes et colonnes")
        parts.append(help)
        return parts.joined(separator: " · ")
    }
}
