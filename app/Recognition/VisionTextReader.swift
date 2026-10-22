import CoreGraphics
import Foundation
import Vision

/// Un mot lu par Vision.
struct VisionWord: Codable, Equatable, Sendable {
    /// Texte du mot, sans espace.
    let text: String
    /// Confiance, de 0 à 1.
    let confidence: Float
    /// Numéro de ligne, commun aux mots d'une même ligne.
    let line: UInt32
    /// Coins supérieur gauche, supérieur droit, inférieur droit et inférieur
    /// gauche (x, y pour chacun), en pixels depuis le haut à gauche de l'image.
    let quad: [Float]
}

/// Lecture du texte d'une image par le framework Vision d'Apple.
///
/// Ce fichier ne dépend que de Vision : il est partagé par l'application
/// (``VisionTextRecognizer``) et par le programme du banc d'essai
/// (`engine/tools/ocr_bench/vision/main.swift`), pour que les mesures portent
/// sur le code livré.
///
/// Vision donne une confiance par ligne, pas par mot. La confiance d'un mot
/// est celle de sa ligne, réduite quand les autres lectures proposées par
/// Vision (jusqu'à trois) ne contiennent pas ce mot : un mot sur lequel les
/// lectures divergent est douteux.
enum VisionTextReader {
    /// Nombre de lectures demandées pour chaque ligne.
    static let candidates = 3

    /// Lit le texte de `image` dans les langues `languages` (« fr-FR »…), les
    /// mots ligne après ligne. `minimumTextHeight` : hauteur minimale du texte,
    /// en fraction de la hauteur de l'image (valeur de Vision par défaut sinon).
    static func read(_ image: CGImage, languages: [String], minimumTextHeight: Float? = nil) throws -> [VisionWord] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = false
        request.recognitionLanguages = languages
        if let minimumTextHeight {
            request.minimumTextHeight = minimumTextHeight
        }
        try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
        let size = CGSize(width: image.width, height: image.height)
        var words: [VisionWord] = []
        for (line, observation) in (request.results ?? []).enumerated() {
            words += Self.words(of: observation, line: UInt32(line), imageSize: size)
        }
        return words
    }

    /// Image en niveaux de gris, 8 bits par pixel, rangée par rangée.
    static func grayImage(width: Int, height: Int, pixels: Data) -> CGImage? {
        guard width > 0, height > 0, pixels.count >= width * height,
              let provider = CGDataProvider(data: pixels as CFData) else { return nil }
        return CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 8,
            bytesPerRow: width,
            space: CGColorSpaceCreateDeviceGray(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        )
    }

    // MARK: - Mots d'une ligne

    private static func words(
        of observation: VNRecognizedTextObservation,
        line: UInt32,
        imageSize: CGSize
    ) -> [VisionWord] {
        let readings = observation.topCandidates(candidates)
        guard let best = readings.first else { return [] }
        let alternatives = readings.dropFirst().map { Set(tokens(of: $0.string).map(\.text)) }
        let lineQuad = Quad(observation, imageSize: imageSize)
        let text = best.string
        let length = max(text.count, 1)
        return tokens(of: text).map { token in
            let agreement = alternatives.isEmpty
                ? 1
                : Float(alternatives.filter { $0.contains(token.text) }.count) / Float(alternatives.count)
            let start = text.distance(from: text.startIndex, to: token.range.lowerBound)
            let end = text.distance(from: text.startIndex, to: token.range.upperBound)
            let share = (start: CGFloat(start) / CGFloat(length), end: CGFloat(end) / CGFloat(length))
            let quad = wordQuad(best, token: token, line: lineQuad, share: share, imageSize: imageSize)
            return VisionWord(
                text: token.text,
                confidence: best.confidence * (0.5 + 0.5 * agreement),
                line: line,
                quad: quad.values
            )
        }
    }

    /// Emprise d'un mot : celle que donne Vision, sauf si elle couvre toute la
    /// ligne (certaines versions de Vision ne situent pas les mots) ; la ligne
    /// est alors partagée au prorata des caractères.
    private static func wordQuad(
        _ reading: VNRecognizedText,
        token: Token,
        line: Quad,
        share: (start: CGFloat, end: CGFloat),
        imageSize: CGSize
    ) -> Quad {
        let isPartOfLine = share.end - share.start < 0.9
        if let box = try? reading.boundingBox(for: token.range) {
            let quad = Quad(box, imageSize: imageSize)
            if !isPartOfLine || quad.width < 0.95 * line.width {
                return quad
            }
        }
        return line.slice(from: share.start, to: share.end)
    }

    private struct Token {
        let text: String
        let range: Range<String.Index>
    }

    /// Suites de caractères séparées par des espaces.
    private static func tokens(of text: String) -> [Token] {
        var tokens: [Token] = []
        var start: String.Index?
        for index in text.indices {
            if text[index].isWhitespace {
                if let begin = start {
                    tokens.append(Token(text: String(text[begin..<index]), range: begin..<index))
                    start = nil
                }
            } else if start == nil {
                start = index
            }
        }
        if let begin = start {
            tokens.append(Token(text: String(text[begin...]), range: begin..<text.endIndex))
        }
        return tokens
    }

    /// Quadrilatère en pixels, origine en haut à gauche.
    private struct Quad {
        var topLeft: CGPoint
        var topRight: CGPoint
        var bottomRight: CGPoint
        var bottomLeft: CGPoint

        /// Emprise d'une observation de Vision (coordonnées normalisées,
        /// origine en bas à gauche).
        init(_ rectangle: VNRectangleObservation, imageSize: CGSize) {
            func pixel(_ point: CGPoint) -> CGPoint {
                CGPoint(x: point.x * imageSize.width, y: (1 - point.y) * imageSize.height)
            }
            topLeft = pixel(rectangle.topLeft)
            topRight = pixel(rectangle.topRight)
            bottomRight = pixel(rectangle.bottomRight)
            bottomLeft = pixel(rectangle.bottomLeft)
        }

        private init(topLeft: CGPoint, topRight: CGPoint, bottomRight: CGPoint, bottomLeft: CGPoint) {
            self.topLeft = topLeft
            self.topRight = topRight
            self.bottomRight = bottomRight
            self.bottomLeft = bottomLeft
        }

        var width: CGFloat {
            hypot(topRight.x - topLeft.x, topRight.y - topLeft.y)
        }

        /// Partie comprise entre les fractions `start` et `end` de la longueur.
        func slice(from start: CGFloat, to end: CGFloat) -> Quad {
            func along(_ origin: CGPoint, _ target: CGPoint, _ fraction: CGFloat) -> CGPoint {
                CGPoint(x: origin.x + (target.x - origin.x) * fraction, y: origin.y + (target.y - origin.y) * fraction)
            }
            return Quad(
                topLeft: along(topLeft, topRight, start),
                topRight: along(topLeft, topRight, end),
                bottomRight: along(bottomLeft, bottomRight, end),
                bottomLeft: along(bottomLeft, bottomRight, start)
            )
        }

        var values: [Float] {
            [topLeft, topRight, bottomRight, bottomLeft].flatMap { [Float($0.x), Float($0.y)] }
        }
    }
}
