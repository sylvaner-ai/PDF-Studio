import CryptoKit
import Foundation
import os

/// Taille et date de modification d'un fichier, pour savoir s'il a changé.
struct FileStamp: Codable, Equatable, Sendable {
    let size: Int64
    let modified: Date

    /// Lit l'empreinte d'un fichier accessible, sur le disque (les valeurs que
    /// `URL` garde en cache pourraient ne pas voir une modification).
    init(of url: URL) throws {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        size = (attributes[.size] as? NSNumber)?.int64Value ?? 0
        modified = attributes[.modificationDate] as? Date ?? .distantPast
    }
}

/// Modifications non enregistrées d'un document, de quoi les retrouver après un
/// arrêt imprévu ou quand l'application a été quittée sans enregistrer.
///
/// L'organisation des pages, les modifications de leur contenu et le texte
/// reconnu des pages scannées sont retenus :
/// les pages renvoient au document (ou à une copie de sa version d'ouverture,
/// s'il a été enregistré depuis) et à des copies des documents dont des pages
/// ont été insérées. Les images ajoutées aux pages sont rangées à part, une
/// seule fois chacune.
struct RecoveryRecord: Codable, Equatable, Sendable {
    struct Page: Codable, Equatable, Sendable {
        /// Document source (0 : le document) ; `nil` pour une page vierge.
        let source: Int?
        let index: Int
        let width: Float
        let height: Float
        let quarterTurns: Int
        /// Modifications du contenu de la page (absentes des enregistrements
        /// antérieurs à l'édition du contenu).
        var edits: [ObjectEdit]?
        /// Couche de texte reconnue de la page, s'il y en a une.
        var layer: [LayerWord]?
        /// Page d'origine d'une page améliorée (absente des enregistrements
        /// antérieurs au mode Amélioration).
        var replaces: Replaced?
    }

    /// Page d'origine d'une page améliorée (cf. ``ReplacedPageItem``).
    struct Replaced: Codable, Equatable, Sendable {
        let source: Int
        let index: Int
        let quarterTurns: Int
    }

    /// Mot de la couche de texte d'une page reconnue (cf. ``LayerWordItem``).
    struct LayerWord: Codable, Equatable, Sendable {
        let text: String
        let quad: [Float]
        let confidence: Float
        let line: UInt32
    }

    /// Modification d'un objet d'une page (cf. ``ObjectEditItem``).
    struct ObjectEdit: Codable, Equatable, Sendable {
        let object: UInt32
        let deleted: Bool
        let offsetX: Float
        let offsetY: Float
        let text: String?
        let font: RecoveryTextFont?
        let fingerprint: String
        /// Modifications d'un tableau (absentes des enregistrements antérieurs
        /// à l'édition des tableaux).
        var table: Table?
        /// Texte retouché d'une ligne scannée, au format du moteur (absent des
        /// enregistrements antérieurs à la retouche des scans).
        var scan: String?
        /// Objet ajouté à la page, en entier (absent des enregistrements
        /// antérieurs à l'ajout de textes et d'images).
        var added: Added?
    }

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

    /// Modifications d'un tableau (cf. ``TableEditItem``).
    struct Table: Codable, Equatable, Sendable {
        let rows: [UInt32]?
        let columns: [UInt32]?
        let cells: [CellText]
    }

    /// Texte saisi dans une cellule vide (cf. ``CellTextItem``).
    struct CellText: Codable, Equatable, Sendable {
        let row: UInt32
        let column: UInt32
        let text: String
        let font: RecoveryTextFont
    }

    var version = 1
    /// Fichier du document.
    let path: String
    /// Empreinte du fichier quand les modifications ont été retenues.
    let stamp: FileStamp
    /// Copie de la version d'ouverture du document, s'il a été enregistré depuis.
    let basePath: String?
    /// Copies des documents dont des pages ont été insérées (sources 1, 2…).
    let sources: [String]
    let pages: [Page]
    let date: Date
}

/// Police d'un texte modifié (cf. ``TextFont``).
enum RecoveryTextFont: Codable, Equatable, Sendable {
    case original
    case installed(postscriptName: String)
    case standard(name: String)
}

