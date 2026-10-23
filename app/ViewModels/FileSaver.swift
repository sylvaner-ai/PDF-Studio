import CryptoKit
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
///
/// Si l'application s'arrête pendant la réécriture (panne, arrêt forcé), le
/// fichier peut rester à moitié écrit. Une note, posée avant d'écrire et
/// retirée après, le signale : à la prochaine ouverture du document, le
/// fichier est rétabli depuis sa copie (cf. ``repairInterruptedWrite(of:journal:)``).
enum FileSaver {
    static let maxBackups = 30
    /// Nombre d'exportations dont les fichiers remplacés sont gardés.
    static let maxExportBackups = 10

    private static let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "enregistrement")

    /// Remplace le contenu d'un fichier existant, après l'avoir sauvegardé.
    ///
    /// - Parameter backups: dossier des sauvegardes ; celui de l'application par défaut.
    /// - Parameter journal: dossier des notes d'écriture en cours ; celui de
    ///   l'application par défaut.
    static func replace(_ url: URL, with data: Data, backups: URL? = nil, journal: URL? = nil) throws {
        let original = try Data(contentsOf: url)
        let directory = try backups ?? backupDirectory()
        let copy = try backUp(original, name: url.lastPathComponent, in: directory)
        pruneBackups(in: directory, keeping: maxBackups, sparing: copy)
        let note = try beginWrite(of: url, backup: copy, data: data, journal: journal)
        do {
            try overwrite(url, with: data)
            guard try Data(contentsOf: url) == data else {
                throw CocoaError(.fileWriteUnknown)
            }
        } catch {
            let reason = error.localizedDescription
            logger.error("Écriture interrompue, restauration de l'original : \(reason, privacy: .public)")
            if (try? overwrite(url, with: original)) != nil {
                try? FileManager.default.removeItem(at: note)
            }
            throw error
        }
        try? FileManager.default.removeItem(at: note)
    }

    /// Ce que la réouverture a constaté d'une écriture interrompue.
    enum Repair: Equatable {
        /// Le fichier avait été entièrement écrit : il est gardé tel quel.
        case completed
        /// Le fichier était incomplet : sa version précédente est rétablie.
        case restored
        /// Le fichier était incomplet et n'a pas pu être rétabli ; la copie de
        /// sa version précédente est à `backup`.
        case failed(backup: String)
    }

    /// Rétablit le fichier `url` si sa dernière réécriture a été interrompue
    /// (note restée en place) ; `nil` s'il n'y a rien à réparer. À appeler à
    /// l'ouverture, avant de lire le fichier.
    static func repairInterruptedWrite(of url: URL, journal: URL? = nil) -> Repair? {
        guard let noteURL = try? journalURL(for: url, in: journal),
              let data = try? Data(contentsOf: noteURL),
              let note = try? JSONDecoder().decode(WriteNote.self, from: data)
        else { return nil }
        let manager = FileManager.default
        if let current = try? Data(contentsOf: url), digest(of: current) == note.digest {
            try? manager.removeItem(at: noteURL)
            return .completed
        }
        guard let previous = try? Data(contentsOf: URL(fileURLWithPath: note.backup)),
              (try? overwrite(url, with: previous)) != nil,
              (try? Data(contentsOf: url)) == previous
        else {
            logger.error("Écriture interrompue : la version précédente n'a pas pu être rétablie")
            return .failed(backup: note.backup)
        }
        try? manager.removeItem(at: noteURL)
        logger.info("Écriture interrompue : la version précédente a été rétablie")
        return .restored
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

    /// Note que `url`, dont la version précédente est copiée à `backup`, va
    /// être réécrit avec `data` ; renvoie l'emplacement de la note, à retirer
    /// une fois l'écriture terminée.
    static func beginWrite(of url: URL, backup: URL, data: Data, journal: URL? = nil) throws -> URL {
        let note = try journalURL(for: url, in: journal)
        try JSONEncoder().encode(WriteNote(path: url.path, backup: backup.path, digest: digest(of: data)))
            .write(to: note, options: .atomic)
        return note
    }

    /// Note d'une réécriture en cours : le fichier, la copie de sa version
    /// précédente et l'empreinte du contenu qui le remplace.
    private struct WriteNote: Codable {
        let path: String
        let backup: String
        let digest: String
    }

    /// Note de la réécriture de `url`, dans `directory` ou à défaut dans le
    /// dossier `Sauvegardes/Écritures en cours`.
    private static func journalURL(for url: URL, in directory: URL?) throws -> URL {
        let folder = try directory ?? backupDirectory().appendingPathComponent("Écritures en cours", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let key = Data(url.standardizedFileURL.path.precomposedStringWithCanonicalMapping.utf8)
        return folder.appendingPathComponent("\(digest(of: key)).json")
    }

    private static func digest(of data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
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

    @discardableResult
    private static func backUp(_ data: Data, name: String, in directory: URL) throws -> URL {
        let url = uniqueURL(in: directory, name: name)
        try data.write(to: url, options: .atomic)
        return url
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
    /// ou des dossiers d'exportation si `folders` est vrai ; `sparing` est
    /// toujours gardée (copie dont dépend une écriture en cours).
    private static func pruneBackups(
        in directory: URL, keeping count: Int, folders: Bool = false, sparing: URL? = nil
    ) {
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
        for (url, _) in dated.sorted(by: { $0.1 > $1.1 }).dropFirst(count)
        where url.standardizedFileURL != sparing?.standardizedFileURL {
            try? manager.removeItem(at: url)
        }
    }
}
