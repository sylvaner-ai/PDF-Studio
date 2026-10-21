import CoreGraphics

/// Entrée du plan du document, sous forme d'arbre pour la barre latérale.
struct OutlineNode: Identifiable, Hashable, Sendable {
    let id: Int
    let title: String
    let page: Int?
    let top: CGFloat?
    /// `nil` pour une feuille, comme l'attend `List(_:children:)`.
    var children: [OutlineNode]?
}

/// Plan du document reconstruit à partir de la liste aplatie du moteur.
struct OutlineTree: Sendable {
    static let empty = OutlineTree(items: [])

    let roots: [OutlineNode]
    private let nodes: [Int: OutlineNode]

    var isEmpty: Bool { roots.isEmpty }

    init(items: [OutlineItem]) {
        var nodes: [Int: OutlineNode] = [:]
        // Pile des ancêtres de l'entrée courante : (indice dans `items`, niveau).
        var stack: [(index: Int, level: Int)] = []
        var children: [Int: [Int]] = [:]
        var rootIndices: [Int] = []
        for (index, item) in items.enumerated() {
            let level = Int(item.level)
            while let last = stack.last, last.level >= level {
                stack.removeLast()
            }
            if let parent = stack.last {
                children[parent.index, default: []].append(index)
            } else {
                rootIndices.append(index)
            }
            stack.append((index, level))
        }
        func build(_ index: Int) -> OutlineNode {
            let item = items[index]
            let kids = children[index]?.map(build)
            let node = OutlineNode(
                id: index,
                title: item.title.isEmpty ? String(localized: "Sans titre") : item.title,
                page: item.page.map(Int.init),
                top: item.top.map { CGFloat($0) },
                children: kids
            )
            nodes[index] = node
            return node
        }
        self.roots = rootIndices.map(build)
        self.nodes = nodes
    }

    func node(id: Int) -> OutlineNode? {
        nodes[id]
    }
}
