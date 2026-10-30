import AppKit

// MARK: - Redimensionner, copier, coller, dupliquer, déplacer

/// Objets des pages manipulés en mode Édition (D71, D72) : un objet se
/// redimensionne par ses coins (texte, image ou tracé d'origine, texte et
/// image ajoutés) ; la sélection se coupe, se copie, se colle et se
/// duplique, sur la même page, sur une autre page ou dans un autre document,
/// et se fait glisser vers une autre page (⌥ : une copie). Chaque action
/// s'annule par ⌘Z en une seule fois.
///
/// La copie reste dans l'application, entière (les objets d'origine en un
/// extrait PDF, les objets ajoutés tels quels), tant que le presse-papiers
/// ne change pas ; il reçoit en plus un PDF et le texte des objets, pour les
/// autres applications. Un PDF ou une image copiés d'une autre application
/// se collent au centre de la page.
extension PageEditing {
    /// Objets copiés dans l'application, reconnus tant que le presse-papiers
    /// n'a pas changé depuis.
    struct CopiedObjects {
        let clipboard: ObjectClipboard
        let changeCount: Int
        /// Collages déjà faits, par page : chacun se décale un peu plus.
        var pastes: [Spot: Int] = [:]
    }

    /// Page d'un document, où des copies ont été collées.
    struct Spot: Hashable {
        let document: ObjectIdentifier
        let page: UInt64
    }

    /// Décalage d'une copie collée sur la page de l'original, ou d'un
    /// collage répété, en points à l'écran.
    static let pasteOffset: Float = 12

    /// Copie en cours, commune aux fenêtres.
    static var copied: CopiedObjects?

    /// Presse-papiers des copies : celui du Mac (un autre pour les tests).
    static var pasteboard = NSPasteboard.general

    // MARK: Redimensionnement

    /// Objet sélectionné seul qui se redimensionne par ses coins.
    var selectedResizable: PageObjectItem? {
        let items = selectedItems
        guard items.count == 1, let item = items.first, item.resizable, item.movable else { return nil }
        return item
    }

    /// Place l'objet `object` dans le cadre `frame` de la page droite ; un
    /// texte ajouté change de corps, un objet d'origine s'agrandit ou se
    /// réduit en entier.
    func resizeObject(_ object: UInt32, on page: UInt64, to frame: CGRect) {
        guard let model, frame.width >= 1, frame.height >= 1 else { return }
        let bounds = PageRect(
            x: Float(frame.minX), y: Float(frame.minY), width: Float(frame.width), height: Float(frame.height)
        )
        var result: PageObjectItem?
        if model.perform({ result = try $0.setObjectBounds(page: page, object: object, bounds: bounds) }),
           let result {
            notice = Self.notice(for: result)
        }
    }

    // MARK: Presse-papiers

    /// Objets sélectionnés qui peuvent être copiés (pas ceux d'un scan).
    private var copyableSelection: [UInt32] {
        selectedItems.filter { $0.movable && !$0.scanned }.map(\.id).sorted()
    }

    var canCopy: Bool { page != nil && !copyableSelection.isEmpty }

    /// Copie les objets sélectionnés.
    func copySelection() {
        guard let page, let session = model?.session else { return }
        do {
            remember(try session.copyObjects(page: page, objects: copyableSelection), from: page)
        } catch {
            model?.report(error)
        }
    }

    /// Coupe les objets sélectionnés : ils sont copiés, puis supprimés.
    func cutSelection() {
        guard let page, let model else { return }
        let ids = copyableSelection
        var clipboard: ObjectClipboard?
        guard !ids.isEmpty, model.perform({ clipboard = try $0.cutObjects(page: page, objects: ids) }),
              let clipboard else { return }
        remember(clipboard, from: page)
        // Recoller sur la page la remet à sa place.
        Self.copied?.pastes[Spot(document: ObjectIdentifier(model), page: page)] = 0
        clearSelection()
    }

    /// Range la copie dans l'application et dans le presse-papiers.
    private func remember(_ clipboard: ObjectClipboard, from page: UInt64) {
        guard let model else { return }
        let pasteboard = Self.pasteboard
        pasteboard.clearContents()
        if !clipboard.pdf.isEmpty {
            pasteboard.setData(clipboard.pdf, forType: .pdf)
        }
        if !clipboard.text.isEmpty {
            pasteboard.setString(clipboard.text, forType: .string)
        }
        Self.copied = CopiedObjects(
            clipboard: clipboard, changeCount: pasteboard.changeCount,
            // Une copie collée sur la page de l'original s'en décale.
            pastes: [Spot(document: ObjectIdentifier(model), page: page): 1]
        )
    }

    /// Copie faite dans l'application, si le presse-papiers la porte encore.
    private var ownCopy: CopiedObjects? {
        guard let copied = Self.copied, copied.changeCount == Self.pasteboard.changeCount else { return nil }
        return copied
    }

    var canPaste: Bool {
        guard model?.canEdit == true else { return false }
        if ownCopy != nil {
            return true
        }
        let pasteboard = Self.pasteboard
        return pasteboard.availableType(from: [.pdf, .png, .tiff, NSPasteboard.PasteboardType("public.jpeg")]) != nil
            || pasteboard.canReadObject(
                forClasses: [NSURL.self],
                options: [.urlReadingFileURLsOnly: true, .urlReadingContentsConformToTypes: ["public.image"]]
            )
    }

