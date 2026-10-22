import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Règles et encodages des exportations : découpage d'un document, noms des
/// fichiers produits, images des pages.
enum PageExporter {
    /// Découpage d'un document en plusieurs fichiers.
    enum SplitRule: Equatable {
        /// Une partie toutes les `n` pages.
        case every(Int)
        /// Une nouvelle partie commence à chacune de ces positions.
        case before([Int])
    }

    enum ImageFormat: String, CaseIterable {
        case png
        case jpeg

        var type: UTType { self == .png ? .png : .jpeg }
        var fileExtension: String { self == .png ? "png" : "jpg" }
    }

    enum ExportError: Error {
        /// Aucune page n'a de texte natif.
        case noText

        var message: String {
            String(localized: """
                Ce document ne contient pas de texte. Les pages numérisées n'en ont pas \
                sans reconnaissance de caractères (OCR).
                """)
        }
    }

    /// Plus grand côté d'une image exportée, en pixels (une affiche à 300 ppp
    /// dépasserait la mémoire disponible).
    static let maxPixels: CGFloat = 10_000

    /// Positions des pages de chaque partie, dans l'ordre ; aucune partie vide.
    static func parts(pageCount: Int, rule: SplitRule) -> [[Int]] {
        guard pageCount > 0 else { return [] }
        let starts: [Int]
        switch rule {
        case let .every(size):
            starts = Array(stride(from: 0, to: pageCount, by: max(size, 1)))
        case let .before(positions):
            starts = [0] + Set(positions.filter { $0 > 0 && $0 < pageCount }).sorted()
        }
        return starts.indices.map { index in
            Array(starts[index]..<(starts[safe: index + 1] ?? pageCount))
        }
    }

    /// Noms numérotés « base – 01.pdf », avec autant de chiffres que le plus
    /// grand numéro, pour que le Finder les range dans l'ordre.
    static func numberedNames(base: String, numbers: [Int], extension ext: String) -> [String] {
        let digits = String(numbers.max() ?? 1).count
        return numbers.map { number in
            let padded = String(repeating: "0", count: max(digits - String(number).count, 0)) + String(number)
            return "\(base) – \(padded).\(ext)"
        }
    }

    /// Facteur d'agrandissement pour une résolution donnée, limité à ``maxPixels``.
    static func scale(forDPI dpi: Int, pageSize: CGSize) -> CGFloat {
        let scale = CGFloat(dpi) / 72
        let longest = max(pageSize.width, pageSize.height, 1)
        return min(scale, maxPixels / longest)
    }

    /// Résolution réelle, en ppp, une fois appliquée la limite de ``maxPixels``.
    static func resolution(forDPI dpi: Int, pageSize: CGSize) -> Int {
        Int((72 * scale(forDPI: dpi, pageSize: pageSize)).rounded())
    }

    /// Image encodée en PNG ou en JPEG, avec sa résolution.
    static func encode(_ image: CGImage, as format: ImageFormat, dpi: Int) throws -> Data {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            data, format.type.identifier as CFString, 1, nil
        ) else { throw CocoaError(.fileWriteUnknown) }
        var properties: [CFString: Any] = [
            kCGImagePropertyDPIWidth: dpi,
            kCGImagePropertyDPIHeight: dpi
        ]
        if format == .jpeg {
            properties[kCGImageDestinationLossyCompressionQuality] = 0.9
        }
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
        return data as Data
    }

    /// Image d'une page, avec sa rotation, à la résolution donnée ; bloquant.
    static func image(of page: DisplayPage, dpi: Int, sources: PageSources) throws -> CGImage {
        let size = page.size(DisplayRotation())
        let scale = Self.scale(forDPI: dpi, pageSize: size)
        guard let source = page.source, let document = sources.document(source.document) else {
            let width = Int((size.width * scale).rounded())
            guard let blank = blankImage(width: width, height: Int((size.height * scale).rounded())) else {
                throw CocoaError(.fileWriteUnknown)
            }
            return blank
        }
        let rotation = Int32(page.rotation(DisplayRotation()).degrees)
        let index = UInt32(source.index)
        let rendered = try sources.edits(for: page).map {
            try document.renderEditedPage(index: index, edits: $0, scale: Float(scale), rotation: rotation)
        } ?? document.renderPage(index: index, scale: Float(scale), rotation: rotation)
        guard let image = rendered.makeCGImage() else { throw CocoaError(.fileWriteUnknown) }
        return image
    }

    /// Page vierge : une image blanche du format donné, en pixels.
    static func blankImage(width: Int, height: Int) -> CGImage? {
        let context = CGContext(
            data: nil, width: max(width, 1), height: max(height, 1), bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
        )
        context?.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        context?.fill(CGRect(x: 0, y: 0, width: width, height: height))
        return context?.makeImage()
    }
}
