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
/// demande toujours confirmation avant de remplacer un fichier (cf. ``FileSaver``),
/// et les modifications non enregistrées sont retenues pour être proposées à la
/// prochaine ouverture (cf. `DocumentViewModel+Recovery.swift`).
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
    /// Modifiée par l'ouverture et par la restauration (cf. `DocumentViewModel+Recovery.swift`).
    var phase: Phase = .opening
    private(set) var title: String
    /// Pages du document, dans l'ordre.
    private(set) var pages: [DisplayPage] = []
    /// Pages sélectionnées dans les vignettes, par identifiant.
    var selection: Set<UInt64> = []
    /// Lecture des pages, grille pour les organiser, ou édition de leur contenu.
    var workspace = Workspace.reading {
        didSet {
            guard workspace != oldValue else { return }
            // Le canevas d'abord : un texte en cours de saisie est validé.
            canvas.setEditing(workspace == .editing ? editing : nil)
            if workspace != .editing {
                editing.endEditing()
            }
        }
    }
    /// Sélection et modification des objets des pages (mode Édition).
    let editing = PageEditing()
    /// Largeur des vignettes de la grille, en points (⌘+ et ⌘- dans la grille).
    var gridThumbnailWidth: CGFloat = 150
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
    /// Exportation en cours, décrite dans la fenêtre ; les pages restent figées.
    var activity: String?
    /// Modifications non enregistrées retrouvées à l'ouverture, en attente d'une réponse.
    var recoveryOffer: RecoveryRecord?

    /// Gestionnaire d'annulation de la fenêtre.
    @ObservationIgnored weak var undoManager: UndoManager?
    /// Appelé quand le document change de fichier (« Enregistrer sous »).
    @ObservationIgnored var onRelocate: ((DocumentReference) -> Void)?
    @ObservationIgnored private(set) var session: EditSession?
    @ObservationIgnored private(set) var sources: PageSources?
    @ObservationIgnored var recovery = RecoveryContext()
    /// Vrai une fois la fenêtre fermée : ce qui se termine ensuite ne modifie plus rien.
    @ObservationIgnored private(set) var isClosed = false
    /// Dernière insertion demandée ; la suivante attend sa fin.
    @ObservationIgnored var insertionTask: Task<Void, Never>?
    @ObservationIgnored private var declaredTitle: String?
    /// Format des pages de chaque document source, en points.
    @ObservationIgnored private var sourceSizes: [[CGSize]] = []

    let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "document")
    /// Identifie la fenêtre du document, pour les pages glissées vers une autre fenêtre.
    let documentID = UUID()
    /// Rangement des modifications non enregistrées ; `nil` s'il n'a pas pu être créé.
    let recoveryStore = try? RecoveryStore.standard()

    init(reference: DocumentReference) {
        self.reference = reference
        self.title = reference.displayName
        OpenDocuments.register(self)
        editing.model = self
    }

    var pageCount: Int { pages.count }

    /// Ouvre le document hors du fil principal.
    func open(password: String? = nil) async {
        phase = .opening
        let reference = reference
        let result = await Task.detached(priority: .userInitiated) {
            Result {
                try reference.withAccess { url -> (PdfDocument, FileStamp?) in
                    // L'empreinte précède la lecture : un fichier modifié entre-temps ne
                    // pourra pas passer pour la version ouverte.
                    let stamp = try? FileStamp(of: url)
                    return (try PdfDocument.open(path: url.path, password: password), stamp)
                }
            }
        }.value
        guard !isClosed else { return }

        switch result {
        case let .success((document, stamp)):
            recovery = RecoveryContext(password: password, stamp: stamp)
            show(document)
            offerRecovery()
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
        endRecovery()
        isClosed = true
        recovery.isActive = false
    }

    func show(_ document: PdfDocument) {
        let session = EditSession(document: document)
        let sources = PageSources(document)
        self.session = session
        self.sources = sources
        sourceSizes = [document.pageSizes().map(\.cgSize)]
        pages = session.pages().map { DisplayPage($0, sizes: sourceSizes) }
        declaredTitle = document.title()
        if let declaredTitle {
            title = declaredTitle
        }
        renderer = PageRenderer(sources: sources)
        canvas = CanvasController(pages: pages)
        editing.reset()
        canvas.setEditing(workspace == .editing ? editing : nil)
        search = SearchModel(sources: sources, pages: pages, canvas: canvas)
        selection = pages.first.map { [$0.id] } ?? []
        phase = .ready
    }

    /// Charge le plan après l'affichage des pages : un plan illisible n'empêche pas la lecture.
    func loadOutline(_ document: PdfDocument) async {
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

    func checkSignature() async {
        guard let session else { return }
        isSigned = await Task.detached(priority: .utility) {
            (try? session.hasSignature()) ?? false
        }.value
    }

    /// Ajoute un document dont des pages pourront être insérées ; renvoie son numéro.
    func addSource(_ document: PdfDocument) -> UInt32? {
        guard let session, let sources else { return nil }
        sources.append(document)
        sourceSizes.append(document.pageSizes().map(\.cgSize))
        return session.addSource(document: document)
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
    /// Pages visées par les commandes. Dans la grille : la sélection. En lecture :
    /// plusieurs vignettes sélectionnées, sinon la page lue (la sélection ne la
    /// suit pas quand la barre latérale montre les signets).
    var targetPositions: [Int] {
        let selected = positions(of: selection)
        if workspace == .organizing || selected.count > 1 {
            return selected
        }
        return pages.indices.contains(canvas.currentPage) ? [canvas.currentPage] : selected
    }

    /// Positions des pages désignées par leur identifiant, dans l'ordre du document.
    func positions(of ids: Set<UInt64>) -> [Int] {
        pages.indices.filter { ids.contains(pages[$0].id) }
    }

    /// Les pages restent figées tant que des modifications retrouvées attendent une
    /// réponse, pendant une exportation, et une fois la fenêtre fermée.
    var canEdit: Bool {
        phase == .ready && session != nil && recoveryOffer == nil && activity == nil && !isClosed
    }

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

    /// Applique une modification et l'inscrit dans l'historique de la fenêtre ;
    /// renvoie `false` si elle a échoué.
    @discardableResult
    func perform(_ edit: (EditSession) throws -> Void) -> Bool {
        guard canEdit, let session else { return false }
        let revision = session.revision()
        do {
            try edit(session)
        } catch {
            report(error)
            return false
        }
        // Une opération sans effet (page déposée à sa place) n'est pas à annuler.
        if session.revision() != revision {
            registerUndo()
        }
        reloadPages()
        return true
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
        // Les rendus doivent trouver les modifications avant que les pages ne changent.
        var edits: [UInt64: ContentEdits] = [:]
        for page in pages where page.editRevision != 0 {
            edits[page.id] = session.contentEdits(page: page.id)
        }
        sources?.setEdits(edits)
        editing.pagesDidChange(pages)
        setModified(session.isModified())
        selection.formIntersection(pages.map(\.id))
        canvas.setPages(pages)
        search?.setPages(pages)
        updateRecovery()
    }

    func setModified(_ modified: Bool) {
        guard !isClosed else { return }
        isModified = modified
        DocumentOpener.shared.setUnsaved(modified, recoverable: isRecoverable, for: self)
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
