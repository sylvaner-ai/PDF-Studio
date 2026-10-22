import XCTest
@testable import PDFStudio

/// Tests de la grille d'organisation et du retour à la lecture.
final class PageGridTests: XCTestCase {
    @MainActor
    func testLesVignettesGardentLesProportionsDeLaPage() {
        let size = PageGridItem.itemSize(thumbnailWidth: 150)
        XCTAssertEqual(size.width, 166)
        let view = PageGridItemView(frame: .zero)

        view.pageSize = CGSize(width: 595, height: 842)
        let portrait = view.pageFrame(in: size)
        XCTAssertEqual(portrait.width, 150)
        XCTAssertEqual(portrait.height, 212)

        view.pageSize = CGSize(width: 842, height: 595)
        let landscape = view.pageFrame(in: size)
        XCTAssertEqual(landscape.width, 150)
        XCTAssertLessThan(landscape.height, portrait.height)
        // Les pages d'une rangée reposent sur la même ligne, au-dessus de leur numéro.
        XCTAssertEqual(landscape.maxY, portrait.maxY)
    }

    @MainActor
    func testLeCanevasMasqueRetientLaPageDemandee() {
        let sizes = [Array(repeating: CGSize(width: 595, height: 842), count: 3)]
        let pages = (0..<3).map { index in
            DisplayPage(
                PageItem(
                    id: UInt64(index), kind: .source(source: 0, index: UInt32(index)), quarterTurns: 0,
                    editRevision: 0
                ),
                sizes: sizes
            )
        }
        let controller = CanvasController(pages: pages)
        controller.go(to: 2)
        XCTAssertEqual(controller.currentPage, 2)
        controller.go(to: 7)
        XCTAssertEqual(controller.currentPage, 2)
    }
}
