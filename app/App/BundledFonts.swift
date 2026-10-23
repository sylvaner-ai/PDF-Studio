import CoreText
import Foundation

/// Polices fournies avec l'application (Carlito, Caladea : métriques de
/// Calibri et Cambria), pour retoucher le texte des pages scannées.
enum BundledFonts {
    /// Les désigne au moteur, qui les ajoute aux polices du Mac pour retrouver
    /// celle d'un scan, et les rend disponibles à la saisie du texte.
    static func register() {
        guard let resources = Bundle.main.resourceURL else { return }
        // Selon la construction, les polices sont dans « Fonts » ou à la racine.
        let directories = [resources.appending(path: "Fonts"), resources]
        setFontDirectories(directories: directories.map(\.path))
        let urls = directories.flatMap { directory in
            ((try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? [])
                .filter { ["ttf", "otf"].contains($0.pathExtension.lowercased()) }
        }
        guard !urls.isEmpty else { return }
        CTFontManagerRegisterFontURLs(urls as CFArray, .process, true, nil)
    }
}
