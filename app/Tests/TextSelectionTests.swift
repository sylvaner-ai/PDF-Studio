import AppKit
import SwiftUI
import XCTest
@testable import PDFStudio

/// Sélection du texte à la souris en mode Lecture : géométrie du texte d'une
/// page, puis sélection et copie dans le canevas, sur le texte natif et sur
/// le texte reconnu d'une page scannée.
@MainActor
final class TextSelectionTests: XCTestCase {
    // MARK: - Texte d'une page

    /// Deux lignes de caractères de 10 × 10 points : « ab cd » (en haut) et « ef » (20 points plus bas).
    private func twoLines(rightToLeft: Bool = false) -> PageTextLayout {
        func line(_ text: String, top: Float) -> SelectableLine {
            let count = text.unicodeScalars.count
            let rects = (0..<count).map { rank in
                PageRect(x: Float(rightToLeft ? count - 1 - rank : rank) * 10, y: top, width: 10, height: 10)
            }
            return SelectableLine(text: text, rects: rects)
        }
        return PageTextLayout([line("ab cd", top: 0), line("ef", top: 20)])
    }

    private func hit(_ unit: Int, _ offset: Int) -> PageTextLayout.Hit {
        PageTextLayout.Hit(unit: unit, offset: offset)
    }

    func testVisePrecisementUnCaractere() {
        let layout = twoLines()
        XCTAssertEqual(layout.count, 7)
        // Avant ou après le caractère, selon la moitié visée.
        XCTAssertEqual(layout.hit(at: CGPoint(x: 12, y: 5)), hit(1, 1))
        XCTAssertEqual(layout.hit(at: CGPoint(x: 18, y: 5)), hit(1, 2))
        // À droite de la fin d'une ligne : la fin de cette ligne, pas le début de la suivante.
        XCTAssertEqual(layout.hit(at: CGPoint(x: 300, y: 5)), hit(4, 5))
        XCTAssertEqual(layout.hit(at: CGPoint(x: 300, y: 25)), hit(6, 7))
        XCTAssertEqual(layout.hit(at: CGPoint(x: -50, y: 25)), hit(5, 5))
        // Entre deux lignes : la plus proche.
        XCTAssertEqual(layout.hit(at: CGPoint(x: 5, y: 12)), hit(0, 0))
        XCTAssertEqual(layout.hit(at: CGPoint(x: 5, y: 18)), hit(5, 5))
        // Au-dessus de la première ligne et sous la dernière.
        XCTAssertEqual(layout.hit(at: CGPoint(x: 5, y: -40)), hit(0, 0))
        XCTAssertEqual(layout.hit(at: CGPoint(x: 500, y: 400)), hit(6, 7))
        XCTAssertNil(PageTextLayout().hit(at: .zero))
    }

    func testSuitLeSensDeLecture() {
        // Texte de droite à gauche : le premier caractère est le plus à droite.
        let layout = twoLines(rightToLeft: true)
        XCTAssertEqual(layout.hit(at: CGPoint(x: 48, y: 5)), hit(0, 0))
        XCTAssertEqual(layout.hit(at: CGPoint(x: 42, y: 5)), hit(0, 1))
        XCTAssertEqual(layout.hit(at: CGPoint(x: -20, y: 5)), hit(4, 5))
    }

    func testMotsEtLignes() {
        let layout = twoLines()
        XCTAssertEqual(layout.wordRange(containing: 3), 3..<5)
        XCTAssertEqual(layout.wordRange(containing: 0), 0..<2)
        XCTAssertEqual(layout.wordRange(containing: 2), 2..<3, "l'espace seule")
        XCTAssertEqual(layout.wordRange(containing: 6), 5..<7, "ne déborde pas sur la ligne précédente")
        XCTAssertEqual(layout.lineRange(containing: 4), 0..<5)
        XCTAssertEqual(layout.lineRange(containing: 5), 5..<7)

        let mixed = PageTextLayout([SelectableLine(
            text: "n°4521,",
            rects: (0..<7).map { PageRect(x: Float($0) * 10, y: 0, width: 10, height: 10) }
        )])
        XCTAssertEqual(mixed.wordRange(containing: 0), 0..<1)
        XCTAssertEqual(mixed.wordRange(containing: 1), 1..<2, "un signe est un mot à part")
        XCTAssertEqual(mixed.wordRange(containing: 4), 2..<6, "les chiffres ensemble")
    }

