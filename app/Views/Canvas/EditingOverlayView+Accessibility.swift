import AppKit

// MARK: - VoiceOver

/// Objets d'une page pour VoiceOver, en mode Édition : chacun est un élément
/// d'accessibilité nommé d'après sa nature et son texte, dans l'ordre de
/// lecture ; l'activer (VO-Espace) le sélectionne (D48).
extension EditingOverlayView {
    override func accessibilityChildren() -> [Any]? {
        var children: [Any] = (super.accessibilityChildren() ?? []).filter { !($0 is PageObjectElement) }
        let objects = items.selectable.inReadingOrder(frame: frame(of:))
        var elements: [UInt32: PageObjectElement] = [:]
        for item in objects {
            let element = accessibilityObjects[item.id] ?? PageObjectElement(overlay: self, objectID: item.id)
            element.describe(item)
            elements[item.id] = element
            children.append(element)
        }
        accessibilityObjects = elements
        return children
    }

    override func accessibilitySelectedChildren() -> [Any]? {
        guard let editing, let page, editing.page == page.id else { return [] }
        return editing.selected.compactMap { accessibilityObjects[$0] }
    }

    /// Emprise d'un objet à l'écran, pour VoiceOver.
    func screenFrame(ofObject id: UInt32) -> CGRect {
        guard let window, let item = items.first(where: { $0.id == id }) else { return .zero }
        return window.convertToScreen(convert(frame(of: item), to: nil))
    }

    /// L'objet `id` est-il sélectionné ?
    func isObjectSelected(_ id: UInt32) -> Bool {
        guard let editing, let page else { return false }
        return editing.page == page.id && editing.selected.contains(id)
    }

    /// Sélection demandée par VoiceOver.
    func selectObject(_ id: UInt32) {
        guard let editing, let page else { return }
        editing.select([id], on: page.id)
    }

    /// Prévient VoiceOver que la sélection a changé.
    func selectionDidChangeForAccessibility() {
        NSAccessibility.post(element: self, notification: .selectedChildrenChanged)
    }
}

/// Un objet de la page, vu par VoiceOver.
final class PageObjectElement: NSAccessibilityElement {
    private weak var overlay: EditingOverlayView?
    let objectID: UInt32

    init(overlay: EditingOverlayView, objectID: UInt32) {
        self.overlay = overlay
        self.objectID = objectID
        super.init()
    }

    /// Nature, texte et état de l'objet.
    func describe(_ item: PageObjectItem) {
        switch item.kind {
        case .text:
            setAccessibilityRole(.staticText)
            setAccessibilityValue(item.text)
        case .image:
            setAccessibilityRole(.image)
        default:
            setAccessibilityRole(.group)
        }
        setAccessibilityLabel(item.accessibilityDescription)
        setAccessibilityHelp(item.movable
            ? String(localized: "Flèches pour déplacer, Suppr pour supprimer, Retour pour modifier le texte.")
            : String(localized: "Cet objet ne peut pas être modifié."))
    }

    // VoiceOver interroge l'élément sur le fil principal ; seules la vue (isolée
    // au fil principal) et le numéro de l'objet entrent dans les fermetures.

    override func accessibilityParent() -> Any? {
        overlay
    }

    override func accessibilityFrame() -> NSRect {
        let (overlay, id) = (overlay, objectID)
        return MainActor.assumeIsolated { overlay?.screenFrame(ofObject: id) ?? .zero }
    }

    override func isAccessibilitySelected() -> Bool {
        let (overlay, id) = (overlay, objectID)
        return MainActor.assumeIsolated { overlay?.isObjectSelected(id) ?? false }
    }

    override func accessibilityPerformPress() -> Bool {
        let (overlay, id) = (overlay, objectID)
        return MainActor.assumeIsolated {
            guard let overlay else { return false }
            overlay.selectObject(id)
            return true
        }
    }
}

extension PageObjectItem {
    /// Nature de l'objet, telle que l'affiche le bandeau du mode Édition.
    var kindName: String {
        if added {
            return kind == .image ? String(localized: "Image ajoutée") : String(localized: "Texte ajouté")
        }
        switch kind {
        case .text: return String(localized: "Bloc de texte")
        case .image: return String(localized: "Image")
        case .path: return String(localized: "Tracé")
        case .form: return String(localized: "Groupe d'objets")
        case .shading: return String(localized: "Dégradé")
        case .table: return String(localized: "Tableau")
        }
    }

    /// Description pour VoiceOver : nature et début du texte.
    var accessibilityDescription: String {
        guard let text = text?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else {
            if let grid {
                return String(localized: "\(kindName), \(grid.rows) lignes × \(grid.columns) colonnes")
            }
            return kindName
        }
        let excerpt = text.count > 80 ? String(text.prefix(80)) + "…" : text
        return String(localized: "\(kindName) : \(excerpt)")
    }
}

extension Array where Element == PageObjectItem {
    /// Objets qu'un clic peut atteindre : ni le fond de page, ni ce qui
    /// appartient à un tableau (le tableau les couvre).
    var selectable: [PageObjectItem] {
        let tables = Set(filter { $0.kind == .table }.map(\.id))
        return filter { item in
            !item.background && !(item.table.map { tables.contains($0) } ?? false)
        }
    }

    /// Ordre de lecture à l'écran : de haut en bas, puis de gauche à droite
    /// pour des objets qui commencent à la même hauteur.
    func inReadingOrder(frame: (PageObjectItem) -> CGRect) -> [PageObjectItem] {
        let frames = Dictionary(map { ($0.id, frame($0)) }, uniquingKeysWith: { first, _ in first })
        return sorted { lhs, rhs in
            let (left, right) = (frames[lhs.id] ?? .zero, frames[rhs.id] ?? .zero)
            return (left.minY.rounded(), left.minX, lhs.id) < (right.minY.rounded(), right.minX, rhs.id)
        }
    }
}
