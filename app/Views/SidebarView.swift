import SwiftUI

/// Barre latérale : vignettes des pages et, s'il existe, plan du document.
struct SidebarView: View {
    let model: DocumentViewModel
    let renderer: PageRenderer

    private enum Panel: Hashable {
        case thumbnails
        case outline
    }

    @State private var panel = Panel.thumbnails

    var body: some View {
        VStack(spacing: 0) {
            if !model.outline.isEmpty {
                Picker("Contenu de la barre latérale", selection: $panel) {
                    Text("Vignettes").tag(Panel.thumbnails)
                    Text("Signets").tag(Panel.outline)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
            if panel == .outline, !model.outline.isEmpty {
                OutlineList(tree: model.outline, controller: model.canvas)
            } else {
                ThumbnailList(pageSizes: model.pageSizes, renderer: renderer, controller: model.canvas)
            }
        }
    }
}

/// Vignettes de toutes les pages ; la page lue est entourée de la couleur d'accent.
struct ThumbnailList: View {
    let pageSizes: [CGSize]
    let renderer: PageRenderer
    let controller: CanvasController

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                LazyVStack(spacing: 12) {
                    ForEach(pageSizes.indices, id: \.self) { index in
                        Button {
                            controller.go(to: index)
                        } label: {
                            ThumbnailCell(
                                index: index,
                                pageSize: controller.rotation.apply(to: pageSizes[index]),
                                rotation: controller.rotation,
                                isCurrent: index == controller.currentPage,
                                renderer: renderer
                            )
                        }
                        .buttonStyle(.plain)
                        .id(index)
                    }
                }
                .padding(12)
            }
            .onChange(of: controller.currentPage) { _, page in
                proxy.scrollTo(page)
            }
            .onAppear {
                proxy.scrollTo(controller.currentPage)
            }
        }
    }
}

/// Une vignette et son numéro de page.
struct ThumbnailCell: View {
    let index: Int
    /// Taille de la page tournée, en points.
    let pageSize: CGSize
    let rotation: DisplayRotation
    let isCurrent: Bool
    let renderer: PageRenderer

    @Environment(\.displayScale) private var displayScale
    @State private var image: PageImage?

    private static let width: CGFloat = 104

    var body: some View {
        let height = (pageSize.height * Self.width / max(pageSize.width, 1)).rounded()
        VStack(spacing: 4) {
            ZStack {
                Color.white
                if let image {
                    Image(decorative: image.cgImage, scale: 1)
                        .resizable()
                        .interpolation(.high)
                }
            }
            .frame(width: Self.width, height: height)
            .shadow(color: .black.opacity(0.12), radius: 2, y: 1)
            .overlay {
                Rectangle()
                    .strokeBorder(Color.accentColor, lineWidth: isCurrent ? 3 : 0)
                    .padding(-4)
            }
            Text("\(index + 1)")
                .font(.caption)
                .foregroundStyle(isCurrent ? .primary : .secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Page \(index + 1)"))
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
        .task(id: "\(rotation.degrees)-\(pixelWidth)") {
            if let rendered = try? await renderer.image(
                page: index, pixelWidth: pixelWidth, pageWidth: pageSize.width, rotation: rotation
            ) {
                image = rendered
            }
        }
    }

    private var pixelWidth: Int {
        Int((Self.width * displayScale).rounded(.up))
    }
}

/// Plan du document ; un clic mène à la page visée.
struct OutlineList: View {
    let tree: OutlineTree
    let controller: CanvasController

    @State private var selection: OutlineNode.ID?

    var body: some View {
        List(tree.roots, children: \.children, selection: $selection) { node in
            Text(node.title)
                .lineLimit(2)
                .foregroundStyle(node.page == nil ? .secondary : .primary)
                .help(node.title)
        }
        .listStyle(.sidebar)
        .onChange(of: selection) { _, id in
            guard let id, let node = tree.node(id: id), let page = node.page else { return }
            controller.go(to: page, top: node.top)
        }
    }
}
