import AppKit

/// Saisie du texte d'un bloc, posée sur la page : Retour valide, ⇧Retour ou
/// ⌥Retour passe à la ligne, Échap annule, un clic ailleurs valide.
@MainActor
final class InlineTextEditor: NSTextView, NSTextViewDelegate {
    /// Fin de la saisie : le texte, ou `nil` si elle est annulée.
    var onFinish: (@MainActor (String?) -> Void)?
    private var finished = false
    /// Historique propre à la saisie, distinct de celui du document.
    private lazy var typingUndo = UndoManager()

    /// `grows` : la saisie s'élargit au fil du texte, sans renvoi à la ligne
    /// (texte ajouté, dont les lignes ne se coupent qu'aux retours voulus).
    func configure(text: String, font: NSFont, color: NSColor = .black, grows: Bool = false) {
        string = text
        self.font = font
        textColor = color
        insertionPointColor = .black
        backgroundColor = .white
        drawsBackground = true
        isRichText = false
        importsGraphics = false
        allowsUndo = true
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
        isAutomaticTextReplacementEnabled = false
        textContainerInset = .zero
        textContainer?.lineFragmentPadding = 0
        textContainer?.widthTracksTextView = !grows
        if grows {
            let unbounded = CGFloat.greatestFiniteMagnitude
            textContainer?.containerSize = CGSize(width: unbounded, height: unbounded)
        }
        isHorizontallyResizable = grows
        isVerticallyResizable = true
        minSize = frame.size
        maxSize = CGSize(width: grows ? .greatestFiniteMagnitude : frame.width, height: .greatestFiniteMagnitude)
        delegate = self
        setAccessibilityLabel(String(localized: "Texte du bloc"))
    }

    /// Termine la saisie, une seule fois.
    func finish(commit: Bool) {
        guard !finished else { return }
        finished = true
        onFinish?(commit ? string : nil)
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        if resigned, !finished {
            // Clic ailleurs : la saisie est validée une fois le changement de
            // répondeur terminé.
            finished = true
            let (text, onFinish) = (string, onFinish)
            Task { @MainActor in onFinish?(text) }
        }
        return resigned
    }

    func undoManager(for view: NSTextView) -> UndoManager? {
        typingUndo
    }

    func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            let flags = NSApp.currentEvent?.modifierFlags ?? []
            if flags.contains(.shift) || flags.contains(.option) {
                textView.insertNewlineIgnoringFieldEditor(nil)
            } else {
                finish(commit: true)
            }
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            finish(commit: false)
            return true
        default:
            return false
        }
    }
}
