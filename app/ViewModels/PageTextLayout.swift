import CoreGraphics
import Foundation

/// Texte d'une page, prêt à être sélectionné à la souris.
///
/// Les caractères de toutes les lignes sont mis bout à bout : une position de
/// la sélection est un rang dans cette suite (de 0 à `count`, bornes
/// comprises). Les emprises sont exprimées sur la page droite, comme celles du
/// moteur (points, origine en haut à gauche), et le restent : c'est le point
/// visé par la souris qui est ramené sur la page droite, pas chaque caractère
/// sur l'écran.
struct PageTextLayout: Sendable {
    /// Une ligne de texte : sa plage de caractères et son sens de lecture.
    struct Line: Sendable {
        let range: Range<Int>
        /// Emprise de la ligne.
        let bounds: CGRect
        /// Les caractères se suivent de gauche à droite ou de droite à gauche
        /// (sinon, de haut en bas ou de bas en haut).
        let isHorizontal: Bool
        /// Le sens de lecture suit les coordonnées croissantes.
        let isForward: Bool

        /// Écarts du point à la ligne : en travers (0 s'il est dans la bande de
        /// la ligne, son épaisseur prolongée sur toute la page) et le long de
        /// la ligne (0 s'il est entre ses extrémités).
        func offsets(of point: CGPoint) -> (across: CGFloat, along: CGFloat) {
            if isHorizontal {
                return (PageTextLayout.gap(from: point.y, to: bounds.minY...bounds.maxY),
                        PageTextLayout.gap(from: point.x, to: bounds.minX...bounds.maxX))
            }
            return (PageTextLayout.gap(from: point.x, to: bounds.minX...bounds.maxX),
                    PageTextLayout.gap(from: point.y, to: bounds.minY...bounds.maxY))
        }
    }

    /// Un point visé : le caractère le plus proche et la position d'insertion
    /// la plus proche de ce point, avant ou après lui.
    struct Hit: Equatable, Sendable {
        let unit: Int
        let offset: Int
    }

    private(set) var scalars: [Unicode.Scalar] = []
    private(set) var rects: [CGRect] = []
    private(set) var lines: [Line] = []

    init() {}

    init(_ source: [SelectableLine]) {
        for line in source {
            let characters = Array(line.text.unicodeScalars)
            let count = min(characters.count, line.rects.count)
            guard count > 0 else { continue }
            let boxes = line.rects.prefix(count).map(\.cgRect)
            let start = scalars.count
            scalars += characters.prefix(count)
            rects += boxes
            lines.append(Self.line(start..<start + count, boxes: boxes))
        }
    }

    var count: Int { scalars.count }
    var isEmpty: Bool { scalars.isEmpty }

    private static func line(_ range: Range<Int>, boxes: [CGRect]) -> Line {
        let bounds = boxes.dropFirst().reduce(boxes[0]) { $0.union($1) }
        let isHorizontal = bounds.width >= bounds.height
        let first = boxes[0], last = boxes[boxes.count - 1]
        let isForward = isHorizontal ? first.midX <= last.midX : first.midY <= last.midY
        return Line(range: range, bounds: bounds, isHorizontal: isHorizontal, isForward: isForward)
    }

    // MARK: - Point visé

    /// Caractère et position d'insertion les plus proches de `point` (sur la
    /// page droite). Une ligne dont la bande contient le point l'emporte, quel
    /// que soit l'éloignement horizontal : le pointeur à droite de la fin d'une
    /// ligne désigne la fin de cette ligne, et non le début de la suivante.
    func hit(at point: CGPoint) -> Hit? {
        guard let index = lineIndex(at: point) else { return nil }
        let line = lines[index]
        let coordinate = line.isHorizontal ? point.x : point.y
        var nearest = line.range.lowerBound
        var nearestGap = CGFloat.infinity
        for unit in line.range {
            let gap = Self.gap(from: coordinate, to: Self.span(of: rects[unit], horizontal: line.isHorizontal))
            if gap < nearestGap {
                (nearest, nearestGap) = (unit, gap)
            }
        }
        let span = Self.span(of: rects[nearest], horizontal: line.isHorizontal)
        let middle = (span.lowerBound + span.upperBound) / 2
        let after = line.isForward ? coordinate > middle : coordinate < middle
        return Hit(unit: nearest, offset: after ? nearest + 1 : nearest)
    }

