import AppKit
import QuartzCore
@testable import PDFStudio

// Mesure du défilement de `PerformanceTests` (`make perf`).

/// Temps d'une étape du travail d'une image, cumulé et au plus.
struct ScrollPhase {
    var total: CFTimeInterval = 0
    var max: CFTimeInterval = 0

    mutating func add(_ time: CFTimeInterval) {
        total += time
        max = Swift.max(max, time)
    }

    func mean(_ frames: Int) -> CFTimeInterval { frames > 0 ? total / Double(frames) : 0 }
}

struct ScrollStats {
    var frames = 0
    var missed = 0
    var period: CFTimeInterval = 1.0 / 60
    var elapsed: CFTimeInterval = 0
    var blankFrames = 0
    /// Défilement et mise à jour des pages proches (vues créées ou réutilisées).
    var layout = ScrollPhase()
    /// Dessin des vues.
    var display = ScrollPhase()
    /// Validation de la transaction Core Animation (dont la préparation des
    /// images de page posées depuis l'image précédente).
    var commit = ScrollPhase()
    var work = ScrollPhase()
    /// Images dont le travail dépasse la moitié de la période de l'écran.
    var slowFrames = 0
    var slowWithNewPage = 0
    var slowWithNewImage = 0
    /// Images manquées alors qu'une image de page venait d'être posée.
    var missedWithNewImage = 0
    var rate: Double { elapsed > 0 ? Double(frames) / elapsed : 0 }
}

/// Durées des étapes du travail d'une image.
struct FrameTimes {
    let layout: CFTimeInterval
    let display: CFTimeInterval
    let commit: CFTimeInterval
}

/// Avance le défilement à chaque image de l'écran et mesure le travail, étape
/// par étape.
@MainActor
final class ScrollDriver: NSObject {
    private let canvas: PageCanvasView
    private let origin: CGPoint
    private let speed: CGFloat
    private let distance: CGFloat
    private var start: CFTimeInterval?
    private var last: CFTimeInterval?
    private var shown: Set<Int> = []
    private(set) var stats = ScrollStats()
    private(set) var isDone = false

    init(canvas: PageCanvasView, origin: CGPoint, speed: CGFloat, distance: CGFloat) {
        self.canvas = canvas
        self.origin = origin
        self.speed = speed
        self.distance = distance
    }

    /// Pages dont l'image est posée.
    private var pagesWithImage: Set<Int> {
        Set(canvas.pageViews.filter { $0.value.showsImage }.keys)
    }

    @objc func step(_ link: CADisplayLink) {
        guard !isDone else { return }
        let now = link.timestamp
        stats.period = link.duration
        let start = self.start ?? now
        self.start = start
        // Images de page posées depuis l'image précédente (rendus terminés).
        let newImage = !pagesWithImage.subtracting(shown).isEmpty
        let measured = last != nil
        if let last {
            stats.frames += 1
            if now - last > 1.5 * link.duration {
                stats.missed += 1
                stats.missedWithNewImage += newImage ? 1 : 0
            }
        }
        last = now

        let before = Set(canvas.pageViews.keys)
        let offset = min(speed * CGFloat(now - start), distance)
        let times = advance(to: offset)
        shown = pagesWithImage
        if measured {
            let newPage = !Set(canvas.pageViews.keys).subtracting(before).isEmpty
            record(times, period: link.duration, newPage: newPage, newImage: newImage)
        }
        if offset >= distance {
            stats.elapsed = now - start
            isDone = true
        }
    }

    /// Fait défiler jusqu'à `offset` ; durées de la mise en page, du dessin et
    /// de la validation.
    private func advance(to offset: CGFloat) -> FrameTimes {
        let begin = CACurrentMediaTime()
        canvas.contentView.scroll(to: CGPoint(x: origin.x, y: origin.y + offset))
        canvas.reflectScrolledClipView(canvas.contentView)
        let laidOut = CACurrentMediaTime()
        canvas.window?.displayIfNeeded()
        let displayed = CACurrentMediaTime()
        CATransaction.flush()
        let committed = CACurrentMediaTime()
        return FrameTimes(layout: laidOut - begin, display: displayed - laidOut, commit: committed - displayed)
    }

    private func record(
        _ times: FrameTimes, period: CFTimeInterval, newPage: Bool, newImage: Bool
    ) {
        let work = times.layout + times.display + times.commit
        stats.layout.add(times.layout)
        stats.display.add(times.display)
        stats.commit.add(times.commit)
        stats.work.add(work)
        if work > period / 2 {
            stats.slowFrames += 1
            stats.slowWithNewPage += newPage ? 1 : 0
            stats.slowWithNewImage += newImage ? 1 : 0
        }
        let visible = canvas.documentVisibleRect
        if canvas.pageViews.values.contains(where: { $0.frame.intersects(visible) && !$0.showsImage }) {
            stats.blankFrames += 1
        }
    }
}
