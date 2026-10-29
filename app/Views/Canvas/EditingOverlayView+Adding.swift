import AppKit

// MARK: - Textes et images ajoutés

extension EditingOverlayView {
    /// Coin d'une image ajoutée, qu'on fait glisser pour la redimensionner.
    enum Corner: CaseIterable {
        case topLeft, topRight, bottomLeft, bottomRight

        var isLeft: Bool { self == .topLeft || self == .bottomLeft }
        var isTop: Bool { self == .topLeft || self == .topRight }

        /// Position du coin sur le cadre `frame`.
        func point(of frame: CGRect) -> CGPoint {
            CGPoint(x: isLeft ? frame.minX : frame.maxX, y: isTop ? frame.minY : frame.maxY)
        }
    }

    /// Plus petit côté d'une image redimensionnée, en points.
    private static let minimumSide: CGFloat = 4

    /// Clic qui commence un texte (outil « Texte ») ou le redimensionnement
    /// d'une image ajoutée ; renvoie `false` s'il ne fait ni l'un ni l'autre.
    func beginAdding(at point: CGPoint) -> Bool {
        guard let editing, let page else { return false }
        if editing.isPlacingText {
            editing.beginNewText(page: page.id, at: rotation.unapply(to: point, pageSize: page.baseSize))
            return true
        }
        guard let handle = imageHandle(at: point) else { return false }
        let box = frame(of: handle.image)
        drag = .resize(object: handle.image.id, corner: handle.corner, frame: box, current: box)
        return true
    }

    /// Image ajoutée sélectionnée seule sur cette page, et le coin sous le
    /// pointeur.
    private func imageHandle(at point: CGPoint) -> (image: PageObjectItem, corner: Corner)? {
        guard let editing, let page, let image = editing.selectedAddedImage, editing.page == page.id else {
            return nil
        }
        let box = frame(of: image)
        let reach = 6 * screenPoint
        let corner = Corner.allCases.first { corner in
            let handle = corner.point(of: box)
            return abs(handle.x - point.x) <= reach && abs(handle.y - point.y) <= reach
        }
        return corner.map { (image: image, corner: $0) }
    }

    /// Cadre `frame` redimensionné en tirant son coin `corner` jusqu'à
    /// `point` : le coin opposé reste en place, les proportions aussi, sauf
    /// si `free` (⇧).
    func resized(_ frame: CGRect, corner: Corner, to point: CGPoint, free: Bool) -> CGRect {
        let anchor = CGPoint(x: corner.isLeft ? frame.maxX : frame.minX, y: corner.isTop ? frame.maxY : frame.minY)
        var width = max(corner.isLeft ? anchor.x - point.x : point.x - anchor.x, 0)
        var height = max(corner.isTop ? anchor.y - point.y : point.y - anchor.y, 0)
        if !free, frame.width > 0, frame.height > 0 {
            let scale = max(width / frame.width, height / frame.height)
            width = frame.width * scale
            height = frame.height * scale
        }
        let minimum = Self.minimumSide
        let ratio = frame.width / max(frame.height, 0.01)
        if width < minimum || height < minimum {
            // Trop petite : ramenée au plus petit côté permis.
            if free {
                width = max(width, minimum)
                height = max(height, minimum)
            } else if ratio >= 1 {
                height = minimum
                width = minimum * ratio
            } else {
                width = minimum
                height = minimum / ratio
            }
        }
        return CGRect(
            x: corner.isLeft ? anchor.x - width : anchor.x,
            y: corner.isTop ? anchor.y - height : anchor.y,
            width: width,
            height: height
        )
    }

    /// Cadre de la vue, sur la page droite.
    func pageFrame(of rect: CGRect) -> CGRect {
        guard let page else { return rect }
        let first = rotation.unapply(to: CGPoint(x: rect.minX, y: rect.minY), pageSize: page.baseSize)
        let second = rotation.unapply(to: CGPoint(x: rect.maxX, y: rect.maxY), pageSize: page.baseSize)
        return CGRect(
            x: min(first.x, second.x), y: min(first.y, second.y),
            width: abs(second.x - first.x), height: abs(second.y - first.y)
        )
    }

    /// Poignées de l'image ajoutée sélectionnée, et son nouveau cadre pendant
    /// qu'on la redimensionne.
    func drawImageHandles(_ selected: [PageObjectItem], unit: CGFloat) {
        guard selected.count == 1, let image = selected.first, image.added, image.kind == .image else { return }
        var box = frame(of: image)
        if case let .resize(object, _, _, current) = drag, object == image.id {
            stroke(current, .controlAccentColor, width: unit, dashed: true)
            box = current
        }
        let side = 7 * unit
        for corner in Corner.allCases {
            let center = corner.point(of: box)
            let handle = CGRect(x: center.x - side / 2, y: center.y - side / 2, width: side, height: side)
            NSColor.white.setFill()
            NSBezierPath(rect: handle).fill()
            stroke(handle, .controlAccentColor, width: unit)
        }
    }

    /// Pointeur de l'outil « Texte », ou d'un coin d'image ajoutée.
    func updateToolCursor(at point: CGPoint) {
        let cursor: NSCursor?
        if editing?.isPlacingText == true {
            cursor = .iBeam
        } else if imageHandle(at: point) != nil {
            cursor = .crosshair
        } else {
            cursor = nil
        }
        if let cursor {
            cursor.set()
            showsToolCursor = true
        } else if showsToolCursor {
            NSCursor.arrow.set()
            showsToolCursor = false
        }
    }

    /// Ouvre la saisie du texte à ajouter, ou met à jour sa police et sa
    /// couleur si elle est déjà ouverte.
    func openNewTextEditor(_ target: PageEditing.NewText, style: AddedTextStyle) {
        guard let page else { return }
        if editorNewText == target, let editor {
            editor.font = style.nsFont
            editor.textColor = style.nsColor
            return
        }
        let corner = rotation.apply(to: CGRect(origin: target.point, size: .zero), pageSize: page.baseSize).origin
        let font = style.nsFont
        openEditor(
            frame: CGRect(origin: corner, size: CGSize(width: font.pointSize * 6, height: 0)),
            text: "", font: font, color: style.nsColor
        )
        editorNewText = target
    }
}
