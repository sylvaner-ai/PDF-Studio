import AppKit

/// Relie la grille AppKit au document : contenu, sélection, glisser-déposer et
/// actions sur les pages.
///
/// Les pages glissées se déplacent dans la grille, ou se copient dans la grille
/// d'une autre fenêtre ; un PDF déposé depuis le Finder y insère ses pages.
@MainActor
final class PageGridCoordinator: NSObject, NSCollectionViewDelegate {
    /// Type des pages glissées : « document page », identifiants du document
    /// (``DocumentViewModel/documentID``) et de la page.
    static let pageType = NSPasteboard.PasteboardType("fr.sylvainrobert.pdfstudio.page")

    private weak var model: DocumentViewModel?
    private let renderer: PageRenderer
    private let collectionView = PageGridCollectionView()
    private let layout = NSCollectionViewFlowLayout()
    private var dataSource: NSCollectionViewDiffableDataSource<Int, UInt64>?
    private var pages: [DisplayPage] = []
    private var positions: [UInt64: Int] = [:]
    private var display = DisplayRotation()
    private var itemSize = NSSize.zero
    /// Positions des pages en cours de glissement.
    private var dragged: IndexSet?

    init(model: DocumentViewModel, renderer: PageRenderer) {
        self.model = model
        self.renderer = renderer
    }

    func makeScrollView() -> NSScrollView {
        layout.sectionInset = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
        layout.minimumInteritemSpacing = 12
        layout.minimumLineSpacing = 16
        collectionView.collectionViewLayout = layout
        collectionView.isSelectable = true
        collectionView.allowsMultipleSelection = true
        collectionView.allowsEmptySelection = true
        collectionView.backgroundColors = [.underPageBackgroundColor]
        collectionView.register(PageGridItem.self, forItemWithIdentifier: PageGridItem.identifier)
        collectionView.registerForDraggedTypes([Self.pageType, .fileURL])
        collectionView.setDraggingSourceOperationMask([.move, .copy], forLocal: true)
        collectionView.delegate = self
        collectionView.coordinator = self
        collectionView.setAccessibilityLabel(String(localized: "Pages du document"))
        dataSource = NSCollectionViewDiffableDataSource(collectionView: collectionView) { [weak self] view, path, id in
            MainActor.assumeIsolated {
                let item = view.makeItem(withIdentifier: PageGridItem.identifier, for: path)
                if let item = item as? PageGridItem {
                    self?.configure(item, id: id)
                }
                return item
            }
        }

        let scrollView = NSScrollView()
        scrollView.documentView = collectionView
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.backgroundColor = .underPageBackgroundColor
        return scrollView
    }

    /// Pages, sélection, rotation d'affichage et taille des vignettes à montrer.
    func update(pages: [DisplayPage], selection: Set<UInt64>, display: DisplayRotation, thumbnailWidth: CGFloat) {
        let itemSize = PageGridItem.itemSize(thumbnailWidth: thumbnailWidth)
        if pages != self.pages || display != self.display || itemSize != self.itemSize {
            let animated = !self.pages.isEmpty && itemSize == self.itemSize
            self.pages = pages
            self.display = display
            positions = Dictionary(uniqueKeysWithValues: pages.enumerated().map { ($1.id, $0) })
            if itemSize != self.itemSize {
                self.itemSize = itemSize
                layout.itemSize = itemSize
            }
            var snapshot = NSDiffableDataSourceSnapshot<Int, UInt64>()
            snapshot.appendSections([0])
            snapshot.appendItems(pages.map(\.id))
            dataSource?.apply(snapshot, animatingDifferences: animated)
            // Les cases déjà affichées gardent leur page : numéro, rotation et taille suivent.
            for case let item as PageGridItem in collectionView.visibleItems() {
                if let path = collectionView.indexPath(for: item), let page = pages[safe: path.item] {
                    configure(item, id: page.id)
                }
            }
        }
        let wanted = Set(selection.compactMap { positions[$0].map { IndexPath(item: $0, section: 0) } })
        if wanted != collectionView.selectionIndexPaths {
            collectionView.selectionIndexPaths = wanted
            if let first = wanted.min() {
                collectionView.scrollToItems(at: [first], scrollPosition: .nearestHorizontalEdge)
            }
        }
    }

    private func configure(_ item: PageGridItem, id: UInt64) {
        guard let position = positions[id], let page = pages[safe: position] else { return }
        item.configure(position: position, page: page, display: display, itemSize: itemSize, renderer: renderer)
    }

    // MARK: Sélection

    private var selectedPositions: [Int] {
        collectionView.selectionIndexPaths.map(\.item).sorted()
    }

