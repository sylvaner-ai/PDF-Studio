import Foundation
import os

/// Rend les pages hors du fil principal et garde les images récentes en cache.
///
/// Au plus trois rendus s'exécutent en même temps. Les demandes en attente sont
/// servies de la plus récente à la plus ancienne, pour que la page visible après
/// un défilement rapide passe en premier ; une demande annulée entre-temps
/// (page sortie de l'écran) est abandonnée sans être rendue.
///
/// Le cache est indexé par page source et non par position : déplacer une page
/// ne demande pas de nouveau rendu. Une page dont le contenu est modifié est
/// rendue avec ses modifications, et son cache indexé aussi par leur état. Une
/// page vierge n'est jamais rendue (la vue la peint en blanc).
final class PageRenderer: Sendable {
    private let sources: PageSources
    private let cache = ImageCache()
    private let limiter = RenderLimiter(maxConcurrent: 3)
    /// Largeurs déjà rendues pour chaque page et rotation, pour retrouver une
    /// image provisoire pendant qu'une version plus nette se prépare.
    private let renderedWidths = OSAllocatedUnfairLock(initialState: [PageKey: Set<Int>]())

    private struct PageKey: Hashable {
        let source: DisplayPage.Source
        let revision: UInt64
        let rotation: Int
    }

    init(sources: PageSources) {
        self.sources = sources
    }

    /// Image complète d'une page, de `pixelWidth` pixels de large à l'écran.
    ///
    /// - Parameter display: rotation d'affichage ; celle de la page s'y ajoute.
    func image(
        of page: DisplayPage, pixelWidth: Int, display: DisplayRotation = DisplayRotation()
    ) async throws -> PageImage {
        let (source, document) = try resolve(page)
        let rotation = page.rotation(display)
        let edits = sources.edits(for: page)
        let revision = edits?.revision() ?? 0
        let key = Self.cacheKey(source: source, revision: revision, pixelWidth: pixelWidth, rotation: rotation)
        if let cached = cache.object(forKey: key) {
            return cached.image
        }
        let scale = Float(CGFloat(pixelWidth) / page.size(display).width)
        let index = UInt32(source.index)
        let degrees = Int32(rotation.degrees)
        let image = try await limited {
            if let edits {
                return try document.renderEditedPage(index: index, edits: edits, scale: scale, rotation: degrees)
            }
            return try document.renderPage(index: index, scale: scale, rotation: degrees)
        }
        let cost = image.cgImage.bytesPerRow * image.cgImage.height
        cache.setObject(ImageBox(image), forKey: key, cost: cost)
        renderedWidths.withLock { widths in
            let pageKey = PageKey(source: source, revision: revision, rotation: rotation.degrees)
            _ = widths[pageKey, default: []].insert(pixelWidth)
        }
        return image
    }

    /// Image déjà en cache la plus proche de `pixelWidth` (la plus nette en
    /// dessous, sinon la plus petite au-dessus), sans rien rendre.
    func cachedImage(of page: DisplayPage, pixelWidth: Int, display: DisplayRotation) -> PageImage? {
        guard let source = page.source else { return nil }
        let rotation = page.rotation(display)
        let revision = page.editRevision
        let pageKey = PageKey(source: source, revision: revision, rotation: rotation.degrees)
        let widths = renderedWidths.withLock { $0[pageKey] ?? [] }
        let ordered = widths.filter { $0 <= pixelWidth }.sorted(by: >) + widths.filter { $0 > pixelWidth }.sorted()
        for width in ordered {
            let key = Self.cacheKey(source: source, revision: revision, pixelWidth: width, rotation: rotation)
            if let cached = cache.object(forKey: key) {
                return cached.image
            }
        }
        return nil
    }

    /// Zone de l'image d'une page à l'échelle `scale` (pixels par point), pour
    /// les forts grossissements. Non mise en cache : la vue garde ses tuiles.
    func region(
        of page: DisplayPage, scale: CGFloat, display: DisplayRotation, pixels rect: CGRect
    ) async throws -> PageImage {
        let (source, document) = try resolve(page)
        let rotation = page.rotation(display)
        let edits = sources.edits(for: page)
        return try await limited {
            if let edits {
                return try document.renderEditedRegion(
                    index: UInt32(source.index),
                    edits: edits,
                    scale: Float(scale),
                    rotation: Int32(rotation.degrees),
                    x: UInt32(rect.minX),
                    y: UInt32(rect.minY),
                    width: UInt32(rect.width),
                    height: UInt32(rect.height)
                )
            }
            return try document.renderRegion(
                index: UInt32(source.index),
                scale: Float(scale),
                rotation: Int32(rotation.degrees),
                x: UInt32(rect.minX),
                y: UInt32(rect.minY),
                width: UInt32(rect.width),
                height: UInt32(rect.height)
            )
        }
    }

    private func resolve(_ page: DisplayPage) throws -> (DisplayPage.Source, PdfDocument) {
        guard let source = page.source, let document = sources.document(source.document) else {
            throw BlankPageError()
        }
        return (source, document)
    }

    /// Exécute un rendu hors du fil principal, dans la limite des rendus simultanés.
    private func limited(_ render: @escaping @Sendable () throws -> RenderedPage) async throws -> PageImage {
        await limiter.acquire()
        do {
            try Task.checkCancellation()
            let image = try await Task.detached(priority: .userInitiated) {
                guard let cgImage = try render().makeCGImage() else {
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

    private static func cacheKey(
        source: DisplayPage.Source, revision: UInt64, pixelWidth: Int, rotation: DisplayRotation
    ) -> NSString {
        "\(source.document)-\(source.index)-\(revision)-\(pixelWidth)-\(rotation.degrees)" as NSString
    }
}

/// Une page vierge n'a rien à rendre.
struct BlankPageError: Error {}

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
