import Foundation
import os

/// Écriture des PDF enregistrés et des exportations sur le disque.
///
/// Avant de remplacer un fichier existant, une copie en est rangée dans les
/// sauvegardes de PDF Studio (`~/Library/Containers/fr.sylvainrobert.pdfstudio/
/// Data/Library/Application Support/PDF Studio/Sauvegardes`), qui gardent les
/// ``maxBackups`` copies les plus récentes. Les fichiers remplacés par une
/// exportation (images, parties d'un document) sont rangés à part, dans un
/// dossier par exportation sous `Sauvegardes/Exportations` : une exportation ne
/// peut pas évincer la copie d'un document. Le fichier d'origine est réécrit en
/// place, ce qui conserve son emplacement, ses étiquettes et sa synchronisation ;
/// le contenu écrit est relu, et l'ancien restauré en cas d'échec.
enum FileSaver {
    static let maxBackups = 30
    /// Nombre d'exportations dont les fichiers remplacés sont gardés.
    static let maxExportBackups = 10

    private static let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "enregistrement")

    /// Remplace le contenu d'un fichier existant, après l'avoir sauvegardé.
    ///
    /// - Parameter backups: dossier des sauvegardes ; celui de l'application par défaut.
    static func replace(_ url: URL, with data: Data, backups: URL? = nil) throws {
        let original = try Data(contentsOf: url)
        let directory = try backups ?? backupDirectory()
        try backUp(original, name: url.lastPathComponent, in: directory)
        pruneBackups(in: directory, keeping: maxBackups)
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
    /// emplacement est d'abord sauvegardé avec les documents.
    static func write(_ data: Data, to url: URL, backups: URL? = nil) throws {
        if let existing = try? Data(contentsOf: url) {
            let directory = try backups ?? backupDirectory()
            try backUp(existing, name: url.lastPathComponent, in: directory)
            pruneBackups(in: directory, keeping: maxBackups)
        }
        try data.write(to: url, options: .atomic)
    }

    /// Écrit un fichier exporté ; un fichier remplacé est d'abord copié dans
    /// `backups`, le dossier de l'exportation (cf. ``exportBackupDirectory(named:)``),
    /// ou à défaut dans celui des exportations.
    static func export(_ data: Data, to url: URL, backups: URL?) throws {
        if let existing = try? Data(contentsOf: url) {
            try backUp(existing, name: url.lastPathComponent, in: backups ?? exportsDirectory())
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

    /// Nouveau dossier pour les fichiers que remplacera une exportation ; seuls
    /// ceux des ``maxExportBackups`` dernières exportations sont gardés.
    static func exportBackupDirectory(named name: String, in parent: URL? = nil) throws -> URL {
        let parent = try parent ?? exportsDirectory()
        let directory = uniqueURL(in: parent, name: name)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        pruneBackups(in: parent, keeping: maxExportBackups, folders: true)
        return directory
    }

    private static func exportsDirectory() throws -> URL {
        let directory = try backupDirectory().appendingPathComponent("Exportations", isDirectory: true)
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
        try data.write(to: uniqueURL(in: directory, name: name), options: .atomic)
    }

    /// « date nom » dans `directory`, numéroté s'il existe déjà.
    private static func uniqueURL(in directory: URL, name: String) -> URL {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "fr_FR")
        formatter.dateFormat = "yyyy-MM-dd HH.mm.ss"
        let stamp = formatter.string(from: Date())
        var url = directory.appendingPathComponent("\(stamp) \(name)")
        var suffix = 2
        while FileManager.default.fileExists(atPath: url.path) {
            url = directory.appendingPathComponent("\(stamp) (\(suffix)) \(name)")
            suffix += 1
        }
        return url
    }

    /// Supprime les sauvegardes au-delà des `count` plus récentes : des fichiers,
    /// ou des dossiers d'exportation si `folders` est vrai.
    private static func pruneBackups(in directory: URL, keeping count: Int, folders: Bool = false) {
        let manager = FileManager.default
        let keys: Set<URLResourceKey> = [.contentModificationDateKey, .isDirectoryKey]
        guard let items = try? manager.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: Array(keys), options: [.skipsHiddenFiles]
        ) else { return }
        let dated = items.compactMap { url -> (URL, Date)? in
            let values = try? url.resourceValues(forKeys: keys)
            guard (values?.isDirectory == true) == folders else { return nil }
            return (url, values?.contentModificationDate ?? .distantPast)
        }
        for (url, _) in dated.sorted(by: { $0.1 > $1.1 }).dropFirst(count) {
            try? manager.removeItem(at: url)
        }
    }
}
