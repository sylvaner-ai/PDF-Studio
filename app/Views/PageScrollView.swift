import SwiftUI

/// Pages du document en défilement continu, ajustées à la largeur de la fenêtre.
struct PageScrollView: View {
    let pageSizes: [CGSize]
    let renderer: PageRenderer

    /// Marge autour des pages et espace entre elles (grille de 4 pt).
    private let margin: CGFloat = 32
    private let spacing: CGFloat = 16
    /// Largeur maximale d'affichage, pour garder une lecture confortable sur grand écran.
    private let maxPageWidth: CGFloat = 1_100

    var body: some View {
        GeometryReader { geometry in
            let zoom = zoomFactor(for: geometry.size.width)
            ScrollView(.vertical) {
                LazyVStack(spacing: spacing) {
                    ForEach(pageSizes.indices, id: \.self) { index in
                        PageView(
                            index: index,
                            pageCount: pageSizes.count,
                            pageSize: pageSizes[index],
                            zoom: zoom,
                            renderer: renderer
                        )
                    }
                }
                .padding(.vertical, margin)
                .frame(maxWidth: .infinity)
            }
        }
    }

    /// Facteur commun à toutes les pages : la plus large occupe la largeur disponible.
    private func zoomFactor(for containerWidth: CGFloat) -> CGFloat {
        let widest = pageSizes.map(\.width).max() ?? 1
        let available = min(max(containerWidth - 2 * margin, 120), maxPageWidth)
        return available / widest
    }
}

/// Une page : fond blanc et ombre douce immédiatement, image dès qu'elle est rendue.
struct PageView: View {
    let index: Int
    let pageCount: Int
    let pageSize: CGSize
    let zoom: CGFloat
    let renderer: PageRenderer

    @Environment(\.displayScale) private var displayScale
    @State private var image: PageImage?

    /// Pas d'arrondi de la largeur rendue, pour ne pas relancer un rendu à
    /// chaque pixel de redimensionnement de la fenêtre.
    private static let pixelStep = 32

    var body: some View {
        let displaySize = CGSize(width: pageSize.width * zoom, height: pageSize.height * zoom)
        ZStack {
            Color.white
            if let image {
                Image(decorative: image.cgImage, scale: 1)
                    .resizable()
                    .interpolation(.high)
            }
        }
        .frame(width: displaySize.width, height: displaySize.height)
        .clipShape(RoundedRectangle(cornerRadius: 2, style: .continuous))
        .shadow(color: .black.opacity(0.12), radius: 6, y: 2)
        .accessibilityElement()
        .accessibilityLabel(Text("Page \(index + 1) sur \(pageCount)"))
        .task(id: pixelWidth(for: displaySize.width)) {
            let width = pixelWidth(for: displaySize.width)
            if let rendered = try? await renderer.image(page: index, pixelWidth: width, pageWidth: pageSize.width) {
                image = rendered
            }
        }
    }

    private func pixelWidth(for displayWidth: CGFloat) -> Int {
        let exact = Int((displayWidth * displayScale).rounded())
        let step = Self.pixelStep
        return max(step, (exact + step - 1) / step * step)
    }
}
