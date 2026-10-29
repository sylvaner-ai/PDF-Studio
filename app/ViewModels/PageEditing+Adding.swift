import AppKit
import UniformTypeIdentifiers

// MARK: - Ajout de textes et d'images

/// Textes et images ajoutés aux pages, en mode Édition (D67). L'outil
/// « Texte » commence un texte là où l'on clique, dans le style choisi
/// (police, corps, couleur) ; une image choisie se pose au centre de la page
/// lue, une image déposée là où elle est lâchée, à sa taille naturelle dans
/// la limite de la moitié de la page. L'objet ajouté est ensuite sélectionné :
/// il se déplace et se supprime comme les autres, un texte se réécrit et
/// change de style, une image se redimensionne par ses coins. Chaque ajout
/// s'annule par ⌘Z.
///
/// Un objet ajouté se lit droit sur la page telle qu'elle est tournée par
/// l'organisation. Une page vierge n'a pas de document où le dessiner : le
/// premier objet qui s'y ajoute lui en donne un, d'une page vierge de son
/// format (cf. ``DocumentViewModel/blankPageSource(for:)``).
extension PageEditing {
    /// Texte en cours de saisie : sa page, et son coin supérieur gauche en
    /// points sur la page droite.
    struct NewText: Equatable {
        let page: UInt64
        let point: CGPoint
    }

    /// Style des textes ajoutés au départ : Helvetica 12 pt, noir.
    nonisolated static let defaultTextStyle = AddedTextStyle(
        font: .standard(name: "Helvetica"), size: 12, red: 0, green: 0, blue: 0
    )

    /// Dernier style choisi, repris par les fenêtres ouvertes ensuite.
    static var lastTextStyle = defaultTextStyle

    /// Plus grande part de la page qu'occupe d'abord une image ajoutée.
    private static let imageShare: CGFloat = 0.5

    // MARK: Texte

    /// Outil « Texte » : le prochain clic sur une page y commence un texte.
    func startPlacingText() {
        clearSelection()
        isPlacingText = true
    }

    func cancelPlacingText() {
        isPlacingText = false
    }

    /// Commence la saisie d'un texte à ajouter au point `point` de la page
    /// droite.
    func beginNewText(page: UInt64, at point: CGPoint) {
        isPlacingText = false
        clearSelection()
        newText = NewText(page: page, point: point)
        onChange?(page)
    }

    /// Termine la saisie du texte à ajouter ; `text` est ajouté s'il n'est
    /// pas vide.
    func endNewText(text: String?) {
        guard let target = newText else { return }
        newText = nil
        onChange?(target.page)
        guard let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        let (style, point) = (textStyle, target.point)
        add(on: target.page) { session, source in
            try session.addText(
                page: target.page, x: Float(point.x), y: Float(point.y), text: text, style: style, blankSource: source
            )
        }
    }

    /// Texte ajouté sélectionné, seul.
    var selectedAddedText: PageObjectItem? {
        let items = selectedItems
        guard items.count == 1, let item = items.first, item.addedText != nil else { return nil }
        return item
    }

    /// Image ajoutée sélectionnée, seule.
    var selectedAddedImage: PageObjectItem? {
        let items = selectedItems
        guard items.count == 1, let item = items.first, item.added, item.kind == .image else { return nil }
        return item
    }

    /// Style montré dans le panneau : celui du texte ajouté sélectionné, ou
    /// celui du prochain texte.
    var shownTextStyle: AddedTextStyle {
        selectedAddedText?.addedText ?? textStyle
    }

    /// Choisit le style des textes ajoutés : le texte en cours de saisie le
    /// prend aussitôt, un texte ajouté sélectionné aussi (⌘Z l'annule).
    func applyTextStyle(_ style: AddedTextStyle) {
        textStyle = style
        Self.lastTextStyle = style
        if let target = newText {
            onChange?(target.page)
        }
        guard let item = selectedAddedText, let page, let model, item.addedText != style else { return }
        var result: PageObjectItem?
        let done = model.perform { result = try $0.setAddedTextStyle(page: page, object: item.id, style: style) }
        if done, let result {
            notice = Self.notice(for: result)
        }
    }

    // MARK: Image

