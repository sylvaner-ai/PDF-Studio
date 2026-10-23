import CoreGraphics

/// Mode d'affichage des pages.
enum DisplayMode: String, CaseIterable, Sendable {
    /// Toutes les pages les unes sous les autres.
    case continuous
    /// Une seule page à la fois.
    case singlePage
}

/// Rotation d'une page à l'écran, en degrés dans le sens horaire : 0, 90, 180
/// ou 270.
///
/// Elle combine la rotation d'affichage, qui ne modifie pas le fichier, et les
/// quarts de tour donnés aux pages par l'organisation (cf. ``DisplayPage``).
struct DisplayRotation: Equatable, Hashable, Sendable {
    private(set) var degrees: Int

    init(degrees: Int = 0) {
        let quarter = Int((Double(degrees) / 90).rounded())
        self.degrees = ((quarter % 4) + 4) % 4 * 90
    }

    var swapsAxes: Bool { degrees == 90 || degrees == 270 }

    func rotated(by delta: Int) -> DisplayRotation {
        DisplayRotation(degrees: degrees + delta)
    }

    /// Taille d'une page une fois tournée.
    func apply(to size: CGSize) -> CGSize {
        swapsAxes ? CGSize(width: size.height, height: size.width) : size
    }

    /// Convertit un rectangle exprimé sur la page droite (points, origine en haut
    /// à gauche, comme le moteur) en rectangle sur la page tournée.
    func apply(to rect: CGRect, pageSize size: CGSize) -> CGRect {
        switch degrees {
        case 90:
            return CGRect(x: size.height - rect.maxY, y: rect.minX, width: rect.height, height: rect.width)
        case 180:
            return CGRect(
                x: size.width - rect.maxX, y: size.height - rect.maxY,
                width: rect.width, height: rect.height
            )
        case 270:
            return CGRect(x: rect.minY, y: size.width - rect.maxX, width: rect.height, height: rect.width)
        default:
            return rect
        }
    }

    /// Convertit un point de la page tournée en point de la page droite
    /// (l'inverse de `apply(to:pageSize:)`, pour un point).
    func unapply(to point: CGPoint, pageSize size: CGSize) -> CGPoint {
        switch degrees {
        case 90: return CGPoint(x: point.y, y: size.height - point.x)
        case 180: return CGPoint(x: size.width - point.x, y: size.height - point.y)
        case 270: return CGPoint(x: size.width - point.y, y: point.x)
        default: return point
        }
    }

    /// Convertit un déplacement sur la page tournée (vers la droite et vers le
    /// bas) en déplacement sur la page droite.
    func unapply(to vector: CGVector) -> CGVector {
        switch degrees {
        case 90: return CGVector(dx: vector.dy, dy: -vector.dx)
        case 180: return CGVector(dx: -vector.dx, dy: -vector.dy)
        case 270: return CGVector(dx: -vector.dy, dy: vector.dx)
        default: return vector
        }
    }

    /// Ordonnée, sur la page tournée, d'une position verticale de la page droite
    /// (destination d'un signet). Sans objet si la page est couchée.
    func applyTop(_ top: CGFloat, pageSize size: CGSize) -> CGFloat? {
        switch degrees {
        case 0: return top
        case 180: return size.height - top
        default: return nil
        }
    }
}

/// Disposition des pages dans le canevas, en points à 100 %.
///
/// Les pages sont centrées horizontalement, séparées par `spacing` et entourées
/// de `margin` (multiples de 4 pt).
struct PageLayout: Equatable, Sendable {
    static let margin: CGFloat = 32
    static let spacing: CGFloat = 16

    /// Cadre de chaque page affichée ; vide pour une page masquée (mode page par page).
    let frames: [CGRect]
    let documentSize: CGSize

    /// - Parameters:
    ///   - sizes: dimensions des pages telles qu'affichées (tournées), en points.
    ///   - visiblePage: en mode page par page, la seule page disposée.
    init(sizes: [CGSize], mode: DisplayMode, visiblePage: Int = 0) {
        let margin = Self.margin
        switch mode {
        case .continuous:
            let width = (sizes.map(\.width).max() ?? 0) + 2 * margin
            var y = margin
            var frames: [CGRect] = []
            frames.reserveCapacity(sizes.count)
            for size in sizes {
                let x = ((width - size.width) / 2).rounded()
                frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
                y += size.height + Self.spacing
            }
            self.frames = frames
            documentSize = CGSize(width: width, height: sizes.isEmpty ? 2 * margin : y - Self.spacing + margin)
        case .singlePage:
            let index = sizes.indices.contains(visiblePage) ? visiblePage : 0
            let size = sizes.isEmpty ? .zero : sizes[index]
            let frame = CGRect(origin: CGPoint(x: margin, y: margin), size: size)
            frames = sizes.indices.map { $0 == index ? frame : .zero }
            documentSize = CGSize(width: size.width + 2 * margin, height: size.height + 2 * margin)
        }
    }

    /// Pages dont le cadre coupe `rect`.
    func pages(intersecting rect: CGRect) -> [Int] {
        frames.indices.filter { !frames[$0].isEmpty && frames[$0].intersects(rect) }
    }

    /// Page lue : celle qui occupe la plus grande hauteur de la zone visible.
    func currentPage(in visible: CGRect) -> Int? {
        var best: (page: Int, height: CGFloat)?
        for page in pages(intersecting: visible) {
            let height = frames[page].intersection(visible).height
            if height > (best?.height ?? 0) {
                best = (page, height)
            }
        }
        return best?.page
    }
}

/// Ajustement automatique du zoom, réappliqué quand la fenêtre change de taille.
enum FitMode: Sendable {
    /// Largeur de la fenêtre, sans dépasser une largeur de lecture confortable.
    case automatic
    /// Largeur de la fenêtre.
    case width
    /// Page entière visible.
    case page
}

/// Grossissements du canevas ; 1 correspond à la taille réelle.
enum Zoom {
    /// Paliers de ⌘+ et ⌘−.
    static let levels: [CGFloat] = [
        0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5, 6, 8, 12, 16
    ]
    /// Largeur de page au-delà de laquelle l'ajustement automatique s'arrête.
    static let comfortablePageWidth: CGFloat = 1_100

    /// Palier suivant (`direction` positive) ou précédent ; les extrêmes sont conservés.
    static func step(from current: CGFloat, direction: Int) -> CGFloat {
        if direction > 0 {
            return levels.first(where: { $0 > current * 1.01 }) ?? levels.last ?? current
        }
        return levels.last(where: { $0 < current * 0.99 }) ?? levels.first ?? current
    }
}

extension PageLayout {
    /// Grossissement qui ajuste le document, ou la page `page`, à `viewport`.
    ///
    /// Le résultat est arrondi par défaut au millième, ce qui évite une barre de
    /// défilement horizontale superflue.
    func fitMagnification(_ mode: FitMode, viewport: CGSize, page: Int) -> CGFloat {
        let margin = Self.margin
        let target: CGFloat
        switch mode {
        case .automatic:
            let widest = documentSize.width - 2 * margin
            target = min(viewport.width / documentSize.width, Zoom.comfortablePageWidth / max(widest, 1))
        case .width:
            target = viewport.width / documentSize.width
        case .page:
            let frame = frames.indices.contains(page) ? frames[page] : .zero
            target = min(viewport.width / (frame.width + 2 * margin), viewport.height / (frame.height + 2 * margin))
        }
        return (target * 1_000).rounded(.down) / 1_000
    }
}