extension RecoveryRecord {
    /// Images rangées pour les objets ajoutés aux pages.
    var imagePaths: [String] {
        pages.flatMap { ($0.edits ?? []).compactMap { $0.added?.imagePath } }
    }
}

extension RecoveryRecord.Page {
    /// Page à retenir ; les images ajoutées sont rangées dans `store`.
    init(
        _ page: DisplayPage, edits: [ObjectEditItem] = [], layer: [LayerWordItem] = [], store: RecoveryStore? = nil
    ) {
        source = page.source?.document
        index = page.source?.index ?? 0
        width = Float(page.baseSize.width)
        height = Float(page.baseSize.height)
        quarterTurns = page.quarterTurns
        self.edits = edits.isEmpty ? nil : edits.map { RecoveryRecord.ObjectEdit($0, store: store) }
        self.layer = layer.isEmpty ? nil : layer.map {
            RecoveryRecord.LayerWord(text: $0.text, quad: $0.quad, confidence: $0.confidence, line: $0.line)
        }
        replaces = page.replaced.map {
            RecoveryRecord.Replaced(source: $0.source.document, index: $0.source.index, quarterTurns: $0.quarterTurns)
        }
    }

    /// Couche de texte au format du moteur.
    var layerItems: [LayerWordItem] {
        (layer ?? []).map { LayerWordItem(text: $0.text, quad: $0.quad, confidence: $0.confidence, line: $0.line) }
    }

    /// Modifications du contenu au format du moteur.
    var contentItems: [ObjectEditItem] {
        (edits ?? []).map(\.item)
    }

    /// Page au format du moteur ; l'identifiant est attribué par le moteur.
    var item: PageItem {
        let kind: PageKind = source.map { .source(source: UInt32($0), index: UInt32(index)) }
            ?? .blank(size: PageSize(width: width, height: height))
        let original = replaces.map {
            ReplacedPageItem(
                kind: .source(source: UInt32($0.source), index: UInt32($0.index)),
                quarterTurns: UInt8($0.quarterTurns & 3)
            )
        }
        return PageItem(
            id: 0, kind: kind, quarterTurns: UInt8(quarterTurns & 3), editRevision: 0, textLayerRevision: 0,
            replaces: original
        )
    }
}

extension RecoveryRecord.ObjectEdit {
    /// Modification à retenir ; l'image d'un objet ajouté est rangée dans
    /// `store`, sinon gardée avec la modification.
    init(_ item: ObjectEditItem, store: RecoveryStore? = nil) {
        object = item.object
        deleted = item.deleted
        offsetX = item.dx
        offsetY = item.dy
        text = item.text
        font = item.font.map { RecoveryTextFont($0) }
        fingerprint = item.fingerprint
        table = item.table.map { table in
            RecoveryRecord.Table(
                rows: table.rows,
                columns: table.columns,
                cells: table.cells.map {
                    RecoveryRecord.CellText(
                        row: $0.row, column: $0.column, text: $0.text, font: RecoveryTextFont($0.font)
                    )
                }
            )
        }
        scan = item.scan
        added = item.added.map { RecoveryRecord.Added($0, store: store) }
    }

