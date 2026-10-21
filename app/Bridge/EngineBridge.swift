import CoreGraphics
import Foundation

// Adaptations Swift des types générés par UniFFI (Bridge/Generated/pdfstudio_ffi.swift).
// Aucune logique PDF ici : uniquement des conversions de types.

/// Image de page prête à afficher.
struct PageImage: @unchecked Sendable {
    // CGImage est immuable : son partage entre fils est sûr.
    let cgImage: CGImage
}

extension RenderedPage {
    /// Convertit les pixels RGBA du moteur en `CGImage`, sans copie supplémentaire.
    func makeCGImage() -> CGImage? {
        guard
            let provider = CGDataProvider(data: pixels as CFData),
            let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)
        else { return nil }
        return CGImage(
            width: Int(width),
            height: Int(height),
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: Int(bytesPerRow),
            space: colorSpace,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }
}

extension EngineError {
    /// Message destiné à l'utilisateur, rédigé en français par le moteur.
    var userMessage: String {
        switch self {
        case let .PasswordRequired(message), let .WrongPassword(message):
            return message
        case let .Failed(message, _):
            return message
        }
    }

    /// Détail technique, pour le journal uniquement.
    var technicalDetail: String? {
        if case let .Failed(_, detail) = self, !detail.isEmpty {
            return detail
        }
        return nil
    }
}

extension PageSize {
    var cgSize: CGSize {
        CGSize(width: CGFloat(width), height: CGFloat(height))
    }
}
