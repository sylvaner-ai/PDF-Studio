import AppKit
import UniformTypeIdentifiers

// MARK: - Insertion de pages

/// Insertion de pages vierges, des pages d'un autre PDF (menu Pages, ou fichier
/// déposé dans la grille) et de pages glissées depuis la grille d'une autre
/// fenêtre. Chaque document inséré devient une source de la session, dont une
/// copie est conservée pour la récupération ; l'insertion s'annule par ⌘Z.
///
/// Ouvrir un document prend du temps : l'endroit de l'insertion est retenu par
/// la page qui la précède, qui peut entre-temps avoir changé de position. Les
/// insertions se font une à une, dans l'ordre où elles ont été demandées.
extension DocumentViewModel {
    /// Position d'insertion : après la dernière page visée, sinon à la fin.
    var insertionPoint: Int {
        targetPositions.last.map { $0 + 1 } ?? pages.count
    }

    /// Page après laquelle insérer devant `position` ; `nil` : en tête.
    private func anchor(before position: Int) -> UInt64? {
        position > 0 ? pages[safe: min(position, pages.count) - 1]?.id : nil
    }

    /// Position qui suit la page `anchor` ; à la fin si elle a été supprimée.
    private func position(after anchor: UInt64?) -> Int {
        guard let anchor else { return 0 }
        return pages.firstIndex { $0.id == anchor }.map { $0 + 1 } ?? pages.count
    }

    /// Insère une page vierge du format de la page qui la précède.
    func insertBlankPage(before position: Int? = nil) {
        guard canEdit else { return }
        let position = min(position ?? insertionPoint, pages.count)
        // Format A4 s'il n'y a pas de page de référence.
        let size = (pages[safe: position - 1] ?? pages.first)?.size(DisplayRotation())
            ?? CGSize(width: 595.28, height: 841.89)
        let blank = PageSize(width: Float(size.width), height: Float(size.height))
        var inserted: UInt32?
        perform { inserted = try $0.insertBlank(at: UInt32(position), size: blank) }
        select(inserted.map { [$0] } ?? [])
    }

    /// Choisit un ou plusieurs PDF et insère toutes leurs pages avant `position`
    /// (par défaut, à la position d'insertion), dans l'ordre de leurs noms : c'est
    /// aussi ainsi que l'on fusionne des documents.
    func chooseDocumentToInsert(before position: Int? = nil) {
        guard canEdit else { return }
        let anchor = anchor(before: position ?? insertionPoint)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = true
        panel.message = String(
            localized: "Les pages des documents choisis seront insérées après la page sélectionnée."
        )
        panel.prompt = String(localized: "Insérer")
        panel.begin { [weak self] response in
            let urls = response == .OK ? panel.urls : []
            Task { @MainActor in
                let sorted = urls.sorted {
                    $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending
                }
                await self?.insertDocuments(sorted, after: anchor)
            }
        }
    }

    /// Insère les pages de plusieurs PDF, dans l'ordre donné, avant `position`.
    func insertDocuments(_ urls: [URL], before position: Int) async {
        await insertDocuments(urls, after: anchor(before: position))
    }

    private func insertDocuments(_ urls: [URL], after anchor: UInt64?) async {
        // Chaque document s'insère devant le suivant : ils gardent leur ordre.
        for url in urls.reversed() {
            await insertDocument(at: url, after: anchor)
        }
    }

    /// Insère toutes les pages d'un fichier PDF après la page `anchor` ; demande
    /// son mot de passe s'il est protégé.
    private func insertDocument(at url: URL, after anchor: UInt64?, password: String? = nil) async {
        guard canEdit else { return }
        let opened = await Task.detached(priority: .userInitiated) {
            Result { try PdfDocument.open(path: url.path, password: password) }
        }.value
        switch opened {
        case let .success(document):
            await insert(document, after: anchor, protected: password != nil)
        case let .failure(error as EngineError):
            switch error {
            case .PasswordRequired, .WrongPassword:
                let wrongAttempt = password != nil
                guard let password = askPassword(for: url.lastPathComponent, wrongAttempt: wrongAttempt) else { return }
                await insertDocument(at: url, after: anchor, password: password)
            case .Failed:
                report(error)
            }
        case let .failure(error):
            logger.error("Insertion impossible : \(error.localizedDescription, privacy: .public)")
            failureMessage = String(localized: "PDF Studio n'a pas pu accéder à ce fichier.")
        }
    }

