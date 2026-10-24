import CoreGraphics
import Foundation
import Observation

/// Mode Amélioration (cf. `docs/AMELIORATION.md`) : redressement des pages
/// scannées, aperçu avant/après de la page affichée, traitement des pages
/// choisies et retour aux pages d'origine.
///
/// Le moteur mesure l'orientation et l'inclinaison de chaque page, la
/// redresse si elle doit l'être (garde-fous : rien sous 0,1°, rien au-delà de
/// 15°, la page est alors « à vérifier ») et en fait une image nouvelle ; les
/// pages traitées remplacent les pages d'origine en une seule étape
/// d'annulation, et l'enregistrement écrit la page améliorée à la place de
/// l'originale (signets, liens et annotations gardés). Les pages restent
/// figées pendant le traitement, page après page, hors du fil principal.
@MainActor
@Observable
final class PageEnhancement {
    /// Pages traitées par « Appliquer ».
    enum Range: Hashable {
        /// Toutes les pages du document.
        case all
        /// Les pages sélectionnées dans la barre latérale.
        case selection
        /// La page affichée.
        case current
    }

    /// Avancement du traitement.
    struct Progress: Equatable, Sendable {
        var done: Int
        let total: Int
        /// Vrai une fois l'arrêt demandé : la page en cours se termine.
        var isStopping = false

        var fraction: Double {
            total == 0 ? 0 : Double(done) / Double(total)
        }
    }

    /// Ce dont dépend le résultat d'une page : la page, son contenu et les réglages.
    struct Key: Hashable {
        let page: UInt64
        let source: DisplayPage.Source?
        let quarterTurns: Int
        let editRevision: UInt64
        let settings: EnhancementSettings
    }

    /// Page améliorée et ses images avant et après, dans le sens final.
    private struct Rendered: Sendable {
        let result: EnhancedPage
        let before: PageImage
        let after: PageImage
    }

    /// Aperçu de la page affichée : avant et après, dans le sens final.
    struct Preview {
        let key: Key
        let result: EnhancedPage
        let report: EnhancementReport
        let before: PageImage
        let after: PageImage
    }

    /// Réglages, retenus d'une fenêtre et d'une session à l'autre.
    var settings: EnhancementSettings {
        didSet {
            guard settings != oldValue else { return }
            UserDefaults.standard.set(enhancementSettingsToJson(settings: settings), forKey: Self.settingsKey)
        }
    }
    var range = Range.all
    /// Aperçu de la page affichée, une fois calculé.
    private(set) var preview: Preview?
    /// Vrai pendant le calcul de l'aperçu.
    private(set) var isPreviewing = false
    /// Message si l'aperçu n'a pas pu être calculé.
    private(set) var previewFailure: String?
    /// Traitement en cours ; les pages restent figées jusqu'à sa fin.
    private(set) var progress: Progress?
    /// Pages marquées « à vérifier » par le dernier traitement (inclinaison au-delà
    /// du plafond), par identifiant.
    private(set) var pagesToCheck: [UInt64] = []

    @ObservationIgnored weak var model: DocumentViewModel?
    /// Traitement en cours, à interrompre.
    @ObservationIgnored private(set) var task: Task<Void, Never>?
    /// Un aperçu se calcule.
    @ObservationIgnored private var isComputingPreview = false
    /// Page dont l'aperçu est demandé, à calculer après celui en cours.
    @ObservationIgnored private var wantedPreview: DisplayPage?

    private static let settingsKey = "enhancementSettings"
    /// Largeur de l'aperçu, en pixels.
    static let previewWidth: CGFloat = 1400

    init() {
        let saved = UserDefaults.standard.string(forKey: Self.settingsKey)
        settings = saved.map { enhancementSettingsFromJson(json: $0) } ?? defaultEnhancementSettings()
    }

    /// Revient aux réglages par défaut.
    func resetSettings() {
        settings = defaultEnhancementSettings()
    }

    func key(for page: DisplayPage) -> Key {
        Key(
            page: page.id, source: page.source, quarterTurns: page.quarterTurns,
            editRevision: page.editRevision, settings: settings
        )
    }

    // MARK: - Aperçu

