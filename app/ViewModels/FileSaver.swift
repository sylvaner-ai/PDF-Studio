import Foundation
import os

/// Écriture des PDF enregistrés sur le disque.
///
/// Avant de remplacer un fichier existant, une copie en est rangée dans les
/// sauvegardes de PDF Studio (`~/Library/Containers/fr.sylvainrobert.pdfstudio/
/// Data/Library/Application Support/PDF Studio/Sauvegardes`), qui gardent les
/// ``maxBackups`` copies les plus récentes. Le fichier d'origine est réécrit en
/// place, ce qui conserve son emplacement, ses étiquettes et sa synchronisation ;
/// le contenu écrit est relu, et l'ancien restauré en cas d'échec.
enum FileSaver {
    static let maxBackups = 30

    private static let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "enregistrement")

    /// Remplace le contenu d'un fichier existant, après l'avoir sauvegardé.
    ///
    /// - Parameter backups: dossier des sauvegardes ; celui de l'application par défaut.
    static func replace(_ url: URL, with data: Data, backups: URL? = nil) throws {
        let original = try Data(contentsOf: url)
        try backUp(original, name: url.lastPathComponent, in: backups ?? backupDirectory())
        do {
            try overwrite(url, with: data)
            guard try Data(contentsOf: url) == data else {
                throw CocoaError(.fileWriteUnknown)
            }
        } catch {
            let reason = error.localizedDescription
            logger.error("Écriture interrompue, restauration de l'original : \(reason, privacy: .public)")
            try? overwrite(url, with: original)
            throw error
        }
    }

    /// Écrit un nouveau fichier (Enregistrer sous) ; un fichier remplacé à cet
    /// emplacement est d'abord sauvegardé.
    static func write(_ data: Data, to url: URL, backups: URL? = nil) throws {
        if let existing = try? Data(contentsOf: url) {
            try backUp(existing, name: url.lastPathComponent, in: backups ?? backupDirectory())
        }
        try data.write(to: url, options: .atomic)
    }

    /// Dossier des sauvegardes, créé au besoin.
    static func backupDirectory() throws -> URL {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        let directory = support
            .appendingPathComponent("PDF Studio", isDirectory: true)
            .appendingPathComponent("Sauvegardes", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    private static func overwrite(_ url: URL, with data: Data) throws {
        let handle = try FileHandle(forWritingTo: url)
        defer { try? handle.close() }
        try handle.truncate(atOffset: 0)
        try handle.write(contentsOf: data)
        try handle.synchronize()
    }

    private static func backUp(_ data: Data, name: String, in directory: URL) throws {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "fr_FR")
        formatter.dateFormat = "yyyy-MM-dd HH.mm.ss"
        let stamp = formatter.string(from: Date())
        var copy = directory.appendingPathComponent("\(stamp) \(name)")
        var suffix = 2
        while FileManager.default.fileExists(atPath: copy.path) {
            copy = directory.appendingPathComponent("\(stamp) (\(suffix)) \(name)")
            suffix += 1
        }
        try data.write(to: copy, options: .atomic)
        pruneBackups(in: directory)
    }

    /// Supprime les sauvegardes au-delà des ``maxBackups`` plus récentes.
    private static func pruneBackups(in directory: URL) {
        let manager = FileManager.default
        guard let files = try? manager.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: [.contentModificationDateKey], options: [.skipsHiddenFiles]
        ), files.count > maxBackups else { return }
        let dated = files.map { url in
            let values = try? url.resourceValues(forKeys: [.contentModificationDateKey])
            return (url, values?.contentModificationDate ?? .distantPast)
        }
        for (url, _) in dated.sorted(by: { $0.1 > $1.1 }).dropFirst(maxBackups) {
            try? manager.removeItem(at: url)
        }
    }
}
