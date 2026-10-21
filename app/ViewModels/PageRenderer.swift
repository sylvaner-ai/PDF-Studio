import Foundation
import os

/// Rend les pages d'un document hors du fil principal et garde les images
/// récentes en cache.
///
/// Au plus trois rendus s'exécutent en même temps. Les demandes en attente sont
/// servies de la plus récente à la plus ancienne, pour que la page visible après
/// un défilement rapide passe en premier ; une demande annulée entre-temps
/// (page sortie de l'écran) est abandonnée sans être rendue.
final class PageRenderer: Sendable {
    private let document: PdfDocument
    private let cache = ImageCache()
    private let limiter = RenderLimiter(maxConcurrent: 3)
    /// Largeurs déjà rendues pour chaque page et rotation, pour retrouver une
    /// image provisoire pendant qu'une version plus nette se prépare.
    private let renderedWidths = OSAllocatedUnfairLock(initialState: [PageKey: Set<Int>]())

    private struct PageKey: Hashable {
        let page: Int
        let rotation: Int
    }

    init(document: PdfDocument) {
        self.document = document
    }

    /// Image complète d'une page, de `pixelWidth` pixels de large une fois tournée.
    ///
    /// - Parameter pageWidth: largeur de la page tournée, en points.
    func image(
        page: Int,
        pixelWidth: Int,
        pageWidth: CGFloat,
        rotation: DisplayRotation = DisplayRotation()
    ) async throws -> PageImage {
        let key = Self.cacheKey(page: page, pixelWidth: pixelWidth, rotation: rotation)
        if let cached = cache.object(forKey: key) {
            return cached.image
        }
        let scale = Float(CGFloat(pixelWidth) / pageWidth)
        let image = try await limited { document in
            try document.renderPage(index: UInt32(page), scale: scale, rotation: Int32(rotation.degrees))
        }
        let cost = image.cgImage.bytesPerRow * image.cgImage.height
        cache.setObject(ImageBox(image), forKey: key, cost: cost)
        renderedWidths.withLock { widths in
            _ = widths[PageKey(page: page, rotation: rotation.degrees), default: []].insert(pixelWidth)
        }
        return image
    }

    /// Image déjà en cache la plus proche de `pixelWidth` (la plus nette en
    /// dessous, sinon la plus petite au-dessus), sans rien rendre.
    func cachedImage(page: Int, pixelWidth: Int, rotation: DisplayRotation) -> PageImage? {
        let pageKey = PageKey(page: page, rotation: rotation.degrees)
        let widths = renderedWidths.withLock { $0[pageKey] ?? [] }
        let ordered = widths.filter { $0 <= pixelWidth }.sorted(by: >) + widths.filter { $0 > pixelWidth }.sorted()
        for width in ordered {
            if let cached = cache.object(forKey: Self.cacheKey(page: page, pixelWidth: width, rotation: rotation)) {
                return cached.image
            }
        }
        return nil
    }

    /// Zone de l'image d'une page à l'échelle `scale` (pixels par point), pour
    /// les forts grossissements. Non mise en cache : la vue garde ses tuiles.
    func region(page: Int, scale: CGFloat, rotation: DisplayRotation, pixels rect: CGRect) async throws -> PageImage {
        try await limited { document in
            try document.renderRegion(
                index: UInt32(page),
                scale: Float(scale),
                rotation: Int32(rotation.degrees),
                x: UInt32(rect.minX),
                y: UInt32(rect.minY),
                width: UInt32(rect.width),
                height: UInt32(rect.height)
            )
        }
    }

    /// Exécute un rendu hors du fil principal, dans la limite des rendus simultanés.
    private func limited(_ render: @escaping @Sendable (PdfDocument) throws -> RenderedPage) async throws -> PageImage {
        await limiter.acquire()
        do {
            try Task.checkCancellation()
            let document = self.document
            let image = try await Task.detached(priority: .userInitiated) {
                guard let cgImage = try render(document).makeCGImage() else {
                    throw ImageConversionError()
                }
                return PageImage(cgImage: cgImage)
            }.value
            await limiter.release()
            return image
        } catch {
            await limiter.release()
            throw error
        }
    }

    private static func cacheKey(page: Int, pixelWidth: Int, rotation: DisplayRotation) -> NSString {
        "\(page)-\(pixelWidth)-\(rotation.degrees)" as NSString
    }
}

/// Les pixels rendus n'ont pas pu être convertis en image.
struct ImageConversionError: Error {}

/// `NSCache` est sûr entre fils ; cette sous-classe le déclare à Swift.
private final class ImageCache: NSCache<NSString, ImageBox>, @unchecked Sendable {
    override init() {
        super.init()
        totalCostLimit = 512 * 1024 * 1024
    }
}

private final class ImageBox: Sendable {
    let image: PageImage
    init(_ image: PageImage) { self.image = image }
}

/// Sémaphore asynchrone qui sert les demandes les plus récentes en premier.
actor RenderLimiter {
    private var available: Int
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(maxConcurrent: Int) {
        available = maxConcurrent
    }

    func acquire() async {
        if available > 0 {
            available -= 1
            return
        }
        await withCheckedContinuation { waiters.append($0) }
    }

    func release() {
        if let next = waiters.popLast() {
            next.resume()
        } else {
            available += 1
        }
    }
}