    /// Page où coller : celle de la sélection, sinon la dernière cliquée,
    /// sinon la page lue.
    var pasteTarget: DisplayPage? {
        guard let model else { return nil }
        let pages = model.pages
        for id in [page, clickedPage].compactMap({ $0 }) {
            if let found = pages.first(where: { $0.id == id }) {
                return found
            }
        }
        return pages[safe: model.canvas.currentPage]
    }

    /// Colle la copie sur la page `target` (par défaut ``pasteTarget``) :
    /// à la place des originaux, décalée sur leur page, ou centrée sur le
    /// point `center` de la page droite (par défaut le centre de la page,
    /// pour une copie venue d'une autre application).
    func paste(on target: DisplayPage? = nil, at center: CGPoint? = nil) {
        guard let model, let target = target ?? pasteTarget else { return }
        if var copied = ownCopy {
            let spot = Spot(document: ObjectIdentifier(model), page: target.id)
            let count = copied.pastes[spot, default: 0]
            copied.pastes[spot] = count + 1
            Self.copied = copied
            let shift = Self.pasteOffset * Float(count)
            let placement = center.map { PastePlacement.centered(x: Float($0.x), y: Float($0.y)) }
                ?? .inPlace(dx: shift, dy: shift)
            let clipboard = copied.clipboard
            place(on: target.id) { session, source in
                try session.pasteObjects(
                    page: target.id, clipboard: clipboard, placement: placement, blankSource: source
                )
            }
            return
        }
        guard let clipboard = Self.externalClipboard(for: target) else {
            model.failureMessage = String(localized: "Le contenu du presse-papiers ne peut pas être collé.")
            return
        }
        let middle = center ?? CGPoint(x: target.baseSize.width / 2, y: target.baseSize.height / 2)
        place(on: target.id) { session, source in
            try session.pasteObjects(
                page: target.id, clipboard: clipboard,
                placement: .centered(x: Float(middle.x), y: Float(middle.y)), blankSource: source
            )
        }
    }

    /// Copie venue d'une autre application : un PDF (sa première page), une
    /// image, ou un fichier image copié dans le Finder, réduits à la moitié
    /// de la page au plus.
    private static func externalClipboard(for target: DisplayPage) -> ObjectClipboard? {
        let pasteboard = Self.pasteboard
        if let pdf = pasteboard.data(forType: .pdf), let size = try? pdfClipSize(data: pdf) {
            let fitted = fittedSize(CGSize(width: CGFloat(size.width), height: CGFloat(size.height)), on: target)
            return try? clipboardFromFile(data: pdf, width: Float(fitted.width), height: Float(fitted.height))
        }
        var file: AddedImageFile?
        let types = [NSPasteboard.PasteboardType.png, NSPasteboard.PasteboardType("public.jpeg"), .tiff]
        if let type = pasteboard.availableType(from: types), let data = pasteboard.data(forType: type) {
            file = AddedImageFile.load(data)
        } else if let url = (pasteboard.readObjects(
            forClasses: [NSURL.self],
            options: [.urlReadingFileURLsOnly: true, .urlReadingContentsConformToTypes: ["public.image"]]
        ) as? [URL])?.first {
            file = AddedImageFile.load(url)
        }
        guard let file else { return nil }
        let fitted = fittedSize(file.size, on: target)
        return try? clipboardFromFile(data: file.data, width: Float(fitted.width), height: Float(fitted.height))
    }

    // MARK: Duplication et déplacement

    /// Duplique les objets sélectionnés, décalés sur leur page.
    func duplicateSelection() {
        guard let page else { return }
        let ids = copyableSelection
        guard !ids.isEmpty else { return }
        place(on: page) { session, _ in try session.duplicateObjects(page: page, objects: ids) }
    }

    /// Fait passer les objets sélectionnés sur la page `target`, leur groupe
    /// centré sur le point `center` de sa page droite ; `keep` en fait une
    /// copie (⌥), les originaux restant à leur place.
    func transferSelection(to target: UInt64, center: CGPoint, keep: Bool) {
        guard let page else { return }
        let ids = copyableSelection
        guard !ids.isEmpty else { return }
        let from = page
        place(on: target) { session, source in
            try session.transferObjects(
                from: from, objects: ids, to: target, x: Float(center.x), y: Float(center.y), keep: keep,
                blankSource: source
            )
        }
    }

    /// Centre, sur la page droite, du groupe des objets sélectionnés.
    var selectionCenter: CGPoint? {
        let frames = selectedItems.filter { copyableSelection.contains($0.id) }.map(\.bounds.cgRect)
        guard let first = frames.first else { return nil }
        let union = frames.dropFirst().reduce(first) { $0.union($1) }
        return CGPoint(x: union.midX, y: union.midY)
    }

    /// Pose des objets sur la page `page` avec `make`, qui reçoit la session
    /// et, pour une page vierge, le numéro du document qui lui est donné ;
    /// les objets posés sont sélectionnés.
    private func place(
        on page: UInt64, _ make: @escaping @MainActor (EditSession, UInt32?) throws -> [PageObjectItem]
    ) {
        guard let model, let target = model.pages.first(where: { $0.id == page }) else { return }
        let finish: @MainActor (UInt32?) -> Void = { [weak self] source in
            guard let self, let model = self.model else { return }
            var result: [PageObjectItem] = []
            guard model.perform({ result = try make($0, source) }), !result.isEmpty else { return }
            self.select(Set(result.map(\.id)), on: page)
        }
        guard target.isBlank else {
            finish(nil)
            return
        }
        Task {
            guard let source = await model.blankPageSource(for: page) else { return }
            finish(source)
        }
    }
}
