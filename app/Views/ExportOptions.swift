import AppKit

/// Options des exportations, affichées sous le sélecteur de dossier.
@MainActor
enum ExportOptions {
    /// Grille « libellé : contrôle », entourée d'une marge.
    static func grid(_ rows: [(String, NSView)]) -> NSView {
        let grid = NSGridView(views: rows.map { [NSTextField(labelWithString: $0.0), $0.1] })
        grid.column(at: 0).xPlacement = .trailing
        grid.rowAlignment = .firstBaseline
        grid.rowSpacing = 8
        grid.columnSpacing = 8
        let size = grid.fittingSize
        let container = NSView(frame: NSRect(x: 0, y: 0, width: size.width + 40, height: size.height + 24))
        grid.frame = NSRect(x: 20, y: 12, width: size.width, height: size.height)
        container.addSubview(grid)
        return container
    }

    static func popUp(_ titles: [String], selected: Int = 0) -> NSPopUpButton {
        let button = NSPopUpButton()
        button.addItems(withTitles: titles)
        button.selectItem(at: selected)
        return button
    }
}

/// Exportation des pages en images : format, résolution et pages concernées.
@MainActor
final class ImageExportOptions {
    static let resolutions = [72, 150, 300]

    let view: NSView
    private let format = ExportOptions.popUp(["PNG", "JPEG"])
    private let resolution = ExportOptions.popUp(ImageExportOptions.resolutions.map { "\($0) ppp" }, selected: 1)
    private let scope: NSPopUpButton

    init(hasSelection: Bool) {
        scope = ExportOptions.popUp(
            [String(localized: "Toutes les pages"), String(localized: "Pages sélectionnées")],
            selected: hasSelection ? 1 : 0
        )
        scope.isEnabled = hasSelection
        view = ExportOptions.grid([
            (String(localized: "Format :"), format),
            (String(localized: "Résolution :"), resolution),
            (String(localized: "Pages :"), scope),
        ])
    }

    var imageFormat: PageExporter.ImageFormat { format.indexOfSelectedItem == 1 ? .jpeg : .png }
    var dpi: Int { Self.resolutions[safe: resolution.indexOfSelectedItem] ?? 150 }
    var selectedOnly: Bool { scope.indexOfSelectedItem == 1 }
}

/// Scission d'un document : des parties d'un même nombre de pages, ou une
/// nouvelle partie à chaque page sélectionnée.
@MainActor
final class SplitOptions: NSObject {
    let view: NSView
    private let mode: NSPopUpButton
    private let count = NSTextField()
    private let stepper = NSStepper()

    init(canCutAtSelection: Bool) {
        mode = ExportOptions.popUp(
            [String(localized: "Par nombre de pages"), String(localized: "Avant chaque page sélectionnée")],
            selected: canCutAtSelection ? 1 : 0
        )
        mode.autoenablesItems = false
        mode.item(at: 1)?.isEnabled = canCutAtSelection
        let formatter = NumberFormatter()
        formatter.minimum = 1
        formatter.maximumFractionDigits = 0
        count.formatter = formatter
        count.integerValue = 1
        count.alignment = .right
        count.widthAnchor.constraint(equalToConstant: 56).isActive = true
        stepper.minValue = 1
        stepper.maxValue = 10_000
        stepper.integerValue = 1
        let pages = NSStackView(views: [count, stepper, NSTextField(labelWithString: String(localized: "pages"))])
        pages.spacing = 4
        view = ExportOptions.grid([
            (String(localized: "Découper :"), mode),
            (String(localized: "Pages par partie :"), pages),
        ])
        super.init()
        stepper.target = self
        stepper.action = #selector(stepperChanged)
        count.target = self
        count.action = #selector(countChanged)
        mode.target = self
        mode.action = #selector(modeChanged)
        modeChanged()
    }

    /// Découpage choisi ; `cuts` : positions des pages sélectionnées.
    func rule(cuts: [Int]) -> PageExporter.SplitRule {
        mode.indexOfSelectedItem == 1 ? .before(cuts) : .every(max(count.integerValue, 1))
    }

    @objc private func stepperChanged() {
        count.integerValue = stepper.integerValue
    }

    @objc private func countChanged() {
        stepper.integerValue = max(count.integerValue, 1)
    }

    @objc private func modeChanged() {
        let everyN = mode.indexOfSelectedItem == 0
        count.isEnabled = everyN
        stepper.isEnabled = everyN
    }
}
