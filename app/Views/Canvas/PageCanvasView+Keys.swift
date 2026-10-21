import AppKit

// MARK: - Lecture au clavier

extension PageCanvasView {
    /// Pas de défilement des flèches, en points.
    private static let lineStep: CGFloat = 48

    /// Touches de lecture, reçues de la vue des pages.
    func handleKey(_ event: NSEvent) -> Bool {
        guard let key = ReadingKey(event) else { return false }
        let visible = pagesView.visibleRect
        let screen = visible.height - 4 * PageLayout.spacing
        switch key {
        case let .documentEdge(end):
            go(to: end ? pages.count - 1 : 0)
        case let .screen(down):
            scrollOrTurn(by: down ? screen : -screen)
        case let .line(down):
            scrollContent(by: CGVector(dx: 0, dy: down ? Self.lineStep : -Self.lineStep))
        case let .side(right) where visible.width < pagesView.bounds.width - 1:
            scrollContent(by: CGVector(dx: right ? Self.lineStep : -Self.lineStep, dy: 0))
        case let .side(right):
            if right {
                goToNextPage()
            } else {
                goToPreviousPage()
            }
        }
        return true
    }

    private func scrollContent(by offset: CGVector) {
        navigationTarget = nil
        let visible = pagesView.visibleRect
        scrollContent(to: CGPoint(x: visible.minX + offset.dx, y: visible.minY + offset.dy), animated: false)
    }

    /// Défile d'un écran ; en mode page par page, passe à la page voisine au bord.
    private func scrollOrTurn(by distance: CGFloat) {
        let visible = pagesView.visibleRect
        if mode == .singlePage {
            if distance > 0, visible.maxY >= pagesView.bounds.maxY - 1, singlePage + 1 < pages.count {
                return go(to: singlePage + 1)
            }
            if distance < 0, visible.minY <= pagesView.bounds.minY + 1, singlePage > 0 {
                go(to: singlePage - 1)
                let bottom = pagesView.bounds.maxY - pagesView.visibleRect.height
                return scrollContent(to: CGPoint(x: visible.minX, y: bottom), animated: false)
            }
        }
        scrollContent(by: CGVector(dx: 0, dy: distance))
    }
}
