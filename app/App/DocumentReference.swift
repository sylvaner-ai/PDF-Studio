import Foundation

/// Référence persistante à un fichier PDF ouvert.
///
/// L'application est isolée par le bac à sable de macOS : l'accès à un fichier
/// choisi par l'utilisateur est conservé sous forme de signet de sécurité, en
/// lecture et en écriture (pour « Enregistrer »). La référence peut ainsi être
/// restaurée au relancement.
/// Deux références désignant le même chemin sont égales.
struct DocumentReference: Codable, Hashable, Sendable {
    let path: String
    private let bookmark: Data
    /// Signet ordinaire, sans portée de sécurité : seulement pour un fichier que
    /// l'application lit sans l'avoir reçu de l'utilisateur (jeu de test des
    /// captures d'écran). Absent des références enregistrées.
    private var unscoped: Bool?

    init(url: URL) throws {
        try self.init(url: url, securityScoped: true)
    }

    init(url: URL, securityScoped: Bool) throws {
        path = url.standardizedFileURL.path
        bookmark = try url.bookmarkData(
            options: securityScoped ? [.withSecurityScope] : [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        unscoped = securityScoped ? nil : true
    }

    var displayName: String {
        (path as NSString).lastPathComponent
    }

    /// Donne accès au fichier le temps de `body`.
    func withAccess<T>(_ body: (URL) throws -> T) throws -> T {
        var isStale = false
        let url = try URL(
            resolvingBookmarkData: bookmark,
            options: unscoped == true ? [] : [.withSecurityScope],
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        )
        let granted = url.startAccessingSecurityScopedResource()
        defer {
            if granted { url.stopAccessingSecurityScopedResource() }
        }
        return try body(url)
    }

    static func == (lhs: DocumentReference, rhs: DocumentReference) -> Bool {
        lhs.path == rhs.path
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(path)
    }
}
