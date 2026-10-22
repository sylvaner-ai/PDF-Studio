import AppKit
import CoreImage

/// Une page du canevas.
///
/// Elle affiche d'abord une image de la page entière, rendue à la résolution de
/// l'écran dans la limite de ``PageTileView/maxBasePixels`` ; au-delà (fort
/// grossissement), des tuiles nettes de la seule zone visible viennent se poser
/// par-dessus. Les occurrences de la recherche sont surlignées en mode
/// « produit », qui garde le texte lisible.
@MainActor
final class PageTileView: NSView {
    /// Plus grand côté de l'image de page entière, en pixels.
    static let maxBasePixels: CGFloat = 4_096
    /// Côté des tuiles de détail, en pixels.
    private static let tilePixels: CGFloat = 1_024
    /// Pas d'arrondi de la largeur rendue, pour ne pas relancer un rendu à chaque pixel.
    private static let pixelStep = 32

    /// Position de la page dans le document.
    private(set) var pageIndex: Int
    private var pageCount: Int
    private let renderer: PageRenderer
    private var page: DisplayPage
    private var display: DisplayRotation
    /// Taille de la page tournée, en points.
    private var pageSize: CGSize
    /// Rotation de la page à l'écran, la sienne et celle de l'affichage.
    private var rotation: DisplayRotation { page.rotation(display) }

    private let baseView = NSImageView()
    private let detailView = FlippedView()
    private let highlightView = HighlightView()
    /// Sélection des objets, en mode Édition.
    private var overlay: EditingOverlayView?

    private var basePixelWidth = 0
    private var baseTask: Task<Void, Never>?
    private var detailScale: CGFloat = 0
    private var detailTiles: [TileKey: NSImageView] = [:]
    private var detailTasks: [TileKey: Task<Void, Never>] = [:]

    private struct TileKey: Hashable {
        let column: Int
        let row: Int
    }

