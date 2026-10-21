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
/// Seule l'organisation des pages est retenue : les pages renvoient au document
/// (ou à une copie de sa version d'ouverture, s'il a été enregistré depuis) et à
/// des copies des documents dont des pages ont été insérées.
struct RecoveryRecord: Codable, Equatable, Sendable {
    struct Page: Codable, Equatable, Sendable {
        /// Document source (0 : le document) ; `nil` pour une page vierge.
        let source: Int?
        let index: Int
        let width: Float
        let height: Float
        let quarterTurns: Int
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

extension RecoveryRecord.Page {
    init(_ page: DisplayPage) {
        source = page.source?.document
        index = page.source?.index ?? 0
        width = Float(page.baseSize.width)
        height = Float(page.baseSize.height)
        quarterTurns = page.quarterTurns
    }

    /// Page au format du moteur ; l'identifiant est attribué par le moteur.
    var item: PageItem {
        let kind: PageKind = source.map { .source(source: UInt32($0), index: UInt32(index)) }
            ?? .blank(size: PageSize(width: width, height: height))
        return PageItem(id: 0, kind: kind, quarterTurns: UInt8(quarterTurns & 3))
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

    /// Supprime des modifications retenues et leurs copies.
    func discard(_ record: RecoveryRecord) {
        removeRecord(for: record.path)
        removeCopy(record.basePath)
        record.sources.forEach(removeCopy)
    }

    /// Supprime les copies qu'aucune modification retenue n'utilise plus (à
    /// appeler au lancement, avant l'ouverture des documents).
    func removeUnusedCopies() {
        let manager = FileManager.default
        guard let files = try? manager.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) else {
            return
        }
        let records = files.filter { $0.pathExtension == "json" }.compactMap { url in
            (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode(RecoveryRecord.self, from: $0) }
        }
        let used = Set(records.flatMap { [$0.basePath].compactMap { $0 } + $0.sources }.map {
            URL(fileURLWithPath: $0).lastPathComponent
        })
        for file in files where Self.isCopy(file) && !used.contains(file.lastPathComponent) {
            try? manager.removeItem(at: file)
        }
    }

    private static func isCopy(_ url: URL) -> Bool {
        let name = url.lastPathComponent
        return url.pathExtension == "pdf" && (name.hasPrefix("base-") || name.hasPrefix("source-"))
    }

    private func recordURL(for path: String) -> URL {
        // Un même chemin peut s'écrire avec des accents composés ou décomposés.
        let key = Data(path.precomposedStringWithCanonicalMapping.utf8)
        let digest = SHA256.hash(data: key).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent("\(digest).json")
    }
}
