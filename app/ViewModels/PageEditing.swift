import CoreGraphics
import Foundation
import Observation

/// Sélection et modification des objets des pages (mode Édition).
///
/// Les objets d'une page (blocs de texte, images, tracés) viennent du moteur,
/// modifications comprises ; ils sont gardés pour chaque page tant que son
/// contenu ne change pas. La sélection porte sur une seule page à la fois.
/// Chaque modification passe par ``DocumentViewModel/perform(_:)`` : elle
/// s'annule par ⌘Z comme l'organisation des pages.
@MainActor
@Observable
final class PageEditing {
    /// Page des objets sélectionnés.
    private(set) var page: UInt64?
    /// Objets sélectionnés, par numéro.
    private(set) var selected: Set<UInt32> = []
    /// Bloc dont le texte est en cours de saisie.
    private(set) var textTarget: TextTarget?
    /// Remarque sur la dernière modification du texte (police de
    /// substitution, débordement, caractères absents).
    private(set) var notice: String?
    /// Change quand les objets connus d'une page changent, pour l'inspecteur.
    private(set) var objectsVersion = 0

    struct TextTarget: Equatable {
        let page: UInt64
        let object: UInt32
    }

    @ObservationIgnored weak var model: DocumentViewModel?
    /// Prévient le canevas que les objets ou la sélection d'une page ont changé.
    @ObservationIgnored var onChange: (@MainActor (UInt64) -> Void)?
    @ObservationIgnored private var objects: [UInt64: (revision: UInt64, items: [PageObjectItem])] = [:]
    @ObservationIgnored private var loading: [UInt64: UInt64] = [:]

    /// Nouveau document affiché : plus rien n'est connu ni sélectionné.
    func reset() {
        objects = [:]
        loading = [:]
        page = nil
        selected = []
        textTarget = nil
        notice = nil
    }

    /// Fin du mode Édition : la sélection et la saisie en cours sont abandonnées.
    func endEditing() {
        let previous = page
        page = nil
        selected = []
        textTarget = nil
        notice = nil
        if let previous {
            onChange?(previous)
        }
    }

    /// Les pages ont changé : la sélection ne survit pas à la disparition de
    /// sa page ni à celle de ses objets (suppression, annulation).
    func pagesDidChange(_ pages: [DisplayPage]) {
        let ids = Set(pages.map(\.id))
        objects = objects.filter { ids.contains($0.key) }
        if let page, !ids.contains(page) {
            self.page = nil
            selected = []
            textTarget = nil
        }
    }

    // MARK: - Objets des pages

    /// Objets connus de la page ; ceux d'un état précédent de la page tant que
    /// les nouveaux se chargent, `nil` si rien n'est encore connu.
    func objects(on page: DisplayPage) -> [PageObjectItem]? {
        guard !page.isBlank else { return [] }
        let known = objects[page.id]
        if known?.revision != page.editRevision {
            load(page)
        }
        return known?.items
    }

    /// Objets sélectionnés, tels que connus.
    func selectedObjects(on page: DisplayPage) -> [PageObjectItem] {
        guard self.page == page.id, let items = objects[page.id]?.items else { return [] }
        return items.filter { selected.contains($0.id) }
    }

    /// Objets sélectionnés de la page sélectionnée, pour l'inspecteur.
    var selectedItems: [PageObjectItem] {
        _ = objectsVersion
        guard let page, let items = objects[page]?.items else { return [] }
        return items.filter { selected.contains($0.id) }
    }

    private func load(_ page: DisplayPage) {
        guard loading[page.id] != page.editRevision, let session = model?.session else { return }
        loading[page.id] = page.editRevision
        let (id, revision) = (page.id, page.editRevision)
        Task { [weak self] in
            let items = await Task.detached(priority: .userInitiated) {
                try? session.pageObjects(page: id)
            }.value
            guard let self, self.loading[id] == revision else { return }
            self.loading[id] = nil
            guard let items else { return }
            self.objects[id] = (revision, items)
            // Des objets sélectionnés ont pu disparaître (annulation d'une copie…).
            if self.page == id {
                self.selected.formIntersection(items.map(\.id))
            }
            self.objectsVersion += 1
            self.onChange?(id)
        }
    }

