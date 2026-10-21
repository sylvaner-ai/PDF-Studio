import AppKit

/// Ajustement automatique du zoom, réappliqué quand la fenêtre change de taille.
enum FitMode: Sendable {
    /// Largeur de la fenêtre, sans dépasser une largeur de lecture confortable.
    case automatic
    /// Largeur de la fenêtre.
    case width
    /// Page entière visible.
    case page
}

/// Canevas des pages : défilement, zoom au pincement et barres de défilement
/// natifs de macOS (`NSScrollView`), avec un grossissement de 1 pour la taille
/// réelle (1 point PDF = 1 point à l'écran).
///
/// Seules les pages proches de la zone visible ont une vue, recyclée au fil du
/// défilement : un document de plusieurs centaines de pages reste léger.
@MainActor
final class PageCanvasView: NSScrollView {
    /// Paliers de zoom de ⌘+ et ⌘−.
    private static let zoomLevels: [CGFloat] = [
        0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5, 6, 8, 12, 16,
    ]
    /// Largeur de page au-delà de laquelle l'ajustement automatique s'arrête.
    private static let comfortablePageWidth: CGFloat = 1_100

    var onCurrentPageChange: (@MainActor (Int) -> Void)?
    private(set) var currentPage = 0

    private let renderer: PageRenderer
    private let pageSizes: [CGSize]
    private let pagesView = PagesDocumentView()
    private var layout: PageLayout
    private var rotation = DisplayRotation()
    private var mode = DisplayMode.continuous
    private var singlePage = 0
    private var fitMode: FitMode? = .automatic
    private var lastFitSize: CGSize = .zero
    private var isLiveMagnifying = false
    private var isFitting = false
    /// Page demandée explicitement (signet, vignette) : elle reste la page
    /// courante tant qu'elle est visible et que l'utilisateur n'a pas défilé.
    private var navigationTarget: Int?

    private var pageViews: [Int: PageTileView] = [:]
    private var reusableViews: [PageTileView] = []
    private var matches: [Int: [SearchMatch]] = [:]
    private var currentMatch: SearchHit?

