import AppKit

// MARK: - Saisie du texte

extension EditingOverlayView {
    func openEditor(for item: PageObjectItem) {
        if let style = item.addedText {
            // Un texte ajouté se lit droit à l'écran : sa saisie le recouvre.
            openEditor(frame: frame(of: item), text: item.text ?? "", font: style.nsFont, color: style.nsColor)
        } else {
            openEditor(bounds: frame(of: item), text: item.text ?? "", font: Self.font(for: item))
        }
        editorObject = item.id
    }

    func openEditor(bounds: CGRect, text: String, font: NSFont) {
        // Le texte se saisit à l'horizontale, même sur une page tournée.
        let size = rotation.swapsAxes ? CGSize(width: bounds.height, height: bounds.width) : bounds.size
        openEditor(frame: CGRect(origin: bounds.origin, size: size), text: text, font: font)
    }

    /// Ouvre la saisie dans le cadre `frame` de la vue (agrandi au besoin) ;
    /// `color` : couleur du texte saisi. Un texte ajouté (`color` donnée)
    /// s'élargit au fil de la saisie, sans renvoi à la ligne.
    func openEditor(frame area: CGRect, text: String, font: NSFont, color: NSColor? = nil) {
        removeEditor()
        let box = CGRect(
            origin: area.origin,
            size: CGSize(width: max(area.width, 80), height: max(area.height, font.pointSize * 1.3))
        )
        let editor = InlineTextEditor(frame: box)
        editor.configure(text: text, font: font, color: color ?? .black, grows: color != nil)
        editor.onFinish = { [weak self] text in
            self?.editorDidFinish(text)
        }
        addSubview(editor)
        self.editor = editor
        window?.makeFirstResponder(editor)
        editor.selectAll(nil)
        needsDisplay = true
    }

    private func editorDidFinish(_ text: String?) {
        let cell = editorCell != nil
        let new = editorNewText != nil
        removeEditor()
        if new {
            editing?.endNewText(text: text)
        } else if cell {
            editing?.endCellEditing(text: text)
        } else {
            editing?.endTextEditing(text: text)
        }
    }

    func removeEditor() {
        guard let editor else { return }
        self.editor = nil
        editorObject = nil
        editorCell = nil
        editorNewText = nil
        editor.onFinish = nil
        let focused = window?.firstResponder === editor
        editor.removeFromSuperview()
        if focused {
            focusPages()
        }
        needsDisplay = true
    }
}
