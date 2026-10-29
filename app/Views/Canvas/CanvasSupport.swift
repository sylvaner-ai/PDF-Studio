import AppKit

/// Vue qui porte les pages ; elle reçoit le clavier pour la lecture.
@MainActor
final class PagesDocumentView: NSView {
    weak var canvas: PageCanvasView? {
        didSet { text.canvas = canvas }
    }
    /// Texte sélectionné à la souris (mode Lecture).
    let text = TextSelection()

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        if canvas?.handleKey(event) != true {
            super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        // Clic entre les pages : plus rien n'est sélectionné.
        canvas?.editing?.clearSelection()
        text.clear()
        super.mouseDown(with: event)
    }

    /// Objets des pages, en mode Édition.
    private var editing: PageEditing? { canvas?.editing }

    /// Édition > Copier : le texte sélectionné, ou en mode Édition les objets
    /// sélectionnés.
    @objc func copy(_ sender: Any?) {
        if let editing {
            editing.copySelection()
        } else {
            text.copySelection()
        }
    }

    /// Édition > Couper, Coller et Supprimer : les objets, en mode Édition.
    @objc func cut(_ sender: Any?) {
        editing?.cutSelection()
    }

    @objc func paste(_ sender: Any?) {
        editing?.paste()
    }

    @objc func delete(_ sender: Any?) {
        editing?.deleteSelection()
    }
}

extension PagesDocumentView: NSMenuItemValidation {
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(copy(_:)):
            return editing.map(\.canCopy) ?? text.hasSelection
        case #selector(cut(_:)):
            return editing?.canCopy == true
        case #selector(paste(_:)):
            return editing?.canPaste == true
        case #selector(delete(_:)):
            return editing?.selectedItems.contains(where: \.movable) == true
        default:
            return true
        }
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
