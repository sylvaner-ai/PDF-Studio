import AppKit
import SwiftUI

/// Présentation des pages d'un document.
enum Workspace: Hashable {
    /// Lecture dans le canevas.
    case reading
    /// Grille de vignettes, pour organiser les pages.
    case organizing
    /// Modification du contenu des pages dans le canevas.
    case editing
}

/// Grille des vignettes de toutes les pages, pour les organiser.
///
/// Sélection au clic, ⌘-clic, ⇧-clic, au rectangle, par ⌘A et aux flèches ;
/// glisser-déposer pour déplacer les pages, ou les copier dans la grille d'une
/// autre fenêtre ; dépôt d'un PDF depuis le Finder pour en insérer les pages ;
/// touche d'effacement pour supprimer ; menu contextuel. Un double-clic ou la
/// touche Entrée ouvre la page en lecture.
struct PageGrid: NSViewRepresentable {
    let model: DocumentViewModel
    let renderer: PageRenderer
    // Lus ici, dans le corps de la vue, pour que SwiftUI suive leurs changements.
    let pages: [DisplayPage]
    let selection: Set<UInt64>
    let display: DisplayRotation
    let thumbnailWidth: CGFloat

    func makeCoordinator() -> PageGridCoordinator {
        PageGridCoordinator(model: model, renderer: renderer)
    }

    func makeNSView(context: Context) -> NSScrollView {
        context.coordinator.makeScrollView()
    }

    func updateNSView(_ view: NSScrollView, context: Context) {
        context.coordinator.update(
            pages: pages, selection: selection, display: display, thumbnailWidth: thumbnailWidth
        )
    }
}

// MARK: - Taille des vignettes et lecture

extension DocumentViewModel {
    static let gridWidths: ClosedRange<CGFloat> = 96...384

    func zoomGrid(in larger: Bool) {
        let width = gridThumbnailWidth * (larger ? 1.25 : 0.8)
        gridThumbnailWidth = min(max(width.rounded(), Self.gridWidths.lowerBound), Self.gridWidths.upperBound)
    }

    /// Ouvre une page en lecture.
    func read(page position: Int) {
        guard pages.indices.contains(position) else { return }
        selection = [pages[position].id]
        canvas.go(to: position)
        workspace = .reading
    }
}

/// Vue AppKit de la grille : la sélection, le menu contextuel, les touches et
/// le double-clic sont confiés au coordinateur.
final class PageGridCollectionView: NSCollectionView {
    weak var coordinator: PageGridCoordinator?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        Task { @MainActor in window.makeFirstResponder(self) }
    }

    override func layout() {
        super.layout()
        // La grille a sa taille : la sélection peut être montrée, hors de la mise en page.
        if let coordinator, coordinator.revealsSelection {
            Task { @MainActor in coordinator.revealSelectionIfPossible() }
        }
    }

    override func keyDown(with event: NSEvent) {
        switch event.specialKey {
        case .delete?, .deleteForward?:
            coordinator?.deleteSelection()
        case .carriageReturn?, .enter?:
            coordinator?.readSelection()
        default:
            super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        guard event.clickCount == 2,
              let indexPath = indexPathForItem(at: convert(event.locationInWindow, from: nil)) else { return }
        coordinator?.read(position: indexPath.item)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard let coordinator else { return nil }
        // Un clic droit hors de la sélection désigne la seule page cliquée.
        if let indexPath = indexPathForItem(at: convert(event.locationInWindow, from: nil)),
           !selectionIndexPaths.contains(indexPath) {
            selectionIndexPaths = [indexPath]
            coordinator.selectionDidChange()
        }
        return coordinator.contextMenu()
    }
}