    func testRectanglesEtTexteDeLaSelection() {
        let layout = twoLines()
        // « b cd » puis « e » : un rectangle par ligne.
        XCTAssertEqual(layout.highlights(in: 1..<6), [
            CGRect(x: 10, y: 0, width: 40, height: 10),
            CGRect(x: 0, y: 20, width: 10, height: 10)
        ])
        XCTAssertEqual(layout.text(in: 1..<6), "b cd\ne")
        XCTAssertEqual(layout.text(in: 0..<7), "ab cd\nef")
        XCTAssertEqual(layout.text(in: 2..<2), "")
        XCTAssertTrue(layout.highlights(in: 3..<3).isEmpty)
    }

    func testDeveloppeLesLigatures() {
        let layout = PageTextLayout([SelectableLine(
            text: "\u{FB01}n \u{FB03}",
            rects: (0..<4).map { PageRect(x: Float($0) * 10, y: 0, width: 10, height: 10) }
        )])
        XCTAssertEqual(layout.text(in: 0..<4), "fin ffi")
    }

    func testRamenePointEcranSurLaPageDroite() {
        let size = CGSize(width: 200, height: 100)
        let rect = CGRect(x: 30, y: 10, width: 20, height: 8)
        for degrees in [0, 90, 180, 270] {
            let rotation = DisplayRotation(degrees: degrees)
            let shown = rotation.apply(to: rect, pageSize: size)
            let point = CGPoint(x: shown.midX, y: shown.midY)
            let straight = rotation.unapply(to: point, pageSize: size)
            XCTAssertEqual(straight.x, rect.midX, accuracy: 0.001, "\(degrees)°")
            XCTAssertEqual(straight.y, rect.midY, accuracy: 0.001, "\(degrees)°")
        }
    }

    // MARK: - Dans le canevas

    private func open(_ name: String) async throws -> DocumentViewModel {
        // Un fichier du bundle de test ne peut pas recevoir de signet de sécurité.
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"))
        let model = DocumentViewModel(reference: try DocumentReference(url: url, securityScoped: false))
        await model.open()
        XCTAssertEqual(model.phase, .ready, name)
        return model
    }

