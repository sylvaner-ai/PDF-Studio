import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import PDFStudio

/// Textes et images ajoutés aux pages (tests/fixtures/edition.pdf) : ajout,
/// style, cadre d'une image, lecture d'un fichier image, récupération.
final class AddingTests: XCTestCase {
    private var directory: URL!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("ajout-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: directory)
    }

    private func open() throws -> PdfDocument {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "edition", withExtension: "pdf"))
        return try PdfDocument.open(path: url.path, password: nil)
    }

    /// PNG de `width` × `height` pixels, rouge, à 144 points par pouce.
    private func png(width: Int, height: Int) throws -> Data {
        let space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        let context = try XCTUnwrap(CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        context.setFillColor(CGColor(srgbRed: 0.9, green: 0, blue: 0, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        let image = try XCTUnwrap(context.makeImage())
        let output = NSMutableData()
        let destination = try XCTUnwrap(
            CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil)
        )
        let properties: [CFString: Any] = [kCGImagePropertyDPIWidth: 144, kCGImagePropertyDPIHeight: 144]
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return output as Data
    }

    @MainActor
    func testPoliceChoisieDansLePanneau() {
        let times = AddedTextFont(.standard(name: "Times-BoldItalic"))
        XCTAssertEqual(times, AddedTextFont(family: "Times", bold: true, italic: true))
        XCTAssertEqual(times.engine, .standard(name: "Times-BoldItalic"))
        XCTAssertEqual(AddedTextFont(family: "Courier", italic: true).engine, .standard(name: "Courier-Oblique"))
        XCTAssertEqual(AddedTextFont(family: "Helvetica").engine, .standard(name: "Helvetica"))
        XCTAssertEqual(AddedTextFont(.original), AddedTextFont(family: "Helvetica"))
        XCTAssertFalse(AddedTextFont.installedFamilies.contains("Helvetica"))
        XCTAssertEqual(AddedTextFont(family: "Times").nsFont(size: 14).pointSize, 14)
    }

    @MainActor
    func testCadreDUneImageAjoutee() {
        let source = DisplayPage.Source(document: 0, index: 0)
        let page = DisplayPage(id: 1, source: source, quarterTurns: 0, baseSize: CGSize(width: 600, height: 800))
        // Réduite à la moitié de la largeur, centrée sur la page.
        let bounds = PageEditing.imageBounds(CGSize(width: 2000, height: 1000), on: page, center: nil)
        XCTAssertEqual(bounds, PageRect(x: 150, y: 325, width: 300, height: 150))
        // Petite : à sa taille, gardée sur la page.
        let corner = PageEditing.imageBounds(CGSize(width: 40, height: 20), on: page, center: CGPoint(x: 5, y: 790))
        XCTAssertEqual(corner, PageRect(x: 0, y: 780, width: 40, height: 20))
        // Page couchée par l'organisation : les côtés s'échangent sur la page droite.
        let turned = DisplayPage(id: 2, source: source, quarterTurns: 1, baseSize: CGSize(width: 600, height: 800))
        let upright = PageEditing.imageBounds(CGSize(width: 2000, height: 1000), on: turned, center: nil)
        XCTAssertEqual(upright, PageRect(x: 200, y: 200, width: 200, height: 400))
    }

    func testLitUnFichierImage() throws {
        let url = directory.appendingPathComponent("rouge.png")
        let data = try png(width: 80, height: 40)
        try data.write(to: url)
        let file = try XCTUnwrap(AddedImageFile.load(url))
        // Gardée telle quelle ; 144 points par pouce : deux pixels par point.
        XCTAssertEqual(file.data, data)
        XCTAssertEqual(file.size, CGSize(width: 40, height: 20))

        let text = directory.appendingPathComponent("faux.png")
        try Data("pas une image".utf8).write(to: text)
        XCTAssertNil(AddedImageFile.load(text))
    }

    func testAjouteRetientEtRestaureTexteEtImage() throws {
        let session = EditSession(document: try open())
        let page = try XCTUnwrap(session.pages().first).id
        let style = AddedTextStyle(font: .standard(name: "Times-Bold"), size: 18, red: 0.8, green: 0, blue: 0)
        let text = try session.addText(page: page, x: 72, y: 600, text: "Ajouté", style: style, blankSource: nil)
        XCTAssertTrue(text.added)
        XCTAssertEqual(text.addedText, style)
        XCTAssertEqual(session.undoName(), "l'ajout du texte")
        let bounds = PageRect(x: 300, y: 600, width: 40, height: 20)
        let image = try session.addImage(
            page: page, data: try png(width: 80, height: 40), bounds: bounds, blankSource: nil
        )
        XCTAssertEqual(image.kind, .image)
        XCTAssertThrowsError(
            try session.addImage(page: page, data: Data("rien".utf8), bounds: bounds, blankSource: nil)
        )

        // Retenus : l'image est rangée à part, une seule fois.
        let store = try RecoveryStore(directory: directory.appendingPathComponent("récupération"))
        let sizes = [try open().pageSizes().map(\.cgSize)]
        let pages = try session.pages().map { item in
            RecoveryRecord.Page(
                DisplayPage(item, sizes: sizes), edits: try session.contentRecord(page: item.id), store: store
            )
        }
        let file = directory.appendingPathComponent("empreinte")
        try Data("contenu".utf8).write(to: file)
        let record = RecoveryRecord(
            path: "/Documents/ajout.pdf", stamp: try FileStamp(of: file), basePath: nil, sources: [],
            pages: pages, date: Date()
        )
        XCTAssertEqual(record.imagePaths.count, 1)
        let decoded = try JSONDecoder().decode(RecoveryRecord.self, from: JSONEncoder().encode(record))
        XCTAssertEqual(decoded, record)

        let restored = EditSession(document: try open())
        try restored.restore(
            pages: decoded.pages.map(\.item), contents: decoded.pages.map(\.contentItems),
            layers: decoded.pages.map(\.layerItems)
        )
        let restoredPage = try XCTUnwrap(restored.pages().first).id
        let added = try restored.pageObjects(page: restoredPage).filter(\.added)
        XCTAssertEqual(added.map(\.kind), [.text, .image])
        XCTAssertEqual(added.first?.text, "Ajouté")
        XCTAssertEqual(added.first?.addedText, style)

        // Les modifications abandonnées, l'image rangée disparaît.
        store.save(record)
        let imagePath = try XCTUnwrap(record.imagePaths.first)
        store.removeUnusedImages()
        XCTAssertTrue(FileManager.default.fileExists(atPath: imagePath))
        store.discard(record)
        XCTAssertFalse(FileManager.default.fileExists(atPath: imagePath))
    }

    @MainActor
    func testAjouteSurUnePageVierge() throws {
        let session = EditSession(document: try open())
        let position = try session.insertBlank(at: 1, size: PageSize(width: 300, height: 400))
        let page = session.pages()[Int(position)].id
        let blank = try PdfDocument.fromData(data: try session.blankPageDocument(page: page), password: nil)
        let source = session.addSource(document: blank)
        _ = try session.addText(
            page: page, x: 20, y: 20, text: "Note", style: PageEditing.defaultTextStyle, blankSource: source
        )
        let saved = try PdfDocument.fromData(data: try session.exportPdf().bytes, password: nil)
        XCTAssertTrue(try saved.plainText(index: 1).contains("Note"))
    }
}