    /// La sélection a changé dans la grille : le document la retient.
    func selectionDidChange() {
        guard let model else { return }
        let ids = Set(selectedPositions.compactMap { pages[safe: $0]?.id })
        if ids != model.selection {
            model.selection = ids
        }
    }

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        selectionDidChange()
    }

    func collectionView(_ collectionView: NSCollectionView, didDeselectItemsAt indexPaths: Set<IndexPath>) {
        selectionDidChange()
    }

    // MARK: Actions

    func read(position: Int) {
        model?.read(page: position)
    }

    func readSelection() {
        if let first = selectedPositions.first {
            read(position: first)
        }
    }

    func deleteSelection() {
        guard let model else { return }
        let positions = selectedPositions
        if model.canDelete(positions) {
            model.delete(positions)
        } else {
            NSSound.beep()
        }
    }

    func contextMenu() -> NSMenu? {
        guard let model, model.canEdit, !selectedPositions.isEmpty else { return nil }
        let menu = NSMenu()
        menu.autoenablesItems = false
        addItem(String(localized: "Faire pivoter à gauche"), #selector(rotateLeft), to: menu)
        addItem(String(localized: "Faire pivoter à droite"), #selector(rotateRight), to: menu)
        menu.addItem(.separator())
        addItem(String(localized: "Dupliquer"), #selector(duplicatePages), to: menu)
        addItem(String(localized: "Supprimer"), #selector(deletePages), to: menu).isEnabled =
            model.canDelete(selectedPositions)
        menu.addItem(.separator())
        addItem(String(localized: "Insérer une page vierge"), #selector(insertBlankPage), to: menu)
        addItem(String(localized: "Insérer des pages d'un fichier…"), #selector(insertDocument), to: menu)
        menu.addItem(.separator())
        addItem(String(localized: "Extraire les pages…"), #selector(extractPages), to: menu)
        return menu
    }

    @discardableResult
    private func addItem(_ title: String, _ action: Selector, to menu: NSMenu) -> NSMenuItem {
        let item = menu.addItem(withTitle: title, action: action, keyEquivalent: "")
        item.target = self
        return item
    }

    @objc private func rotateLeft() {
        model?.rotate(selectedPositions, clockwise: false)
    }

    @objc private func rotateRight() {
        model?.rotate(selectedPositions, clockwise: true)
    }

    @objc private func duplicatePages() {
        model?.duplicate(selectedPositions)
    }

    @objc private func deletePages() {
        deleteSelection()
    }

    @objc private func insertBlankPage() {
        model?.insertBlankPage(before: selectedPositions.last.map { $0 + 1 })
    }

    @objc private func insertDocument() {
        model?.chooseDocumentToInsert(before: selectedPositions.last.map { $0 + 1 })
    }

    @objc private func extractPages() {
        model?.extractPages(selectedPositions)
    }
}

// MARK: - Glisser-déposer

extension PageGridCoordinator {
    func collectionView(
        _ collectionView: NSCollectionView, canDragItemsAt indexPaths: Set<IndexPath>, with event: NSEvent
    ) -> Bool {
        model?.canEdit == true
    }

    func collectionView(
        _ collectionView: NSCollectionView, pasteboardWriterForItemAt indexPath: IndexPath
    ) -> NSPasteboardWriting? {
        guard let model, let page = pages[safe: indexPath.item] else { return nil }
        let item = NSPasteboardItem()
        item.setString("\(model.documentID.uuidString) \(page.id)", forType: Self.pageType)
        return item
    }

    func collectionView(
        _ collectionView: NSCollectionView, draggingSession session: NSDraggingSession,
        willBeginAt screenPoint: NSPoint, forItemsAt indexPaths: Set<IndexPath>
    ) {
        dragged = IndexSet(indexPaths.map(\.item))
    }

    func collectionView(
        _ collectionView: NSCollectionView, draggingSession session: NSDraggingSession,
        endedAt screenPoint: NSPoint, dragOperation operation: NSDragOperation
    ) {
        dragged = nil
    }

    func collectionView(
        _ collectionView: NSCollectionView, validateDrop draggingInfo: NSDraggingInfo,
        proposedIndexPath proposedDropIndexPath: AutoreleasingUnsafeMutablePointer<NSIndexPath>,
        dropOperation proposedDropOperation: UnsafeMutablePointer<NSCollectionView.DropOperation>
    ) -> NSDragOperation {
        guard model?.canEdit == true else { return [] }
        let operation: NSDragOperation
        if dragged != nil, draggingInfo.draggingSource as? NSCollectionView === collectionView {
            operation = .move
        } else if !draggedPages(draggingInfo).isEmpty || !droppedDocuments(draggingInfo).isEmpty {
            operation = .copy
        } else {
            return []
        }
        // On dépose entre deux pages.
        if proposedDropOperation.pointee == .on {
            proposedDropOperation.pointee = .before
        }
        return operation
    }

    func collectionView(
        _ collectionView: NSCollectionView, acceptDrop draggingInfo: NSDraggingInfo,
        indexPath: IndexPath, dropOperation: NSCollectionView.DropOperation
    ) -> Bool {
        guard let model else { return false }
        let position = indexPath.item
        if let dragged, draggingInfo.draggingSource as? NSCollectionView === collectionView {
            model.move(dragged, before: position)
            return true
        }
        let pages = draggedPages(draggingInfo)
        if let origin = pages.first.flatMap({ OpenDocuments.model($0.document) }), origin !== model {
            Task { await model.insertPages(pages.map(\.page), from: origin, before: position) }
            return true
        }
        let documents = droppedDocuments(draggingInfo)
        guard !documents.isEmpty else { return false }
        Task { await model.insertDocuments(documents, before: position) }
        return true
    }

    /// Pages glissées depuis la grille d'une fenêtre (document, page).
    private func draggedPages(_ info: NSDraggingInfo) -> [(document: UUID, page: UInt64)] {
        (info.draggingPasteboard.pasteboardItems ?? []).compactMap { item in
            let parts = item.string(forType: Self.pageType)?.split(separator: " ") ?? []
            guard parts.count == 2, let document = UUID(uuidString: String(parts[0])),
                  let page = UInt64(parts[1]) else { return nil }
            return (document, page)
        }
    }

    /// Fichiers PDF déposés depuis le Finder.
    private func droppedDocuments(_ info: NSDraggingInfo) -> [URL] {
        let urls = info.draggingPasteboard.readObjects(
            forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]
        ) as? [URL] ?? []
        return urls.filter(DocumentOpener.isPDF)
    }
}
