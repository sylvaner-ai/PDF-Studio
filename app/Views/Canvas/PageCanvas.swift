import SwiftUI

/// Canevas AppKit des pages, intégré à SwiftUI. Les pages lui parviennent par
/// le ``CanvasController``, y compris après une réorganisation.
struct PageCanvas: NSViewRepresentable {
    let renderer: PageRenderer
    let controller: CanvasController

    func makeNSView(context: Context) -> PageCanvasView {
        let canvas = PageCanvasView(pages: controller.pages, renderer: renderer)
        controller.attach(canvas)
        return canvas
    }

    func updateNSView(_ canvas: PageCanvasView, context: Context) {}
}
