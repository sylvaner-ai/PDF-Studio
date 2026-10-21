import AppKit

/// Canevas des pages : défilement, zoom au pincement et barres de défilement
/// natifs de macOS (`NSScrollView`), avec un grossissement de 1 pour la taille
/// réelle (1 point PDF = 1 point à l'écran).
///
/// Seules les pages proches de la zone visible ont une vue, recyclée au fil du
/// défilement : un document de plusieurs centaines de pages reste léger.
@MainActor
final class PageCanvasView: NSScrollView {
    /// Pas de défilement des flèches, en points.
    private static let lineStep: CGFloat = 48

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
        minMagnification = Zoom.levels.first ?? 0.1
        maxMagnification = Zoom.levels.last ?? 16
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

    // MARK: - Événements AppKit

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard let direction = Zoom.direction(for: event) else {
            return super.performKeyEquivalent(with: event)
        }
        zoom(by: direction)
        return true
    }

    override func scrollWheel(with event: NSEvent) {
        navigationTarget = nil
        super.scrollWheel(with: event)
    }

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
}

// MARK: - Commandes

extension PageCanvasView {
    func zoom(by direction: Int) {
        fitMode = nil
        setMagnification(Zoom.step(from: magnification, direction: direction), centeredAt: visibleCenter)
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
        scrollContent(to: CGPoint(x: pagesView.visibleRect.minX, y: y), animated: animated)
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
        scrollContent(to: CGPoint(x: x, y: target.midY - visible.height / 3), animated: true)
        updateCurrentPage()
    }

    /// Touches de lecture, reçues de la vue des pages.
    func handleKey(_ event: NSEvent) -> Bool {
        guard let key = ReadingKey(event) else { return false }
        let visible = pagesView.visibleRect
        let screen = visible.height - 4 * PageLayout.spacing
        switch key {
        case let .documentEdge(end):
            go(to: end ? pageSizes.count - 1 : 0)
        case let .screen(down):
            scrollOrTurn(by: down ? screen : -screen)
        case let .line(down):
            scrollContent(by: CGVector(dx: 0, dy: down ? Self.lineStep : -Self.lineStep))
        case let .side(right) where visible.width < pagesView.bounds.width - 1:
            scrollContent(by: CGVector(dx: right ? Self.lineStep : -Self.lineStep, dy: 0))
        case let .side(right):
            if right {
                goToNextPage()
            } else {
                goToPreviousPage()
            }
        }
        return true
    }
}

// MARK: - Disposition et rendu

extension PageCanvasView {
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
        let page = mode == .singlePage ? singlePage : currentPage
        let target = layout.fitMagnification(fitMode, viewport: contentSize, page: page)
        magnification = max(minMagnification, min(maxMagnification, target))
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
        scrollContent(to: CGPoint(x: pagesView.visibleRect.minX, y: y), animated: false)
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

    private func scrollContent(by offset: CGVector) {
        navigationTarget = nil
        let visible = pagesView.visibleRect
        scrollContent(to: CGPoint(x: visible.minX + offset.dx, y: visible.minY + offset.dy), animated: false)
    }

    /// Défile d'un écran ; en mode page par page, passe à la page voisine au bord.
    private func scrollOrTurn(by distance: CGFloat) {
        let visible = pagesView.visibleRect
        if mode == .singlePage {
            if distance > 0, visible.maxY >= pagesView.bounds.maxY - 1, singlePage + 1 < pageSizes.count {
                return go(to: singlePage + 1)
            }
            if distance < 0, visible.minY <= pagesView.bounds.minY + 1, singlePage > 0 {
                go(to: singlePage - 1)
                let bottom = pagesView.bounds.maxY - pagesView.visibleRect.height
                return scrollContent(to: CGPoint(x: visible.minX, y: bottom), animated: false)
            }
        }
        scrollContent(by: CGVector(dx: 0, dy: distance))
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
