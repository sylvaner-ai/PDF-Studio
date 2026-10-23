import AppKit

/// Une vignette de la grille d'organisation : la page, ombrée, et son numéro ;
/// un cadre de la couleur d'accentuation signale la sélection.
final class PageGridItem: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("PageGridItem")

    /// Hauteur réservée au numéro de page, sous la vignette.
    static let labelHeight: CGFloat = 22
    /// Marge autour de la vignette, où s'affiche le cadre de sélection.
    static let inset: CGFloat = 8

    /// Taille d'une case de la grille pour une largeur de vignette donnée : une
    /// page A4 en portrait la remplit, les autres formats y sont centrés.
    static func itemSize(thumbnailWidth: CGFloat) -> NSSize {
        NSSize(
            width: thumbnailWidth + 2 * inset,
            height: (thumbnailWidth * 1.415).rounded() + 2 * inset + labelHeight
        )
    }

    private let itemView = PageGridItemView(frame: .zero)
    private var renderTask: Task<Void, Never>?
    private var request: RenderRequest?

    /// Ce qui détermine l'image : la relancer seulement quand cela change.
    private struct RenderRequest: Equatable {
        let page: DisplayPage
        let display: DisplayRotation
        let pixelWidth: Int
    }

    override func loadView() {
        view = itemView
    }

    override var isSelected: Bool {
        didSet { updateSelection() }
    }

    override var highlightState: NSCollectionViewItem.HighlightState {
        didSet { updateSelection() }
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        renderTask?.cancel()
        renderTask = nil
        request = nil
        itemView.image = nil
    }

    /// Affiche la page `page`, à la position `position`, dans une case de taille
    /// `itemSize`, et lance son rendu si besoin.
    func configure(
        position: Int, page: DisplayPage, display: DisplayRotation, itemSize: NSSize, renderer: PageRenderer
    ) {
        itemView.number = position + 1
        itemView.pageSize = page.size(display)
        itemView.setAccessibilityLabel(String(localized: "Page \(position + 1)"))
        let scale = itemView.window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        let pixelWidth = Int((itemView.pageFrame(in: itemSize).width * scale).rounded(.up))
        let request = RenderRequest(page: page, display: display, pixelWidth: pixelWidth)
        guard request != self.request, pixelWidth > 0 else { return }
        self.request = request
        renderTask?.cancel()
        guard !page.isBlank else {
            itemView.image = nil
            return
        }
        itemView.image = renderer.cachedImage(of: page, pixelWidth: pixelWidth, display: display)?.cgImage
        renderTask = Task { [weak self] in
            guard let image = try? await renderer.image(of: page, pixelWidth: pixelWidth, display: display),
                  let self, self.request == request else { return }
            self.itemView.image = image.cgImage
        }
    }

    private func updateSelection() {
        itemView.isHighlighted = isSelected || highlightState == .forSelection
    }
}

/// Vue d'une vignette ; ses éléments sont placés à la main, la case ayant une
/// taille fixe.
final class PageGridItemView: NSView {
    var image: CGImage? {
        didSet { pageLayer.contents = image }
    }

    var number = 0 {
        didSet { label.stringValue = "\(number)" }
    }

    /// Format de la page affichée, pour en garder les proportions.
    var pageSize = CGSize(width: 210, height: 297) {
        didSet { needsLayout = true }
    }

    var isHighlighted = false {
        didSet { updateColors() }
    }

    private let pageLayer = CALayer()
    private let selectionLayer = CALayer()
    private let label = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        selectionLayer.cornerRadius = Design.Radius.small
        selectionLayer.cornerCurve = .continuous
        selectionLayer.borderWidth = 3
        pageLayer.backgroundColor = NSColor.white.cgColor
        pageLayer.contentsGravity = .resize
        pageLayer.minificationFilter = .trilinear
        pageLayer.shadowRadius = 2
        pageLayer.shadowOffset = CGSize(width: 0, height: -1)
        layer?.addSublayer(selectionLayer)
        layer?.addSublayer(pageLayer)
        label.font = .preferredFont(forTextStyle: .caption1)
        label.textColor = .secondaryLabelColor
        label.alignment = .center
        addSubview(label)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        updateColors()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) n'est pas pris en charge")
    }

    override var isFlipped: Bool { true }

    /// Emplacement de la page dans une case : centrée, aux proportions de la page.
    func pageFrame(in size: CGSize) -> CGRect {
        let inset = PageGridItem.inset
        let box = CGRect(
            x: inset, y: inset,
            width: max(size.width - 2 * inset, 1),
            height: max(size.height - 2 * inset - PageGridItem.labelHeight, 1)
        )
        let scale = min(box.width / max(pageSize.width, 1), box.height / max(pageSize.height, 1))
        let size = CGSize(width: (pageSize.width * scale).rounded(), height: (pageSize.height * scale).rounded())
        return CGRect(
            x: box.midX - size.width / 2, y: box.maxY - size.height, width: size.width, height: size.height
        ).integral
    }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        let frame = pageFrame(in: bounds.size)
        pageLayer.frame = frame
        pageLayer.shadowPath = CGPath(rect: CGRect(origin: .zero, size: frame.size), transform: nil)
        selectionLayer.frame = frame.insetBy(dx: -5, dy: -5)
        CATransaction.commit()
        label.frame = CGRect(
            x: 0, y: bounds.height - PageGridItem.labelHeight, width: bounds.width, height: PageGridItem.labelHeight
        )
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
    }

    private func updateColors() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            selectionLayer.borderColor = isHighlighted ? NSColor.controlAccentColor.cgColor : nil
            pageLayer.borderColor = NSColor.separatorColor.cgColor
        }
        pageLayer.shadowOpacity = Design.pageShadowOpacity(dark: effectiveAppearance.isDark) * 1.5
        pageLayer.borderWidth = 0.5
    }
}
