import Security
import XCTest
@testable import PDFStudio

/// Réécriture d'un fichier interrompue par un arrêt imprévu : le fichier est
/// rétabli à la réouverture (cf. ``FileSaver/repairInterruptedWrite(of:journal:)``).
final class InterruptedWriteTests: XCTestCase {
    private var directory: URL!
    private var backups: URL!
    private var journal: URL!
    private var url: URL!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("écriture-\(UUID())")
        backups = directory.appendingPathComponent("sauvegardes")
        journal = directory.appendingPathComponent("écritures en cours")
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        url = directory.appendingPathComponent("contrat.pdf")
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: directory)
    }

    /// Simule un arrêt pendant la réécriture : la note est posée, la copie
    /// faite, et le fichier ne contient que `written`.
    private func interrupt(original: Data, new: Data, written: Data) throws {
        try original.write(to: url)
        let backup = backups.appendingPathComponent("copie contrat.pdf")
        try original.write(to: backup)
        _ = try FileSaver.beginWrite(of: url, backup: backup, data: new, journal: journal)
        try written.write(to: url)
    }

    private var notes: [URL] {
        (try? FileManager.default.contentsOfDirectory(at: journal, includingPropertiesForKeys: nil)) ?? []
    }

    func testUnEnregistrementCompletNeLaissePasDeNote() throws {
        try Data("ancien".utf8).write(to: url)
        try FileSaver.replace(url, with: Data("nouveau".utf8), backups: backups, journal: journal)
        XCTAssertTrue(notes.isEmpty)
        XCTAssertNil(FileSaver.repairInterruptedWrite(of: url, journal: journal))
        XCTAssertEqual(try Data(contentsOf: url), Data("nouveau".utf8))
    }

    func testRetablitUnFichierAMoitieEcrit() throws {
        let original = Data(repeating: 1, count: 4096)
        let new = Data(repeating: 2, count: 8192)
        try interrupt(original: original, new: new, written: new.prefix(1000))
        XCTAssertEqual(notes.count, 1)

        XCTAssertEqual(FileSaver.repairInterruptedWrite(of: url, journal: journal), .restored)
        XCTAssertEqual(try Data(contentsOf: url), original)
        XCTAssertTrue(notes.isEmpty)
        // Rien de plus à réparer ensuite.
        XCTAssertNil(FileSaver.repairInterruptedWrite(of: url, journal: journal))
    }

    func testGardeUnFichierEntierementEcrit() throws {
        let new = Data("version enregistrée".utf8)
        try interrupt(original: Data("version précédente".utf8), new: new, written: new)
        XCTAssertEqual(FileSaver.repairInterruptedWrite(of: url, journal: journal), .completed)
        XCTAssertEqual(try Data(contentsOf: url), new)
        XCTAssertTrue(notes.isEmpty)
    }

    func testSignaleUneCopieIntrouvable() throws {
        try interrupt(original: Data("précédente".utf8), new: Data("nouvelle".utf8), written: Data())
        let backup = backups.appendingPathComponent("copie contrat.pdf")
        try FileManager.default.removeItem(at: backup)
        XCTAssertEqual(FileSaver.repairInterruptedWrite(of: url, journal: journal), .failed(backup: backup.path))
        // La note reste : rien n'a été réparé.
        XCTAssertEqual(notes.count, 1)
    }

    func testLaCopieDUneEcritureEnCoursNEstJamaisSupprimee() throws {
        try Data("0".utf8).write(to: url)
        for version in 1...(FileSaver.maxBackups + 2) {
            try FileSaver.replace(url, with: Data("\(version)".utf8), backups: backups, journal: journal)
        }
        let copies = try FileManager.default.contentsOfDirectory(at: backups, includingPropertiesForKeys: nil)
        XCTAssertEqual(copies.count, FileSaver.maxBackups)
        // La plus récente est la version qui précédait la dernière écriture.
        XCTAssertTrue(try copies.contains { try Data(contentsOf: $0) == Data("\(FileSaver.maxBackups + 1)".utf8) })
    }
}

