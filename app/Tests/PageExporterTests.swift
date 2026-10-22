import ImageIO
import XCTest
@testable import PDFStudio

/// Tests des règles d'exportation : découpage, noms de fichiers, images.
final class PageExporterTests: XCTestCase {
    func testDecoupeParNombreDePages() {
        XCTAssertEqual(PageExporter.parts(pageCount: 5, rule: .every(2)), [[0, 1], [2, 3], [4]])
        XCTAssertEqual(PageExporter.parts(pageCount: 3, rule: .every(1)), [[0], [1], [2]])
        XCTAssertEqual(PageExporter.parts(pageCount: 3, rule: .every(10)), [[0, 1, 2]])
        XCTAssertEqual(PageExporter.parts(pageCount: 0, rule: .every(1)), [])
    }

    func testDecoupeAvantLesPagesSelectionnees() {
        XCTAssertEqual(PageExporter.parts(pageCount: 6, rule: .before([4, 2])), [[0, 1], [2, 3], [4, 5]])
        // La première page, les doublons et les positions hors du document ne coupent rien.
        XCTAssertEqual(PageExporter.parts(pageCount: 4, rule: .before([0, 2, 2, 9])), [[0, 1], [2, 3]])
    }

    func testNumeroteLesFichiersDansLOrdre() {
        XCTAssertEqual(
            PageExporter.numberedNames(base: "Contrat", numbers: [1, 2], extension: "pdf"),
            ["Contrat – 1.pdf", "Contrat – 2.pdf"]
        )
        XCTAssertEqual(
            PageExporter.numberedNames(base: "Contrat", numbers: [3, 12], extension: "png"),
            ["Contrat – 03.png", "Contrat – 12.png"]
        )
    }

    func testLimiteLaTailleDesImages() {
        let portrait = CGSize(width: 595, height: 842)
        XCTAssertEqual(PageExporter.scale(forDPI: 144, pageSize: portrait), 2)
        let poster = CGSize(width: 5000, height: 7000)
        XCTAssertEqual(PageExporter.scale(forDPI: 300, pageSize: poster) * 7000, PageExporter.maxPixels, accuracy: 0.5)
        // L'image réduite enregistre sa résolution réelle : 10 000 px pour 7 000 pt.
        XCTAssertEqual(PageExporter.resolution(forDPI: 300, pageSize: poster), 103)
        XCTAssertEqual(PageExporter.resolution(forDPI: 150, pageSize: portrait), 150)
    }

    func testEncodeUneImageAvecSaResolution() throws {
        let image = try XCTUnwrap(PageExporter.blankImage(width: 40, height: 60))
        for format in PageExporter.ImageFormat.allCases {
            let data = try PageExporter.encode(image, as: format, dpi: 150)
            let source = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
            XCTAssertEqual(CGImageSourceGetType(source) as String?, format.type.identifier)
            let properties = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
            XCTAssertEqual(properties[kCGImagePropertyPixelWidth] as? Int, 40)
            XCTAssertEqual(properties[kCGImagePropertyPixelHeight] as? Int, 60)
            // Le PNG range la résolution en pixels par mètre : elle revient arrondie.
            let dpi = (properties[kCGImagePropertyDPIWidth] as? NSNumber)?.doubleValue ?? 0
            XCTAssertEqual(dpi, 150, accuracy: 0.5)
        }
    }

    func testRendUnePageEtUnePageVierge() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "organisation", withExtension: "pdf"))
        let document = try PdfDocument.open(path: url.path, password: nil)
        let session = EditSession(document: document)
        try session.rotate(positions: [0], quarterTurns: 1)
        _ = try session.insertBlank(at: 1, size: PageSize(width: 72, height: 144))
        let sizes = [document.pageSizes().map(\.cgSize)]
        let pages = session.pages().map { DisplayPage($0, sizes: sizes) }
        let sources = PageSources(document)

        // Page tournée d'un quart de tour : l'image est en paysage.
        let turned = try PageExporter.image(of: pages[0], dpi: 72, sources: sources)
        XCTAssertGreaterThan(turned.width, turned.height)
        let blank = try PageExporter.image(of: pages[1], dpi: 144, sources: sources)
        XCTAssertEqual(blank.width, 144)
        XCTAssertEqual(blank.height, 288)
    }
}
