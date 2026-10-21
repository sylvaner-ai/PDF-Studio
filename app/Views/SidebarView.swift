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
                OutlineList(model: model)
            } else {
                ThumbnailList(model: model, renderer: renderer)
            }
        }
    }
}

/// Vignettes de toutes les pages, à sélectionner et réorganiser.
///
/// Un clic sur une vignette, puis les flèches haut et bas, passent d'une page à
/// l'autre ; en retour, la sélection suit la page lue dans le canevas, sauf si
/// plusieurs pages sont sélectionnées (⌘-clic, ⇧-clic, ⌘A). Les vignettes se
/// déplacent par glisser-déposer ; la touche d'effacement supprime la sélection.
struct ThumbnailList: View {
    @Bindable var model: DocumentViewModel
    let renderer: PageRenderer

    var body: some View {
        let controller = model.canvas
        ScrollViewReader { proxy in
            List(selection: $model.selection) {
                ForEach(Array(model.pages.enumerated()), id: \.element.id) { position, page in
                    ThumbnailCell(
                        position: position, page: page, display: controller.rotation, renderer: renderer
                    )
                }
                .onMove { positions, before in
                    model.move(positions, before: before)
                }
            }
            .listStyle(.sidebar)
            .contextMenu(forSelectionType: UInt64.self) { ids in
                PageActions(model: model, positions: model.positions(of: ids))
            }
            .onDeleteCommand {
                model.delete(model.targetPositions)
            }
            .onChange(of: model.selection) { _, selection in
                guard selection.count == 1, let id = selection.first,
                      let position = model.pages.firstIndex(where: { $0.id == id }),
                      position != controller.currentPage else { return }
                controller.go(to: position)
            }
            .onChange(of: controller.currentPage) { _, page in
                guard let id = model.pages[safe: page]?.id else { return }
                if model.selection.count <= 1 {
                    model.selection = [id]
                }
                proxy.scrollTo(id)
            }
            .onAppear {
                guard let id = model.pages[safe: controller.currentPage]?.id else { return }
                proxy.scrollTo(id)
            }
        }
    }
}

/// Une vignette et son numéro de page.
struct ThumbnailCell: View {
    let position: Int
    let page: DisplayPage
    let display: DisplayRotation
    let renderer: PageRenderer

    @Environment(\.displayScale) private var displayScale
    @State private var image: PageImage?

    private static let width: CGFloat = 104

    var body: some View {
        let size = page.size(display)
        let height = (size.height * Self.width / max(size.width, 1)).rounded()
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
            Text("\(position + 1)")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 4)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Page \(position + 1)"))
        .task(id: RenderRequest(page: page, display: display, pixelWidth: pixelWidth)) {
            if let rendered = try? await renderer.image(of: page, pixelWidth: pixelWidth, display: display) {
                image = rendered
            } else if page.isBlank {
                image = nil
            }
        }
    }

    private var pixelWidth: Int {
        Int((Self.width * displayScale).rounded(.up))
    }

    /// Ce qui détermine l'image : la relancer quand la page tourne.
    private struct RenderRequest: Equatable {
        let page: DisplayPage
        let display: DisplayRotation
        let pixelWidth: Int
    }
}

/// Actions sur des pages, pour le menu contextuel des vignettes.
struct PageActions: View {
    let model: DocumentViewModel
    let positions: [Int]

    var body: some View {
        Button("Faire pivoter à gauche") { model.rotate(positions, clockwise: false) }
            .disabled(positions.isEmpty)
        Button("Faire pivoter à droite") { model.rotate(positions, clockwise: true) }
            .disabled(positions.isEmpty)
        Divider()
        Button("Dupliquer") { model.duplicate(positions) }
            .disabled(positions.isEmpty)
        Button("Supprimer") { model.delete(positions) }
            .disabled(!model.canDelete(positions))
        Divider()
        Button("Insérer une page vierge") { model.insertBlankPage(before: positions.last.map { $0 + 1 }) }
            .disabled(!model.canEdit)
        Button("Insérer des pages d'un fichier…") {
            model.chooseDocumentToInsert(before: positions.last.map { $0 + 1 })
        }
        .disabled(!model.canEdit)
    }
}

/// Plan du document ; un clic mène à la page visée, à sa position actuelle.
/// Un signet dont la page a été supprimée apparaît en gris.
struct OutlineList: View {
    let model: DocumentViewModel

    @State private var selection: OutlineNode.ID?

    var body: some View {
        let tree = model.outline
        List(tree.roots, children: \.children, selection: $selection) { node in
            Text(node.title)
                .lineLimit(2)
                .foregroundStyle(isAvailable(node) ? .primary : .secondary)
                .help(node.title)
        }
        .listStyle(.sidebar)
        .onChange(of: selection) { _, id in
            guard let id, let node = tree.node(id: id) else { return }
            model.go(to: node)
        }
    }

    private func isAvailable(_ node: OutlineNode) -> Bool {
        node.page.flatMap(model.position(ofOriginalPage:)) != nil
    }
}
