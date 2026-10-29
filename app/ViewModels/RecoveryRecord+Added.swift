import Foundation

// MARK: - Objets ajoutés aux pages

extension RecoveryRecord {
    /// Objet ajouté à une page (cf. ``AddedObjectItem``) ; positions dans
    /// l'espace de la page PDF.
    enum Added: Codable, Equatable, Sendable {
        case text(text: String, style: Style, x: Float, y: Float, turns: UInt8)
        /// `path` : image rangée par ``RecoveryStore/keepImage(_:)`` ; `data` :
        /// ses octets, quand elle n'a pas pu l'être.
        case image(path: String?, data: Data?, frame: [Float], turns: UInt8)

        /// Image rangée à part.
        var imagePath: String? {
            if case let .image(path, _, _, _) = self {
                return path
            }
            return nil
        }
    }

    /// Style d'un texte ajouté (cf. ``AddedTextStyle``).
    struct Style: Codable, Equatable, Sendable {
        let font: RecoveryTextFont
        let size: Float
        let red: Float
        let green: Float
        let blue: Float
    }
}

extension RecoveryRecord {
    /// Images rangées pour les objets ajoutés aux pages.
    var imagePaths: [String] {
        pages.flatMap { ($0.edits ?? []).compactMap { $0.added?.imagePath } }
    }
}

extension RecoveryRecord.Added {
    init(_ item: AddedObjectItem, store: RecoveryStore?) {
        switch item {
        case let .text(text, style, x, y, turns):
            self = .text(
                text: text,
                style: RecoveryRecord.Style(
                    font: RecoveryTextFont(style.font), size: style.size,
                    red: style.red, green: style.green, blue: style.blue
                ),
                x: x, y: y, turns: turns
            )
        case let .image(data, left, bottom, right, top, turns):
            let path = try? store?.keepImage(data)
            self = .image(path: path, data: path == nil ? data : nil, frame: [left, bottom, right, top], turns: turns)
        }
    }

    /// Objet au format du moteur ; une image qui ne se relit pas est vide, et
    /// le moteur l'ignore.
    var item: AddedObjectItem {
        switch self {
        case let .text(text, style, x, y, turns):
            let engine = AddedTextStyle(
                font: style.font.engine, size: style.size, red: style.red, green: style.green, blue: style.blue
            )
            return .text(text: text, style: engine, x: x, y: y, turns: turns)
        case let .image(path, data, frame, turns):
            let bytes = data ?? path.flatMap { FileManager.default.contents(atPath: $0) } ?? Data()
            let edges = frame.count == 4 ? frame : [0, 0, 0, 0]
            return .image(data: bytes, x0: edges[0], y0: edges[1], x1: edges[2], y1: edges[3], turns: turns)
        }
    }
}

extension RecoveryTextFont {
    init(_ font: TextFont) {
        switch font {
        case .original: self = .original
        case let .installed(postscriptName): self = .installed(postscriptName: postscriptName)
        case let .standard(name): self = .standard(name: name)
        }
    }

    var engine: TextFont {
        switch self {
        case .original: return .original
        case let .installed(postscriptName): return .installed(postscriptName: postscriptName)
        case let .standard(name): return .standard(name: name)
        }
    }
}
