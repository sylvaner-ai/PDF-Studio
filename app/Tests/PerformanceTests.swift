import AppKit
import QuartzCore
import SwiftUI
import XCTest
@testable import PDFStudio

/// Mesures des exigences de performance du cahier des charges (§6) sur le
/// Mac : ouverture d'un document, défilement, reconnaissance d'une page,
/// retouche d'une ligne scannée, récupération d'un gros document (`make perf`).
///
/// Ignoré par les tests ordinaires : il ne s'exécute que si la variable
/// d'environnement `PDFSTUDIO_PERF` est définie. Chaque mesure écrit son
/// tableau dans les caches du bac à sable de l'application, d'où `make perf`
/// les réunit en un rapport. Les fenêtres apparaissent quelques secondes.
@MainActor
final class PerformanceTests: XCTestCase {
    private static let windowSize = CGSize(width: 1080, height: 760)

    override func setUpWithError() throws {
        guard ProcessInfo.processInfo.environment["PDFSTUDIO_PERF"] != nil else {
            throw XCTSkip("Mesures de performance : lancer `make perf`.")
        }
    }

    // MARK: - Ouverture

    /// Exigence : un PDF de 100 pages s'ouvre en moins de 2 s.
    func test1Ouverture() async throws {
        // Le premier document ouvert paie l'initialisation du moteur et des polices.
        let warmUp = CACurrentMediaTime()
        try await open("texte_simple").close()
        let initialization = CACurrentMediaTime() - warmUp

        var lines = [
            "## Ouverture",
            "",
            "De la demande d'ouverture à la première page rendue à la largeur d'une fenêtre "
                + "Retina (1 600 pixels). Initialisation du moteur, au premier document : "
                + "\(ms(initialization)) ms.",
            "",
            "| Document | Pages | Document prêt (ms) | Première page (ms) | Total (ms) |",
            "|---|---:|---:|---:|---:|"
        ]
        for name in ["multipage_100", "volumineux_600", "edition", "tableaux", "scan_simule"] {
            let start = CACurrentMediaTime()
            let model = try await open(name)
            let ready = CACurrentMediaTime()
            let renderer = try XCTUnwrap(model.renderer)
            let page = try XCTUnwrap(model.pages.first)
            _ = try await renderer.image(of: page, pixelWidth: 1600)
            let shown = CACurrentMediaTime()
            model.close()
            lines.append("| \(name).pdf | \(model.pageCount) | \(ms(ready - start)) | \(ms(shown - ready)) | "
                + "\(ms(shown - start)) |")
            if name == "multipage_100" {
                XCTAssertLessThan(shown - start, 2, "ouverture de 100 pages")
            }
        }
        try write(lines, as: "1-ouverture")
    }

    // MARK: - Défilement

    /// Exigence : défilement à 60 images par seconde. Le défilement est
    /// commandé image par image par l'horloge de l'écran ; une image manquée
    /// est un intervalle de plus d'une fois et demie la période de l'écran.
    func test2Defilement() throws {
        var rows: [String] = []
        var period: CFTimeInterval = 1.0 / 60
        for name in ["multipage_100", "volumineux_600", "tableaux"] {
            let stats = try scroll(name, speed: 2000, pages: 25)
            period = stats.period
            rows.append(
                "| \(name).pdf | \(stats.frames) | \(String(format: "%.1f", stats.rate)) | "
                    + "\(stats.missed) (\(percent(stats.missed, of: stats.frames))) | "
                    + "\(String(format: "%.2f", stats.meanWork * 1000)) | "
                    + "\(String(format: "%.1f", stats.maxWork * 1000)) | "
                    + "\(percent(stats.blankFrames, of: stats.frames)) |"
            )
        }
        try write([
            "## Défilement",
            "",
            "Défilement continu à 2 000 points par seconde (environ deux pages A4 et demie), sur "
                + "25 pages au plus, fenêtre de 1 080 × 760 points, ajustement à la largeur ; écran "
                + "à \(Int((1 / period).rounded())) images par seconde. *Travail* : temps passé sur le fil "
                + "principal pour chaque image (mise en page et dessin). *Pages blanches* : part des "
                + "images où une page visible attend encore son rendu.",
            "",
            "| Document | Images | Images/s | Images manquées | Travail moyen (ms) | Travail max (ms) "
                + "| Pages blanches |",
            "|---|---:|---:|---:|---:|---:|---:|"
        ] + rows, as: "2-defilement")
    }

