import AppKit

/// Touche de lecture du canevas.
enum ReadingKey: Equatable {
    /// Début ou fin du document : ⌘↑, ⌘↓, Début, Fin.
    case documentEdge(end: Bool)
    /// Un écran plus bas ou plus haut : espace, Maj-espace, page suivante ou précédente.
    case screen(down: Bool)
    /// Un pas plus bas ou plus haut : flèches.
    case line(down: Bool)
    /// Flèche droite ou gauche.
    case side(right: Bool)

    @MainActor
    init?(_ event: NSEvent) {
        let flags = event.modifierFlags
        switch (event.keyCode, flags.contains(.command)) {
        case (125, true), (119, _): self = .documentEdge(end: true)
        case (126, true), (115, _): self = .documentEdge(end: false)
        case (49, _): self = .screen(down: !flags.contains(.shift))
        case (121, _): self = .screen(down: true)
        case (116, _): self = .screen(down: false)
        case (125, false): self = .line(down: true)
        case (126, false): self = .line(down: false)
        case (124, _): self = .side(right: true)
        case (123, _): self = .side(right: false)
        default: return nil
        }
    }
}

extension Zoom {
    /// Sens du zoom demandé au clavier : ⌘= ou ⌘+ agrandit (le « + » demande
    /// Majuscule sur un clavier AZERTY), ⌘− réduit.
    @MainActor
    static func direction(for event: NSEvent) -> Int? {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.subtracting(.shift) == .command else { return nil }
        switch event.charactersIgnoringModifiers {
        case "=", "+": return 1
        case "-": return -1
        default: return nil
        }
    }
}
