import AppKit

// MARK: - Recherche

extension PageCanvasView {
    /// Occurrences à surligner, page par page, et occurrence courante.
    func showMatches(_ matches: [Int: [SearchMatch]], current: SearchHit?) {
        self.matches = matches
        currentMatch = current
        pageViews.values.forEach(applyMatches(to:))
    }

    /// Fait défiler juste assez pour montrer une occurrence.
    func reveal(_ hit: SearchHit) {
        guard let match = matches[hit.page]?[safe: hit.index], let first = match.rects.first,
              let page = pages[safe: hit.page] else { return }
        if mode == .singlePage, hit.page != singlePage {
            go(to: hit.page)
        }
        let frame = layout.frames[hit.page]
        let target = page.rotation(rotation).apply(to: first.cgRect, pageSize: page.baseSize)
            .offsetBy(dx: frame.minX, dy: frame.minY)
        let visible = pagesView.visibleRect
        navigationTarget = hit.page
        guard !visible.insetBy(dx: 8, dy: visible.height * 0.1).contains(target) else {
            updateCurrentPage()
            return
        }
        let x = visible.width < pagesView.bounds.width ? target.midX - visible.width / 2 : visible.minX
        scrollContent(to: CGPoint(x: x, y: target.midY - visible.height / 3), animated: true)
        updateCurrentPage()
    }

    func applyMatches(to view: PageTileView) {
        let page = view.pageIndex
        let current = currentMatch?.page == page ? currentMatch?.index : nil
        view.showMatches(matches[page] ?? [], current: current)
    }
}
