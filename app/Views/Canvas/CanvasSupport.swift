import AppKit

/// Vue qui porte les pages ; elle reçoit le clavier pour la lecture.
@MainActor
final class PagesDocumentView: NSView {
    weak var canvas: PageCanvasView?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        if canvas?.handleKey(event) != true {
            super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        super.mouseDown(with: event)
    }
}

/// Garde le document centré quand il est plus petit que la fenêtre.
final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var rect = super.constrainBoundsRect(proposedBounds)
        guard let documentView else { return rect }
        let document = documentView.frame
        if rect.width > document.width {
            rect.origin.x = document.minX - (rect.width - document.width) / 2
        }
        if rect.height > document.height {
            rect.origin.y = document.minY - (rect.height - document.height) / 2
        }
        return rect
    }
}

extension NSScrollView {
    /// Fait défiler le contenu pour amener `point` au coin haut gauche de la zone
    /// visible, dans les limites du document. L'animation est brève et omise si
    /// « Réduire les animations » est activé.
    func scrollContent(to point: CGPoint, animated: Bool) {
        let clip = contentView
        let proposed = CGRect(origin: point, size: clip.bounds.size)
        let origin = clip.constrainBoundsRect(proposed).origin
        guard animated, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else {
            clip.scroll(to: origin)
            reflectScrolledClipView(clip)
            return
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            context.allowsImplicitAnimation = true
            clip.animator().setBoundsOrigin(origin)
        } completionHandler: { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.reflectScrolledClipView(self.contentView)
            }
        }
    }
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
