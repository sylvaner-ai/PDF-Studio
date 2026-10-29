import AppKit

/// Sélection et déplacement des objets d'une page, en mode Édition.
///
/// Les objets viennent de ``PageEditing`` ; leurs emprises, données sur la page
/// droite, sont tournées comme la page à l'écran. Un clic sélectionne le plus
/// petit objet sous le pointeur (le fond de page est ignoré), ⇧-clic l'ajoute
/// ou le retire, un glisser déplace la sélection ou trace un rectangle de
/// sélection, un double-clic sur un texte ouvre sa saisie. Un tableau se
/// sélectionne d'un bloc, par-dessus ses textes et ses filets : le clic
/// désigne aussi sa cellule courante, un double-clic ouvre la saisie de la
/// cellule, un clic droit le menu de ses lignes et colonnes. Les mots
/// reconnus douteux d'une page scannée sont surlignés en orange. L'outil
/// « Texte » commence un texte au clic ; une image ajoutée se redimensionne
/// par ses coins (cf. `EditingOverlayView+Adding.swift`).
@MainActor
final class EditingOverlayView: NSView {
    weak var editing: PageEditing?
    private(set) var page: DisplayPage?
    /// Rotation de la page à l'écran.
    private(set) var rotation = DisplayRotation()
    private var hovered: UInt32?
    var drag: Drag?
    private(set) var editor: InlineTextEditor?
    private var editorObject: UInt32?
    var editorCell: PageEditing.CellTarget?
    /// Texte à ajouter dont la saisie est ouverte.
    var editorNewText: PageEditing.NewText?
    /// Vrai si le pointeur montre l'outil « Texte » ou un coin d'image.
    var showsToolCursor = false
    /// Éléments d'accessibilité des objets, gardés d'une lecture à l'autre
    /// (cf. `EditingOverlayView+Accessibility.swift`).
    var accessibilityObjects: [UInt32: PageObjectElement] = [:]

    enum Drag {
        /// Déplacement de la sélection depuis `start` ; il commence au-delà
        /// d'une petite distance, pour qu'un clic ne déplace rien.
        case move(start: CGPoint, offset: CGVector, started: Bool)
        /// Rectangle de sélection ; `base` : sélection à compléter (⇧).
        case marquee(start: CGPoint, current: CGPoint, base: Set<UInt32>)
        /// Redimensionnement d'une image ajoutée par son coin `corner`, de
        /// son cadre `frame` au cadre `current` (dans la vue).
        case resize(object: UInt32, corner: Corner, frame: CGRect, current: CGRect)
    }

    override var isFlipped: Bool { true }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    func configure(page: DisplayPage, rotation: DisplayRotation) {
        if let previous = self.page, previous.id != page.id {
            // Vue réutilisée pour une autre page.
            editor?.finish(commit: true)
            hovered = nil
            drag = nil
        }
        self.page = page
        self.rotation = rotation
        update()
    }

    /// Fin du mode Édition : un texte en cours de saisie est validé.
    func close() {
        editor?.finish(commit: true)
        removeEditor()
    }

    /// Les objets ou la sélection ont changé.
    func update() {
        guard let editing, let page else { return }
        if let target = editing.textTarget, target.page == page.id {
            if editorObject != target.object, let item = items.first(where: { $0.id == target.object }) {
                openEditor(for: item)
            }
        } else if let target = editing.newText, target.page == page.id {
            openNewTextEditor(target, style: editing.textStyle)
        } else if let target = editing.cellTarget, target.page == page.id {
            if editorCell != target {
                openCellEditor(for: target)
            }
        } else if editor != nil {
            removeEditor()
        }
        needsDisplay = true
        selectionDidChangeForAccessibility()
    }

    // MARK: - Géométrie

    var items: [PageObjectItem] {
        guard let page, let editing else { return [] }
        return editing.objects(on: page) ?? []
    }

    /// Emprise d'un objet dans la vue.
    func frame(of item: PageObjectItem) -> CGRect {
        guard let page else { return .zero }
        return rotation.apply(to: item.bounds.cgRect, pageSize: page.baseSize)
    }