    var item: ObjectEditItem {
        ObjectEditItem(
            object: object, deleted: deleted, dx: offsetX, dy: offsetY,
            text: text, font: font?.engine, fingerprint: fingerprint,
            table: table.map { table in
                TableEditItem(
                    rows: table.rows,
                    columns: table.columns,
                    cells: table.cells.map {
                        CellTextItem(row: $0.row, column: $0.column, text: $0.text, font: $0.font.engine)
                    }
                )
            },
            scan: scan,
            added: added?.item
        )
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

/// Rangement des modifications non enregistrées, par défaut dans le dossier
/// `Application Support/PDF Studio/Récupération` du conteneur de l'application.
struct RecoveryStore: Sendable {
    let directory: URL

    private static let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "récupération")

    /// Rangement de l'application.
    static func standard() throws -> RecoveryStore {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        return try RecoveryStore(
            directory: support
                .appendingPathComponent("PDF Studio", isDirectory: true)
                .appendingPathComponent("Récupération", isDirectory: true)
        )
    }

    /// Rangement dans `directory`, créé au besoin.
    init(directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory
    }

    /// Modifications retenues pour ce fichier, s'il y en a.
    func record(for path: String) -> RecoveryRecord? {
        guard let data = try? Data(contentsOf: recordURL(for: path)) else { return nil }
        return try? JSONDecoder().decode(RecoveryRecord.self, from: data)
    }

    func save(_ record: RecoveryRecord) {
        do {
            let data = try JSONEncoder().encode(record)
            try data.write(to: recordURL(for: record.path), options: .atomic)
        } catch {
            Self.logger.error("Récupération non écrite : \(error.localizedDescription, privacy: .public)")
        }
    }

    func removeRecord(for path: String) {
        try? FileManager.default.removeItem(at: recordURL(for: path))
    }

    /// Copie conservée pour la récupération.
    enum Copy: String {
        /// Version d'ouverture d'un document, avant son premier enregistrement.
        case base
        /// Document dont des pages ont été insérées.
        case source
    }

    /// Range une copie ; renvoie son chemin.
    func makeCopy(_ data: Data, of kind: Copy) throws -> String {
        let copy = directory.appendingPathComponent("\(kind.rawValue)-\(UUID().uuidString).pdf")
        try data.write(to: copy, options: .atomic)
        return copy.path
    }

    /// Supprime une copie (seulement dans ce rangement).
    func removeCopy(_ path: String?) {
        guard let path else { return }
        let copy = URL(fileURLWithPath: path)
        let folder = copy.deletingLastPathComponent().resolvingSymlinksInPath().path
        guard Self.isCopy(copy), folder == directory.resolvingSymlinksInPath().path else { return }
        try? FileManager.default.removeItem(at: copy)
    }

    /// Range les octets d'une image ajoutée à une page ; renvoie son chemin.
    /// Une même image n'est rangée qu'une fois, sous le nom de son empreinte.
    func keepImage(_ data: Data) throws -> String {
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        let image = directory.appendingPathComponent("image-\(digest).bin")
        if !FileManager.default.fileExists(atPath: image.path) {
            try data.write(to: image, options: .atomic)
        }
        return image.path
    }

    /// Supprime des modifications retenues et leurs copies.
    func discard(_ record: RecoveryRecord) {
        removeRecord(for: record.path)
        removeCopy(record.basePath)
        record.sources.forEach(removeCopy)
        removeUnusedImages()
    }

    /// Supprime les images rangées qu'aucune modification retenue n'utilise
    /// plus. Les images se rangent sur la file de ``RecoveryWriter``, en même
    /// temps que les modifications qui les utilisent : c'est là qu'il faut
    /// appeler cette fonction.
    func removeUnusedImages() {
        removeUnused { Self.isImage($0) }
    }

    /// Supprime les copies qu'aucune modification retenue n'utilise plus (à
    /// appeler au lancement, avant l'ouverture des documents).
    func removeUnusedCopies() {
        removeUnused { Self.isCopy($0) || Self.isImage($0) }
    }

    /// Supprime les fichiers retenus par `candidate` qu'aucune modification
    /// retenue n'utilise.
    private func removeUnused(where candidate: (URL) -> Bool) {
        let manager = FileManager.default
        guard let files = try? manager.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) else {
            return
        }
        let records = files.filter { $0.pathExtension == "json" }.compactMap { url in
            (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode(RecoveryRecord.self, from: $0) }
        }
        let used = Set(records.flatMap { [$0.basePath].compactMap { $0 } + $0.sources + $0.imagePaths }.map {
            URL(fileURLWithPath: $0).lastPathComponent
        })
        for file in files where candidate(file) && !used.contains(file.lastPathComponent) {
            try? manager.removeItem(at: file)
        }
    }

    private static func isCopy(_ url: URL) -> Bool {
        let name = url.lastPathComponent
        return url.pathExtension == "pdf" && (name.hasPrefix("base-") || name.hasPrefix("source-"))
    }

    private static func isImage(_ url: URL) -> Bool {
        url.pathExtension == "bin" && url.lastPathComponent.hasPrefix("image-")
    }

    private func recordURL(for path: String) -> URL {
        // Un même chemin peut s'écrire avec des accents composés ou décomposés.
        let key = Data(path.precomposedStringWithCanonicalMapping.utf8)
        let digest = SHA256.hash(data: key).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent("\(digest).json")
    }
}
