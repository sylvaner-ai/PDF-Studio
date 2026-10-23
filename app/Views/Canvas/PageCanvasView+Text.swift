import AppKit

// MARK: - Sélection du texte

extension PageCanvasView {
    /// Texte surligné et zone de texte de chaque page affichée.
    func refreshText() {
        pageViews.values.forEach(refreshText(of:))
    }

    /// Montre sur la page la sélection du texte et lit son texte s'il le faut.
    /// En mode Édition, les pages n'ont ni sélection ni curseur de texte.
    func refreshText(of view: PageTileView) {
        guard editing == nil, let page = pages[safe: view.pageIndex] else {
            view.showText(highlights: [], lines: [], isSelectable: false)
            return
        }
        pagesView.text.load(page)
        let text = pagesView.text
        view.showText(highlights: text.highlights(on: page), lines: text.lineBounds(on: page), isSelectable: true)
    }

    // MARK: Souris

    func textMouseDown(_ event: NSEvent) {
        window?.makeFirstResponder(pagesView)
        let point = pagesView.convert(event.locationInWindow, from: nil)
        let extend = event.modifierFlags.contains(.shift)
        pagesView.text.mouseDown(at: point, clickCount: event.clickCount, extend: extend)
    }

    func textMouseDragged(_ event: NSEvent) {
        let point = pagesView.convert(event.locationInWindow, from: nil)
        pagesView.text.mouseDragged(to: point, event: event)
    }

    func textMouseUp(_ event: NSEvent) {
        pagesView.text.mouseUp()
    }
}
