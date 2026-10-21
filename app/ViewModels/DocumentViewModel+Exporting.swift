import AppKit
import UniformTypeIdentifiers

// MARK: - Extraction, scission et exportations

/// Les pages sortent du document sans le modifier : extraction des pages
/// sélectionnées dans un nouveau PDF, scission en plusieurs PDF, images des
/// pages (PNG ou JPEG) et texte natif (.txt). Le travail se fait hors du fil
/// principal ; un fichier remplacé est d'abord sauvegardé (cf. ``FileSaver``).
extension DocumentViewModel {
    /// Nom du document sans extension, pour nommer les fichiers produits.
    var baseName: String {
        (reference.displayName as NSString).deletingPathExtension
    }

    /// Enregistre les pages désignées dans un nouveau document.
    func extractPages(_ positions: [Int]) {
        guard canEdit, !positions.isEmpty, let session else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.nameFieldStringValue = "\(baseName) (extrait).pdf"
        panel.canCreateDirectories = true
        panel.message = positions.count == 1
            ? String(localized: "La page sera enregistrée dans un nouveau document.")
            : String(localized: "Les \(positions.count) pages seront enregistrées dans un nouveau document.")
        let pages = positions.map { UInt32($0) }
        panel.begin { [weak self] response in
            let url = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let url else { return }
                await self.run(String(localized: "Extraction…")) {
                    try FileSaver.write(try session.exportPages(positions: pages), to: url)
                }
            }
        }
    }

    /// Scinde le document en plusieurs PDF, rangés dans un dossier choisi.
    func split() {
        guard canEdit, pages.count > 1 else { return }
        let cuts = positions(of: selection)
        let options = SplitOptions(canCutAtSelection: cuts.contains { $0 > 0 })
        let panel = folderPanel(
            message: String(localized: "Choisissez le dossier où enregistrer les parties du document."),
            prompt: String(localized: "Scinder")
        )
        panel.accessoryView = options.view
        panel.begin { [weak self] response in
            let folder = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let folder else { return }
                let parts = PageExporter.parts(pageCount: self.pages.count, rule: options.rule(cuts: cuts))
                await self.writeParts(parts, to: folder)
            }
        }
    }

    /// Enregistre des pages en images PNG ou JPEG, dans un dossier choisi.
    func exportImages() {
        guard canEdit else { return }
        let selected = positions(of: selection)
        let options = ImageExportOptions(hasSelection: !selected.isEmpty && selected.count < pages.count)
        let panel = folderPanel(
            message: String(localized: "Choisissez le dossier où enregistrer les images des pages."),
            prompt: String(localized: "Exporter")
        )
        panel.accessoryView = options.view
        panel.begin { [weak self] response in
            let folder = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let folder else { return }
                let positions = options.selectedOnly ? selected : Array(self.pages.indices)
                await self.writeImages(of: positions, format: options.imageFormat, dpi: options.dpi, to: folder)
            }
        }
    }

    /// Enregistre le texte de toutes les pages dans un fichier .txt.
    func exportText() {
        guard canEdit else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.plainText]
        panel.nameFieldStringValue = "\(baseName).txt"
        panel.canCreateDirectories = true
        panel.begin { [weak self] response in
            let url = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let url else { return }
                await self.writeText(to: url)
            }
        }
    }

    // MARK: Écriture

    private func writeParts(_ parts: [[Int]], to folder: URL) async {
        guard parts.count > 1, let session else {
            failureMessage = String(
                localized: "Ce découpage ne donne qu'une partie : le document n'a pas été scindé."
            )
            return
        }
        let names = PageExporter.numberedNames(base: baseName, numbers: Array(1...parts.count), extension: "pdf")
        let urls = names.map { folder.appendingPathComponent($0) }
        guard confirmReplacing(urls) else { return }
        let positions = parts.map { $0.map { UInt32($0) } }
        await run(String(localized: "Scission…")) {
            for (part, url) in zip(positions, urls) {
                try FileSaver.write(try session.exportPages(positions: part), to: url)
            }
        }
    }

    private func writeImages(
        of positions: [Int], format: PageExporter.ImageFormat, dpi: Int, to folder: URL
    ) async {
        guard !positions.isEmpty, let sources else { return }
        let names = PageExporter.numberedNames(
            base: baseName, numbers: positions.map { $0 + 1 }, extension: format.fileExtension
        )
        let urls = names.map { folder.appendingPathComponent($0) }
        guard confirmReplacing(urls) else { return }
        let pages = positions.map { self.pages[$0] }
        await run(String(localized: "Exportation des images…")) {
            for (page, url) in zip(pages, urls) {
                let image = try PageExporter.image(of: page, dpi: dpi, sources: sources)
                try FileSaver.write(try PageExporter.encode(image, as: format, dpi: dpi), to: url)
            }
        }
    }

    private func writeText(to url: URL) async {
        guard let sources else { return }
        let pages = self.pages
        await run(String(localized: "Exportation du texte…")) {
            let texts = try pages.map { page -> String in
                guard let source = page.source, let document = sources.document(source.document) else { return "" }
                return try document.plainText(index: UInt32(source.index))
            }
            guard texts.contains(where: { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else {
                throw PageExporter.ExportError.noText
            }
            // Un saut de page sépare les pages, comme le fait pdftotext.
            try FileSaver.write(Data(texts.joined(separator: "\u{0C}").utf8), to: url)
        }
    }

    /// Exécute une exportation hors du fil principal, en l'indiquant dans la fenêtre.
    private func run(_ label: String, _ work: @escaping @Sendable () throws -> Void) async {
        activity = label
        let result = await Task.detached(priority: .userInitiated) { Result { try work() } }.value
        activity = nil
        guard case let .failure(error) = result else { return }
        if let error = error as? PageExporter.ExportError {
            failureMessage = error.message
        } else if error is EngineError {
            report(error)
        } else {
            logger.error("Exportation impossible : \(error.localizedDescription, privacy: .public)")
            failureMessage = String(localized: "L'exportation n'a pas abouti. \(error.localizedDescription)")
        }
    }

    /// Demande avant de remplacer des fichiers existants (ils sont d'abord sauvegardés).
    private func confirmReplacing(_ urls: [URL]) -> Bool {
        let existing = urls.filter { FileManager.default.fileExists(atPath: $0.path) }
        guard let first = existing.first else { return true }
        let alert = NSAlert()
        alert.messageText = existing.count == 1
            ? String(localized: "« \(first.lastPathComponent) » existe déjà dans ce dossier. Le remplacer ?")
            : String(localized: "\(existing.count) fichiers de ces noms existent déjà dans ce dossier. Les remplacer ?")
        alert.informativeText = String(
            localized: "Une copie des fichiers remplacés est conservée dans les sauvegardes de PDF Studio."
        )
        alert.addButton(withTitle: String(localized: "Remplacer"))
        alert.addButton(withTitle: String(localized: "Annuler"))
        return alert.runModal() == .alertFirstButtonReturn
    }

    private func folderPanel(message: String, prompt: String) -> NSOpenPanel {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = message
        panel.prompt = prompt
        panel.isAccessoryViewDisclosed = true
        return panel
    }
}