    private static func span(of rect: CGRect, horizontal: Bool) -> ClosedRange<CGFloat> {
        horizontal ? rect.minX...rect.maxX : rect.minY...rect.maxY
    }

    /// Distance de `value` à `span` (0 si elle la contient).
    fileprivate static func gap(from value: CGFloat, to span: ClosedRange<CGFloat>) -> CGFloat {
        value < span.lowerBound ? span.lowerBound - value : max(value - span.upperBound, 0)
    }

    /// La ligne visée : parmi celles dont la bande contient le point, la plus
    /// proche le long de la ligne ; sinon la plus proche en travers (l'écart le
    /// long de la ligne compte quatre fois moins, pour que le pointeur sous le
    /// texte désigne la dernière ligne, et non la plus large).
    private func lineIndex(at point: CGPoint) -> Int? {
        var inside: (index: Int, gap: CGFloat)?
        var closest: (index: Int, distance: CGFloat)?
        for (index, line) in lines.enumerated() {
            let (across, along) = line.offsets(of: point)
            if across == 0 {
                if along < inside?.gap ?? .infinity {
                    inside = (index, along)
                }
            } else {
                let distance = across * across + along * along / 16
                if distance < closest?.distance ?? .infinity {
                    closest = (index, distance)
                }
            }
        }
        return inside?.index ?? closest?.index
    }

    // MARK: - Mots et lignes

    /// Le mot qui contient `unit` : la suite de lettres et de chiffres, ou de
    /// signes, ou d'espaces, qui l'entoure sur sa ligne.
    func wordRange(containing unit: Int) -> Range<Int> {
        guard let line = line(containing: unit) else { return unit..<unit }
        let kind = Self.kind(of: scalars[unit])
        var start = unit, end = unit + 1
        while start > line.range.lowerBound, Self.kind(of: scalars[start - 1]) == kind {
            start -= 1
        }
        while end < line.range.upperBound, Self.kind(of: scalars[end]) == kind {
            end += 1
        }
        return start..<end
    }

    /// La ligne qui contient `unit`.
    func lineRange(containing unit: Int) -> Range<Int> {
        line(containing: unit)?.range ?? unit..<unit
    }

    private func line(containing unit: Int) -> Line? {
        lines.first { $0.range.contains(unit) }
    }

    private enum Kind {
        case word, space, symbol
    }

    private static func kind(of scalar: Unicode.Scalar) -> Kind {
        if CharacterSet.whitespaces.contains(scalar) {
            return .space
        }
        return CharacterSet.alphanumerics.contains(scalar) ? .word : .symbol
    }

    // MARK: - Résultat de la sélection

    /// Rectangles à surligner pour les caractères de `range` : un par ligne
    /// touchée.
    func highlights(in range: Range<Int>) -> [CGRect] {
        lines.compactMap { line in
            let shown = line.range.clamped(to: range)
            guard !shown.isEmpty else { return nil }
            return rects[shown].dropFirst().reduce(rects[shown.lowerBound]) { $0.union($1) }
        }
    }

    /// Texte des caractères de `range`, une ligne par ligne. Les ligatures
    /// typographiques (« ﬁ », « ﬂ »…) sont développées : elles se copient comme
    /// elles se lisent.
    func text(in range: Range<Int>) -> String {
        let lines = self.lines.compactMap { line -> String? in
            let shown = line.range.clamped(to: range)
            guard !shown.isEmpty else { return nil }
            var text = ""
            for scalar in scalars[shown] {
                text += Self.expansion(of: scalar) ?? String(scalar)
            }
            return text
        }
        return lines.joined(separator: "\n")
    }

    private static func expansion(of scalar: Unicode.Scalar) -> String? {
        switch scalar {
        case "\u{FB00}": return "ff"
        case "\u{FB01}": return "fi"
        case "\u{FB02}": return "fl"
        case "\u{FB03}": return "ffi"
        case "\u{FB04}": return "ffl"
        case "\u{FB05}", "\u{FB06}": return "st"
        default: return nil
        }
    }
}

extension CGRect {
    /// Carré de la distance du point à ce rectangle (0 s'il le contient).
    func squaredDistance(to point: CGPoint) -> CGFloat {
        let horizontal = max(minX - point.x, 0, point.x - maxX)
        let vertical = max(minY - point.y, 0, point.y - maxY)
        return horizontal * horizontal + vertical * vertical
    }
}
