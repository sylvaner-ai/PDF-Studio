import Foundation
import Observation

/// Une occurrence trouvée : position de la page et rang de l'occurrence sur la page.
struct SearchHit: Equatable, Hashable, Sendable {
    let page: Int
    let index: Int
}

/// Recherche plein texte dans le document, page par page et au fil de la frappe.
///
/// Les résultats s'affichent dès qu'une page en contient ; une nouvelle saisie
/// interrompt la recherche en cours, de même qu'une réorganisation des pages,
/// après laquelle la recherche reprend sans déplacer la lecture. La première
/// occurrence montrée est la première à partir de la page lue.
@MainActor
@Observable
final class SearchModel {
    private(set) var query = ""
    private(set) var hits: [SearchHit] = []
    private(set) var currentHit: Int?
    private(set) var isSearching = false
    /// Incrémenté pour demander le focus du champ de recherche (⌘F).
    private(set) var focusRequest = 0
    /// Le panneau de recherche est affiché (⌘F, menu Édition).
    private(set) var isPresented = false

    @ObservationIgnored private var matchesByPage: [Int: [SearchMatch]] = [:]
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var startPage = 0
    /// Faux quand la recherche reprend après une réorganisation : les résultats
    /// s'affichent sans faire défiler les pages.
    @ObservationIgnored private var revealsFirstHit = true
    /// Occurrence montrée avant une réorganisation (identifiant de la page, rang).
    @ObservationIgnored private var resumedHit: (page: UInt64, index: Int)?
    @ObservationIgnored private let sources: PageSources
    @ObservationIgnored private var pages: [DisplayPage]
    @ObservationIgnored private let canvas: CanvasController

    init(sources: PageSources, pages: [DisplayPage], canvas: CanvasController) {
        self.sources = sources
        self.pages = pages
        self.canvas = canvas
    }

    var hasHits: Bool { !hits.isEmpty }

    /// Texte d'état : « 3 sur 12 », « Aucun résultat » ou rien.
    var status: String? {
        guard !query.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
        if let currentHit {
            return String(localized: "\(currentHit + 1) sur \(hits.count)")
        }
        return isSearching ? nil : String(localized: "Aucun résultat")
    }

    /// Affiche le panneau de recherche et donne le focus à son champ.
    func requestFocus() {
        isPresented = true
        focusRequest += 1
    }

    /// Ferme le panneau : les occurrences ne sont plus surlignées et le
    /// clavier revient aux pages.
    func dismiss() {
        guard isPresented else { return }
        isPresented = false
        update(query: "")
        canvas.focusPages()
    }

    /// Pages réorganisées : les positions changent, la recherche reprend.
    func setPages(_ pages: [DisplayPage]) {
        guard pages != self.pages else { return }
        resumedHit = currentHit.flatMap { hit in
            self.pages[safe: hits[hit].page].map { (page: $0.id, index: hits[hit].index) }
        }
        self.pages = pages
        let query = self.query
        self.query = ""
        update(query: query, reveal: false)
    }

    func update(query: String) {
        update(query: query, reveal: true)
    }

    private func update(query: String, reveal: Bool) {
        guard query != self.query else { return }
        self.query = query
        revealsFirstHit = reveal
        if reveal {
            resumedHit = nil
        }
        task?.cancel()
        hits = []
        currentHit = nil
        matchesByPage = [:]
        publish(reveal: false)

        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else {
            isSearching = false
            return
        }
        isSearching = true
        startPage = canvas.currentPage
        let (sources, pages) = (sources, pages)
        task = Task { [weak self] in
            // Laisse finir la frappe avant de lancer la recherche.
            try? await Task.sleep(for: .milliseconds(200))
            for (page, displayed) in pages.enumerated() {
                guard !Task.isCancelled else { return }
                guard let source = displayed.source, let document = sources.document(source.document) else {
                    continue
                }
                let found = await Task.detached(priority: .userInitiated) {
                    (try? document.searchPage(index: UInt32(source.index), query: needle)) ?? []
                }.value
                guard let self, !Task.isCancelled else { return }
                if !found.isEmpty {
                    self.append(found, page: page)
                }
            }
            guard let self, !Task.isCancelled else { return }
            self.isSearching = false
            if self.currentHit == nil, !self.hits.isEmpty {
                self.select(0, reveal: self.revealsFirstHit)
            }
        }
    }

    func next() {
        guard !hits.isEmpty else { return }
        select(((currentHit ?? -1) + 1) % hits.count)
    }

    func previous() {
        guard !hits.isEmpty else { return }
        select(((currentHit ?? 0) - 1 + hits.count) % hits.count)
    }

    private func append(_ matches: [SearchMatch], page: Int) {
        matchesByPage[page] = matches
        let first = hits.count
        hits += matches.indices.map { SearchHit(page: page, index: $0) }
        if currentHit == nil, let resumed = resumedHit {
            // Après une réorganisation : l'occurrence qui était montrée, là où elle est.
            if pages[safe: page]?.id == resumed.page, matches.indices.contains(resumed.index) {
                currentHit = first + resumed.index
                resumedHit = nil
            }
            publish(reveal: false)
        } else if currentHit == nil, page >= startPage {
            select(first, reveal: revealsFirstHit)
        } else {
            publish(reveal: false)
        }
    }

    private func select(_ index: Int, reveal: Bool = true) {
        currentHit = index
        publish(reveal: reveal)
    }

    private func publish(reveal: Bool) {
        canvas.showMatches(matchesByPage, current: currentHit.map { hits[$0] }, reveal: reveal)
    }
}
