import AppKit
import UniformTypeIdentifiers

// MARK: - Extraction, scission et exportations

/// Les pages sortent du document sans le modifier : extraction des pages
/// sélectionnées dans un nouveau PDF, scission en plusieurs PDF, images des
/// pages (PNG ou JPEG) et texte natif (.txt). Les pages sont désignées par leur
/// identifiant tant que le panneau est ouvert, puis figées à sa confirmation :
/// le travail, hors du fil principal, ne voit pas les modifications faites
/// ensuite. Un fichier remplacé est d'abord sauvegardé (cf. ``FileSaver``) ; le
/// fichier d'un document ouvert ne l'est jamais.
extension DocumentViewModel {
    /// Nom du document sans extension, pour nommer les fichiers produits.
    var baseName: String {
        (reference.displayName as NSString).deletingPathExtension
    }

    /// Pages choisies explicitement : la sélection de la grille, ou plusieurs
    /// vignettes en lecture (où la sélection d'une seule suit la page lue).
    var chosenPages: Set<UInt64> {
        workspace == .organizing || selection.count > 1 ? selection : []
    }

    /// Enregistre les pages désignées dans un nouveau document.
    func extractPages(_ positions: [Int]) {
        guard canEdit, !positions.isEmpty else { return }
        let ids = positions.compactMap { pages[safe: $0]?.id }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.nameFieldStringValue = "\(baseName) (extrait).pdf"
        panel.canCreateDirectories = true
        panel.message = positions.count == 1
            ? String(localized: "La page sera enregistrée dans un nouveau document.")
            : String(localized: "Les \(positions.count) pages seront enregistrées dans un nouveau document.")
        panel.begin { [weak self] response in
            let url = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let url, let session = self.session, self.canExport(to: [url]) else { return }
                let backups = self.backups(replacing: [url])
                await self.run(String(localized: "Extraction…")) {
                    try FileSaver.export(try session.exportPages(ids: ids), to: url, backups: backups)
                }
            }
        }
    }

    /// Scinde le document en plusieurs PDF, rangés dans un dossier choisi.
    func split() {
        guard canEdit, pages.count > 1 else { return }
        let cutPages = chosenPages
        let options = SplitOptions(canCutAtSelection: positions(of: cutPages).contains { $0 > 0 })
        let panel = folderPanel(
            message: String(localized: "Choisissez le dossier où enregistrer les parties du document."),
            prompt: String(localized: "Scinder")
        )
        panel.accessoryView = options.view
        panel.begin { [weak self] response in
            let folder = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let folder else { return }
                let rule = options.rule(cuts: self.positions(of: cutPages))
                let parts = PageExporter.parts(pageCount: self.pages.count, rule: rule)
                await self.writeParts(parts.map { $0.map { self.pages[$0].id } }, to: folder)
            }
        }
    }

    /// Enregistre des pages en images PNG ou JPEG, dans un dossier choisi.
    func exportImages() {
        guard canEdit else { return }
        let chosen = chosenPages
        let options = ImageExportOptions(hasSelection: !chosen.isEmpty && chosen.count < pages.count)
        let panel = folderPanel(
            message: String(localized: "Choisissez le dossier où enregistrer les images des pages."),
            prompt: String(localized: "Exporter")
        )
        panel.accessoryView = options.view
        panel.begin { [weak self] response in
            let folder = response == .OK ? panel.url : nil
            Task { @MainActor in
                guard let self, let folder else { return }
                let positions = options.selectedOnly ? self.positions(of: chosen) : Array(self.pages.indices)
                let pages = positions.map { (number: $0 + 1, page: self.pages[$0]) }
                await self.writeImages(of: pages, format: options.imageFormat, dpi: options.dpi, to: folder)
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

    /// Écrit chaque partie (des identifiants de pages) dans son fichier.
    private func writeParts(_ parts: [[UInt64]], to folder: URL) async {
        guard parts.count > 1, let session else {
            failureMessage = String(
                localized: "Ce découpage ne donne qu'une partie : le document n'a pas été scindé."
            )
            return
        }
        // Toutes les parties sont découpées dans le même état des pages.
        let snapshot = session.snapshot()
        let names = PageExporter.numberedNames(base: baseName, numbers: Array(1...parts.count), extension: "pdf")
        let urls = names.map { folder.appendingPathComponent($0) }
        guard canExport(to: urls), confirmReplacing(urls) else { return }
        let backups = backups(replacing: urls)
        await run(String(localized: "Scission…")) {
            for (part, url) in zip(parts, urls) {
                try FileSaver.export(try snapshot.exportPages(ids: part), to: url, backups: backups)
            }
        }
    }

    /// Écrit l'image de chaque page, nommée d'après son numéro.
    private func writeImages(
        of pages: [(number: Int, page: DisplayPage)], format: PageExporter.ImageFormat, dpi: Int, to folder: URL
    ) async {
        guard !pages.isEmpty, let sources else { return }
        let names = PageExporter.numberedNames(
            base: baseName, numbers: pages.map { $0.number }, extension: format.fileExtension
        )
        let urls = names.map { folder.appendingPathComponent($0) }
        guard canExport(to: urls), confirmReplacing(urls) else { return }
        let backups = backups(replacing: urls)
        let images = pages.map { $0.page }
        await run(String(localized: "Exportation des images…")) {
            for (page, url) in zip(images, urls) {
                let image = try PageExporter.image(of: page, dpi: dpi, sources: sources)
                // Une très grande page est réduite : sa résolution réelle est enregistrée.
                let resolution = PageExporter.resolution(forDPI: dpi, pageSize: page.size(DisplayRotation()))
                let data = try PageExporter.encode(image, as: format, dpi: resolution)
                try FileSaver.export(data, to: url, backups: backups)
            }
        }
    }

    private func writeText(to url: URL) async {
        guard let sources, canExport(to: [url]) else { return }
        let backups = backups(replacing: [url])
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
            try FileSaver.export(Data(texts.joined(separator: "\u{0C}").utf8), to: url, backups: backups)
        }
    }

    /// Exécute une exportation hors du fil principal, en l'indiquant dans la fenêtre.
    private func run(_ label: String, _ work: @escaping @Sendable () throws -> Void) async {
        guard activity == nil else { return }
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

    /// À la confirmation d'un panneau : aucune autre opération n'est en cours, et
    /// aucun fichier visé n'est celui d'un document ouvert, qui serait remplacé
    /// sous sa fenêtre.
    private func canExport(to urls: [URL]) -> Bool {
        guard !isClosed else { return false }
        guard canEdit else {
            failureMessage = String(localized: "Une autre opération est en cours. Réessayez une fois terminée.")
            return false
        }
        guard !urls.contains(where: { DocumentOpener.shared.isShown(path: $0.standardizedFileURL.path) }) else {
            failureMessage = String(localized: """
                Ce fichier est ouvert dans une fenêtre de PDF Studio. Choisissez un autre nom ou un autre dossier.
                """)
            return false
        }
        return true
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

    /// Dossier où conserver les fichiers que l'exportation va remplacer ; `nil`
    /// s'il n'y en a pas (ou s'il n'a pas pu être créé : ils iront dans le
    /// dossier commun des exportations).
    private func backups(replacing urls: [URL]) -> URL? {
        guard urls.contains(where: { FileManager.default.fileExists(atPath: $0.path) }) else { return nil }
        return try? FileSaver.exportBackupDirectory(named: baseName)
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