    private func show(_ model: DocumentViewModel) throws -> (NSWindow, PageCanvasView) {
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 1080, height: 760),
            styleMask: [.titled, .closable, .resizable, .fullSizeContentView], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: DocumentView(model: model))
        window.makeKeyAndOrderFront(nil)
        var canvas: PageCanvasView?
        wait { canvas = self.find(PageCanvasView.self, in: window.contentView); return canvas != nil }
        return (window, try XCTUnwrap(canvas))
    }

    private func find<T: NSView>(_ type: T.Type, in view: NSView?) -> T? {
        guard let view else { return nil }
        return view as? T ?? view.subviews.lazy.compactMap { self.find(type, in: $0) }.first
    }

    private func wait(upTo seconds: TimeInterval = 10, until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        }
    }

    /// Point de la vue des pages qui correspond à `point` sur la première page (page droite, non tournée).
    private func viewPoint(_ point: CGPoint, in canvas: PageCanvasView) -> CGPoint {
        let frame = canvas.layout.frames[0]
        return CGPoint(x: frame.minX + point.x, y: frame.minY + point.y)
    }

    /// Mot de la couche de texte, en points PDF (ordonnées vers le haut).
    private func word(_ text: String, x: Float, y: Float, width: Float, line: UInt32) -> LayerWordItem {
        LayerWordItem(
            text: text, quad: [x, y, x + width, y, x + width, y + 12, x, y + 12], confidence: 0.9, line: line
        )
    }

    func testSelectionneEtCopieLeTexteReconnuDUnScan() async throws {
        let model = try await open("scan_simule")
        let (window, canvas) = try show(model)
        defer {
            window.close()
            model.close()
        }
        let text = canvas.pagesView.text
        let page = try XCTUnwrap(model.pages.first)
        XCTAssertTrue(text.lineBounds(on: page).isEmpty, "une page scannée n'a pas de texte avant la lecture")
        text.mouseDown(at: viewPoint(CGPoint(x: 80, y: 136), in: canvas), clickCount: 1, extend: false)
        XCTAssertFalse(text.hasSelection)

        // Deux lignes reconnues : en haut de la page (A4, 842 points), 130 et 150 points depuis le haut.
        model.perform { session in
            session.applyRecognitions(results: [PageRecognition(
                page: page.id,
                words: [
                    self.word("Facture", x: 72, y: 700, width: 60, line: 0),
                    self.word("n°", x: 138, y: 700, width: 20, line: 0),
                    self.word("4521", x: 164, y: 700, width: 40, line: 0),
                    self.word("Merci", x: 72, y: 680, width: 40, line: 1),
                    self.word("beaucoup", x: 118, y: 680, width: 60, line: 1)
                ],
                quarterTurns: 0, engine: "essai", meanConfidence: 0.9, doubtfulWords: 0
            )])
        }
        let recognized = try XCTUnwrap(model.pages.first)
        XCTAssertNotEqual(recognized.textRevision, 0)
        wait { text.lineBounds(on: recognized).count == 2 }
        XCTAssertEqual(text.lineBounds(on: recognized).count, 2)

        // Du début de la première ligne à la fin de la seconde.
        text.mouseDown(at: viewPoint(CGPoint(x: 73, y: 136), in: canvas), clickCount: 1, extend: false)
        text.mouseDragged(to: viewPoint(CGPoint(x: 177, y: 156), in: canvas))
        text.mouseUp()
        XCTAssertTrue(text.hasSelection)
        let selected = await text.selectedText()
        XCTAssertEqual(selected, "Facture n° 4521\nMerci beaucoup")
        XCTAssertEqual(text.highlights(on: recognized).count, 2)

        // Double-clic : un mot ; triple-clic : la ligne.
        text.mouseDown(at: viewPoint(CGPoint(x: 90, y: 156), in: canvas), clickCount: 2, extend: false)
        let word = await text.selectedText()
        XCTAssertEqual(word, "Merci")
        text.mouseDown(at: viewPoint(CGPoint(x: 90, y: 156), in: canvas), clickCount: 3, extend: false)
        let line = await text.selectedText()
        XCTAssertEqual(line, "Merci beaucoup")

        // ⇧-clic étend la sélection jusqu'au point visé.
        text.mouseDown(at: viewPoint(CGPoint(x: 75, y: 136), in: canvas), clickCount: 1, extend: false)
        text.mouseDown(at: viewPoint(CGPoint(x: 150, y: 136), in: canvas), clickCount: 1, extend: true)
        let extended = await text.selectedText()
        XCTAssertEqual(extended, "Facture n")

        try await assertCopies(text, from: canvas.pagesView, expecting: "Facture n")

        // En mode Édition, le texte ne se sélectionne pas.
        model.workspace = .editing
        XCTAssertFalse(text.hasSelection)
        XCTAssertTrue(text.highlights(on: recognized).isEmpty)
    }

    func testSelectionneLeTexteNatifEtSuitLesModifications() async throws {
        let model = try await open("texte_simple")
        let (window, canvas) = try show(model)
        defer {
            window.close()
            model.close()
        }
        let text = canvas.pagesView.text
        let page = try XCTUnwrap(model.pages.first)
        wait { !text.lineBounds(on: page).isEmpty }
        let first = try XCTUnwrap(text.lineBounds(on: page).first)

        // Triple-clic : la première ligne de la page, telle que la lit le moteur.
        let start = viewPoint(CGPoint(x: first.minX + 2, y: first.midY), in: canvas)
        text.mouseDown(at: start, clickCount: 3, extend: false)
        let session = try XCTUnwrap(model.session)
        let expected = try XCTUnwrap(session.selectableText(page: page.id).first?.text)
        let selected = await text.selectedText()
        XCTAssertEqual(selected, expected)
        XCTAssertFalse(expected.isEmpty)

        // Rotation de la page : la sélection reste sur le même texte.
        model.rotate([0], clockwise: true)
        let turned = try XCTUnwrap(model.pages.first)
        XCTAssertEqual(turned.quarterTurns, 1)
        let again = await text.selectedText()
        XCTAssertEqual(again, expected)
        XCTAssertFalse(text.highlights(on: turned).isEmpty)
    }

    /// ⌘C (la commande Copier du menu Édition) place la sélection dans le presse-papiers.
    private func assertCopies(
        _ text: TextSelection, from view: PagesDocumentView, expecting expected: String
    ) async throws {
        let pasteboard = NSPasteboard.general
        let previous = pasteboard.string(forType: .string)
        defer {
            pasteboard.clearContents()
            if let previous {
                pasteboard.setString(previous, forType: .string)
            }
        }
        let item = NSMenuItem(title: "Copier", action: #selector(PagesDocumentView.copy(_:)), keyEquivalent: "c")
        XCTAssertTrue(view.validateMenuItem(item))
        view.copy(nil)
        wait { pasteboard.string(forType: .string) == expected }
        XCTAssertEqual(pasteboard.string(forType: .string), expected)

        text.clear()
        XCTAssertFalse(view.validateMenuItem(item), "Copier est grisé sans sélection")
    }
}