    /// Fait défiler `pages` pages du document à `speed` points par seconde.
    private func scroll(_ name: String, speed: CGFloat, pages: Int) throws -> ScrollStats {
        let model = DocumentViewModel(reference: try reference(name))
        Task { await model.open() }
        wait(upTo: 5) { model.phase == .ready }
        let window = makeWindow(DocumentView(model: model))
        defer {
            window.close()
            model.close()
        }
        wait(seconds: 1.5)
        let canvas = try XCTUnwrap(find(PageCanvasView.self, in: window.contentView))
        let visible = canvas.documentVisibleRect
        let height = canvas.documentView?.frame.height ?? 0
        let pageHeight = height / CGFloat(max(model.pageCount, 1))
        let distance = min(height - visible.height, pageHeight * CGFloat(pages))
        let driver = ScrollDriver(canvas: canvas, origin: visible.origin, speed: speed, distance: distance)
        let link = canvas.displayLink(target: driver, selector: #selector(ScrollDriver.step(_:)))
        link.add(to: .main, forMode: .common)
        wait(upTo: 120) { driver.isDone }
        link.invalidate()
        return driver.stats
    }

    // MARK: - Reconnaissance du texte

    /// Exigence : OCR d'une page A4 à 300 ppp en moins de 5 s.
    func test3Reconnaissance() throws {
        var lines = [
            "## Reconnaissance du texte (Vision)",
            "",
            "Page rendue à 300 ppp, préparée (fond, inclinaison, orientation) puis lue par Vision ; "
                + "le temps comprend le rendu et la conversion des mots en couche de texte.",
            "",
            "| Document | Mots lus | Temps (s) |",
            "|---|---:|---:|"
        ]
        for name in ["scan_simule", "scan_retouche"] {
            let session = EditSession(document: try PdfDocument.open(path: fixture(name).path, password: nil))
            let page = try XCTUnwrap(session.pages().first?.id)
            let start = CACurrentMediaTime()
            let result = try session.recognizePage(
                page: page, recognizer: VisionTextRecognizer(), settings: defaultRecognitionSettings()
            )
            let elapsed = CACurrentMediaTime() - start
            lines.append("| \(name).pdf | \(result.words.count) | \(String(format: "%.2f", elapsed)) |")
            XCTAssertLessThan(elapsed, 5, name)
        }
        try write(lines, as: "3-reconnaissance")
    }

    // MARK: - Retouche d'une ligne scannée

    /// Première réécriture d'une ligne scannée : recherche de la police parmi
    /// le catalogue (la fenêtre attend sa fin), puis réécriture d'une autre ligne.
    func test4Retouche() throws {
        BundledFonts.register()
        let session = EditSession(document: try PdfDocument.open(path: fixture("scan_retouche").path, password: nil))
        let page = try XCTUnwrap(session.pagesToRecognize().first)
        let result = try session.recognizePage(
            page: page, recognizer: VisionTextRecognizer(), settings: defaultRecognitionSettings()
        )
        session.applyRecognitions(results: [result])
        let lines = try session.pageObjects(page: page).filter {
            $0.kind == .text && $0.editable && ($0.text?.count ?? 0) >= 12
        }
        XCTAssertGreaterThanOrEqual(lines.count, 2)
        var rows: [String] = []
        for (rank, line) in lines.prefix(3).enumerated() {
            let text = (line.text ?? "") + " 2"
            let start = CACurrentMediaTime()
            let edited = try session.replaceText(page: page, object: line.id, text: text)
            let elapsed = CACurrentMediaTime() - start
            let found = edited.scan?.candidates.first.map { "\($0.family) (\(Int(($0.score * 100).rounded())) %)" }
            rows.append("| \(rank + 1) | \(ms(elapsed)) | \(found ?? "—") |")
        }
        try write([
            "## Retouche d'une ligne scannée",
            "",
            "Réécriture d'une ligne de `scan_retouche.pdf` lue par Vision : mesure de la ligne, "
                + "recherche de la police parmi le catalogue (polices fournies et polices "
                + "bureautiques installées), rendu de l'aperçu. La première réécriture établit aussi "
                + "le catalogue.",
            "",
            "| Ligne | Temps (ms) | Police retrouvée |",
            "|---:|---:|---|"
        ] + rows, as: "4-retouche")
    }

    // MARK: - Récupération d'un gros document

    /// Une modification d'un document de 600 pages, toutes reconnues : temps
    /// sur le fil principal, puis écriture de la récupération (hors du fil
    /// principal).
    func test5Recuperation() async throws {
        let model = try await open("volumineux_600")
        defer {
            model.discardChanges()
            model.close()
            RecoveryWriter.shared.flush()
        }
        let session = try XCTUnwrap(model.session)
        let items = session.pages()
        let layers = items.indices.map { _ in Self.syntheticLayer(words: 300) }
        model.perform {
            try $0.restore(
                pages: items, contents: [[ObjectEditItem]](repeating: [], count: items.count), layers: layers
            )
        }
        RecoveryWriter.shared.flush()

        var mainThread: [CFTimeInterval] = []
        for turn in 0..<10 {
            let start = CACurrentMediaTime()
            model.rotate([turn % model.pageCount], clockwise: true)
            mainThread.append(CACurrentMediaTime() - start)
        }
        let writeStart = CACurrentMediaTime()
        RecoveryWriter.shared.flush()
        let writing = CACurrentMediaTime() - writeStart
        let store = try XCTUnwrap(model.recoveryStore)
        let record = try XCTUnwrap(store.record(for: model.reference.path))
        XCTAssertEqual(record.pages.count, 600)
        XCTAssertEqual(record.pages.compactMap(\.layer).count, 600)
        let size = try JSONEncoder().encode(record).count
        try write([
            "## Récupération d'un gros document",
            "",
            "`volumineux_600.pdf`, 600 pages munies chacune d'une couche de texte de 300 mots, "
                + "puis 10 rotations de page.",
            "",
            "| Mesure | Valeur |",
            "|---|---:|",
            "| Modification, fil principal (moyenne) | \(ms(mainThread.reduce(0, +) / 10)) ms |",
            "| Modification, fil principal (max) | \(ms(mainThread.max() ?? 0)) ms |",
            "| Écriture de la récupération (hors fil principal) | \(ms(writing)) ms |",
            "| Taille du fichier de récupération | \(String(format: "%.1f", Double(size) / 1_048_576)) Mo |"
        ], as: "5-recuperation")
    }

    /// Couche de texte factice : `words` mots sur des lignes de dix.
    private static func syntheticLayer(words: Int) -> [LayerWordItem] {
        (0..<words).map { index in
            let x = Float(50 + (index % 10) * 50)
            let y = Float(780 - (index / 10) * 22)
            return LayerWordItem(
                text: "mot\(index)", quad: [x, y, x + 40, y, x + 40, y + 12, x, y + 12],
                confidence: 0.9, line: UInt32(index / 10)
            )
        }
    }
}

// MARK: - Outils

extension PerformanceTests {
    private func fixture(_ name: String) throws -> URL {
        try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"), "\(name).pdf absent")
    }

