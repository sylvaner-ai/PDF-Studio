import Foundation
import Observation

/// Une occurrence trouvée : page et rang de l'occurrence sur la page.
struct SearchHit: Equatable, Hashable, Sendable {
    let page: Int
    let index: Int
}

/// Recherche plein texte dans le document, page par page et au fil de la frappe.
///
/// Les résultats s'affichent dès qu'une page en contient ; une nouvelle saisie
/// interrompt la recherche en cours. La première occurrence montrée est la
/// première à partir de la page lue.
@MainActor
@Observable
final class SearchModel {
    private(set) var query = ""
    private(set) var hits: [SearchHit] = []
    private(set) var currentHit: Int?
    private(set) var isSearching = false
    /// Incrémenté pour demander le focus du champ de recherche (⌘F).
    private(set) var focusRequest = 0

    @ObservationIgnored private var matchesByPage: [Int: [SearchMatch]] = [:]
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var startPage = 0
    @ObservationIgnored private let document: PdfDocument
    @ObservationIgnored private let canvas: CanvasController

    init(document: PdfDocument, canvas: CanvasController) {
        self.document = document
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

    func requestFocus() {
        focusRequest += 1
    }

    func update(query: String) {
        guard query != self.query else { return }
        self.query = query
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
        let (document, pageCount) = (document, canvas.pageCount)
        task = Task { [weak self] in
            // Laisse finir la frappe avant de lancer la recherche.
            try? await Task.sleep(for: .milliseconds(200))
            for page in 0..<pageCount {
                guard !Task.isCancelled else { return }
                let found = await Task.detached(priority: .userInitiated) {
                    (try? document.searchPage(index: UInt32(page), query: needle)) ?? []
                }.value
                guard let self, !Task.isCancelled else { return }
                if !found.isEmpty {
                    self.append(found, page: page)
                }
            }
            guard let self, !Task.isCancelled else { return }
            self.isSearching = false
            if self.currentHit == nil, !self.hits.isEmpty {
                self.select(0)
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
        if currentHit == nil, page >= startPage {
            select(first)
        } else {
            publish(reveal: false)
        }
    }

    private func select(_ index: Int) {
        currentHit = index
        publish(reveal: true)
    }

    private func publish(reveal: Bool) {
        canvas.showMatches(matchesByPage, current: currentHit.map { hits[$0] }, reveal: reveal)
    }
}