    /// Pages glissées depuis la grille d'une autre fenêtre : elles sont copiées,
    /// telles qu'elles y apparaissent, avant la position `position`.
    func insertPages(_ ids: [UInt64], from origin: DocumentViewModel, before position: Int) async {
        guard canEdit, let session = origin.session else { return }
        let anchor = anchor(before: position)
        // Dans l'ordre du document d'origine.
        let ordered = origin.positions(of: Set(ids)).map { origin.pages[$0].id }
        guard !ordered.isEmpty else { return }
        // Les pages d'un document protégé le restent : le même mot de passe les ouvre.
        let password = origin.recovery.password
        let copied = await Task.detached(priority: .userInitiated) {
            Result { try PdfDocument.fromData(data: session.exportPages(ids: ordered), password: password) }
        }.value
        switch copied {
        case let .success(document):
            await insert(document, after: anchor, protected: password != nil)
        case let .failure(error):
            report(error)
        }
    }

    /// Insère toutes les pages d'un document ouvert après la page `anchor`, une
    /// fois terminées les insertions demandées avant : les sources et leurs
    /// copies de récupération se suivent ainsi dans le même ordre.
    private func insert(_ document: PdfDocument, after anchor: UInt64?, protected: Bool) async {
        let previous = insertionTask
        let task = Task { @MainActor in
            await previous?.value
            await self.insertNow(document, after: anchor, protected: protected)
        }
        insertionTask = task
        await task.value
    }

    private func insertNow(_ document: PdfDocument, after anchor: UInt64?, protected: Bool) async {
        guard canEdit, let source = addSource(document) else { return }
        await retainCopy(ofSource: source, protected: protected)
        guard canEdit else {
            if !isClosed {
                failureMessage = String(
                    localized: "Les pages n'ont pas été insérées : une autre opération était en cours."
                )
            }
            return
        }
        let insertion = UInt32(position(after: anchor))
        let indices = Array(0..<document.pageCount())
        var inserted: [UInt32] = []
        perform { inserted = try $0.insertPages(at: insertion, source: source, indices: indices) }
        select(inserted)
        // En mode Édition, les pages scannées insérées sont reconnues aussitôt.
        prepareEditing()
    }

    /// Document d'une page vierge du format de la page vierge `page`, ajouté
    /// aux sources une fois terminées les insertions demandées avant : les
    /// objets ajoutés à la page s'y dessinent (cf. `PageEditing+Adding.swift`).
    /// Renvoie son numéro de source, `nil` si une autre opération a commencé.
    func blankPageSource(for page: UInt64) async -> UInt32? {
        let previous = insertionTask
        let task = Task { @MainActor [weak self] () -> UInt32? in
            await previous?.value
            return await self?.addBlankPageSource(for: page)
        }
        insertionTask = Task { _ = await task.value }
        return await task.value
    }

    private func addBlankPageSource(for page: UInt64) async -> UInt32? {
        guard canEdit, let session else { return nil }
        let opened = await Task.detached(priority: .userInitiated) {
            Result { try PdfDocument.fromData(data: session.blankPageDocument(page: page), password: nil) }
        }.value
        guard canEdit else { return nil }
        switch opened {
        case let .success(document):
            guard let source = addSource(document) else { return nil }
            await retainCopy(ofSource: source, protected: false)
            return canEdit ? source : nil
        case let .failure(error):
            report(error)
            return nil
        }
    }

    private func select(_ positions: [UInt32]) {
        selection = Set(positions.compactMap { pages[safe: Int($0)]?.id })
        if let first = positions.first {
            canvas.go(to: Int(first))
        }
    }

    private func askPassword(for name: String, wrongAttempt: Bool) -> String? {
        let alert = NSAlert()
        alert.messageText = wrongAttempt
            ? String(localized: "Le mot de passe est incorrect.")
            : String(localized: "« \(name) » est protégé par un mot de passe.")
        alert.informativeText = String(localized: "Saisissez-le pour en insérer les pages.")
        let field = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        alert.accessoryView = field
        alert.addButton(withTitle: String(localized: "Insérer"))
        alert.addButton(withTitle: String(localized: "Annuler"))
        alert.window.initialFirstResponder = field
        return alert.runModal() == .alertFirstButtonReturn ? field.stringValue : nil
    }
}

/// Documents ouverts, pour retrouver l'origine des pages glissées d'une fenêtre
/// à l'autre.
@MainActor
enum OpenDocuments {
    private struct Entry {
        weak var model: DocumentViewModel?
    }

    private static var entries: [UUID: Entry] = [:]

    static func register(_ model: DocumentViewModel) {
        entries = entries.filter { $0.value.model != nil }
        entries[model.documentID] = Entry(model: model)
    }

    static func model(_ id: UUID) -> DocumentViewModel? {
        entries[id]?.model
    }
}