    init(pageSizes: [CGSize], renderer: PageRenderer) {
        self.pageSizes = pageSizes
        self.renderer = renderer
        layout = PageLayout(pageSizes: pageSizes, rotation: DisplayRotation(), mode: .continuous)
        super.init(frame: .zero)

        contentView = CenteringClipView()
        drawsBackground = true
        backgroundColor = .underPageBackgroundColor
        hasVerticalScroller = true
        hasHorizontalScroller = true
        autohidesScrollers = true
        allowsMagnification = true
        minMagnification = Self.zoomLevels.first ?? 0.1
        maxMagnification = Self.zoomLevels.last ?? 16
        documentView = pagesView
        pagesView.canvas = self
        pagesView.frame = CGRect(origin: .zero, size: layout.documentSize)

        contentView.postsBoundsChangedNotifications = true
        let center = NotificationCenter.default
        center.addObserver(
            self, selector: #selector(clipBoundsDidChange(_:)),
            name: NSView.boundsDidChangeNotification, object: contentView
        )
        center.addObserver(
            self, selector: #selector(liveMagnifyWillStart(_:)),
            name: NSScrollView.willStartLiveMagnifyNotification, object: self
        )
        center.addObserver(
            self, selector: #selector(liveMagnifyDidEnd(_:)),
            name: NSScrollView.didEndLiveMagnifyNotification, object: self
        )
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) n'est pas pris en charge")
    }

    // MARK: - Commandes

    func zoom(by direction: Int) {
        fitMode = nil
        let current = magnification
        let target = direction > 0
            ? Self.zoomLevels.first(where: { $0 > current * 1.01 }) ?? maxMagnification
            : Self.zoomLevels.last(where: { $0 < current * 0.99 }) ?? minMagnification
        setMagnification(target, centeredAt: visibleCenter)
    }

    func actualSize() {
        fitMode = nil
        setMagnification(1, centeredAt: visibleCenter)
    }

    func fit(_ mode: FitMode) {
        fitMode = mode
        lastFitSize = .zero
        applyFit()
    }

    func setRotation(_ rotation: DisplayRotation) {
        guard rotation != self.rotation else { return }
        let anchor = currentAnchor()
        self.rotation = rotation
        rebuildLayout()
        refit(force: true)
        restore(anchor)
    }

    func setDisplayMode(_ mode: DisplayMode) {
        guard mode != self.mode else { return }
        let page = currentPage
        self.mode = mode
        singlePage = page
        rebuildLayout()
        refit(force: true)
        go(to: page)
    }

    /// Affiche une page, éventuellement à une position précise (en points depuis
    /// le haut de la page droite, comme les signets).
    func go(to page: Int, top: CGFloat? = nil, animated: Bool = false) {
        guard pageSizes.indices.contains(page) else { return }
        if mode == .singlePage, page != singlePage {
            singlePage = page
            rebuildLayout()
            refit(force: true)
        }
        let frame = layout.frames[page]
        var y = frame.minY - PageLayout.spacing
        if let top, let rotatedTop = rotation.applyTop(top, pageSize: pageSizes[page]) {
            y = frame.minY + rotatedTop - PageLayout.spacing
        }
        navigationTarget = page
        scroll(to: CGPoint(x: pagesView.visibleRect.minX, y: y), animated: animated)
        updateCurrentPage()
    }

    func goToNextPage() { go(to: min(currentPage + 1, pageSizes.count - 1)) }
    func goToPreviousPage() { go(to: max(currentPage - 1, 0)) }

    /// Occurrences à surligner, page par page, et occurrence courante.
    func showMatches(_ matches: [Int: [SearchMatch]], current: SearchHit?) {
        self.matches = matches
        currentMatch = current
        pageViews.values.forEach(applyMatches(to:))
    }

    /// Fait défiler juste assez pour montrer une occurrence.
    func reveal(_ hit: SearchHit) {
        guard let match = matches[hit.page]?[safe: hit.index], let first = match.rects.first else { return }
        if mode == .singlePage, hit.page != singlePage {
            go(to: hit.page)
        }
        let frame = layout.frames[hit.page]
        let target = rotation.apply(to: first.cgRect, pageSize: pageSizes[hit.page])
            .offsetBy(dx: frame.minX, dy: frame.minY)
        let visible = pagesView.visibleRect
        navigationTarget = hit.page
        guard !visible.insetBy(dx: 8, dy: visible.height * 0.1).contains(target) else {
            updateCurrentPage()
            return
        }
        let x = visible.width < pagesView.bounds.width ? target.midX - visible.width / 2 : visible.minX
        scroll(to: CGPoint(x: x, y: target.midY - visible.height / 3), animated: true)
        updateCurrentPage()
    }

    // MARK: - Clavier

    /// ⌘= et ⌘+ agrandissent (le « + » demande Majuscule sur un clavier AZERTY), ⌘− réduit.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if flags.subtracting(.shift) == .command, let key = event.charactersIgnoringModifiers {
            switch key {
            case "=", "+":
                zoom(by: 1)
                return true
            case "-":
                zoom(by: -1)
                return true
            default:
                break
            }
        }
        return super.performKeyEquivalent(with: event)
    }

    /// Touches de lecture, reçues de la vue des pages.
    func handleKey(_ event: NSEvent) -> Bool {
        let visible = pagesView.visibleRect
        let pageStep = visible.height - 4 * PageLayout.spacing
        let shift = event.modifierFlags.contains(.shift)
        let command = event.modifierFlags.contains(.command)
        switch Int(event.keyCode) {
        case 125 where command: // ⌘↓ : fin du document
            go(to: pageSizes.count - 1)
        case 126 where command: // ⌘↑ : début du document
            go(to: 0)
        case 49: // espace
            scrollOrTurn(by: shift ? -pageStep : pageStep)
        case 121: // page suivante
            scrollOrTurn(by: pageStep)
        case 116: // page précédente
            scrollOrTurn(by: -pageStep)
        case 125: // flèche bas
            scrollBy(dx: 0, dy: 48)
        case 126: // flèche haut
            scrollBy(dx: 0, dy: -48)
        case 123, 124: // flèches gauche et droite
            let forward = event.keyCode == 124
            if visible.width < pagesView.bounds.width - 1 {
                scrollBy(dx: forward ? 48 : -48, dy: 0)
            } else if forward {
                goToNextPage()
            } else {
                goToPreviousPage()
            }
        case 115: // début
            go(to: 0)
        case 119: // fin
            go(to: pageSizes.count - 1)
        default:
            return false
        }
        return true
    }

    override func scrollWheel(with event: NSEvent) {
        navigationTarget = nil
        super.scrollWheel(with: event)
    }

    // MARK: - Disposition et rendu

    override func tile() {
        super.tile()
        refit(force: false)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        refit(force: false)
        updateVisiblePages()
        let pagesView = self.pagesView
        Task { @MainActor in window.makeFirstResponder(pagesView) }
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        refreshPages()
    }

    @objc private func clipBoundsDidChange(_ notification: Notification) {
        updateVisiblePages()
    }

    @objc private func liveMagnifyWillStart(_ notification: Notification) {
        isLiveMagnifying = true
        fitMode = nil
    }

    @objc private func liveMagnifyDidEnd(_ notification: Notification) {
        isLiveMagnifying = false
        refreshPages()
    }

    private var visibleCenter: CGPoint {
        let visible = pagesView.visibleRect
        return CGPoint(x: visible.midX, y: visible.midY)
    }

    /// Réapplique l'ajustement automatique si la taille visible a changé,
    /// ou dans tous les cas avec `force` (nouvelle disposition des pages).
    private func refit(force: Bool) {
        guard fitMode != nil else { return }
        if force {
            lastFitSize = .zero
        }
        if contentSize != lastFitSize {
            applyFit()
        }
    }

    private func applyFit() {
        guard let fitMode, !isFitting, contentSize.width > 1, contentSize.height > 1, !pageSizes.isEmpty else {
            return
        }
        isFitting = true
        defer { isFitting = false }
        lastFitSize = contentSize
        let anchor = currentAnchor()
        let margin = PageLayout.margin
        let page = layout.frames[mode == .singlePage ? singlePage : currentPage]
        let target: CGFloat
        switch fitMode {
        case .automatic:
            let widest = layout.documentSize.width - 2 * margin
            target = min(contentSize.width / layout.documentSize.width, Self.comfortablePageWidth / max(widest, 1))
        case .width:
            target = contentSize.width / layout.documentSize.width
        case .page:
            target = min(
                contentSize.width / (page.width + 2 * margin),
                contentSize.height / (page.height + 2 * margin)
            )
        }
        // Un léger arrondi par défaut évite une barre de défilement horizontale superflue.
        magnification = max(minMagnification, min(maxMagnification, (target * 1_000).rounded(.down) / 1_000))
        if fitMode == .page {
            go(to: anchor.page)
        } else {
            restore(anchor)
        }
    }

    /// Position de lecture : page courante et fraction de sa hauteur au bord haut.
    private func currentAnchor() -> (page: Int, offset: CGFloat) {
        guard layout.frames.indices.contains(currentPage) else { return (0, 0) }
        let frame = layout.frames[currentPage]
        let offset = (pagesView.visibleRect.minY - frame.minY) / max(frame.height, 1)
        return (currentPage, offset)
    }

    private func restore(_ anchor: (page: Int, offset: CGFloat)) {
        guard layout.frames.indices.contains(anchor.page) else { return }
        let frame = layout.frames[anchor.page]
        let y = frame.minY + anchor.offset * frame.height
        scroll(to: CGPoint(x: pagesView.visibleRect.minX, y: y), animated: false)
        updateVisiblePages()
    }

    private func rebuildLayout() {
        layout = PageLayout(pageSizes: pageSizes, rotation: rotation, mode: mode, visiblePage: singlePage)
        pagesView.frame = CGRect(origin: .zero, size: layout.documentSize)
        for view in pageViews.values {
            view.cancelRendering()
            view.removeFromSuperview()
        }
        pageViews.removeAll()
        reusableViews.removeAll()
        updateVisiblePages()
    }

    private func scroll(to point: CGPoint, animated: Bool) {
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

    private func scrollBy(dx: CGFloat, dy: CGFloat) {
        navigationTarget = nil
        let visible = pagesView.visibleRect
        scroll(to: CGPoint(x: visible.minX + dx, y: visible.minY + dy), animated: false)
    }

    /// Défile d'un écran ; en mode page par page, passe à la page voisine au bord.
    private func scrollOrTurn(by dy: CGFloat) {
        let visible = pagesView.visibleRect
        if mode == .singlePage {
            if dy > 0, visible.maxY >= pagesView.bounds.maxY - 1, singlePage + 1 < pageSizes.count {
                return go(to: singlePage + 1)
            }
            if dy < 0, visible.minY <= pagesView.bounds.minY + 1, singlePage > 0 {
                go(to: singlePage - 1)
                let bottom = pagesView.bounds.maxY - pagesView.visibleRect.height
                return scroll(to: CGPoint(x: visible.minX, y: bottom), animated: false)
            }
        }
        scrollBy(dx: 0, dy: dy)
    }

    /// Crée les vues des pages proches de la zone visible et libère les autres.
    private func updateVisiblePages() {
        let visible = pagesView.visibleRect
        let wanted = Set(layout.pages(intersecting: visible.insetBy(dx: 0, dy: -max(visible.height, 200))))
        for (index, view) in pageViews where !wanted.contains(index) {
            view.cancelRendering()
            view.removeFromSuperview()
            pageViews[index] = nil
            if reusableViews.count < 8 {
                reusableViews.append(view)
            }
        }
        var added: [PageTileView] = []
        for index in wanted.sorted() where pageViews[index] == nil {
            let view = makePageView(index)
            view.frame = layout.frames[index]
            pagesView.addSubview(view)
            pageViews[index] = view
            applyMatches(to: view)
            added.append(view)
        }
        updateCurrentPage()
        // Pendant un pincement, seules les pages qui apparaissent sont rendues ;
        // les autres le seront à la fin du geste, à leur nouvelle résolution.
        refreshPages(isLiveMagnifying ? added : Array(pageViews.values))
    }

    private func makePageView(_ index: Int) -> PageTileView {
        let size = rotation.apply(to: pageSizes[index])
        if let view = reusableViews.popLast() {
            view.configure(page: index, pageCount: pageSizes.count, pageSize: size, rotation: rotation)
            return view
        }
        return PageTileView(
            page: index, pageCount: pageSizes.count, pageSize: size, rotation: rotation, renderer: renderer
        )
    }

    private func refreshPages(_ views: [PageTileView]? = nil) {
        let pixelsPerPoint = magnification * (window?.backingScaleFactor ?? 2)
        let visible = pagesView.visibleRect
        for view in views ?? Array(pageViews.values) {
            let shown = visible.intersection(view.frame)
            let local = shown.isNull ? .zero : shown.offsetBy(dx: -view.frame.minX, dy: -view.frame.minY)
            view.refresh(pixelsPerPoint: pixelsPerPoint, visibleRect: local)
        }
    }

    private func updateCurrentPage() {
        let page: Int
        if mode == .singlePage {
            page = singlePage
        } else if let target = navigationTarget, layout.frames[target].intersects(pagesView.visibleRect) {
            page = target
        } else {
            page = layout.currentPage(in: pagesView.visibleRect) ?? currentPage
        }
        guard page != currentPage else { return }
        currentPage = page
        onCurrentPageChange?(page)
    }

    private func applyMatches(to view: PageTileView) {
        let page = view.pageIndex
        let current = currentMatch?.page == page ? currentMatch?.index : nil
        view.showMatches(matches[page] ?? [], current: current, unrotatedSize: pageSizes[page])
    }
}

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

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
