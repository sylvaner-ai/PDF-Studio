import CoreGraphics
import Observation

/// Pilote le canevas des pages depuis SwiftUI (menus, barre latérale, recherche)
/// et publie la page courante.
@MainActor
@Observable
final class CanvasController {
    let pageCount: Int
    private(set) var currentPage = 0
    private(set) var rotation = DisplayRotation()
    private(set) var displayMode = DisplayMode.continuous

    @ObservationIgnored private weak var canvas: PageCanvasView?
    @ObservationIgnored private var matches: [Int: [SearchMatch]] = [:]
    @ObservationIgnored private var currentMatch: SearchHit?

    init(pageCount: Int) {
        self.pageCount = pageCount
    }

    func attach(_ canvas: PageCanvasView) {
        self.canvas = canvas
        canvas.setRotation(rotation)
        canvas.setDisplayMode(displayMode)
        canvas.showMatches(matches, current: currentMatch)
        canvas.onCurrentPageChange = { [weak self] page in
            self?.currentPage = page
        }
    }

    var canGoBack: Bool { currentPage > 0 }
    var canGoForward: Bool { currentPage + 1 < pageCount }

    func zoomIn() { canvas?.zoom(by: 1) }
    func zoomOut() { canvas?.zoom(by: -1) }
    func actualSize() { canvas?.actualSize() }
    func fit(_ mode: FitMode) { canvas?.fit(mode) }

    func rotate(clockwise: Bool) {
        rotation = rotation.rotated(by: clockwise ? 90 : -90)
        canvas?.setRotation(rotation)
    }

    func setDisplayMode(_ mode: DisplayMode) {
        displayMode = mode
        canvas?.setDisplayMode(mode)
    }

    func go(to page: Int, top: CGFloat? = nil) {
        canvas?.go(to: page, top: top, animated: false)
    }

    func goToNextPage() { canvas?.goToNextPage() }
    func goToPreviousPage() { canvas?.goToPreviousPage() }
    func goToFirstPage() { go(to: 0) }
    func goToLastPage() { go(to: pageCount - 1) }

    /// Surligne les occurrences ; `reveal` fait défiler jusqu'à l'occurrence courante.
    func showMatches(_ matches: [Int: [SearchMatch]], current: SearchHit?, reveal: Bool) {
        self.matches = matches
        currentMatch = current
        canvas?.showMatches(matches, current: current)
        if reveal, let current {
            canvas?.reveal(current)
        }
    }
}
