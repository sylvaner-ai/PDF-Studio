import AppKit
import UniformTypeIdentifiers

// MARK: - Insertion de pages

/// Insertion de pages vierges, des pages d'un autre PDF (menu Pages, ou fichier
/// déposé dans la grille) et de pages glissées depuis la grille d'une autre
/// fenêtre. Chaque document inséré devient une source de la session, dont une
/// copie est conservée pour la récupération ; l'insertion s'annule par ⌘Z.
extension DocumentViewModel {
    /// Position d'insertion : après la dernière page visée, sinon à la fin.
    var insertionPoint: Int {
        targetPositions.last.map { $0 + 1 } ?? pages.count
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

    /// Choisit un PDF et insère toutes ses pages avant `position` (par défaut, à
    /// la position d'insertion).
    func chooseDocumentToInsert(before position: Int? = nil) {
        guard canEdit else { return }
        let position = position ?? insertionPoint
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf]
        panel.message = String(
            localized: "Les pages du document choisi seront insérées après la page sélectionnée."
        )
        panel.prompt = String(localized: "Insérer")
        panel.begin { [weak self] response in
            let url = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let url else { return }
                await self.insertDocument(at: url, before: position)
            }
        }
    }

    /// Insère toutes les pages d'un fichier PDF avant la position `position` ;
    /// demande son mot de passe s'il est protégé.
    func insertDocument(at url: URL, before position: Int, password: String? = nil) async {
        guard canEdit else { return }
        let opened = await Task.detached(priority: .userInitiated) {
            Result { try PdfDocument.open(path: url.path, password: password) }
        }.value
        switch opened {
        case let .success(document):
            await insert(document, before: position, protected: password != nil)
        case let .failure(error as EngineError):
            switch error {
            case .PasswordRequired, .WrongPassword:
                let wrongAttempt = password != nil
                guard let password = askPassword(for: url.lastPathComponent, wrongAttempt: wrongAttempt) else { return }
                await insertDocument(at: url, before: position, password: password)
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
        let positions = origin.positions(of: Set(ids)).map { UInt32($0) }
        guard !positions.isEmpty else { return }
        // Les pages d'un document protégé le restent : le même mot de passe les ouvre.
        let password = origin.recovery.password
        let copied = await Task.detached(priority: .userInitiated) {
            Result { try PdfDocument.fromData(data: session.exportPages(positions: positions), password: password) }
        }.value
        switch copied {
        case let .success(document):
            await insert(document, before: position, protected: password != nil)
        case let .failure(error):
            report(error)
        }
    }

    /// Insère toutes les pages d'un document ouvert avant la position `position`.
    private func insert(_ document: PdfDocument, before position: Int, protected: Bool) async {
        guard canEdit, let source = addSource(document) else { return }
        await retainCopy(ofSource: source, protected: protected)
        let at = UInt32(min(position, pages.count))
        let indices = Array(0..<document.pageCount())
        var inserted: [UInt32] = []
        perform { inserted = try $0.insertPages(at: at, source: source, indices: indices) }
        select(inserted)
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
