import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Image à ajouter à une page : les octets que le moteur intègre (JPEG ou
/// PNG) et sa taille naturelle à l'écran, en points.
struct AddedImageFile: Sendable {
    /// Plus grand côté accepté par le moteur, en pixels ; une image plus
    /// grande est réduite.
    static let maxPixels: CGFloat = 12_000

    let data: Data
    let size: CGSize

    /// Lit le fichier image `url` ; `nil` s'il est illisible. Un JPEG ou un
    /// PNG droit est gardé tel quel ; une image d'un autre format (HEIC,
    /// TIFF…), tournée par son orientation (photo) ou trop grande est
    /// réencodée droite, en JPEG si elle est opaque, sinon en PNG.
    static func load(_ url: URL) -> AddedImageFile? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue,
              let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue,
              width > 0, height > 0
        else { return nil }
        let orientation = (properties[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1
        let resolution = { (key: CFString) -> CGFloat in
            let dpi = (properties[key] as? NSNumber)?.doubleValue ?? 72
            return dpi >= 1 ? CGFloat(dpi) : 72
        }
        // Les orientations 5 à 8 couchent l'image : ses côtés s'échangent.
        let upright = orientation >= 5
            ? CGSize(width: height, height: width)
            : CGSize(width: width, height: height)
        let size = CGSize(
            width: upright.width * 72 / resolution(kCGImagePropertyDPIWidth),
            height: upright.height * 72 / resolution(kCGImagePropertyDPIHeight)
        )
        let type = CGImageSourceGetType(source).map { $0 as String }
        let kept = [UTType.jpeg.identifier, UTType.png.identifier]
        if orientation == 1, let type, kept.contains(type), max(width, height) <= maxPixels,
           let data = try? Data(contentsOf: url) {
            return AddedImageFile(data: data, size: size)
        }
        return reencoded(source, longestSide: min(max(width, height), maxPixels)).map {
            AddedImageFile(data: $0, size: size)
        }
    }

    /// Image droite, réduite au besoin à `longestSide` pixels, en JPEG si elle
    /// est opaque, sinon en PNG.
    private static func reencoded(_ source: CGImageSource, longestSide: CGFloat) -> Data? {
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: longestSide
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let opaque = [.none, .noneSkipFirst, .noneSkipLast].contains(image.alphaInfo)
        let type = opaque ? UTType.jpeg : UTType.png
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, type.identifier as CFString, 1, nil) else {
            return nil
        }
        let quality: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: 0.92]
        CGImageDestinationAddImage(destination, image, opaque ? quality as CFDictionary : nil)
        return CGImageDestinationFinalize(destination) ? output as Data : nil
    }
}