    private func reference(_ name: String) throws -> DocumentReference {
        // Un fichier du bundle de test ne peut pas recevoir de signet de sécurité.
        try DocumentReference(url: fixture(name), securityScoped: false)
    }

    private func open(_ name: String) async throws -> DocumentViewModel {
        let model = DocumentViewModel(reference: try reference(name))
        await model.open()
        XCTAssertEqual(model.phase, .ready, name)
        return model
    }

    private func makeWindow(_ view: some View) -> NSWindow {
        let window = NSWindow(
            contentRect: CGRect(origin: .zero, size: Self.windowSize),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.toolbarStyle = .unified
        let host = NSHostingView(rootView: view)
        host.sceneBridgingOptions = [.toolbars, .title]
        window.contentView = host
        window.center()
        window.makeKeyAndOrderFront(nil)
        return window
    }

    private func find<T: NSView>(_ type: T.Type, in view: NSView?) -> T? {
        guard let view else { return nil }
        if let found = view as? T {
            return found
        }
        for child in view.subviews {
            if let found = find(type, in: child) {
                return found
            }
        }
        return nil
    }

    /// Écrit un tableau de mesures dans les caches du bac à sable.
    private func write(_ lines: [String], as name: String) throws {
        let caches = try XCTUnwrap(FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first)
        let folder = caches.appendingPathComponent("Performances", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let text = lines.joined(separator: "\n") + "\n\n"
        try Data(text.utf8).write(to: folder.appendingPathComponent("\(name).md"))
        print(text)
    }

    private func ms(_ seconds: CFTimeInterval) -> String {
        String(format: "%.0f", seconds * 1000)
    }

    private func percent(_ count: Int, of total: Int) -> String {
        total == 0 ? "—" : String(format: "%.1f %%", 100 * Double(count) / Double(total))
    }

    private func wait(seconds: TimeInterval) {
        RunLoop.main.run(until: Date().addingTimeInterval(seconds))
    }

    private func wait(upTo seconds: TimeInterval, until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        }
    }
}

// MARK: - Défilement

private struct ScrollStats {
    var frames = 0
    var missed = 0
    var period: CFTimeInterval = 1.0 / 60
    var elapsed: CFTimeInterval = 0
    var meanWork: CFTimeInterval = 0
    var maxWork: CFTimeInterval = 0
    var blankFrames = 0
    var rate: Double { elapsed > 0 ? Double(frames) / elapsed : 0 }
}

/// Avance le défilement à chaque image de l'écran et mesure le travail.
@MainActor
private final class ScrollDriver: NSObject {
    private let canvas: PageCanvasView
    private let origin: CGPoint
    private let speed: CGFloat
    private let distance: CGFloat
    private var start: CFTimeInterval?
    private var last: CFTimeInterval?
    private var totalWork: CFTimeInterval = 0
    private(set) var stats = ScrollStats()
    private(set) var isDone = false

