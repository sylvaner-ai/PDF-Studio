import Foundation

/// Pages dont le texte est à reconnaître.
enum RecognitionScope: CaseIterable, Sendable {
    /// La page lue.
    case currentPage
    /// Les pages sélectionnées dans les vignettes.
    case selection
    /// Les pages sans texte (scans), détectées par le moteur.
    case scannedPages
    /// Toutes les pages du document.
    case allPages
}

/// Avancement d'une reconnaissance du texte.
struct RecognitionProgress: Equatable, Sendable {
    /// Pages lues.
    var done: Int
    /// Pages à lire.
    let total: Int
    /// Vrai une fois l'arrêt demandé : la page en cours se termine.
    var isStopping = false

    var fraction: Double {
        total == 0 ? 0 : Double(done) / Double(total)
    }
}

// MARK: - Reconnaissance du texte (OCR)

/// Le texte des pages scannées est lu par Vision (``VisionTextRecognizer``),
/// page après page, hors du fil principal. Le moteur prépare chaque image
/// (fond, contraste, inclinaison, orientation) et convertit les mots lus en
/// couche de texte invisible, cherchable et enregistrée avec le document.
/// Les pages restent figées pendant la lecture ; à la fin, ou à l'arrêt, les
/// pages lues reçoivent leur couche en une seule étape d'annulation, et les
/// pages couchées ou retournées sont remises à l'endroit.
extension DocumentViewModel {
    /// Pages visées par `scope`, dans l'ordre du document.
    func recognitionTargets(_ scope: RecognitionScope) async -> [UInt64] {
        switch scope {
        case .currentPage:
            return pages[safe: canvas.currentPage].map { [$0.id] } ?? []
        case .selection:
            return positions(of: selection).map { pages[$0] }.filter { !$0.isBlank }.map(\.id)
        case .allPages:
            return pages.filter { !$0.isBlank }.map(\.id)
        case .scannedPages:
            guard let session else { return [] }
            return await Task.detached(priority: .userInitiated) {
                (try? session.pagesToRecognize()) ?? []
            }.value
        }
    }

    /// Lance la reconnaissance du texte des pages visées par `scope`.
    func recognizeText(_ scope: RecognitionScope) {
        guard canEdit, recognitionTask == nil else { return }
        recognitionTask = Task { [weak self] in
            await self?.runRecognition(scope)
            self?.recognitionTask = nil
        }
    }

    /// Arrête la reconnaissance après la page en cours ; les pages déjà lues
    /// reçoivent leur couche de texte.
    func stopRecognition() {
        guard recognition != nil else { return }
        recognition?.isStopping = true
        recognitionTask?.cancel()
    }

    private func runRecognition(_ scope: RecognitionScope) async {
        let targets = await recognitionTargets(scope)
        guard let session, canEdit else { return }
        guard !targets.isEmpty else {
            show(notice: scope == .scannedPages
                 ? String(localized: "Aucune page scannée à reconnaître : toutes les pages ont déjà du texte.")
                 : String(localized: "Aucune page à reconnaître."))
            return
        }
        recognition = RecognitionProgress(done: 0, total: targets.count)
        let recognizer = VisionTextRecognizer()
        let settings = defaultRecognitionSettings()
        var results: [PageRecognition] = []
        var failures = 0
        for page in targets {
            guard !Task.isCancelled, !isClosed else { break }
            let result = await Task.detached(priority: .userInitiated) {
                Result { try session.recognizePage(page: page, recognizer: recognizer, settings: settings) }
            }.value
            switch result {
            case let .success(read):
                results.append(read)
            case let .failure(error):
                failures += 1
                let detail = (error as? EngineError)?.technicalDetail ?? "\(error)"
                logger.error("Reconnaissance impossible : \(detail, privacy: .public)")
            }
            recognition?.done += 1
        }
        recognition = nil
        guard !isClosed else { return }
        if results.contains(where: { !$0.words.isEmpty || $0.quarterTurns != 0 }) {
            perform { $0.applyRecognitions(results: results) }
        }
        summarize(results, failures: failures, interrupted: results.count + failures < targets.count)
    }

    /// Bilan de la reconnaissance.
    private func summarize(_ results: [PageRecognition], failures: Int, interrupted: Bool) {
        let read = results.filter { !$0.words.isEmpty }
        let words = read.reduce(0) { $0 + $1.words.count }
        let doubtful = read.reduce(0) { $0 + Int($1.doubtfulWords) }
        var parts: [String] = []
        if read.isEmpty {
            parts.append(String(localized: "Aucun texte reconnu."))
        } else {
            let pageCount = read.count == 1
                ? String(localized: "1 page")
                : String(localized: "\(read.count) pages")
            let wordCount = words == 1 ? String(localized: "1 mot") : String(localized: "\(words) mots")
            parts.append(String(localized: "Texte reconnu : \(pageCount), \(wordCount)."))
            if doubtful > 0 {
                parts.append(doubtful == 1
                    ? String(localized: "1 mot douteux, signalé en mode Édition.")
                    : String(localized: "\(doubtful) mots douteux, signalés en mode Édition."))
            }
        }
        if interrupted {
            parts.append(String(localized: "Reconnaissance arrêtée avant la fin."))
        }
        if failures > 0 {
            failureMessage = failures == 1
                ? String(localized: "Le texte d'une page n'a pas pu être reconnu.")
                : String(localized: "Le texte de \(failures) pages n'a pas pu être reconnu.")
        }
        show(notice: parts.joined(separator: " "))
    }

    /// Affiche une information passagère pendant quelques secondes.
    func show(notice text: String) {
        notice = text
        noticeTask?.cancel()
        noticeTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(6))
            guard !Task.isCancelled, let self, self.notice == text else { return }
            self.notice = nil
        }
    }
}
