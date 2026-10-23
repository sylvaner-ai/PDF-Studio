import Foundation

/// Pages scannées en mode Édition : réglages du texte réécrit d'une ligne
/// (police candidate, corps, graisse, interlettrage, flou, grain, couleur).
///
/// La police et les réglages sont retrouvés sur la ligne à sa première
/// modification ; chaque changement est une modification annulable (⌘Z).
extension PageEditing {
    /// Ligne scannée sélectionnée seule, dont le texte a été réécrit.
    var selectedScanLine: PageObjectItem? {
        let items = selectedItems
        guard items.count == 1, let item = items.first, item.scanned, item.scan != nil else { return nil }
        return item
    }

    /// Applique des réglages au texte réécrit de la ligne sélectionnée.
    func setScanStyle(_ style: ScanStyleItem) {
        guard let line = selectedScanLine, line.scan?.style != style else { return }
        updateScanLine { try $0.setScanStyle(page: $1, object: line.id, style: style) }
    }

    /// Réécrit la ligne sélectionnée avec la police candidate de rang `index`.
    func chooseScanCandidate(_ index: Int) {
        guard let line = selectedScanLine, let scan = line.scan, scan.candidates.indices.contains(index) else {
            return
        }
        updateScanLine { try $0.chooseScanCandidate(page: $1, object: line.id, candidate: UInt32(index)) }
    }

    /// Rend à la ligne sélectionnée les réglages mesurés sur le scan.
    func resetScanStyle() {
        guard let line = selectedScanLine, line.scan?.style != line.scan?.measured else { return }
        updateScanLine { try $0.resetScanStyle(page: $1, object: line.id) }
    }

    private func updateScanLine(_ change: (EditSession, UInt64) throws -> PageObjectItem) {
        guard let page, let model else { return }
        var result: PageObjectItem?
        guard model.perform({ result = try change($0, page) }), let result else { return }
        notice = Self.notice(for: result)
    }
}

extension ScanTextItem {
    /// Rang de la police candidate employée, `nil` si aucune ne l'est.
    var chosenCandidate: Int? {
        candidates.firstIndex { $0.font == style.font }
    }
}