    init(canvas: PageCanvasView, origin: CGPoint, speed: CGFloat, distance: CGFloat) {
        self.canvas = canvas
        self.origin = origin
        self.speed = speed
        self.distance = distance
    }

    @objc func step(_ link: CADisplayLink) {
        guard !isDone else { return }
        let now = link.timestamp
        stats.period = link.duration
        let start = self.start ?? now
        self.start = start
        if let last {
            stats.frames += 1
            if now - last > 1.5 * link.duration {
                stats.missed += 1
            }
        }
        last = now
        let begin = CACurrentMediaTime()
        let offset = min(speed * CGFloat(now - start), distance)
        canvas.contentView.scroll(to: CGPoint(x: origin.x, y: origin.y + offset))
        canvas.reflectScrolledClipView(canvas.contentView)
        canvas.window?.displayIfNeeded()
        let work = CACurrentMediaTime() - begin
        if stats.frames > 0 {
            totalWork += work
            stats.maxWork = max(stats.maxWork, work)
            stats.meanWork = totalWork / Double(stats.frames)
            let shown = canvas.documentVisibleRect
            if canvas.pageViews.values.contains(where: { $0.frame.intersects(shown) && !$0.showsImage }) {
                stats.blankFrames += 1
            }
        }
        if offset >= distance {
            stats.elapsed = now - start
            isDone = true
        }
    }
}
