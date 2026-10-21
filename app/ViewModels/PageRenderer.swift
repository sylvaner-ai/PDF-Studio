import Foundation

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

    init(document: PdfDocument) {
        self.document = document
    }

    func image(page: Int, pixelWidth: Int, pageWidth: CGFloat) async throws -> PageImage {
        let key = "\(page)-\(pixelWidth)" as NSString
        if let cached = cache.object(forKey: key) {
            return cached.image
        }

        await limiter.acquire()
        do {
            try Task.checkCancellation()
            let scale = Float(CGFloat(pixelWidth) / pageWidth)
            let document = self.document
            let image = try await Task.detached(priority: .userInitiated) {
                let rendered = try document.renderPage(index: UInt32(page), scale: scale)
                guard let cgImage = rendered.makeCGImage() else {
                    throw ImageConversionError()
                }
                return PageImage(cgImage: cgImage)
            }.value
            await limiter.release()
            let cost = image.cgImage.bytesPerRow * image.cgImage.height
            cache.setObject(ImageBox(image), forKey: key, cost: cost)
            return image
        } catch {
            await limiter.release()
            throw error
        }
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
