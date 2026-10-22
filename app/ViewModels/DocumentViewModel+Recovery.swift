import Foundation

/// Ce qu'il faut pour retenir les modifications non enregistrées d'un document.
struct RecoveryContext {
    /// Mot de passe du document, gardé en mémoire seulement, pour rouvrir sa copie d'ouverture.
    var password: String?
    /// Empreinte du fichier à l'ouverture ou au dernier enregistrement.
    var stamp: FileStamp?
    /// Copie de la version d'ouverture, faite au premier enregistrement.
    var basePath: String?
    /// Copies des documents dont des pages ont été insérées (sources 1, 2…).
    var extraSources: [String] = []
    /// Vrai quand la fenêtre a la charge des modifications retenues pour son fichier.
    var isActive = false
    /// Faux si la version d'ouverture n'a pas pu être conservée : plus rien n'est retenu.
    var isAvailable = true
}

// MARK: - Récupération des modifications non enregistrées

/// Tant que des modifications ne sont pas enregistrées, l'organisation des pages,
/// les modifications de leur contenu et le texte reconnu sont retenus à chaque
/// changement (cf. ``RecoveryStore``). Si l'application s'arrête ou est quittée
/// entre-temps, ils sont proposés à la prochaine ouverture du fichier. Les pages renvoient au fichier tel qu'il a été ouvert ; avant le
/// premier enregistrement, qui le remplace, une copie en est conservée, comme
/// des documents dont des pages sont insérées.
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
            store.discard(record)
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
        recoveryStore?.discard(record)
    }

    /// Restaure les modifications proposées ; elles s'annulent comme les autres.
    func restoreRecovery() {
        guard let record = recoveryOffer else { return }
        recoveryOffer = nil
        Task { await restore(record) }
    }

    /// Retient l'organisation et le contenu des pages tant qu'ils ne sont pas enregistrés.
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
            pages: pages.map { page in
                let edits = page.editRevision == 0 ? [] : (try? session?.contentRecord(page: page.id)) ?? []
                let layer = page.textRevision == 0 ? [] : session?.textLayerRecord(page: page.id) ?? []
                return RecoveryRecord.Page(page, edits: edits, layer: layer)
            },
            date: Date()
        ))
    }

    /// Avant le premier enregistrement, conserve la version d'ouverture du
    /// document, à laquelle renvoient les pages retenues.
    func prepareRecoveryBase() async {
        guard recovery.isAvailable, recovery.basePath == nil, let store = recoveryStore, let session else { return }
        let base = await Task.detached(priority: .userInitiated) {
            try? store.makeCopy(session.sourceBytes(source: 0), of: .base)
        }.value
        if let base {
            // Retenu avant que le fichier ne soit touché : si l'écriture échoue en
            // chemin, les modifications restent rattachées à cette copie.
            recovery.basePath = base
            updateRecovery()
        } else {
            logger.error("Copie d'ouverture impossible : les modifications ne seront plus retenues")
            disableRecovery()
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
        store.removeCopy(recovery.basePath)
        recovery.extraSources.forEach(store.removeCopy)
    }

    /// Plus rien ne peut être retenu pour ce document (copie impossible, document
    /// inséré protégé par un mot de passe, qui n'est jamais écrit sur le disque).
    func disableRecovery() {
        recovery.isAvailable = false
        updateRecovery()
        setModified(isModified)
    }

    /// Les modifications pourront être proposées à la prochaine ouverture.
    var isRecoverable: Bool {
        recovery.isAvailable && recovery.stamp != nil && recoveryStore != nil
    }

    /// Conserve une copie d'un document dont des pages vont être insérées
    /// (source `source` de la session).
    func retainCopy(ofSource source: UInt32, protected: Bool) async {
        guard recovery.isAvailable, let store = recoveryStore, let session else { return }
        var copy: String?
        if !protected {
            copy = await Task.detached(priority: .userInitiated) {
                try? store.makeCopy(session.sourceBytes(source: source), of: .source)
            }.value
        }
        guard let copy, Int(source) == recovery.extraSources.count + 1 else {
            disableRecovery()
            return
        }
        recovery.extraSources.append(copy)
    }

    private func restore(_ record: RecoveryRecord) async {
        phase = .opening
        let password = recovery.password
        let opened = await Task.detached(priority: .userInitiated) {
            Result { () -> (PdfDocument?, [PdfDocument]) in
                let base = try record.basePath.map { try PdfDocument.open(path: $0, password: password) }
                let others = try record.sources.map { try PdfDocument.open(path: $0, password: nil) }
                return (base, others)
            }
        }.value
        // Fenêtre fermée entre-temps : les modifications restent proposées.
        guard !isClosed else { return }
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
            // Les pages reposent sur une version antérieure du fichier : même en
            // annulant la restauration, elles diffèrent du fichier enregistré.
            session?.forgetSaved()
        } else {
            phase = .ready
        }
        others.forEach { _ = addSource($0) }
        recovery.extraSources = record.sources
        let restored = perform {
            try $0.restore(
                pages: record.pages.map(\.item),
                contents: record.pages.map(\.contentItems),
                layers: record.pages.map(\.layerItems)
            )
        }
        guard restored else {
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
        recoveryStore?.discard(record)
        recovery.basePath = nil
        recovery.extraSources = []
        failureMessage = String(localized: "Les modifications non enregistrées n'ont pas pu être restaurées.")
    }
}
