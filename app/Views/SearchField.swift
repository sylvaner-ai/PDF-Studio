import AppKit
import SwiftUI

/// Champ de recherche natif de macOS (`NSSearchField`).
///
/// Entrée passe à l'occurrence suivante, Majuscule-Entrée à la précédente,
/// Échap appelle `onCancel` (à défaut, efface la recherche). Le champ prend le
/// focus dès qu'il apparaît, puis à chaque changement de `focusRequest`.
struct SearchField: NSViewRepresentable {
    let text: String
    let focusRequest: Int
    let onChange: @MainActor (String) -> Void
    let onNext: @MainActor () -> Void
    let onPrevious: @MainActor () -> Void
    var onCancel: (@MainActor () -> Void)?

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = String(localized: "Rechercher")
        field.sendsWholeSearchString = false
        field.delegate = context.coordinator
        // L'action est aussi envoyée par le bouton d'effacement et par Échap.
        field.target = context.coordinator
        field.action = #selector(Coordinator.searchChanged(_:))
        field.setAccessibilityLabel(String(localized: "Rechercher dans le document"))
        Task { @MainActor in
            field.window?.makeFirstResponder(field)
        }
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text {
            field.stringValue = text
        }
        if focusRequest != context.coordinator.focusRequest {
            context.coordinator.focusRequest = focusRequest
            Task { @MainActor in
                field.window?.makeFirstResponder(field)
            }
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    @MainActor
    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var parent: SearchField
        var focusRequest: Int

        init(parent: SearchField) {
            self.parent = parent
            focusRequest = parent.focusRequest
        }

        @objc func searchChanged(_ sender: NSSearchField) {
            parent.onChange(sender.stringValue)
        }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSSearchField else { return }
            parent.onChange(field.stringValue)
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.cancelOperation(_:)), let onCancel = parent.onCancel {
                onCancel()
                return true
            }
            guard selector == #selector(NSResponder.insertNewline(_:)) else { return false }
            if NSApp.currentEvent?.modifierFlags.contains(.shift) == true {
                parent.onPrevious()
            } else {
                parent.onNext()
            }
            return true
        }
    }
}

/// Panneau de recherche, en haut à droite des pages (⌘F) : champ, compteur
/// d'occurrences, occurrence précédente ou suivante, fermeture (Échap).
struct SearchPanel: View {
    let search: SearchModel

    var body: some View {
        HStack(spacing: Design.Spacing.small) {
            SearchField(
                text: search.query,
                focusRequest: search.focusRequest,
                onChange: { search.update(query: $0) },
                onNext: { search.next() },
                onPrevious: { search.previous() },
                onCancel: { search.dismiss() }
            )
            .frame(width: 220)
            if let status = search.status {
                Text(status)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .fixedSize()
            }
            Button { search.previous() } label: {
                Image(systemName: "chevron.up")
            }
            .help(Text("Occurrence précédente (⇧⌘G)"))
            .disabled(!search.hasHits)
            Button { search.next() } label: {
                Image(systemName: "chevron.down")
            }
            .help(Text("Occurrence suivante (⌘G)"))
            .disabled(!search.hasHits)
            Button { search.dismiss() } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .help(Text("Fermer la recherche (Échap)"))
            .accessibilityLabel(Text("Fermer la recherche"))
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, Design.Spacing.medium)
        .padding(.vertical, Design.Spacing.small)
        .floatingPanel()
        .padding(Design.Spacing.medium)
    }
}
