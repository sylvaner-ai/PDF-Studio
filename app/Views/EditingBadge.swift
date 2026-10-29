import SwiftUI

/// Bandeau du mode Édition, au bas du canevas : ce qui est sélectionné (nature,
/// police et corps d'un texte), la remarque sur le dernier texte modifié, ou
/// la marche à suivre.
struct EditingBadge: View {
    let editing: PageEditing

    var body: some View {
        VStack(spacing: Design.Spacing.tight) {
            Text(summary)
            if let detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let notice = editing.notice {
                // Une mise en garde, pas une couleur d'accent : le triangle jaune du système.
                Label {
                    Text(notice)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .symbolRenderingMode(.multicolor)
                }
                .font(.caption)
            }
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, Design.Spacing.medium)
        .padding(.vertical, Design.Spacing.small)
        .floatingPanel()
        .padding(Design.Spacing.medium)
        .frame(maxWidth: 560)
        .accessibilityElement(children: .combine)
        .allowsHitTesting(false)
    }

    private var summary: String {
        if editing.isPlacingText {
            return String(localized: "Cliquez sur la page à l'endroit où commencera le texte.")
        }
        if editing.textTarget != nil || editing.cellTarget != nil || editing.newText != nil {
            return String(localized: "Retour pour valider, ⇧Retour pour passer à la ligne, Échap pour annuler.")
        }
        let items = editing.selectedItems
        switch items.count {
        case 0:
            return String(localized: "Cliquez sur un texte, une image, un tracé ou un tableau pour le sélectionner.")
        case 1:
            return items[0].kindName
        default:
            return String(localized: "\(items.count) objets sélectionnés")
        }
    }

    private var detail: String? {
        if editing.isPlacingText {
            return String(localized: "Police, corps et couleur se choisissent dans le panneau. Échap pour annuler.")
        }
        guard editing.textTarget == nil, editing.cellTarget == nil, editing.newText == nil else { return nil }
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
            return Self.tableDetail(
                grid, cell: editing.currentCell, range: editing.selectedCells.isEmpty ? nil : editing.selectedBands,
                scanned: item.scanned
            )
        }
        if item.added, item.kind == .image {
            return String(localized: """
                faites glisser une poignée pour redimensionner (⇧ : sans garder les proportions) · \
                flèches pour déplacer, ⌫ pour supprimer
                """)
        }
        var parts: [String] = []
        if item.scanned {
            parts.append(String(localized: "retrouvé dans l'image scannée"))
        }
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
        if item.table != nil {
            parts.append(String(localized: "texte d'une cellule : il se déplace dans sa cellule"))
        }
        if item.resizable {
            parts.append(String(localized: "une poignée pour redimensionner"))
        }
        return parts.joined(separator: " · ")
    }

    /// Taille du tableau, cellule courante et marche à suivre.
    private static func tableDetail(
        _ grid: TableGridItem, cell: TableCellItem?,
        range: (rows: ClosedRange<UInt32>, columns: ClosedRange<UInt32>)?, scanned: Bool
    ) -> String {
        var parts = [String(localized: "\(grid.rows) lignes × \(grid.columns) colonnes")]
        if let range {
            parts.append(String(localized: """
                lignes \(range.rows.lowerBound + 1) à \(range.rows.upperBound + 1), \
                colonnes \(range.columns.lowerBound + 1) à \(range.columns.upperBound + 1)
                """))
        } else if let cell {
            parts.append(String(localized: "cellule ligne \(cell.row + 1), colonne \(cell.column + 1)"))
            if cell.overflow {
                parts.append(String(localized: "le texte saisi dépasse de la cellule"))
            }
        }
        if scanned {
            // Les lignes et les colonnes d'un tableau scanné ne se réorganisent pas.
            parts.append(String(localized: "tableau scanné : double-clic pour modifier le texte d'une cellule"))
        } else {
            parts.append(
                String(localized: """
                    double-clic pour modifier une cellule, ⇧-clic pour choisir plusieurs cellules, \
                    ⌥-clic sur un texte pour le déplacer dans sa cellule, \
                    menu Tableau pour leurs lignes et colonnes
                    """)
            )
        }
        return parts.joined(separator: " · ")
    }
}
