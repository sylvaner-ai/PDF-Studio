import CoreGraphics
import Foundation

/// Écriture des modifications retenues (cf. ``RecoveryStore``), hors du fil
/// principal.
///
/// Les demandes de toutes les fenêtres passent par une même file série et
/// s'exécutent dans l'ordre. Les écritures rapprochées d'un même document sont
/// regroupées : la dernière demandée est faite au plus ``delay`` après la
/// première, ce qui borne ce qu'un arrêt imprévu peut faire perdre. Une
/// suppression passe après l'écriture en cours et annule celle qui attend.
/// Quitter l'application écrit aussitôt ce qui attend (``flush()``).
///
/// La file a la priorité des actions de l'utilisateur, comme les autres
/// appels au moteur : le fil d'un document prend la priorité de la requête
/// qu'il exécute, et une requête de rendu ne doit jamais attendre derrière une
/// requête de moindre priorité (inversion de priorité).
final class RecoveryWriter: @unchecked Sendable {
    static let shared = RecoveryWriter()

    /// Attente au plus entre une modification et son écriture.
    static let delay: TimeInterval = 0.5

    private let queue = DispatchQueue(label: "fr.sylvainrobert.pdfstudio.recuperation", qos: .userInitiated)
    /// Écritures en attente, par fichier de document ; lues et modifiées sur
    /// `queue` seulement.
    private var pending: [String: @Sendable () -> Void] = [:]

    /// Prévoit l'écriture `write` pour le document `path` ; elle remplace celle
    /// qui attendait encore.
    func schedule(for path: String, _ write: @escaping @Sendable () -> Void) {
        queue.async {
            let waiting = self.pending[path] != nil
            self.pending[path] = write
            if !waiting {
                self.queue.asyncAfter(deadline: .now() + Self.delay) {
                    self.pending.removeValue(forKey: path)?()
                }
            }
        }
    }

    /// Exécute `work` pour le document `path` après ce qui est en cours,
    /// l'écriture en attente étant abandonnée.
    func perform(for path: String, _ work: @escaping @Sendable () -> Void) {
        queue.async {
            self.pending[path] = nil
            work()
        }
    }

    /// Comme ``perform(for:_:)``, en attendant la fin de `work`.
    func performNow(for path: String, _ work: @escaping @Sendable () -> Void) async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            perform(for: path) {
                work()
                continuation.resume()
            }
        }
    }

    /// Fait aussitôt les écritures en attente et attend leur fin (fin de
    /// l'application, ouverture d'un document, tests).
    func flush() {
        queue.sync {
            let waiting = pending
            pending = [:]
            waiting.values.forEach { $0() }
        }
    }
}

/// Écriture (ou suppression, `removes`) des modifications retenues d'un document.
struct RecoveryJob {
    let path: String
    let removes: Bool
    let write: @Sendable () -> Void
}

/// État d'un document à retenir, lu dans le moteur sur la file d'écriture.
struct RecoverySnapshot: Sendable {
    let path: String
    let stamp: FileStamp
    let basePath: String?
    let sources: [String]
    let session: EditSession
    /// Format des pages de chaque document source.
    let sizes: [[CGSize]]
    let cache: RecoveryPageCache

    /// Modifications retenues ; `nil` si une page vient d'un document dont la
    /// copie n'est pas encore faite (une écriture suivante le complétera).
    func record() -> RecoveryRecord? {
        let pages = session.pages().map { DisplayPage($0, sizes: sizes) }
        guard pages.allSatisfy({ ($0.source?.document ?? 0) <= sources.count }) else { return nil }
        return RecoveryRecord(
            path: path,
            stamp: stamp,
            basePath: basePath,
            sources: sources,
            pages: pages.map { page in
                RecoveryRecord.Page(
                    page, edits: cache.edits(of: page, in: session), layer: cache.layer(of: page, in: session)
                )
            },
            date: Date()
        )
    }
}

/// Modifications et couches de texte des pages déjà lues dans le moteur, par
/// état de la page : seule la page modifiée est relue à l'écriture suivante.
/// Utilisé sur la file de ``RecoveryWriter`` seulement.
final class RecoveryPageCache: @unchecked Sendable {
    private var edits: [UInt64: (revision: UInt64, items: [ObjectEditItem])] = [:]
    private var layers: [UInt64: (revision: UInt64, items: [LayerWordItem])] = [:]

    func edits(of page: DisplayPage, in session: EditSession) -> [ObjectEditItem] {
        guard page.editRevision != 0 else { return [] }
        if let known = edits[page.id], known.revision == page.editRevision {
            return known.items
        }
        // Une page qui ne se lit plus (supprimée entre-temps) n'est pas retenue.
        guard let items = try? session.contentRecord(page: page.id) else { return [] }
        edits[page.id] = (page.editRevision, items)
        return items
    }

    func layer(of page: DisplayPage, in session: EditSession) -> [LayerWordItem] {
        guard page.textRevision != 0 else { return [] }
        if let known = layers[page.id], known.revision == page.textRevision {
            return known.items
        }
        let items = session.textLayerRecord(page: page.id)
        layers[page.id] = (page.textRevision, items)
        return items
    }
}
