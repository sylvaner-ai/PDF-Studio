import AppKit

/// Sélection du texte des pages à la souris, en mode Lecture, et copie.
///
/// Le texte vient du moteur, page après page, au moment où la page apparaît :
/// texte natif et texte reconnu (OCR), enregistré ou non. Un clic place le
/// point d'insertion, un glisser sélectionne (jusque sur d'autres pages, avec
/// défilement automatique), ⇧-clic étend, un double-clic prend le mot et un
/// triple-clic la ligne. ⌘C copie le texte sélectionné, une ligne par ligne.
///
/// Une position est un rang de caractère dans le texte d'une page ; elle suit
/// l'identifiant de la page, pas sa place, et la sélection survit donc à une
/// réorganisation. Elle est abandonnée si le texte de la page change.
@MainActor
final class TextSelection {
    /// Un point d'insertion dans le texte d'une page.
    struct Position: Equatable {
        let page: UInt64
        let offset: Int
    }

    /// Ce que le glisser sélectionne à chaque pas.
    private enum Granularity {
        case character, word, line
    }

    /// Texte lu pour une page, avec l'état de la page à ce moment : il ne vaut
    /// plus si la page a été modifiée ou reconnue depuis.
    private struct Entry {
        let layout: PageTextLayout
        let page: DisplayPage

        func isValid(for current: DisplayPage) -> Bool {
            page.editRevision == current.editRevision && page.textRevision == current.textRevision
        }
    }

    /// Point visé : sa page, le texte de la page et le caractère le plus proche.
    private struct Target {
        let page: DisplayPage
        let layout: PageTextLayout
        let hit: PageTextLayout.Hit
    }

    /// Lecture du texte d'une page par le moteur ; sans elle, rien ne se sélectionne.
    var source: (@Sendable (UInt64) async -> [SelectableLine])? {
        didSet { canvas?.refreshText() }
    }
    weak var canvas: PageCanvasView?

    private(set) var anchor: Position?
    private(set) var focus: Position?
    private var entries: [UInt64: Entry] = [:]
    private var loading: [UInt64: Task<Void, Never>] = [:]
    private var granularity = Granularity.character
    /// Mot ou ligne du premier clic : le glisser s'y ajoute.
    private var base: (start: Position, end: Position)?
    private var autoscroll: Timer?
    private var lastEvent: NSEvent?

    /// Du texte est sélectionné.
    var hasSelection: Bool {
        anchor != nil && anchor != focus
    }

    // MARK: - Texte des pages

    /// Texte de la page, s'il est lu et à jour.
    private func cachedLayout(of page: DisplayPage) -> PageTextLayout? {
        guard let entry = entries[page.id] else { return nil }
        guard entry.isValid(for: page) else {
            entries[page.id] = nil
            if anchor?.page == page.id || focus?.page == page.id {
                clear()
            }
            return nil
        }
        return entry.layout
    }

    /// Lit le texte de la page si ce n'est pas fait ; la page se met à jour à la fin.
    func load(_ page: DisplayPage) {
        guard source != nil, !page.isBlank, cachedLayout(of: page) == nil, loading[page.id] == nil else { return }
        loading[page.id] = Task { [weak self] in
            _ = await self?.read(page)
            self?.loading[page.id] = nil
            self?.canvas?.refreshText()
        }
    }

    /// Texte de la page, lu s'il le faut.
    private func read(_ page: DisplayPage) async -> PageTextLayout? {
        if let known = cachedLayout(of: page) {
            return known
        }
        guard let source else { return nil }
        let lines = await source(page.id)
        // La page a pu changer pendant la lecture.
        guard canvas?.pages.first(where: { $0.id == page.id }) == page else { return nil }
        let text = PageTextLayout(lines)
        entries[page.id] = Entry(layout: text, page: page)
        trimEntries()
        return text
    }

    /// Garde le texte des pages récemment lues, et de celles de la sélection.
    private func trimEntries() {
        let limit = 64
        guard entries.count > limit else { return }
        var kept = Set(canvas?.pageViews.values.map(\.pageID) ?? [])
        kept.formUnion([anchor?.page, focus?.page].compactMap { $0 })
        for id in Array(entries.keys) where !kept.contains(id) && entries.count > limit {
            entries[id] = nil
        }
    }

    // MARK: - Ce que montrent les pages

    /// Rectangles surlignés sur la page (page droite).
    func highlights(on page: DisplayPage) -> [CGRect] {
        guard let layout = cachedLayout(of: page),
              let range = selectedRange(on: page, layout: layout) else { return [] }
        return layout.highlights(in: range)
    }

    /// Emprise de chaque ligne de texte de la page (page droite) : là où le
    /// pointeur devient une barre de texte.
    func lineBounds(on page: DisplayPage) -> [CGRect] {
        cachedLayout(of: page)?.lines.map(\.bounds) ?? []
    }

    /// Caractères sélectionnés sur la page.
    private func selectedRange(on page: DisplayPage, layout: PageTextLayout) -> Range<Int>? {
        guard let (start, end) = orderedSelection(), let pages = canvas?.pages,
              let first = pages.firstIndex(where: { $0.id == start.page }),
              let last = pages.firstIndex(where: { $0.id == end.page }),
              let index = pages.firstIndex(where: { $0.id == page.id }),
              index >= first, index <= last else { return nil }
        let lower = index == first ? start.offset : 0
        let upper = index == last ? end.offset : layout.count
        let range = max(lower, 0)..<min(upper, layout.count)
        return range.lowerBound < range.upperBound ? range : nil
    }

