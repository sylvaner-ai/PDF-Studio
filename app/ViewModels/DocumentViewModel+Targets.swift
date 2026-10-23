import Foundation

// MARK: - Navigation

extension DocumentViewModel {
    /// Position actuelle de la page `index` du document ouvert (destination d'un
    /// signet) ; `nil` si elle a été supprimée.
    func position(ofOriginalPage index: Int) -> Int? {
        let source = DisplayPage.Source(document: 0, index: index)
        return pages.firstIndex { $0.source == source }
    }

    func go(to node: OutlineNode) {
        guard let page = node.page, let position = position(ofOriginalPage: page) else { return }
        canvas.go(to: position, top: node.top)
    }
}

// MARK: - Pages visées par les commandes

extension DocumentViewModel {
    /// Pages visées par les commandes. Dans la grille : la sélection. En lecture :
    /// plusieurs vignettes sélectionnées, sinon la page lue (la sélection ne la
    /// suit pas quand la barre latérale montre les signets).
    var targetPositions: [Int] {
        let selected = positions(of: selection)
        if workspace == .organizing || selected.count > 1 {
            return selected
        }
        return pages.indices.contains(canvas.currentPage) ? [canvas.currentPage] : selected
    }

    /// Nombre de pages visées (``targetPositions``), sans lire la page courante :
    /// les menus qui en dépendent ne sont pas recalculés à chaque page qui défile.
    var targetCount: Int {
        if workspace == .organizing || selectionCount > 1 {
            return positions(of: selection).count
        }
        return pages.isEmpty ? 0 : 1
    }

    /// Les pages visées peuvent être supprimées (il en reste au moins une).
    var canDeleteTargets: Bool {
        canEdit && targetCount > 0 && targetCount < pages.count
    }

    /// Positions des pages désignées par leur identifiant, dans l'ordre du document.
    func positions(of ids: Set<UInt64>) -> [Int] {
        pages.indices.filter { ids.contains(pages[$0].id) }
    }
}