    /// Calcule l'aperçu de `page`, s'il n'est pas déjà celui affiché.
    ///
    /// Un seul aperçu se calcule à la fois : le moteur ne peut pas
    /// interrompre une page commencée, et les demandes faites entre-temps (on
    /// feuillette les pages) ne s'empilent pas : seule la dernière est
    /// calculée, une fois la page en cours terminée.
    func loadPreview(of page: DisplayPage) async {
        wantedPreview = page
        guard !isComputingPreview else { return }
        isComputingPreview = true
        defer {
            isComputingPreview = false
            isPreviewing = false
        }
        while let page = wantedPreview {
            wantedPreview = nil
            let key = self.key(for: page)
            guard preview?.key != key, let session = model?.session else { continue }
            isPreviewing = true
            previewFailure = nil
            let result = await Self.render(page, settings: settings, session: session)
            // Une autre page ou d'autres réglages entre-temps : ce résultat est périmé.
            guard wantedPreview == nil, let current = model?.pages.first(where: { $0.id == page.id }),
                  self.key(for: current) == key else { continue }
            switch result {
            case let .success(rendered):
                preview = Preview(
                    key: key, result: rendered.result, report: rendered.result.report(),
                    before: rendered.before, after: rendered.after
                )
            case let .failure(error):
                preview = nil
                previewFailure = (error as? EngineError)?.userMessage
                    ?? String(localized: "L'aperçu n'a pas pu être calculé.")
            }
        }
    }

    /// Améliore `page` et rend ses images avant et après, hors du fil principal.
    private static func render(
        _ page: DisplayPage, settings: EnhancementSettings, session: EditSession
    ) async -> Result<Rendered, Error> {
        let recognizer: VisionTextRecognizer? = settings.orientation ? VisionTextRecognizer() : nil
        let size = page.baseSize
        let turns = page.quarterTurns
        let width = previewWidth
        return await Task.detached(priority: .userInitiated) {
            Result { () -> Rendered in
                let result = try session.enhancePage(page: page.id, settings: settings, recognizer: recognizer)
                let report = result.report()
                // Les deux images dans le sens final, pour être superposées.
                let final = report.quarterTurns != 0 ? Int(report.quarterTurns) : turns
                let rotation = DisplayRotation(degrees: 90 * final)
                let scale = Float(width / max(rotation.apply(to: size).width, 1))
                let degrees = Int32(rotation.degrees)
                let before = try result.renderOriginal(scale: scale, rotation: degrees).makeCGImage()
                let after = try result.render(scale: scale, rotation: degrees).makeCGImage()
                guard let before, let after else {
                    throw EngineError.Failed(
                        message: String(localized: "L'aperçu n'a pas pu être affiché."), detail: "image"
                    )
                }
                return Rendered(result: result, before: PageImage(cgImage: before), after: PageImage(cgImage: after))
            }
        }.value
    }

    // MARK: - Traitement

    /// Pages visées par « Appliquer » et « Rétablir l'original ».
    var targets: [DisplayPage] {
        guard let model else { return [] }
        switch range {
        case .all:
            return model.pages
        case .selection:
            return model.positions(of: model.selection).map { model.pages[$0] }
        case .current:
            return model.pages[safe: model.canvas.currentPage].map { [$0] } ?? []
        }
    }

    var canApply: Bool {
        model?.canEdit == true && !targets.isEmpty && (settings.deskew || settings.orientation)
    }

    /// Des pages visées sont améliorées : elles peuvent revenir à l'original.
    var canRestore: Bool {
        model?.canEdit == true && targets.contains { $0.replaced != nil }
    }

    /// Améliore les pages visées, puis les applique en une étape d'annulation.
    func apply() {
        guard canApply, task == nil else { return }
        let pages = targets
        progress = Progress(done: 0, total: pages.count)
        task = Task { [weak self] in
            await self?.run(pages)
            self?.task = nil
        }
    }

    /// Arrête le traitement après la page en cours ; les pages déjà traitées
    /// sont appliquées.
    func stop() {
        guard progress != nil else { return }
        progress?.isStopping = true
        task?.cancel()
    }

    /// La fenêtre se ferme.
    func cancel() {
        task?.cancel()
    }

    /// Rend aux pages visées améliorées leur page d'origine (annulable).
    func restoreOriginals() {
        guard canRestore, let model else { return }
        let ids = targets.filter { $0.replaced != nil }.map(\.id)
        model.perform { try $0.restoreOriginalPages(pages: ids) }
        pagesToCheck.removeAll { ids.contains($0) }
    }