    /// Début et fin de la sélection, dans l'ordre du document.
    private func orderedSelection() -> (Position, Position)? {
        guard let anchor, let focus, anchor != focus else { return nil }
        return isBefore(focus, anchor) ? (focus, anchor) : (anchor, focus)
    }

    private func isBefore(_ lhs: Position, _ rhs: Position) -> Bool {
        guard lhs.page != rhs.page, let pages = canvas?.pages,
              let left = pages.firstIndex(where: { $0.id == lhs.page }),
              let right = pages.firstIndex(where: { $0.id == rhs.page }) else {
            return lhs.offset < rhs.offset
        }
        return left < right
    }

    // MARK: - Souris

    /// Clic en `point` (coordonnées de la vue des pages).
    func mouseDown(at point: CGPoint, clickCount: Int, extend: Bool) {
        lastEvent = nil
        guard let target = target(at: point) else {
            clear()
            return
        }
        granularity = clickCount >= 3 ? .line : (clickCount == 2 ? .word : .character)
        let range = characters(of: target)
        let start = Position(page: target.page.id, offset: range.lowerBound)
        let end = Position(page: target.page.id, offset: range.upperBound)
        if extend, let anchor, granularity == .character {
            focus = Position(page: target.page.id, offset: target.hit.offset)
            base = (anchor, anchor)
        } else {
            (anchor, focus) = (start, end)
            base = (start, end)
        }
        canvas?.refreshText()
    }

    /// Le pointeur se déplace, bouton enfoncé.
    func mouseDragged(to point: CGPoint, event: NSEvent? = nil) {
        updateFocus(at: point)
        // Sans événement, pas de défilement automatique.
        guard let event else { return }
        lastEvent = event
        if autoscroll == nil {
            let timer = Timer(timeInterval: 1.0 / 30, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.autoscrollStep() }
            }
            RunLoop.main.add(timer, forMode: .common)
            autoscroll = timer
        }
    }

    func mouseUp() {
        autoscroll?.invalidate()
        autoscroll = nil
        lastEvent = nil
    }

    private func updateFocus(at point: CGPoint) {
        guard let target = target(at: point), let base else { return }
        let range = characters(of: target)
        let start = Position(page: target.page.id, offset: range.lowerBound)
        let end = Position(page: target.page.id, offset: range.upperBound)
        if isBefore(start, base.start) {
            (anchor, focus) = (base.end, start)
        } else {
            (anchor, focus) = (base.start, end)
        }
        canvas?.refreshText()
    }

    /// Fait défiler tant que le pointeur reste hors de la zone visible.
    private func autoscrollStep() {
        guard let canvas, let event = lastEvent else { return }
        _ = canvas.pagesView.autoscroll(with: event)
        updateFocus(at: canvas.pagesView.convert(event.locationInWindow, from: nil))
    }

    /// Caractères visés selon le nombre de clics : aucun (point d'insertion),
    /// un mot ou une ligne.
    private func characters(of target: Target) -> Range<Int> {
        switch granularity {
        case .character: return target.hit.offset..<target.hit.offset
        case .word: return target.layout.wordRange(containing: target.hit.unit)
        case .line: return target.layout.lineRange(containing: target.hit.unit)
        }
    }

    /// Page et caractère les plus proches de `point` (coordonnées de la vue des pages).
    private func target(at point: CGPoint) -> Target? {
        guard let canvas else { return nil }
        var nearest: (index: Int, distance: CGFloat)?
        for (index, frame) in canvas.layout.frames.enumerated() where !frame.isEmpty {
            let distance = frame.squaredDistance(to: point)
            if distance < nearest?.distance ?? .infinity {
                nearest = (index, distance)
            }
        }
        guard let index = nearest?.index, let page = canvas.pages[safe: index],
              let layout = cachedLayout(of: page) else { return nil }
        let frame = canvas.layout.frames[index]
        let local = CGPoint(x: point.x - frame.minX, y: point.y - frame.minY)
        let straight = page.rotation(canvas.rotation).unapply(to: local, pageSize: page.baseSize)
        return layout.hit(at: straight).map { Target(page: page, layout: layout, hit: $0) }
    }

    // MARK: - Fin de la sélection et copie

    func clear() {
        mouseUp()
        guard anchor != nil || focus != nil else { return }
        anchor = nil
        focus = nil
        base = nil
        canvas?.refreshText()
    }

    /// Texte sélectionné ; les pages de la sélection dont le texte n'est pas
    /// encore lu le sont d'abord.
    func selectedText() async -> String? {
        guard let (start, end) = orderedSelection(), let pages = canvas?.pages,
              let first = pages.firstIndex(where: { $0.id == start.page }),
              let last = pages.firstIndex(where: { $0.id == end.page }), first <= last else { return nil }
        var parts: [String] = []
        for page in pages[first...last] {
            guard let layout = await read(page), let range = selectedRange(on: page, layout: layout) else { continue }
            parts.append(layout.text(in: range))
        }
        let text = parts.joined(separator: "\n")
        return text.isEmpty ? nil : text
    }

    /// Place le texte sélectionné dans le presse-papiers.
    func copySelection() {
        Task { [weak self] in
            guard let text = await self?.selectedText() else { return }
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            pasteboard.setString(text, forType: .string)
        }
    }
}