    /// Choisit une image et l'ajoute au centre de la page lue.
    func chooseImage() {
        // Le mode Édition peut encore se préparer : l'ajout attendra le choix.
        guard let model, model.phase == .ready else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = false
        panel.message = String(localized: "L'image choisie sera ajoutée au centre de la page.")
        panel.prompt = String(localized: "Ajouter")
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            Task { @MainActor in self?.addImage(from: url) }
        }
    }

    /// Ajoute l'image du fichier `url` à la page `page` (par défaut la page
    /// lue), centrée sur le point `center` de la page droite (par défaut le
    /// centre de la page).
    func addImage(from url: URL, on page: UInt64? = nil, at center: CGPoint? = nil) {
        guard let model, model.phase == .ready else { return }
        let index = page.flatMap { id in model.pages.firstIndex { $0.id == id } } ?? model.canvas.currentPage
        guard let target = model.pages[safe: index] else { return }
        Task { [weak self] in
            let file = await Task.detached(priority: .userInitiated) { AddedImageFile.load(url) }.value
            guard let self else { return }
            guard let file else {
                model.failureMessage = String(localized: "Cette image ne peut pas être lue.")
                return
            }
            let bounds = Self.imageBounds(file.size, on: target, center: center)
            self.add(on: target.id) { session, source in
                try session.addImage(page: target.id, data: file.data, bounds: bounds, blankSource: source)
            }
        }
    }

    /// Cadre d'une image de taille naturelle `size` (en points, à l'écran)
    /// sur la page droite : réduite à la moitié de la page au plus, centrée
    /// sur `center`, gardée sur la page. Sur une page couchée par
    /// l'organisation, ses côtés sont échangés : elle se montre droite.
    static func imageBounds(_ size: CGSize, on page: DisplayPage, center: CGPoint?) -> PageRect {
        let turned = page.quarterTurns % 2 != 0
        let shown = turned ? CGSize(width: page.baseSize.height, height: page.baseSize.width) : page.baseSize
        let scale = min(
            1, shown.width * imageShare / max(size.width, 1), shown.height * imageShare / max(size.height, 1)
        )
        let fitted = CGSize(width: max(size.width * scale, 1), height: max(size.height * scale, 1))
        let frame = turned ? CGSize(width: fitted.height, height: fitted.width) : fitted
        let middle = center ?? CGPoint(x: page.baseSize.width / 2, y: page.baseSize.height / 2)
        let x = min(max(middle.x - frame.width / 2, 0), max(page.baseSize.width - frame.width, 0))
        let y = min(max(middle.y - frame.height / 2, 0), max(page.baseSize.height - frame.height, 0))
        return PageRect(x: Float(x), y: Float(y), width: Float(frame.width), height: Float(frame.height))
    }

    /// Place l'image ajoutée `object` dans le cadre `frame` de la page droite.
    func resizeImage(_ object: UInt32, on page: UInt64, to frame: CGRect) {
        guard let model, frame.width >= 1, frame.height >= 1 else { return }
        let bounds = PageRect(
            x: Float(frame.minX), y: Float(frame.minY), width: Float(frame.width), height: Float(frame.height)
        )
        model.perform { _ = try $0.setObjectBounds(page: page, object: object, bounds: bounds) }
    }

    // MARK: Ajout

    /// Ajoute un objet à la page `page` avec `make`, qui reçoit la session et,
    /// pour une page vierge, le numéro du document qui lui est donné ; l'objet
    /// ajouté est sélectionné.
    private func add(
        on page: UInt64, _ make: @escaping @MainActor (EditSession, UInt32?) throws -> PageObjectItem
    ) {
        guard let model, let target = model.pages.first(where: { $0.id == page }) else { return }
        guard target.isBlank else {
            finishAdding(on: page) { try make($0, nil) }
            return
        }
        Task { [weak self] in
            guard let source = await model.blankPageSource(for: page) else { return }
            self?.finishAdding(on: page) { try make($0, source) }
        }
    }

    private func finishAdding(on page: UInt64, _ make: (EditSession) throws -> PageObjectItem) {
        guard let model else { return }
        var result: PageObjectItem?
        guard model.perform({ result = try make($0) }), let result else { return }
        select([result.id], on: page)
        notice = Self.notice(for: result)
    }
}
