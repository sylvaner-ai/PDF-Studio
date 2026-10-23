import Foundation

/// Comparaison avant/après : les pages sont montrées sans les modifications
/// de leur contenu (textes, objets déplacés, retouches des scans), le temps de
/// les comparer ; rien n'est modifiable pendant ce temps. Toute modification,
/// annulation comprise, ramène aux pages modifiées.
extension DocumentViewModel {
    /// Vrai si des pages modifiées peuvent être comparées à l'original.
    var canCompare: Bool {
        phase == .ready && workspace != .organizing && pages.contains { $0.editRevision != 0 }
    }

    /// Montre les pages d'origine ou les pages modifiées, selon `isComparing`.
    func showComparison() {
        // Le canevas d'abord : un texte en cours de saisie est validé.
        canvas.setEditing(workspace == .editing && !isComparing ? editing : nil)
        canvas.setPages(isComparing ? pages.map(\.original) : pages)
    }
}

extension DisplayPage {
    /// La même page, sans les modifications de son contenu.
    var original: DisplayPage {
        var page = self
        page.editRevision = 0
        return page
    }
}
