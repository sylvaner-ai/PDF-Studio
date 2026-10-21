import CoreGraphics
import Foundation
import Observation
import os

/// État d'un document affiché dans une fenêtre, et organisation de ses pages.
///
/// Les pages sont réorganisées dans une session du moteur (``EditSession``) ; le
/// fichier n'est jamais modifié tant que l'utilisateur n'enregistre pas. Chaque
/// modification s'inscrit dans le gestionnaire d'annulation de la fenêtre, qui
/// commande l'historique du moteur (menus Annuler et Rétablir). L'enregistrement
/// demande toujours confirmation avant de remplacer un fichier (cf. ``FileSaver``).
@MainActor
@Observable
final class DocumentViewModel {
    enum Phase: Equatable {
        case opening
        case needsPassword(wrongAttempt: Bool)
        case ready
        case failed(message: String)
    }

    /// Fichier du document ; il change après « Enregistrer sous ».
    private(set) var reference: DocumentReference
    private(set) var phase: Phase = .opening
    private(set) var title: String
    /// Pages du document, dans l'ordre.
    private(set) var pages: [DisplayPage] = []
    /// Pages sélectionnées dans les vignettes, par identifiant.
    var selection: Set<UInt64> = []
    private(set) var renderer: PageRenderer?
    private(set) var canvas = CanvasController(pages: [])
    private(set) var search: SearchModel?
    private(set) var outline = OutlineTree.empty
    /// Vrai si les pages ont changé depuis l'ouverture ou le dernier enregistrement.
    private(set) var isModified = false
    /// Vrai si le document porte une signature numérique, qu'un enregistrement invaliderait.
    private(set) var isSigned = false
    /// Confirmation et progression de l'enregistrement (cf. `DocumentViewModel+Saving.swift`).
    var saving = SavingState()
    /// Message d'une opération qui a échoué, à montrer à l'utilisateur.
    var failureMessage: String?

    /// Gestionnaire d'annulation de la fenêtre.
    @ObservationIgnored weak var undoManager: UndoManager?
    /// Appelé quand le document change de fichier (« Enregistrer sous »).
    @ObservationIgnored var onRelocate: ((DocumentReference) -> Void)?
    @ObservationIgnored private(set) var session: EditSession?
    @ObservationIgnored private var declaredTitle: String?
    /// Format des pages de chaque document source, en points.
    @ObservationIgnored private var sourceSizes: [[CGSize]] = []

    let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "document")

    init(reference: DocumentReference) {
        self.reference = reference
        self.title = reference.displayName
    }

    var pageCount: Int { pages.count }

    /// Ouvre le document hors du fil principal.
    func open(password: String? = nil) async {
        phase = .opening
        let reference = reference
        let result = await Task.detached(priority: .userInitiated) {
            Result {
                try reference.withAccess { url in
                    try PdfDocument.open(path: url.path, password: password)
                }
            }
        }.value

        switch result {
        case let .success(document):
            show(document)
            await loadOutline(document)
            await checkSignature()
        case let .failure(error as EngineError):
            switch error {
            case .PasswordRequired:
                phase = .needsPassword(wrongAttempt: false)
            case .WrongPassword:
                phase = .needsPassword(wrongAttempt: true)
            case .Failed:
                logger.error("Ouverture impossible : \(error.technicalDetail ?? "", privacy: .public)")
                phase = .failed(message: error.userMessage)
            }
        case let .failure(error):
            logger.error("Accès au fichier refusé : \(error.localizedDescription, privacy: .public)")
            phase = .failed(message: String(localized: "PDF Studio n'a pas pu accéder à ce fichier."))
        }
    }

    /// La fenêtre se ferme : ses actions d'annulation n'ont plus d'objet.
    func close() {
        undoManager?.removeAllActions(withTarget: self)
        DocumentOpener.shared.setUnsaved(false, for: self)
    }

    private func show(_ document: PdfDocument) {
        let session = EditSession(document: document)
        let sources = PageSources(document)
        self.session = session
        sourceSizes = [document.pageSizes().map(\.cgSize)]
        pages = session.pages().map { DisplayPage($0, sizes: sourceSizes) }
        declaredTitle = document.title()
        if let declaredTitle {
            title = declaredTitle
        }
        renderer = PageRenderer(sources: sources)
        canvas = CanvasController(pages: pages)
        search = SearchModel(sources: sources, pages: pages, canvas: canvas)
        selection = pages.first.map { [$0.id] } ?? []
        phase = .ready
    }

    /// Charge le plan après l'affichage des pages : un plan illisible n'empêche pas la lecture.
    private func loadOutline(_ document: PdfDocument) async {
        let items = await Task.detached(priority: .utility) { () -> [OutlineItem]? in
            try? document.outline()
        }.value
        guard let items else {
            logger.error("Plan du document illisible")
            return
        }
        outline = OutlineTree(items: items)
    }

    /// Le document est désormais enregistré dans un autre fichier.
    func relocate(to reference: DocumentReference) {
        self.reference = reference
        title = declaredTitle ?? reference.displayName
        onRelocate?(reference)
    }

    private func checkSignature() async {
        guard let session else { return }
        isSigned = await Task.detached(priority: .utility) {
            (try? session.hasSignature()) ?? false
        }.value
    }
}

