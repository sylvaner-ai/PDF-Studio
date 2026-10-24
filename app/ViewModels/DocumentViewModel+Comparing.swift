import Foundation

/// Comparaison avant/après : les pages sont montrées sans les modifications
/// de leur contenu (textes, objets déplacés, retouches des scans) et, pour
/// les pages améliorées, dans leur version d'origine, le temps de les
/// comparer ; rien n'est modifiable pendant ce temps. Toute modification,
/// annulation comprise, ramène aux pages modifiées.
extension DocumentViewModel {
    /// Vrai si des pages modifiées peuvent être comparées à l'original.
    var canCompare: Bool {
        phase == .ready && workspace != .organizing
            && pages.contains { $0.editRevision != 0 || $0.replaced != nil }
    }

    /// Montre les pages d'origine ou les pages modifiées, selon `isComparing`.
    func showComparison() {
        // Le canevas d'abord : un texte en cours de saisie est validé.
        updateCanvasEditing()
        canvas.setPages(isComparing ? pages.map(\.original) : pages)
    }
}

extension DisplayPage {
    /// La même page, sans les modifications de son contenu ; pour une page
    /// améliorée, sa page d'origine (même format), dans son sens d'origine.
    var original: DisplayPage {
        guard let replaced else {
            var page = self
            page.editRevision = 0
            return page
        }
        return DisplayPage(id: id, source: replaced.source, quarterTurns: replaced.quarterTurns, baseSize: baseSize)
    }
}