    init(position: Int, page: DisplayPage, pageCount: Int, display: DisplayRotation, renderer: PageRenderer) {
        self.pageIndex = position
        self.page = page
        self.pageCount = pageCount
        self.display = display
        self.pageSize = page.size(display)
        self.renderer = renderer
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = NSColor.white.cgColor
        layer?.shadowColor = NSColor.black.cgColor
        layer?.shadowOpacity = 0.12
        layer?.shadowRadius = 6
        layer?.shadowOffset = CGSize(width: 0, height: -2)

        setFrameSize(pageSize)
        for view in [baseView, detailView, highlightView] as [NSView] {
            view.frame = bounds
            view.autoresizingMask = [.width, .height]
            addSubview(view)
        }
        baseView.imageScaling = .scaleAxesIndependently
        baseView.imageFrameStyle = .none
        baseView.isEditable = false
        baseView.setAccessibilityElement(false)
        highlightView.compositingFilter = CIFilter(name: "CIMultiplyBlendMode")
        updateAccessibility()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) n'est pas pris en charge")
    }

    override var isFlipped: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        layer?.shadowPath = CGPath(rect: CGRect(origin: .zero, size: newSize), transform: nil)
    }

    /// Réaffecte la vue à une autre page (réutilisation lors du défilement).
    func configure(position: Int, page: DisplayPage, pageCount: Int, display: DisplayRotation) {
        let moved = position != pageIndex || pageCount != self.pageCount
        pageIndex = position
        self.pageCount = pageCount
        if moved {
            updateAccessibility()
        }
        guard page != self.page || display != self.display else { return }
        if display == self.display, page.hasSameLayout(as: self.page) {
            // Contenu modifié : l'image actuelle reste affichée jusqu'au
            // nouveau rendu, que demandera le prochain `refresh`.
            self.page = page
            baseTask?.cancel()
            basePixelWidth = 0
            clearDetail()
            overlay?.configure(page: page, rotation: rotation)
            return
        }
        self.page = page
        self.display = display
        pageSize = page.size(display)
        setFrameSize(pageSize)
        baseTask?.cancel()
        basePixelWidth = 0
        baseView.image = nil
        clearDetail()
        highlightView.update(matches: [], current: nil)
        overlay?.configure(page: page, rotation: rotation)
        updateAccessibility()
    }

    /// Identifiant de la page affichée.
    var pageID: UInt64 { page.id }

    /// Mode Édition (`editing` non nul) ou lecture.
    func setEditing(_ editing: PageEditing?) {
        guard let editing else {
            overlay?.close()
            overlay?.removeFromSuperview()
            overlay = nil
            return
        }
        let overlay = self.overlay ?? EditingOverlayView(frame: bounds)
        if self.overlay == nil {
            overlay.autoresizingMask = [.width, .height]
            addSubview(overlay)
            self.overlay = overlay
        }
        overlay.editing = editing
        overlay.configure(page: page, rotation: rotation)
    }

    /// Les objets ou la sélection de la page ont changé.
    func editingDidChange() {
        overlay?.update()
    }

    /// Ajuste la netteté de l'affichage au grossissement courant.
    ///
    /// - Parameters:
    ///   - pixelsPerPoint: grossissement × facteur de l'écran (2 sur Retina).
    ///   - visibleRect: partie visible de la page, dans ses coordonnées.
    func refresh(pixelsPerPoint: CGFloat, visibleRect: CGRect) {
        updateBase(pixelsPerPoint: pixelsPerPoint)
        updateDetail(pixelsPerPoint: pixelsPerPoint, visibleRect: visibleRect)
    }

    /// Surligne les occurrences de la recherche (rectangles sur la page droite).
    func showMatches(_ matches: [SearchMatch], current: Int?) {
        let (rotation, size) = (self.rotation, page.baseSize)
        let convert = { (match: SearchMatch) in
            match.rects.map { rotation.apply(to: $0.cgRect, pageSize: size) }
        }
        highlightView.update(matches: matches.map(convert), current: current)
    }

    func cancelRendering() {
        baseTask?.cancel()
        detailTasks.values.forEach { $0.cancel() }
        detailTasks.removeAll()
    }

    // MARK: - Image de la page entière

    private func updateBase(pixelsPerPoint: CGFloat) {
        let wanted = basePixelWidth(for: pixelsPerPoint)
        guard wanted != basePixelWidth, !page.isBlank else { return }
        basePixelWidth = wanted
        baseTask?.cancel()

        if let cached = renderer.cachedImage(of: page, pixelWidth: wanted, display: display) {
            setBase(cached)
            if cached.cgImage.width >= wanted - 1 { return }
        }
        let (page, display) = (self.page, self.display)
        baseTask = Task { [weak self, renderer] in
            guard let image = try? await renderer.image(of: page, pixelWidth: wanted, display: display) else { return }
            guard let self, !Task.isCancelled, self.isShowing(page, display), self.basePixelWidth == wanted else {
                return
            }
            self.setBase(image)
        }
    }

    private func isShowing(_ page: DisplayPage, _ display: DisplayRotation) -> Bool {
        self.page == page && self.display == display
    }

    private func basePixelWidth(for pixelsPerPoint: CGFloat) -> Int {
        let longest = max(pageSize.width, pageSize.height)
        let cap = Self.maxBasePixels / max(longest, 1)
        let exact = Int((pageSize.width * min(pixelsPerPoint, cap)).rounded(.up))
        let step = Self.pixelStep
        return max(step, (exact + step - 1) / step * step)
    }

    private func setBase(_ image: PageImage) {
        baseView.image = NSImage(cgImage: image.cgImage, size: pageSize)
    }

    // MARK: - Tuiles de détail

    private func updateDetail(pixelsPerPoint scale: CGFloat, visibleRect: CGRect) {
        let longest = max(pageSize.width, pageSize.height)
        let needsDetail = longest * scale > Self.maxBasePixels * 1.05 && !visibleRect.isEmpty && !page.isBlank
        guard needsDetail else {
            clearDetail()
            return
        }
        if abs(scale - detailScale) > 0.001 {
            clearDetail()
            detailScale = scale
        }

        // Grille fixe de tuiles dans l'image complète à cette échelle ; on garde
        // une marge d'une demi-tuile autour de la zone visible.
        let tile = Self.tilePixels
        let imageSize = CGSize(
            width: (pageSize.width * scale).rounded(.up),
            height: (pageSize.height * scale).rounded(.up)
        )
        let wanted = CGRect(
            x: visibleRect.minX * scale, y: visibleRect.minY * scale,
            width: visibleRect.width * scale, height: visibleRect.height * scale
        ).insetBy(dx: -tile / 2, dy: -tile / 2)
            .intersection(CGRect(origin: .zero, size: imageSize))
        guard !wanted.isNull, !wanted.isEmpty else { return }

        let firstColumn = Int(wanted.minX / tile), firstRow = Int(wanted.minY / tile)
        let columns = firstColumn...max(firstColumn, Int((wanted.maxX - 1) / tile))
        let rows = firstRow...max(firstRow, Int((wanted.maxY - 1) / tile))
        var needed = Set<TileKey>()
        for row in rows {
            for column in columns {
                needed.insert(TileKey(column: column, row: row))
            }
        }
        for key in Set(detailTiles.keys).union(detailTasks.keys).subtracting(needed) {
            detailTasks.removeValue(forKey: key)?.cancel()
            detailTiles.removeValue(forKey: key)?.removeFromSuperview()
        }
        for key in needed where detailTiles[key] == nil && detailTasks[key] == nil {
            let pixels = CGRect(x: CGFloat(key.column) * tile, y: CGFloat(key.row) * tile, width: tile, height: tile)
                .intersection(CGRect(origin: .zero, size: imageSize))
            requestTile(key, pixels: pixels, scale: scale)
        }
    }

    private func requestTile(_ key: TileKey, pixels: CGRect, scale: CGFloat) {
        let (page, display) = (self.page, self.display)
        detailTasks[key] = Task { [weak self, renderer] in
            let image = try? await renderer.region(of: page, scale: scale, display: display, pixels: pixels)
            guard let self, !Task.isCancelled, self.isShowing(page, display), self.detailScale == scale else { return }
            self.detailTasks[key] = nil
            guard let image else { return }
            let view = NSImageView(frame: CGRect(
                x: pixels.minX / scale, y: pixels.minY / scale,
                width: CGFloat(image.cgImage.width) / scale, height: CGFloat(image.cgImage.height) / scale
            ))
            view.imageScaling = .scaleAxesIndependently
            view.image = NSImage(cgImage: image.cgImage, size: view.frame.size)
            self.detailView.addSubview(view)
            self.detailTiles[key] = view
        }
    }

    private func clearDetail() {
        detailTasks.values.forEach { $0.cancel() }
        detailTasks.removeAll()
        detailTiles.values.forEach { $0.removeFromSuperview() }
        detailTiles.removeAll()
        detailScale = 0
    }

    private func updateAccessibility() {
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel(String(localized: "Page \(pageIndex + 1) sur \(pageCount)"))
    }
}

/// Conteneur aux coordonnées orientées vers le bas, comme la page.
final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

/// Surlignage des occurrences trouvées ; l'occurrence courante est plus marquée.
@MainActor
final class HighlightView: NSView {
    private var matches: [[CGRect]] = []
    private var current: Int?

    override var isFlipped: Bool { true }

    func update(matches: [[CGRect]], current: Int?) {
        guard matches != self.matches || current != self.current else { return }
        self.matches = matches
        self.current = current
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        for (index, rects) in matches.enumerated() {
            let alpha: CGFloat = index == current ? 0.9 : 0.45
            NSColor.findHighlightColor.withAlphaComponent(alpha).setFill()
            for rect in rects where rect.intersects(dirtyRect) {
                NSBezierPath(rect: rect.insetBy(dx: -1, dy: -1)).fill()
            }
        }
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

extension PageRect {
    var cgRect: CGRect {
        CGRect(x: CGFloat(x), y: CGFloat(y), width: CGFloat(width), height: CGFloat(height))
    }
}