// MARK: - Navigation

extension DocumentViewModel {
    /// Position actuelle de la page `index` du document ouvert (destination d'un
    /// signet) ; `nil` si elle a été supprimée.
    func position(ofOriginalPage index: Int) -> Int? {
        let source = DisplayPage.Source(document: 0, index: index)
        return pages.firstIndex { $0.source == source }
    }

    func go(to node: OutlineNode) {
        guard let page = node.page, let position = position(ofOriginalPage: page) else { return }
        canvas.go(to: position, top: node.top)
    }
}

// MARK: - Organisation des pages

extension DocumentViewModel {
    /// Pages visées par les commandes : la sélection des vignettes, sinon la page lue.
    var targetPositions: [Int] {
        let selected = positions(of: selection)
        if selected.isEmpty, pages.indices.contains(canvas.currentPage) {
            return [canvas.currentPage]
        }
        return selected
    }

    /// Positions des pages désignées par leur identifiant, dans l'ordre du document.
    func positions(of ids: Set<UInt64>) -> [Int] {
        pages.indices.filter { ids.contains(pages[$0].id) }
    }

    var canEdit: Bool { phase == .ready && session != nil }

    /// Un document garde au moins une page.
    func canDelete(_ positions: [Int]) -> Bool {
        canEdit && !positions.isEmpty && positions.count < pages.count
    }

    func rotate(_ positions: [Int], clockwise: Bool) {
        perform { try $0.rotate(positions: positions.engine, quarterTurns: clockwise ? 1 : -1) }
    }

    func delete(_ positions: [Int]) {
        guard canDelete(positions), let first = positions.min() else { return }
        perform { try $0.delete(positions: positions.engine) }
        // La page qui prend la place de la première page supprimée devient la sélection.
        if let page = pages[safe: min(first, pages.count - 1)] {
            selection = [page.id]
        }
    }

    func duplicate(_ positions: [Int]) {
        var copies: [UInt32] = []
        perform { copies = try $0.duplicate(positions: positions.engine) }
        selection = Set(copies.compactMap { pages[safe: Int($0)]?.id })
    }

    /// Déplace des pages devant la position `before` (glisser-déposer des vignettes).
    func move(_ positions: IndexSet, before: Int) {
        perform { _ = try $0.movePages(positions: Array(positions).engine, before: UInt32(before)) }
    }

    /// Applique une modification et l'inscrit dans l'historique de la fenêtre.
    private func perform(_ edit: (EditSession) throws -> Void) {
        guard let session else { return }
        do {
            try edit(session)
        } catch {
            report(error)
            return
        }
        registerUndo()
        reloadPages()
    }

    private func registerUndo() {
        guard let undoManager, let session else { return }
        undoManager.registerUndo(withTarget: self) { model in
            MainActor.assumeIsolated { model.undoEdit() }
        }
        undoManager.setActionName(session.undoName() ?? "")
    }

    private func undoEdit() {
        guard let session, session.undo() else { return }
        undoManager?.registerUndo(withTarget: self) { model in
            MainActor.assumeIsolated { model.redoEdit() }
        }
        undoManager?.setActionName(session.redoName() ?? "")
        reloadPages()
    }

    private func redoEdit() {
        guard let session, session.redo() else { return }
        registerUndo()
        reloadPages()
    }

    private func reloadPages() {
        guard let session else { return }
        pages = session.pages().map { DisplayPage($0, sizes: sourceSizes) }
        setModified(session.isModified())
        selection.formIntersection(pages.map(\.id))
        canvas.setPages(pages)
        search?.setPages(pages)
    }

    func setModified(_ modified: Bool) {
        isModified = modified
        DocumentOpener.shared.setUnsaved(modified, for: self)
    }

    func report(_ error: Error) {
        let engine = error as? EngineError
        logger.error("Opération impossible : \(engine?.technicalDetail ?? "\(error)", privacy: .public)")
        failureMessage = engine?.userMessage ?? String(localized: "L'opération n'a pas pu être effectuée.")
    }
}

private extension Array where Element == Int {
    /// Positions au format du moteur.
    var engine: [UInt32] { map { UInt32($0) } }
}
