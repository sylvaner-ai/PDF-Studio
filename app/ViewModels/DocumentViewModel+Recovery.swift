import Foundation

/// Ce qu'il faut pour retenir les modifications non enregistrées d'un document.
struct RecoveryContext {
    /// Mot de passe du document, gardé en mémoire seulement, pour rouvrir sa copie d'ouverture.
    var password: String?
    /// Empreinte du fichier à l'ouverture ou au dernier enregistrement.
    var stamp: FileStamp?
    /// Copie de la version d'ouverture, faite au premier enregistrement.
    var basePath: String?
    /// Documents dont des pages ont été insérées (sources 1, 2…).
    var extraSources: [DocumentReference] = []
    /// Vrai quand la fenêtre a la charge des modifications retenues pour son fichier.
    var isActive = false
    /// Faux si la version d'ouverture n'a pas pu être conservée : plus rien n'est retenu.
    var isAvailable = true
}

// MARK: - Récupération des modifications non enregistrées

/// Tant que des modifications ne sont pas enregistrées, l'organisation des pages
/// est retenue à chaque changement (cf. ``RecoveryStore``). Si l'application
/// s'arrête ou est quittée entre-temps, elle est proposée à la prochaine ouverture
/// du fichier. Les pages renvoient au fichier tel qu'il a été ouvert ; avant le
/// premier enregistrement, qui le remplace, une copie en est conservée.
extension DocumentViewModel {
    /// Le document vient de s'ouvrir : propose les modifications retenues pour son fichier.
    func offerRecovery() {
        recovery.isActive = true
        guard let store = recoveryStore, let record = store.record(for: reference.path) else { return }
        // Sans copie d'ouverture, les pages renvoient au fichier : il ne doit pas avoir changé.
        let usable = record.basePath.map { FileManager.default.fileExists(atPath: $0) }
            ?? (record.stamp == recovery.stamp)
        guard usable, record.version == 1, !record.pages.isEmpty else {
            logger.info("Modifications retenues périmées, supprimées")
            store.removeRecord(for: reference.path)
            store.removeBase(record.basePath)
            return
        }
        recoveryOffer = record
    }

    /// Vrai si le fichier a changé depuis que les modifications proposées ont été retenues.
    var recoveredFileChanged: Bool {
        recoveryOffer.map { $0.stamp != recovery.stamp } ?? false
    }

    /// Supprime les modifications proposées.
    func declineRecovery() {
        guard let record = recoveryOffer else { return }
        recoveryOffer = nil
        recoveryStore?.removeRecord(for: reference.path)
        recoveryStore?.removeBase(record.basePath)
    }

    /// Restaure les modifications proposées ; elles s'annulent comme les autres.
    func restoreRecovery() {
        guard let record = recoveryOffer else { return }
        recoveryOffer = nil
        Task { await restore(record) }
    }

    /// Retient l'organisation des pages tant qu'elle n'est pas enregistrée.
    func updateRecovery() {
        guard recovery.isActive, recoveryOffer == nil, let store = recoveryStore else { return }
        guard isModified, recovery.isAvailable, let stamp = recovery.stamp else {
            store.removeRecord(for: reference.path)
            return
        }
        store.save(RecoveryRecord(
            path: reference.path,
            stamp: stamp,
            basePath: recovery.basePath,
            sources: recovery.extraSources,
            pages: pages.map { RecoveryRecord.Page($0) },
            date: Date()
        ))
    }

    /// Avant le premier enregistrement, conserve la version d'ouverture du
    /// document, à laquelle renvoient les pages retenues.
    func prepareRecoveryBase() async {
        guard recovery.isAvailable, recovery.basePath == nil, let store = recoveryStore, let session else { return }
        let base = await Task.detached(priority: .userInitiated) {
            try? store.makeBase(session.openingBytes())
        }.value
        if let base {
            recovery.basePath = base
        } else {
            logger.error("Copie d'ouverture impossible : les modifications ne seront plus retenues")
            recovery.isAvailable = false
        }
    }

    /// Le document vient d'être enregistré, éventuellement dans un autre fichier
    /// que `previousPath`.
    func didSave(stamp: FileStamp?, previousPath: String) {
        if previousPath != reference.path {
            recoveryStore?.removeRecord(for: previousPath)
        }
        recovery.stamp = stamp
        updateRecovery()
    }

    /// La fenêtre se ferme : des modifications non enregistrées restent proposées
    /// à la prochaine ouverture ; sinon, il n'y a plus rien à retenir.
    func endRecovery() {
        guard recovery.isActive, !isModified, recoveryOffer == nil, let store = recoveryStore else { return }
        store.removeRecord(for: reference.path)
        store.removeBase(recovery.basePath)
    }

    private func restore(_ record: RecoveryRecord) async {
        phase = .opening
        let password = recovery.password
        let opened = await Task.detached(priority: .userInitiated) {
            Result { () -> (PdfDocument?, [PdfDocument]) in
                let base = try record.basePath.map { try PdfDocument.open(path: $0, password: password) }
                let others = try record.sources.map { source in
                    try source.withAccess { try PdfDocument.open(path: $0.path, password: nil) }
                }
                return (base, others)
            }
        }.value
        guard case let .success((base, others)) = opened else {
            if case let .failure(error) = opened {
                logger.error("Restauration impossible : \(error.localizedDescription, privacy: .public)")
            }
            abandon(record)
            phase = .ready
            return
        }
        if let base {
            show(base)
            recovery.basePath = record.basePath
        } else {
            phase = .ready
        }
        others.forEach { _ = addSource($0) }
        recovery.extraSources = record.sources
        guard perform({ try $0.restore(pages: record.pages.map(\.item)) }) else {
            abandon(record)
            // Les pages affichées sont celles de la copie d'ouverture : le fichier est rouvert.
            if base != nil {
                await open(password: password)
            }
            return
        }
        if let base {
            await loadOutline(base)
            await checkSignature()
        }
    }

    /// Les modifications retenues ne peuvent pas être restaurées : elles sont supprimées.
    private func abandon(_ record: RecoveryRecord) {
        recoveryStore?.removeRecord(for: reference.path)
        recoveryStore?.removeBase(record.basePath)
        recovery.basePath = nil
        recovery.extraSources = []
        failureMessage = String(localized: "Les modifications non enregistrées n'ont pas pu être restaurées.")
    }
}
