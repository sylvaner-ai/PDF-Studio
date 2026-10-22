import AppKit
import UniformTypeIdentifiers

/// Confirmation et progression d'un enregistrement.
struct SavingState {
    enum Prompt: Equatable {
        /// Remplacer le fichier d'origine, en prévenant si une signature sera
        /// invalidée ou si une autre application a modifié le fichier entre-temps.
        case replace(signed: Bool, changedOnDisk: Bool)
    }

    /// Confirmation demandée avant d'écrire sur le disque.
    var prompt: Prompt?
    var inProgress = false
    /// À faire une fois l'enregistrement réussi (fermer la fenêtre, par exemple).
    var completion: (@MainActor () -> Void)?
}

/// Fichier écrit : nouvel emplacement s'il change, empreinte et révision écrite.
private struct WrittenFile: Sendable {
    let relocated: DocumentReference?
    let stamp: FileStamp?
    let revision: UInt64
}

// MARK: - Enregistrement

/// Enregistrer (⌘S) remplace le fichier d'origine après confirmation et copie de
/// sauvegarde ; Enregistrer sous (⇧⌘S) écrit à l'emplacement choisi, qui devient
/// celui de la fenêtre. Le PDF est produit hors du fil principal et relu par le
/// moteur avant d'être écrit (cf. ``FileSaver``). Au premier enregistrement, la
/// version d'ouverture est conservée pour la récupération.
extension DocumentViewModel {
    var canSave: Bool { canEdit && isModified && !saving.inProgress }
    var canSaveAs: Bool { canEdit && !saving.inProgress }

    /// Enregistre les modifications, après confirmation ; `completion` suit un
    /// enregistrement réussi (ou vient aussitôt s'il n'y a rien à enregistrer).
    func save(then completion: (@MainActor () -> Void)? = nil) {
        guard isModified else {
            completion?()
            return
        }
        guard canSave else { return }
        saving.completion = completion
        saving.prompt = .replace(signed: isSigned, changedOnDisk: fileChangedOnDisk)
    }

    /// Vrai si le fichier a changé sur le disque depuis l'ouverture ou le dernier
    /// enregistrement : une autre application l'a modifié.
    private var fileChangedOnDisk: Bool {
        guard let stamp = recovery.stamp,
              let current = try? reference.withAccess({ try FileStamp(of: $0) }) else { return false }
        return current != stamp
    }

    /// Empreinte actuelle du fichier du document, lue hors du fil principal.
    private func currentStamp() async -> FileStamp? {
        let reference = reference
        return await Task.detached(priority: .utility) {
            try? reference.withAccess { try FileStamp(of: $0) }
        }.value
    }

    func cancelSave() {
        saving.prompt = nil
        saving.completion = nil
    }

    /// La fenêtre se ferme sans enregistrer : les modifications sont abandonnées.
    func discardChanges() {
        setModified(false)
    }

    /// Remplace le fichier d'origine par la version modifiée (l'original est sauvegardé).
    func replaceOriginal() {
        saving.prompt = nil
        let reference = reference
        write { data in
            try reference.withAccess { try FileSaver.replace($0, with: data) }
            return nil
        }
    }

    /// Enregistre le document à l'emplacement choisi, qui devient celui de la fenêtre.
    func saveAs() {
        saving.prompt = nil
        guard canSaveAs else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.nameFieldStringValue = reference.displayName
        panel.canCreateDirectories = true
        if isSigned {
            panel.message = String(
                localized: "La signature numérique de ce document ne sera pas valide dans la copie."
            )
        }
        panel.begin { [weak self] response in
            let url = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self else { return }
                guard let url else {
                    self.saving.completion = nil
                    return
                }
                self.saveCopy(to: url)
            }
        }
    }

    private func saveCopy(to url: URL) {
        let path = url.standardizedFileURL.path
        if path != reference.path, DocumentOpener.shared.isShown(path: path) {
            saving.completion = nil
            failureMessage = String(localized: "Ce fichier est ouvert dans une autre fenêtre. Fermez-la d'abord.")
            return
        }
        write { data in
            try FileSaver.write(data, to: url)
            return try DocumentReference(url: url)
        }
    }

    /// Produit le PDF hors du fil principal et le confie à `store`, qui renvoie le
    /// nouveau fichier du document s'il change ; retient ensuite l'état enregistré.
    /// Quitter l'application attend la fin de l'écriture (cf. ``DocumentOpener``).
    private func write(_ store: @escaping @Sendable (Data) throws -> DocumentReference?) {
        guard let session, !saving.inProgress else { return }
        saving.inProgress = true
        DocumentOpener.shared.beginSave()
        let original = reference
        Task {
            defer { DocumentOpener.shared.endSave() }
            await prepareRecoveryBase()
            let result = await Task.detached(priority: .userInitiated) {
                Result { () -> WrittenFile in
                    let exported = try session.exportPdf()
                    let relocated = try store(exported.bytes)
                    let stamp = try? (relocated ?? original).withAccess { try FileStamp(of: $0) }
                    return WrittenFile(relocated: relocated, stamp: stamp, revision: exported.revision)
                }
            }.value
            saving.inProgress = false
            switch result {
            case let .success(written):
                if let relocated = written.relocated {
                    relocate(to: relocated)
                }
                // Une modification faite pendant l'écriture reste à enregistrer.
                session.markSaved(revision: written.revision)
                setModified(session.isModified())
                didSave(stamp: written.stamp, previousPath: original.path)
                let completion = saving.completion
                saving.completion = nil
                completion?()
            case let .failure(error):
                saving.completion = nil
                // Un original restauré après un échec porte une nouvelle date : les
                // modifications retenues doivent rester rattachées au fichier.
                if let stamp = await currentStamp() {
                    recovery.stamp = stamp
                }
                updateRecovery()
                reportSaveFailure(error)
            }
        }
    }

    private func reportSaveFailure(_ error: Error) {
        if error is EngineError {
            return report(error)
        }
        logger.error("Enregistrement impossible : \(error.localizedDescription, privacy: .public)")
        let nsError = error as NSError
        let denied = nsError.domain == NSCocoaErrorDomain && nsError.code == NSFileWriteNoPermissionError
            || nsError.domain == NSPOSIXErrorDomain && [Int(EPERM), Int(EACCES)].contains(nsError.code)
        failureMessage = denied
            ? String(localized: """
                PDF Studio n'a pas l'autorisation de modifier ce fichier. \
                Choisissez Fichier › Enregistrer sous… pour en enregistrer une copie.
                """)
            : String(localized: "Le document n'a pas pu être enregistré. \(error.localizedDescription)")
    }
}
