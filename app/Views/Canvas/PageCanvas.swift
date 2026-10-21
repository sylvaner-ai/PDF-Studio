import SwiftUI

/// Canevas AppKit des pages, intégré à SwiftUI.
struct PageCanvas: NSViewRepresentable {
    let pageSizes: [CGSize]
    let renderer: PageRenderer
    let controller: CanvasController

    func makeNSView(context: Context) -> PageCanvasView {
        let canvas = PageCanvasView(pageSizes: pageSizes, renderer: renderer)
        controller.attach(canvas)
        return canvas
    }

    func updateNSView(_ canvas: PageCanvasView, context: Context) {}
}
