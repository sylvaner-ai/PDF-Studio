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
/// « Texte » commence un texte au clic ; un objet se redimensionne par ses
/// coins (cf. `EditingOverlayView+Adding.swift`). La sélection glissée sur
/// une autre page y passe, ⌥ en fait une copie ; le clic droit ouvre le
/// menu des objets (cf. `EditingOverlayView+Objects.swift`).
@MainActor
final class EditingOverlayView: NSView {
    weak var editing: PageEditing?
    private(set) var page: DisplayPage?
    /// Rotation de la page à l'écran.
    private(set) var rotation = DisplayRotation()
    private var hovered: UInt32?
    var drag: Drag?
    /// Saisie ouverte (cf. `EditingOverlayView+Editor.swift`) et bloc saisi.
    var editor: InlineTextEditor?
    var editorObject: UInt32?
    var editorCell: PageEditing.CellTarget?
    /// Texte à ajouter dont la saisie est ouverte.
    var editorNewText: PageEditing.NewText?
    /// Vrai si le pointeur montre l'outil « Texte » ou un coin d'objet.
    var showsToolCursor = false
    /// Point du dernier clic droit, sur la page droite : « Coller » y pose
    /// la copie (cf. `EditingOverlayView+Objects.swift`).
    var menuPoint: CGPoint?
    /// Éléments d'accessibilité des objets, gardés d'une lecture à l'autre
    /// (cf. `EditingOverlayView+Accessibility.swift`).
    var accessibilityObjects: [UInt32: PageObjectElement] = [:]

    enum Drag {
        /// Déplacement de la sélection depuis `start` ; il commence au-delà
        /// d'une petite distance, pour qu'un clic ne déplace rien.
        case move(start: CGPoint, offset: CGVector, started: Bool)
        /// Rectangle de sélection ; `base` : sélection à compléter (⇧).
        case marquee(start: CGPoint, current: CGPoint, base: Set<UInt32>)
        /// Redimensionnement d'un objet par son coin `corner`, de
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

    func focusPages() {
        if let canvas = enclosingScrollView as? PageCanvasView {
            window?.makeFirstResponder(canvas.pagesView)
        }
    }

    // MARK: - Souris

    override func mouseDown(with event: NSEvent) {
        // Valide aussi un texte en cours de saisie.
        focusPages()
        let point = convert(event.locationInWindow, from: nil)
        guard let editing, let page else { return }
        editing.clickedPage = page.id
        guard !beginAdding(at: point) else { return }
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
        if hit.kind == .table, clickTable(hit, at: point, count: event.clickCount, extend: extend) {
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
            // Un texte ajouté change de corps : il garde ses proportions.
            let free = event.modifierFlags.contains(.shift) && items.first { $0.id == object }?.addedText == nil
            let current = resized(frame, corner: corner, to: point, free: free)
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
            finishMove(by: offset, at: location(of: event), copy: event.modifierFlags.contains(.option))
        case let .marquee(start, current, base):
            let area = CGRect(
                x: min(start.x, current.x), y: min(start.y, current.y),
                width: abs(current.x - start.x), height: abs(current.y - start.y)
            )
            guard area.width >= screenPoint || area.height >= screenPoint else { return }
            let touched = hittableItems.filter { frame(of: $0).intersects(area) }.map(\.id)
            editing.select(base.union(touched), on: page.id)
        case let .resize(object, _, frame, current) where current != frame:
            editing.resizeObject(object, on: page.id, to: pageFrame(of: current))
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
            drawGrid(grid, current: editing.currentCell, range: editing.selectedCells, unit: unit)
        }
        if offset == .zero {
            drawResizeHandles(selected, unit: unit)
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
        let point = location(of: event)
        return tableMenu(at: point) ?? objectMenu(at: point)
    }
}
