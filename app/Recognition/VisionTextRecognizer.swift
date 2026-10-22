import CoreGraphics
import Foundation

/// Moteur de reconnaissance de l'application : Vision, fourni au moteur Rust
/// (``TextRecognizer``), qui lui transmet l'image préparée d'une page.
///
/// Choix de Vision et réglages : cf. `docs/OCR_BENCHMARK.md` et
/// `docs/DECISIONS.md`. Appelé hors du fil principal, une page à la fois.
final class VisionTextRecognizer: TextRecognizer {
    func name() -> String {
        "Vision"
    }

    func recognize(image: OcrImage, languages: [String]) throws -> [RecognizedText] {
        guard let cgImage = VisionTextReader.grayImage(
            width: Int(image.width), height: Int(image.height), pixels: image.pixels
        ) else {
            throw EngineError.Failed(
                message: String(localized: "La reconnaissance du texte a échoué."),
                detail: "image \(image.width) × \(image.height) incohérente"
            )
        }
        do {
            return try VisionTextReader.read(cgImage, languages: languages).map { word in
                RecognizedText(text: word.text, confidence: word.confidence, line: word.line, quad: word.quad)
            }
        } catch {
            throw EngineError.Failed(
                message: String(localized: "La reconnaissance du texte a échoué."),
                detail: "Vision : \(error.localizedDescription)"
            )
        }
    }
}