    private func run(_ pages: [DisplayPage]) async {
        guard let model, let session = model.session else {
            progress = nil
            return
        }
        let settings = settings
        let recognizer: VisionTextRecognizer? = settings.orientation ? VisionTextRecognizer() : nil
        var results: [EnhancedPage] = []
        var failures = 0
        for page in pages {
            guard !Task.isCancelled, !model.isClosed else { break }
            // L'aperçu de la page, s'il est à jour, n'est pas recalculé.
            if let preview, preview.key == key(for: page) {
                results.append(preview.result)
            } else {
                let result = await Task.detached(priority: .userInitiated) {
                    Result { try session.enhancePage(page: page.id, settings: settings, recognizer: recognizer) }
                }.value
                switch result {
                case let .success(enhanced):
                    results.append(enhanced)
                case let .failure(error):
                    failures += 1
                    let detail = (error as? EngineError)?.technicalDetail ?? "\(error)"
                    model.logger.error("Amélioration impossible : \(detail, privacy: .public)")
                }
            }
            progress?.done += 1
        }
        let interrupted = results.count + failures < pages.count
        progress = nil
        guard !model.isClosed else { return }
        let reports = results.map { $0.report() }
        let changed = results.filter { $0.report().changesPixels || $0.report().quarterTurns != 0 }
        if !changed.isEmpty, await commit(changed, session: session) {
            preview = nil
        }
        pagesToCheck = reports.filter { $0.status == .toCheck }.map(\.page)
        summarize(reports, failures: failures, interrupted: interrupted)
    }

    /// Applique les pages améliorées `changed` ; renvoie `false` en cas d'échec.
    private func commit(_ changed: [EnhancedPage], session: EditSession) async -> Bool {
        guard let model else { return false }
        let built = await Task.detached(priority: .userInitiated) {
            Result { try session.enhancedDocument(pages: changed) }
        }.value
        var source: UInt32?
        switch built {
        case let .success(document):
            if let document, let added = model.addSource(document) {
                source = added
                await model.retainCopy(ofSource: added, protected: false)
            }
        case let .failure(error):
            model.report(error)
            return false
        }
        return model.perform { try $0.applyEnhancements(pages: changed, source: source) }
    }

    /// Bilan du traitement, en bas de la fenêtre.
    private func summarize(_ reports: [EnhancementReport], failures: Int, interrupted: Bool) {
        guard let model else { return }
        var parts = Self.outcome(of: reports)
        if interrupted {
            parts.append(String(localized: "Traitement arrêté avant la fin."))
        }
        if failures > 0 {
            model.failureMessage = failures == 1
                ? String(localized: "Une page n'a pas pu être améliorée.")
                : String(localized: "\(failures) pages n'ont pas pu être améliorées.")
        }
        model.show(notice: parts.joined(separator: " "))
    }

    /// Pages redressées, remises à l'endroit et à vérifier.
    private static func outcome(of reports: [EnhancementReport]) -> [String] {
        let corrected = reports.filter { $0.status == .corrected }.count
        let turned = reports.filter { $0.quarterTurns != 0 }.count
        let toCheck = reports.filter { $0.status == .toCheck }.count
        let scanned = reports.filter { $0.status != .notScanned && $0.status != .retouched }.count
        var parts: [String] = []
        if scanned == 0 {
            parts.append(String(localized: "Aucune page scannée à redresser."))
        } else if corrected == 0, turned == 0 {
            parts.append(String(localized: "Pages déjà droites : rien à corriger."))
        } else {
            if corrected > 0 {
                parts.append(corrected == 1
                    ? String(localized: "1 page redressée.")
                    : String(localized: "\(corrected) pages redressées."))
            }
            if turned > 0 {
                parts.append(turned == 1
                    ? String(localized: "1 page remise à l'endroit.")
                    : String(localized: "\(turned) pages remises à l'endroit."))
            }
        }
        if toCheck > 0 {
            parts.append(toCheck == 1
                ? String(localized: "1 page à vérifier (inclinaison trop forte).")
                : String(localized: "\(toCheck) pages à vérifier (inclinaison trop forte)."))
        }
        return parts
    }
}

extension EnhancementReport {
    /// Conclusion lisible, pour le panneau.
    var summary: String {
        let angle = measuredAngle.map { $0.formatted(.number.precision(.fractionLength(2))) }
        let turned = quarterTurns != 0 ? " " + String(localized: "Page remise à l'endroit.") : ""
        switch status {
        case .straight:
            return String(localized: "Page droite : aucune correction.") + turned
        case .corrected:
            return String(localized: "Inclinaison corrigée : \(angle ?? "?")°.") + turned
        case .toCheck:
            return String(localized: "Inclinaison de \(angle ?? "?")°, au-delà du plafond : à vérifier.") + turned
        case .unmeasurable:
            return String(localized: "Pas assez de texte pour mesurer l'inclinaison.") + turned
        case .disabled:
            return quarterTurns != 0
                ? String(localized: "Page remise à l'endroit.")
                : String(localized: "Redressement désactivé.")
        case .notScanned:
            return String(localized: "Cette page n'est pas un scan : elle reste telle quelle.")
        case .retouched:
            return String(localized: "Cette page a été modifiée en Édition : elle reste telle quelle.")
        }
    }
}
