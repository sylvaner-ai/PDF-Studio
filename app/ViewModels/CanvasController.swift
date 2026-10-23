import CoreGraphics
import Observation

/// Pilote le canevas des pages depuis SwiftUI (menus, barre latérale, recherche)
/// et publie la page courante.
@MainActor
@Observable
final class CanvasController {
    private(set) var pages: [DisplayPage]
    private(set) var currentPage = 0
    private(set) var rotation = DisplayRotation()
    private(set) var displayMode = DisplayMode.continuous

    @ObservationIgnored private weak var canvas: PageCanvasView?
    @ObservationIgnored private var matches: [Int: [SearchMatch]] = [:]
    @ObservationIgnored private var currentMatch: SearchHit?
    @ObservationIgnored private var editing: PageEditing?

    init(pages: [DisplayPage]) {
        self.pages = pages
        canGoForward = pages.count > 1
    }

    var pageCount: Int { pages.count }

    func attach(_ canvas: PageCanvasView) {
        self.canvas = canvas
        canvas.setPages(pages)
        canvas.setRotation(rotation)
        canvas.setDisplayMode(displayMode)
        canvas.start(at: currentPage)
        canvas.showMatches(matches, current: currentMatch)
        canvas.setEditing(editing)
        canvas.onCurrentPageChange = { [weak self] page in
            self?.show(page)
        }
    }

    /// Page précédente et page suivante possibles : gardés à part de
    /// `currentPage` pour que les menus ne soient recalculés qu'au début et à la
    /// fin du document, et non à chaque page qui défile.
    private(set) var canGoBack = false
    private(set) var canGoForward = false

    /// Publie la page lue.
    private func show(_ page: Int) {
        if page != currentPage {
            currentPage = page
        }
        let (back, forward) = (page > 0, page + 1 < pages.count)
        if back != canGoBack {
            canGoBack = back
        }
        if forward != canGoForward {
            canGoForward = forward
        }
    }

    func zoomIn() { canvas?.zoom(by: 1) }
    func zoomOut() { canvas?.zoom(by: -1) }
    func actualSize() { canvas?.actualSize() }
    func fit(_ mode: FitMode) { canvas?.fit(mode) }

    /// Fait pivoter l'affichage de toutes les pages, sans modifier le document.
    func rotateDisplay(clockwise: Bool) {
        rotation = rotation.rotated(by: clockwise ? 90 : -90)
        canvas?.setRotation(rotation)
    }

    /// Pages réorganisées : le canevas reste sur la page lue si elle existe encore.
    func setPages(_ pages: [DisplayPage]) {
        guard pages != self.pages else { return }
        let currentID = self.pages[safe: currentPage]?.id
        self.pages = pages
        matches = [:]
        currentMatch = nil
        if let canvas {
            canvas.setPages(pages)
            show(canvas.currentPage)
        } else if let index = currentID.flatMap({ id in pages.firstIndex { $0.id == id } }) {
            show(index)
        } else {
            show(min(currentPage, pages.count - 1))
        }
    }

    /// Mode Édition : les objets des pages se sélectionnent et se modifient
    /// dans le canevas ; `nil` revient à la lecture.
    func setEditing(_ editing: PageEditing?) {
        self.editing = editing
        canvas?.setEditing(editing)
    }

    func setDisplayMode(_ mode: DisplayMode) {
        displayMode = mode
        canvas?.setDisplayMode(mode)
    }

    func go(to page: Int, top: CGFloat? = nil) {
        guard let canvas else {
            // Canevas masqué (grille d'organisation) : il s'ouvrira sur cette page.
            if pages.indices.contains(page) {
                show(page)
            }
            return
        }
        canvas.go(to: page, top: top, animated: false)
    }

    /// Rend le clavier aux pages (après la fermeture de la recherche).
    func focusPages() {
        guard let canvas else { return }
        canvas.window?.makeFirstResponder(canvas.pagesView)
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