    // MARK: - Sélection

    /// Sélectionne des objets d'une page ; `extend` ajoute ou retire ceux-ci
    /// de la sélection (⇧-clic).
    func select(_ ids: Set<UInt32>, on page: UInt64, extend: Bool = false) {
        let previous = self.page
        if extend, self.page == page {
            selected.formSymmetricDifference(ids)
        } else {
            self.page = page
            selected = ids
        }
        if selected.isEmpty {
            self.page = nil
        }
        notice = nil
        if let previous, previous != page {
            onChange?(previous)
        }
        onChange?(page)
    }

    func clearSelection() {
        guard let page else { return }
        select([], on: page)
    }

    var hasSelection: Bool { page != nil && !selected.isEmpty }

    // MARK: - Modifications

    /// Objets sélectionnés qui peuvent être déplacés et supprimés.
    private var movableSelection: [UInt32] {
        selectedItems.filter(\.movable).map(\.id).sorted()
    }

    /// Supprime les objets sélectionnés (sauf ceux qui sont verrouillés).
    func deleteSelection() {
        guard let page, let model else { return }
        let ids = movableSelection
        guard !ids.isEmpty else { return }
        if model.perform({ try $0.deleteObjects(page: page, objects: ids) }) {
            selected = []
            self.page = nil
            onChange?(page)
        }
    }

    /// Déplace les objets sélectionnés, en points sur la page droite (sans les
    /// quarts de tour de l'organisation), vers la droite et vers le bas.
    func moveSelection(by offset: CGVector) {
        guard let page, let model, offset.dx != 0 || offset.dy != 0 else { return }
        let ids = movableSelection
        guard !ids.isEmpty else { return }
        model.perform { try $0.moveObjects(page: page, objects: ids, dx: Float(offset.dx), dy: Float(offset.dy)) }
    }

    /// Ouvre la saisie du texte d'un bloc.
    func beginTextEditing(page: UInt64, object: UInt32) {
        select([object], on: page)
        textTarget = TextTarget(page: page, object: object)
        onChange?(page)
    }

    /// Termine la saisie ; `text` remplace le texte du bloc s'il a changé.
    func endTextEditing(text: String?) {
        guard let target = textTarget else { return }
        textTarget = nil
        defer { onChange?(target.page) }
        let original = objects[target.page]?.items.first { $0.id == target.object }?.text
        guard let text, text != original, let model else { return }
        var result: PageObjectItem?
        let done = model.perform { result = try $0.replaceText(page: target.page, object: target.object, text: text) }
        guard done, let result else { return }
        notice = Self.notice(for: result)
    }

    /// Ce qu'il faut savoir du texte modifié.
    private static func notice(for item: PageObjectItem) -> String? {
        var lines: [String] = []
        if item.substituted, let font = item.displayFontName {
            lines.append(String(localized: "Police de substitution : \(font)."))
        }
        if item.overflow {
            lines.append(String(localized: "Le texte dépasse du bloc d'origine."))
        }
        if !item.missing.isEmpty {
            lines.append(String(localized: "Caractères absents de la police, omis : \(item.missing)"))
        }
        return lines.isEmpty ? nil : lines.joined(separator: " ")
    }
}

extension PageObjectItem {
    /// Nom de la police, sans le préfixe des polices réduites aux caractères
    /// utilisés (« ABCDEF+Nom »).
    var displayFontName: String? {
        guard let name = fontName else { return nil }
        if let plus = name.firstIndex(of: "+"), name.distance(from: name.startIndex, to: plus) == 6 {
            return String(name[name.index(after: plus)...])
        }
        return name
    }
}
