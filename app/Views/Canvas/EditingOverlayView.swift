import AppKit

/// Sélection et déplacement des objets d'une page, en mode Édition.
///
/// Les objets viennent de ``PageEditing`` ; leurs emprises, données sur la page
/// droite, sont tournées comme la page à l'écran. Un clic sélectionne le plus
/// petit objet sous le pointeur (le fond de page est ignoré), ⇧-clic l'ajoute
/// ou le retire, un glisser déplace la sélection ou trace un rectangle de
/// sélection, un double-clic sur un texte ouvre sa saisie.
@MainActor
final class EditingOverlayView: NSView {
    weak var editing: PageEditing?
    private var page: DisplayPage?
    /// Rotation de la page à l'écran.
    private var rotation = DisplayRotation()
    private var hovered: UInt32?
    private var drag: Drag?
    private var editor: InlineTextEditor?
    private var editorObject: UInt32?

    private enum Drag {
        /// Déplacement de la sélection depuis `start` ; il commence au-delà
        /// d'une petite distance, pour qu'un clic ne déplace rien.
        case move(start: CGPoint, offset: CGVector, started: Bool)
        /// Rectangle de sélection ; `base` : sélection à compléter (⇧).
        case marquee(start: CGPoint, current: CGPoint, base: Set<UInt32>)
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
        } else if editor != nil {
            removeEditor()
        }
        needsDisplay = true
    }

    // MARK: - Géométrie

    private var items: [PageObjectItem] {
        guard let page, let editing else { return [] }
        return editing.objects(on: page) ?? []
    }

    /// Emprise d'un objet dans la vue.
    private func frame(of item: PageObjectItem) -> CGRect {
        guard let page else { return .zero }
        return rotation.apply(to: item.bounds.cgRect, pageSize: page.baseSize)
    }

    /// Un point de l'écran, en unités de la vue (qui suit le grossissement).
    private var screenPoint: CGFloat {
        max(abs(convert(NSSize(width: 1, height: 1), from: nil).width), 0.01)
    }

    /// Objet sous le pointeur : le plus petit, à égalité le plus haut placé.
    private func object(at point: CGPoint) -> PageObjectItem? {
        let tolerance = 3 * screenPoint
        return items.reversed()
            .filter { !$0.background && frame(of: $0).insetBy(dx: -tolerance, dy: -tolerance).contains(point) }
            .min { area(frame(of: $0)) < area(frame(of: $1)) }
    }

    private func area(_ rect: CGRect) -> CGFloat { rect.width * rect.height }

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
            let touched = items.filter { !$0.background && frame(of: $0).intersects(area) }.map(\.id)
            editing.select(base.union(touched), on: page.id)
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
        let id = object(at: convert(event.locationInWindow, from: nil))?.id
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

    private func stroke(_ rect: CGRect, _ color: NSColor, width: CGFloat, dashed: Bool = false) {
        let path = NSBezierPath(rect: rect)
        path.lineWidth = width
        if dashed {
            path.setLineDash([4 * width, 3 * width], count: 2, phase: 0)
        }
        color.setStroke()
        path.stroke()
    }

    // MARK: - Saisie du texte

    private func openEditor(for item: PageObjectItem) {
        removeEditor()
        let bounds = frame(of: item)
        // Le texte se saisit à l'horizontale, même sur une page tournée.
        let size = rotation.swapsAxes ? CGSize(width: bounds.height, height: bounds.width) : bounds.size
        let font = Self.font(for: item)
        let frame = CGRect(
            origin: bounds.origin,
            size: CGSize(width: max(size.width, 80), height: max(size.height, font.pointSize * 1.3))
        )
        let editor = InlineTextEditor(frame: frame)
        editor.configure(text: item.text ?? "", font: font)
        editor.onFinish = { [weak self] text in
            self?.editorDidFinish(text)
        }
        addSubview(editor)
        self.editor = editor
        editorObject = item.id
        window?.makeFirstResponder(editor)
        editor.selectAll(nil)
        needsDisplay = true
    }

    private func editorDidFinish(_ text: String?) {
        removeEditor()
        editing?.endTextEditing(text: text)
    }

    private func removeEditor() {
        guard let editor else { return }
        self.editor = nil
        editorObject = nil
        editor.onFinish = nil
        let focused = window?.firstResponder === editor
        editor.removeFromSuperview()
        if focused {
            focusPages()
        }
        needsDisplay = true
    }

    /// Police de saisie : celle du bloc si elle est installée sur le Mac.
    private static func font(for item: PageObjectItem) -> NSFont {
        let size = CGFloat(max(item.fontSize, 4))
        return item.displayFontName.flatMap { NSFont(name: $0, size: size) } ?? .systemFont(ofSize: size)
    }
}

/// Saisie du texte d'un bloc, posée sur la page : Retour valide, ⇧Retour ou
/// ⌥Retour passe à la ligne, Échap annule, un clic ailleurs valide.
@MainActor
final class InlineTextEditor: NSTextView, NSTextViewDelegate {
    /// Fin de la saisie : le texte, ou `nil` si elle est annulée.
    var onFinish: (@MainActor (String?) -> Void)?
    private var finished = false
    /// Historique propre à la saisie, distinct de celui du document.
    private lazy var typingUndo = UndoManager()

    func configure(text: String, font: NSFont) {
        string = text
        self.font = font
        textColor = .black
        insertionPointColor = .black
        backgroundColor = .white
        drawsBackground = true
        isRichText = false
        importsGraphics = false
        allowsUndo = true
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
        isAutomaticTextReplacementEnabled = false
        textContainerInset = .zero
        textContainer?.lineFragmentPadding = 0
        textContainer?.widthTracksTextView = true
        isHorizontallyResizable = false
        isVerticallyResizable = true
        minSize = frame.size
        maxSize = CGSize(width: frame.width, height: .greatestFiniteMagnitude)
        delegate = self
        setAccessibilityLabel(String(localized: "Texte du bloc"))
    }

    /// Termine la saisie, une seule fois.
    func finish(commit: Bool) {
        guard !finished else { return }
        finished = true
        onFinish?(commit ? string : nil)
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        if resigned, !finished {
            // Clic ailleurs : la saisie est validée une fois le changement de
            // répondeur terminé.
            finished = true
            let (text, onFinish) = (string, onFinish)
            Task { @MainActor in onFinish?(text) }
        }
        return resigned
    }

    func undoManager(for view: NSTextView) -> UndoManager? {
        typingUndo
    }

    func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            let flags = NSApp.currentEvent?.modifierFlags ?? []
            if flags.contains(.shift) || flags.contains(.option) {
                textView.insertNewlineIgnoringFieldEditor(nil)
            } else {
                finish(commit: true)
            }
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            finish(commit: false)
            return true
        default:
            return false
        }
    }
}
