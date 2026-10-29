import AppKit

/// Police d'un texte ajouté telle que la choisit le panneau : une famille,
/// en gras ou en italique.
///
/// Helvetica, Times et Courier sont les polices standard du PDF : un
/// lecteur les a toujours, elles ne sont pas intégrées au document (tant que
/// leurs caractères suffisent). Une autre famille est une police du Mac,
/// intégrée au document si sa licence le permet, sinon remplacée par la
/// police standard la plus proche.
struct AddedTextFont: Equatable {
    var family: String
    var bold: Bool
    var italic: Bool

    /// Familles des polices standard.
    static let standardFamilies = ["Helvetica", "Times", "Courier"]

    /// Noms PostScript des polices standard : normal, gras, italique, gras
    /// italique.
    private static let standardNames = [
        "Helvetica": ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique"],
        "Times": ["Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic"],
        "Courier": ["Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique"]
    ]

    /// Familles des polices du Mac, sans les polices standard.
    @MainActor static var installedFamilies: [String] {
        NSFontManager.shared.availableFontFamilies
            .filter { !$0.hasPrefix(".") && !standardFamilies.contains($0) }
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    init(family: String, bold: Bool = false, italic: Bool = false) {
        self.family = family
        self.bold = bold
        self.italic = italic
    }

    /// Police décrite par le moteur.
    @MainActor init(_ font: TextFont) {
        switch font {
        case .original:
            self.init(family: "Helvetica")
        case let .standard(name):
            let family = Self.standardNames.first { $0.value.contains(name) }
            let index = family?.value.firstIndex(of: name) ?? 0
            self.init(family: family?.key ?? "Helvetica", bold: index % 2 == 1, italic: index >= 2)
        case let .installed(postscriptName):
            guard let font = NSFont(name: postscriptName, size: 12), let family = font.familyName else {
                self.init(family: "Helvetica")
                return
            }
            let traits = NSFontManager.shared.traits(of: font)
            self.init(family: family, bold: traits.contains(.boldFontMask), italic: traits.contains(.italicFontMask))
        }
    }

    /// Police à transmettre au moteur.
    @MainActor var engine: TextFont {
        if let names = Self.standardNames[family] {
            return .standard(name: names[(bold ? 1 : 0) + (italic ? 2 : 0)])
        }
        return .installed(postscriptName: installedFont(size: 12)?.fontName ?? family)
    }

    /// Police du Mac qui montre le texte pendant sa saisie.
    @MainActor func nsFont(size: CGFloat) -> NSFont {
        if let names = Self.standardNames[family] {
            let name = names[(bold ? 1 : 0) + (italic ? 2 : 0)]
            return NSFont(name: name, size: size) ?? .systemFont(ofSize: size)
        }
        return installedFont(size: size) ?? .systemFont(ofSize: size)
    }

    /// Membre de la famille du Mac le plus proche du style voulu.
    @MainActor private func installedFont(size: CGFloat) -> NSFont? {
        var traits: NSFontTraitMask = []
        if bold {
            traits.insert(.boldFontMask)
        }
        if italic {
            traits.insert(.italicFontMask)
        }
        let manager = NSFontManager.shared
        return manager.font(withFamily: family, traits: traits, weight: 5, size: size)
            ?? manager.font(withFamily: family, traits: [], weight: 5, size: size)
    }
}

extension AddedTextStyle {
    /// Police de saisie du texte, au corps du style.
    @MainActor var nsFont: NSFont {
        AddedTextFont(font).nsFont(size: CGFloat(max(size, 1)))
    }

    /// Couleur du texte.
    var nsColor: NSColor {
        NSColor(srgbRed: CGFloat(red), green: CGFloat(green), blue: CGFloat(blue), alpha: 1)
    }
}