    /// Un point de l'écran, en unités de la vue (qui suit le grossissement).
    var screenPoint: CGFloat {
        max(abs(convert(NSSize(width: 1, height: 1), from: nil).width), 0.01)
    }

    private func focusPages() {
        if let canvas = enclosingScrollView as? PageCanvasView {
            window?.makeFirstResponder(canvas.pagesView)
        }
    }

    // MARK: - Souris

    override func mouseDown(with event: NSEvent) {
        // Valide aussi un texte en cours de saisie.
        focusPages()
        guard let editing, let page else { return }
        let point = convert(event.locationInWindow, from: nil)
        if beginAdding(at: point) {
            return
        }
        let extend = event.modifierFlags.contains(.shift)
        guard let hit = object(at: point) else {
            let base = extend && editing.page == page.id ? editing.selected : []
            if !extend {
                editing.clearSelection()
            }
            drag = .marquee(start: point, current: point, base: base)
            return
        }
        if event.clickCount == 2, hit.editable, !extend {
            editing.beginTextEditing(page: page.id, object: hit.id)
            return
        }
        if hit.kind == .table, !extend, let cell = cell(at: point, in: hit) {
            if event.clickCount == 2 {
                editing.beginCellEditing(cell)
                return
            }
            editing.selectCell(cell)
            drag = .move(start: point, offset: .zero, started: false)
            return
        }
        let isSelected = editing.page == page.id && editing.selected.contains(hit.id)
        if extend {
            editing.select([hit.id], on: page.id, extend: true)
        } else if !isSelected {
            editing.select([hit.id], on: page.id)
        }
        if editing.page == page.id, editing.selected.contains(hit.id) {
            drag = .move(start: point, offset: .zero, started: false)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        switch drag {
        case let .move(start, _, started):
            let offset = CGVector(dx: point.x - start.x, dy: point.y - start.y)
            guard started || hypot(offset.dx, offset.dy) >= 3 * screenPoint else { return }
            drag = .move(start: start, offset: offset, started: true)
        case let .marquee(start, _, base):
            drag = .marquee(start: start, current: point, base: base)
        case let .resize(object, corner, frame, _):
            let current = resized(frame, corner: corner, to: point, free: event.modifierFlags.contains(.shift))
            drag = .resize(object: object, corner: corner, frame: frame, current: current)
        case nil:
            return
        }
        autoscroll(with: event)
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        defer {
            drag = nil
            needsDisplay = true
        }
        guard let editing, let page else { return }
        switch drag {
        case let .move(_, offset, true):
            editing.moveSelection(by: rotation.unapply(to: offset))
        case let .marquee(start, current, base):
            let area = CGRect(
                x: min(start.x, current.x), y: min(start.y, current.y),
                width: abs(current.x - start.x), height: abs(current.y - start.y)
            )
            guard area.width >= screenPoint || area.height >= screenPoint else { return }
            let touched = hittableItems.filter { frame(of: $0).intersects(area) }.map(\.id)
            editing.select(base.union(touched), on: page.id)
        case let .resize(object, _, frame, current) where current != frame:
            editing.resizeImage(object, on: page.id, to: pageFrame(of: current))
        default:
            break
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(
            rect: .zero,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self
        ))
    }

    override func mouseMoved(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        updateToolCursor(at: point)
        let id = object(at: point)?.id
        guard id != hovered else { return }
        hovered = id
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        guard hovered != nil else { return }
        hovered = nil
        needsDisplay = true
    }

    // MARK: - Dessin

    override func draw(_ dirtyRect: NSRect) {
        guard let editing, let page else { return }
        let unit = screenPoint
        let accent = NSColor.controlAccentColor
        let selected = editing.selectedObjects(on: page)
        let selectedIDs = Set(selected.map(\.id))
        drawDoubtfulWords(editing.doubtfulWords(on: page), page: page, unit: unit)

        if drag == nil, let hovered, !selectedIDs.contains(hovered),
           let item = items.first(where: { $0.id == hovered }) {
            stroke(frame(of: item).insetBy(dx: -2 * unit, dy: -2 * unit), accent.withAlphaComponent(0.5), width: unit)
        }

        var offset = CGVector.zero
        if case let .move(_, value, true) = drag {
            offset = value
        }
        for item in selected where item.id != editorObject {
            let outline = frame(of: item).insetBy(dx: -2 * unit, dy: -2 * unit)
            guard item.movable else {
                // Objet verrouillé : sélectionnable pour l'inspecteur seulement.
                stroke(outline, .secondaryLabelColor, width: unit, dashed: true)
                continue
            }
            accent.withAlphaComponent(0.08).setFill()
            NSBezierPath(rect: outline).fill()
            stroke(outline, accent, width: 1.5 * unit)
            if offset.dx != 0 || offset.dy != 0 {
                stroke(outline.offsetBy(dx: offset.dx, dy: offset.dy), accent, width: unit, dashed: true)
            }
        }

        if offset == .zero, selected.count == 1, let table = selected.first, let grid = table.grid {
            drawGrid(grid, current: editing.currentCell, unit: unit)
        }
        if offset == .zero {
            drawImageHandles(selected, unit: unit)
        }

        if case let .marquee(start, current, _) = drag {
            let area = CGRect(
                x: min(start.x, current.x), y: min(start.y, current.y),
                width: abs(current.x - start.x), height: abs(current.y - start.y)
            )
            accent.withAlphaComponent(0.1).setFill()
            NSBezierPath(rect: area).fill()
            stroke(area, accent, width: unit)
        }

        if let editor {
            stroke(editor.frame.insetBy(dx: -2 * unit, dy: -2 * unit), accent, width: 1.5 * unit)
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        tableMenu(at: convert(event.locationInWindow, from: nil))
    }

    // MARK: - Saisie du texte

    private func openEditor(for item: PageObjectItem) {
        if let style = item.addedText {
            // Un texte ajouté se lit droit à l'écran : sa saisie le recouvre.
            openEditor(frame: frame(of: item), text: item.text ?? "", font: style.nsFont, color: style.nsColor)
        } else {
            openEditor(bounds: frame(of: item), text: item.text ?? "", font: Self.font(for: item))
        }
        editorObject = item.id
    }

    func openEditor(bounds: CGRect, text: String, font: NSFont) {
        // Le texte se saisit à l'horizontale, même sur une page tournée.
        let size = rotation.swapsAxes ? CGSize(width: bounds.height, height: bounds.width) : bounds.size
        openEditor(frame: CGRect(origin: bounds.origin, size: size), text: text, font: font)
    }

    /// Ouvre la saisie dans le cadre `frame` de la vue (agrandi au besoin) ;
    /// `color` : couleur du texte saisi. Un texte ajouté (`color` donnée)
    /// s'élargit au fil de la saisie, sans renvoi à la ligne.
    func openEditor(frame area: CGRect, text: String, font: NSFont, color: NSColor? = nil) {
        removeEditor()
        let box = CGRect(
            origin: area.origin,
            size: CGSize(width: max(area.width, 80), height: max(area.height, font.pointSize * 1.3))
        )
        let editor = InlineTextEditor(frame: box)
        editor.configure(text: text, font: font, color: color ?? .black, grows: color != nil)
        editor.onFinish = { [weak self] text in
            self?.editorDidFinish(text)
        }
        addSubview(editor)
        self.editor = editor
        window?.makeFirstResponder(editor)
        editor.selectAll(nil)
        needsDisplay = true
    }

    private func editorDidFinish(_ text: String?) {
        let cell = editorCell != nil
        let new = editorNewText != nil
        removeEditor()
        if new {
            editing?.endNewText(text: text)
        } else if cell {
            editing?.endCellEditing(text: text)
        } else {
            editing?.endTextEditing(text: text)
        }
    }

    private func removeEditor() {
        guard let editor else { return }
        self.editor = nil
        editorObject = nil
        editorCell = nil
        editorNewText = nil
        editor.onFinish = nil
        let focused = window?.firstResponder === editor
        editor.removeFromSuperview()
        if focused {
            focusPages()
        }
        needsDisplay = true
    }
}
