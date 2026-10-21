import XCTest
@testable import PDFStudio

/// Tests du pont Swift ↔ Rust sur le jeu de PDF de référence (tests/fixtures).
final class EngineBridgeTests: XCTestCase {
    private func fixture(_ name: String) throws -> String {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "pdf"),
            "fixture \(name).pdf absente du bundle de test"
        )
        return url.path
    }

    func testOuvreEtRendLaPremierePage() throws {
        let document = try PdfDocument.open(path: fixture("texte_simple"), password: nil)
        XCTAssertEqual(document.pageCount(), 1)
        XCTAssertEqual(document.title(), "Texte simple")

        let page = try document.renderPage(index: 0, scale: 2)
        XCTAssertEqual(page.width, 1191)
        XCTAssertEqual(page.height, 1684)
        let image = try XCTUnwrap(page.makeCGImage())
        XCTAssertEqual(image.width, 1191)
        XCTAssertEqual(image.height, 1684)
    }

    func testDemandeLeMotDePasse() throws {
        let path = try fixture("chiffre")
        XCTAssertThrowsError(try PdfDocument.open(path: path, password: nil)) { error in
            guard case .PasswordRequired = error as? EngineError else {
                return XCTFail("PasswordRequired attendu, obtenu \(error)")
            }
        }
        XCTAssertThrowsError(try PdfDocument.open(path: path, password: "mauvais")) { error in
            guard case .WrongPassword = error as? EngineError else {
                return XCTFail("WrongPassword attendu, obtenu \(error)")
            }
        }
        XCTAssertEqual(try PdfDocument.open(path: path, password: "motdepasse").pageCount(), 1)
    }

    func testMessageDErreurEnFrancais() throws {
        XCTAssertThrowsError(try PdfDocument.open(path: fixture("pas_un_pdf"), password: nil)) { error in
            let message = (error as? EngineError)?.userMessage ?? ""
            XCTAssertTrue(message.hasPrefix("Ce fichier n'est pas un PDF valide"), message)
        }
    }

    /// Cahier des charges §6 : ouverture d'un PDF de 100 pages en moins de 2 s.
    func testOuvreCentPagesEnMoinsDeDeuxSecondes() throws {
        let path = try fixture("multipage_100")
        let start = Date()
        let document = try PdfDocument.open(path: path, password: nil)
        let elapsed = Date().timeIntervalSince(start)
        XCTAssertEqual(document.pageCount(), 100)
        XCTAssertLessThan(elapsed, 2.0)
    }

    func testLeRenduMetEnCache() async throws {
        let document = try PdfDocument.open(path: fixture("multipage_100"), password: nil)
        let renderer = PageRenderer(document: document)
        let first = try await renderer.image(page: 3, pixelWidth: 640, pageWidth: 595.28)
        let second = try await renderer.image(page: 3, pixelWidth: 640, pageWidth: 595.28)
        XCTAssertLessThanOrEqual(abs(first.cgImage.width - 640), 1)
        XCTAssertTrue(first.cgImage === second.cgImage, "la deuxième demande doit venir du cache")
    }
}