/// Arrêt imprévu de l'application avec des modifications non enregistrées :
/// elles sont retenues, puis proposées et restaurées à la réouverture.
@MainActor
final class CrashRecoveryTests: XCTestCase {
    private var directory: URL!
    private var url: URL!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("arrêt-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        url = directory.appendingPathComponent("édition.pdf")
        let fixture = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "edition", withExtension: "pdf"))
        try FileManager.default.copyItem(at: fixture, to: url)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private func openModel() async throws -> DocumentViewModel {
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready)
        return model
    }

    private func waitUntil(_ condition: () -> Bool) async {
        for _ in 0..<200 where !condition() {
            try? await Task.sleep(for: .milliseconds(25))
        }
    }

    func testRestaureLesModificationsApresUnArretImprevu() async throws {
        let original = try Data(contentsOf: url)
        let crashed = try await openModel()
        let store = try XCTUnwrap(crashed.recoveryStore)
        let session = try XCTUnwrap(crashed.session)
        let first = try XCTUnwrap(crashed.pages.first)

        // Une page tournée, un objet déplacé.
        crashed.rotate([0], clockwise: true)
        let object = try XCTUnwrap(session.pageObjects(page: first.id).first { $0.movable && $0.kind == .text })
        XCTAssertTrue(crashed.perform { try $0.moveObjects(page: first.id, objects: [object.id], dx: 12, dy: 0) })
        XCTAssertTrue(crashed.isModified)

        // L'écriture se fait hors du fil principal, peu après la modification.
        RecoveryWriter.shared.flush()
        let record = try XCTUnwrap(store.record(for: url.path))
        XCTAssertEqual(record.pages.first?.quarterTurns, 1)
        XCTAssertEqual(record.pages.first?.edits?.count, 1)
        // Retenir les modifications ne touche jamais au document.
        XCTAssertEqual(try Data(contentsOf: url), original)

        // L'application s'arrête sans fermer la fenêtre ; le document est rouvert.
        let reopened = try await openModel()
        let offer = try XCTUnwrap(reopened.recoveryOffer)
        XCTAssertEqual(offer.pages.count, record.pages.count)
        XCTAssertFalse(reopened.canEdit, "les pages restent figées jusqu'à la réponse")
        reopened.restoreRecovery()
        await waitUntil { reopened.pages.first?.quarterTurns == 1 && reopened.pages.first?.editRevision != 0 }
        XCTAssertEqual(reopened.pages.first?.quarterTurns, 1)
        let restored = try XCTUnwrap(reopened.session)
        let page = try XCTUnwrap(reopened.pages.first)
        XCTAssertEqual(try restored.contentRecord(page: page.id).map(\.object), [object.id])
        XCTAssertTrue(reopened.isModified)
        XCTAssertEqual(try Data(contentsOf: url), original)

        // Fermée sans enregistrer, la fenêtre ne laisse rien derrière elle.
        reopened.discardChanges()
        reopened.close()
        crashed.discardChanges()
        crashed.close()
        RecoveryWriter.shared.flush()
        XCTAssertNil(store.record(for: url.path))
        XCTAssertEqual(try Data(contentsOf: url), original)
    }

    func testIgnorerSupprimeLesModificationsRetenues() async throws {
        let crashed = try await openModel()
        let store = try XCTUnwrap(crashed.recoveryStore)
        crashed.rotate([0], clockwise: false)
        RecoveryWriter.shared.flush()
        XCTAssertNotNil(store.record(for: url.path))

        let reopened = try await openModel()
        XCTAssertNotNil(reopened.recoveryOffer)
        reopened.declineRecovery()
        RecoveryWriter.shared.flush()
        XCTAssertNil(store.record(for: url.path))
        XCTAssertEqual(reopened.pages.first?.quarterTurns, 0)
        reopened.close()
        crashed.discardChanges()
        crashed.close()
        RecoveryWriter.shared.flush()
    }

    func testEnregistrerDemandeConfirmationAvantDeToucherAuFichier() async throws {
        let original = try Data(contentsOf: url)
        let modified = try FileStamp(of: url)
        let model = try await openModel()
        model.rotate([0], clockwise: true)
        model.save()
        // ⌘S ne fait que demander confirmation : le fichier est intact.
        XCTAssertEqual(model.saving.prompt, .replace(signed: false, changedOnDisk: false))
        XCTAssertEqual(try Data(contentsOf: url), original)
        model.cancelSave()
        XCTAssertNil(model.saving.prompt)
        XCTAssertEqual(try Data(contentsOf: url), original)
        XCTAssertEqual(try FileStamp(of: url), modified)
        model.discardChanges()
        model.close()
        RecoveryWriter.shared.flush()
    }
}

/// Confidentialité (§6) : l'application ne peut émettre aucune connexion.
final class PrivacyTests: XCTestCase {
    func testLApplicationNePeutPasSeConnecterAuReseau() throws {
        let task = try XCTUnwrap(SecTaskCreateFromSelf(nil))
        func entitlement(_ name: String) -> CFTypeRef? {
            SecTaskCopyValueForEntitlement(task, name as CFString, nil)
        }
        // Bac à sable de macOS, sans autorisation réseau : le système refuse
        // toute connexion entrante ou sortante.
        XCTAssertEqual(entitlement("com.apple.security.app-sandbox") as? Bool, true)
        XCTAssertNil(entitlement("com.apple.security.network.client"))
        XCTAssertNil(entitlement("com.apple.security.network.server"))
    }
}
