import CoreGraphics
import os

/// Une page du document affiché : page d'un document source ou page vierge,
/// avec la rotation ajoutée par l'organisation.
///
/// L'identifiant suit la page quand elle se déplace : les vignettes, la sélection
/// et le cache des images restent valables après une réorganisation.
struct DisplayPage: Identifiable, Hashable, Sendable {
    /// Page `index` du document source `document` (0 : le document ouvert).
    struct Source: Hashable, Sendable {
        let document: Int
        let index: Int
    }

    let id: UInt64
    /// Page affichée ; `nil` pour une page vierge.
    let source: Source?
    /// Quarts de tour dans le sens horaire ajoutés à la rotation propre de la page.
    let quarterTurns: Int
    /// Format de la page avant ces quarts de tour, en points (rotation propre du
    /// PDF comprise, comme la mesure le moteur).
    let baseSize: CGSize

    var isBlank: Bool { source == nil }

    /// Rotation à appliquer à la page, rotation d'affichage comprise.
    func rotation(_ display: DisplayRotation) -> DisplayRotation {
        display.rotated(by: 90 * quarterTurns)
    }

    /// Format de la page à l'écran.
    func size(_ display: DisplayRotation) -> CGSize {
        rotation(display).apply(to: baseSize)
    }
}

extension DisplayPage {
    /// Page décrite par le moteur ; `sizes` donne le format des pages de chaque
    /// document source.
    init(_ item: PageItem, sizes: [[CGSize]]) {
        id = item.id
        quarterTurns = Int(item.quarterTurns)
        switch item.kind {
        case let .source(document, index):
            source = Source(document: Int(document), index: Int(index))
            let pages = sizes.indices.contains(Int(document)) ? sizes[Int(document)] : []
            baseSize = pages.indices.contains(Int(index)) ? pages[Int(index)] : CGSize(width: 595, height: 842)
        case let .blank(size):
            source = nil
            baseSize = size.cgSize
        }
    }
}

/// Documents dont proviennent les pages affichées : le document ouvert, puis
/// ceux dont des pages ont été insérées. Partagé entre le fil principal et les
/// rendus.
final class PageSources: Sendable {
    private let documents: OSAllocatedUnfairLock<[PdfDocument]>

    init(_ document: PdfDocument) {
        documents = OSAllocatedUnfairLock(initialState: [document])
    }

    func document(_ index: Int) -> PdfDocument? {
        documents.withLock { $0.indices.contains(index) ? $0[index] : nil }
    }

    /// Ajoute un document ; renvoie son numéro.
    @discardableResult
    func append(_ document: PdfDocument) -> Int {
        documents.withLock { documents in
            documents.append(document)
            return documents.count - 1
        }
    }
}
