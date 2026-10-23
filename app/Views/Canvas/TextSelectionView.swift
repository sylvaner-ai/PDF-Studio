import AppKit

/// Texte sélectionné d'une page, posé sur son image, en mode Lecture.
///
/// La vue surligne la sélection (couleur d'accentuation du système, assez
/// translucide pour laisser lire le texte), transforme le pointeur en barre de
/// texte au-dessus des lignes et transmet la souris au canevas, qui tient la
/// sélection pour tout le document. En mode Édition, elle laisse passer la
/// souris aux objets de la page.
@MainActor
final class TextSelectionView: NSView {
    private var highlights: [CGRect] = []
    private var lines: [CGRect] = []
    private var isSelectable = false

    override var isFlipped: Bool { true }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    /// Sélection à surligner et zone de chaque ligne de texte (coordonnées de la page à l'écran).
    func update(highlights: [CGRect], lines: [CGRect], isSelectable: Bool) {
        if highlights != self.highlights {
            self.highlights = highlights
            needsDisplay = true
        }
        if lines != self.lines || isSelectable != self.isSelectable {
            self.lines = lines
            self.isSelectable = isSelectable
            window?.invalidateCursorRects(for: self)
        }
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        isSelectable ? super.hitTest(point) : nil
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlAccentColor.withAlphaComponent(0.3).setFill()
        for rect in highlights where rect.intersects(dirtyRect) {
            NSBezierPath.fill(rect)
        }
    }

    override func resetCursorRects() {
        guard isSelectable else { return }
        for line in lines {
            addCursorRect(line.intersection(bounds), cursor: .iBeam)
        }
    }

    // MARK: - Souris

    private var canvas: PageCanvasView? { enclosingScrollView as? PageCanvasView }

    override func mouseDown(with event: NSEvent) { canvas?.textMouseDown(event) }
    override func mouseDragged(with event: NSEvent) { canvas?.textMouseDragged(event) }
    override func mouseUp(with event: NSEvent) { canvas?.textMouseUp(event) }
}
