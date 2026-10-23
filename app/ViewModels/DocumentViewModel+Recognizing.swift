import Foundation

/// Avancement de la préparation du mode Édition : recherche des pages
/// scannées, puis reconnaissance de leur texte.
struct RecognitionProgress: Equatable, Sendable {
    /// Pages lues.
    var done: Int
    /// Pages à lire ; 0 pendant la recherche des pages scannées.
    let total: Int
    /// Vrai une fois l'arrêt demandé : la page en cours se termine.
    var isStopping = false

    /// Vrai pendant la recherche des pages scannées, avant leur lecture.
    var isSearching: Bool {
        total == 0
    }

    var fraction: Double {
        total == 0 ? 0 : Double(done) / Double(total)
    }
}

// MARK: - Mode Édition

extension DocumentViewModel {
    /// Vrai si les objets des pages se sélectionnent et se modifient dans le
    /// canevas : en mode Édition, hors de la comparaison avec l'original, et
    /// une fois lu le texte des pages scannées (pendant leur lecture, les
    /// pages sont montrées comme en lecture).
    var isEditingCanvas: Bool {
        workspace == .editing && !isComparing && recognition?.isSearching != false
    }

    /// Met le canevas en mode Édition, ou le ramène à la lecture.
    func updateCanvasEditing() {
        canvas.setEditing(isEditingCanvas ? editing : nil)
    }
}

// MARK: - Reconnaissance du texte (OCR), à l'entrée en mode Édition

/// La reconnaissance du texte fait partie du mode Édition (D58) : à chaque
/// entrée dans ce mode, le moteur cherche les pages scannées pas encore
/// reconnues (aucun texte, ou seulement le texte invisible d'un OCR
/// précédent). Un PDF natif passe aussitôt en Édition ; sinon, le texte de
/// toutes ses pages scannées est d'abord lu par Vision
/// (``VisionTextRecognizer``), page après page, hors du fil principal. Le
/// moteur prépare chaque image (fond, contraste, inclinaison, orientation) et
/// convertit les mots lus en couche de texte invisible, cherchable et
/// enregistrée avec le document. Les pages restent figées pendant la lecture,
/// montrées comme en lecture ; à la fin, ou à l'arrêt, les pages lues
/// reçoivent leur couche en une seule étape d'annulation, les pages couchées
/// ou retournées sont remises à l'endroit, et les objets des pages deviennent
/// modifiables.
extension DocumentViewModel {
    /// Prépare le mode Édition : cherche les pages scannées et lit leur texte.
    /// Sans effet hors du mode Édition, pendant une autre opération (la
    /// préparation se fera à la prochaine entrée en Édition) ou si une
    /// préparation est déjà en cours.
    func prepareEditing() {
        guard workspace == .editing, canEdit, recognitionTask == nil else { return }
        recognition = RecognitionProgress(done: 0, total: 0)
        recognitionTask = Task { [weak self] in
            await self?.runPreparation()
            self?.recognitionTask = nil
        }
    }

    /// Arrête la préparation du mode Édition : la recherche des pages
    /// scannées, ou la lecture après la page en cours ; les pages déjà lues
    /// reçoivent leur couche de texte.
    func stopRecognition() {
        guard recognition != nil else { return }
        recognition?.isStopping = true
        recognitionTask?.cancel()
    }

    private func runPreparation() async {
        let targets = await scannedPages()
        // Recherche arrêtée, ou mode Édition quitté entre-temps : rien n'est lu.
        guard let session, !targets.isEmpty, !Task.isCancelled, !isClosed, workspace == .editing else {
            recognition = nil
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
        // Une page lue sans texte (photo, dessin) n'est pas relue à la prochaine
        // entrée en Édition.
        pagesWithoutText.formUnion(results.filter { $0.words.isEmpty && $0.quarterTurns == 0 }.map(\.page))
        if results.contains(where: { !$0.words.isEmpty || $0.quarterTurns != 0 }) {
            perform { $0.applyRecognitions(results: results) }
        }
        summarize(results, failures: failures, interrupted: results.count + failures < targets.count)
    }

    /// Pages scannées pas encore reconnues, dans l'ordre du document, sauf
    /// celles déjà lues sans y trouver de texte.
    private func scannedPages() async -> [UInt64] {
        guard let session else { return [] }
        let found = await Task.detached(priority: .userInitiated) {
            (try? session.pagesToRecognize()) ?? []
        }.value
        return found.filter { !pagesWithoutText.contains($0) }
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
                    ? String(localized: "1 mot douteux, surligné en orange.")
                    : String(localized: "\(doubtful) mots douteux, surlignés en orange."))
            }
        }
        if interrupted {
            parts.append(String(localized: """
                Reconnaissance arrêtée avant la fin : les pages restantes seront lues \
                à la prochaine entrée en Édition.
                """))
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
