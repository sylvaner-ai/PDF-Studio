import AppKit

// MARK: - Textes et images ajoutés

extension EditingOverlayView {
    /// Poignée d'un objet, qu'on fait glisser pour le redimensionner : un
    /// coin (les deux côtés, proportions gardées) ou le milieu d'un côté
    /// (largeur ou hauteur seule).
    enum Corner: CaseIterable {
        case topLeft, top, topRight, left, right, bottomLeft, bottom, bottomRight

        /// -1 si la poignée tire le bord gauche, 1 le bord droit, 0 aucun.
        var horizontal: Int {
            switch self {
            case .topLeft, .left, .bottomLeft: return -1
            case .topRight, .right, .bottomRight: return 1
            case .top, .bottom: return 0
            }
        }

        /// -1 si la poignée tire le bord haut, 1 le bord bas, 0 aucun.
        var vertical: Int {
            switch self {
            case .topLeft, .top, .topRight: return -1
            case .bottomLeft, .bottom, .bottomRight: return 1
            case .left, .right: return 0
            }
        }

        var isCorner: Bool { horizontal != 0 && vertical != 0 }

        /// Position de la poignée sur le cadre `frame`.
        func point(of frame: CGRect) -> CGPoint {
            let x = horizontal < 0 ? frame.minX : (horizontal > 0 ? frame.maxX : frame.midX)
            let y = vertical < 0 ? frame.minY : (vertical > 0 ? frame.maxY : frame.midY)
            return CGPoint(x: x, y: y)
        }
    }

    /// Plus petit côté d'un objet redimensionné, en points.
    private static let minimumSide: CGFloat = 4

    /// Clic qui commence un texte (outil « Texte ») ou le redimensionnement
    /// d'un objet ; renvoie `false` s'il ne fait ni l'un ni l'autre.
    func beginAdding(at point: CGPoint) -> Bool {
        guard let editing, let page else { return false }
        if editing.isPlacingText {
            editing.beginNewText(page: page.id, at: rotation.unapply(to: point, pageSize: page.baseSize))
            return true
        }
        guard let handle = objectHandle(at: point) else { return false }
        let box = frame(of: handle.object)
        drag = .resize(object: handle.object.id, corner: handle.corner, frame: box, current: box)
        return true
    }

    /// Objet redimensionnable sélectionné seul sur cette page, et le coin
    /// sous le pointeur.
    private func objectHandle(at point: CGPoint) -> (object: PageObjectItem, corner: Corner)? {
        guard let editing, let page, let object = editing.selectedResizable, editing.page == page.id else {
            return nil
        }
        let box = frame(of: object)
        let reach = 6 * screenPoint
        let corner = Self.handles(for: object).first { corner in
            let handle = corner.point(of: box)
            return abs(handle.x - point.x) <= reach && abs(handle.y - point.y) <= reach
        }
        return corner.map { (object: object, corner: $0) }
    }

    /// Poignées de l'objet : les coins, et le milieu des côtés sauf pour un
    /// texte ajouté (il change de corps, proportions gardées).
    static func handles(for item: PageObjectItem) -> [Corner] {
        item.addedText == nil ? Corner.allCases : Corner.allCases.filter(\.isCorner)
    }

    /// Cadre `frame` redimensionné en tirant son coin `corner` jusqu'à
    /// `point` : le coin opposé reste en place, les proportions aussi, sauf
    /// si `free` (⇧).
    func resized(_ frame: CGRect, corner: Corner, to point: CGPoint, free: Bool) -> CGRect {
        let (sideX, sideY) = (corner.horizontal, corner.vertical)
        let minimum = Self.minimumSide
        var width = frame.width
        var height = frame.height
        if sideX != 0 {
            width = max(sideX < 0 ? frame.maxX - point.x : point.x - frame.minX, 0)
        }
        if sideY != 0 {
            height = max(sideY < 0 ? frame.maxY - point.y : point.y - frame.minY, 0)
        }
        if corner.isCorner, !free, frame.width > 0, frame.height > 0 {
            let scale = max(width / frame.width, height / frame.height)
            width = frame.width * scale
            height = frame.height * scale
        }
        if corner.isCorner, !free, width < minimum || height < minimum {
            // Trop petite : ramenée au plus petit côté permis, proportions gardées.
            let ratio = frame.width / max(frame.height, 0.01)
            if ratio >= 1 {
                height = minimum
                width = minimum * ratio
            } else {
                width = minimum
                height = minimum / ratio
            }
        }
        width = max(width, minimum)
        height = max(height, minimum)
        return CGRect(
            x: sideX < 0 ? frame.maxX - width : frame.minX,
            y: sideY < 0 ? frame.maxY - height : frame.minY,
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

    /// Poignées de l'objet redimensionnable sélectionné, et son nouveau cadre
    /// pendant qu'on le redimensionne.
    func drawResizeHandles(_ selected: [PageObjectItem], unit: CGFloat) {
        guard selected.count == 1, let item = selected.first, item.resizable, item.movable else { return }
        var box = frame(of: item)
        if case let .resize(object, _, _, current) = drag, object == item.id {
            stroke(current, .controlAccentColor, width: unit, dashed: true)
            box = current
        }
        let side = 7 * unit
        for corner in Self.handles(for: item) {
            let center = corner.point(of: box)
            let handle = CGRect(x: center.x - side / 2, y: center.y - side / 2, width: side, height: side)
            NSColor.white.setFill()
            NSBezierPath(rect: handle).fill()
            stroke(handle, .controlAccentColor, width: unit)
        }
    }

    /// Pointeur de l'outil « Texte », ou d'un coin d'objet.
    func updateToolCursor(at point: CGPoint) {
        let cursor: NSCursor?
        if editing?.isPlacingText == true {
            cursor = .iBeam
        } else if objectHandle(at: point) != nil {
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
