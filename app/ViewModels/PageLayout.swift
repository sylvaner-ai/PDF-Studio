import CoreGraphics

/// Mode d'affichage des pages.
enum DisplayMode: String, CaseIterable, Sendable {
    /// Toutes les pages les unes sous les autres.
    case continuous
    /// Une seule page à la fois.
    case singlePage
}

/// Rotation d'affichage, en degrés dans le sens horaire : 0, 90, 180 ou 270.
///
/// Le fichier n'est pas modifié : seule la présentation tourne.
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
    ///   - pageSizes: dimensions des pages droites, en points.
    ///   - visiblePage: en mode page par page, la seule page disposée.
    init(pageSizes: [CGSize], rotation: DisplayRotation, mode: DisplayMode, visiblePage: Int = 0) {
        let sizes = pageSizes.map(rotation.apply(to:))
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
