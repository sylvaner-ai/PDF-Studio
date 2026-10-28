import AppKit
import SwiftUI

/// Relie un document à sa fenêtre AppKit : pastille du bouton de fermeture quand
/// des modifications ne sont pas enregistrées, et confirmation avant de fermer.
struct DocumentWindowBridge: NSViewRepresentable {
    let model: DocumentViewModel
    let isModified: Bool

    func makeNSView(context: Context) -> BridgeView {
        BridgeView(model: model)
    }

    func updateNSView(_ view: BridgeView, context: Context) {
        view.window?.isDocumentEdited = isModified
    }

    /// Vue invisible qui repère sa fenêtre.
    final class BridgeView: NSView {
        private weak var model: DocumentViewModel?

        init(model: DocumentViewModel) {
            self.model = model
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("init(coder:) n'est pas pris en charge")
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard let window, let model else { return }
            WindowCloseGuard.install(on: window, model: model)
        }
    }
}

/// Délégué placé devant celui de SwiftUI, auquel il transmet tout le reste : il
/// demande s'il faut enregistrer avant de fermer un document modifié (fenêtre ou
/// onglet), et ouvre un nouvel onglet depuis le bouton + de la barre d'onglets.
/// Il vit aussi longtemps que sa fenêtre.
@MainActor
final class WindowCloseGuard: NSObject, NSWindowDelegate {
    private static var guards: [ObjectIdentifier: WindowCloseGuard] = [:]

    private weak var model: DocumentViewModel?
    nonisolated(unsafe) private weak var swiftUIDelegate: NSWindowDelegate?

    /// Sans modèle (écran d'accueil), le délégué déjà installé garde le sien ; il
    /// se replace devant un délégué que SwiftUI aurait installé depuis.
    static func install(on window: NSWindow, model: DocumentViewModel?) {
        let key = ObjectIdentifier(window)
        if let existing = guards[key] {
            if let model { existing.model = model }
            if window.delegate !== existing {
                existing.swiftUIDelegate = window.delegate
                window.delegate = existing
            }
            return
        }
        let closeGuard = WindowCloseGuard()
        closeGuard.model = model
        closeGuard.swiftUIDelegate = window.delegate
        window.delegate = closeGuard
        guards[key] = closeGuard
    }

    func windowShouldClose(_ window: NSWindow) -> Bool {
        guard let model, model.isModified else {
            return swiftUIDelegate?.windowShouldClose?(window) ?? true
        }
        askToSave(model, before: window)
        return false
    }

    func windowWillClose(_ notification: Notification) {
        swiftUIDelegate?.windowWillClose?(notification)
        guard let window = notification.object as? NSWindow else { return }
        window.delegate = swiftUIDelegate
        Self.guards[ObjectIdentifier(window)] = nil
    }

    /// Bouton + de la barre d'onglets ; sa présence suffit à l'afficher.
    @objc func newWindowForTab(_ sender: Any?) {
        DocumentOpener.shared.openNewTab()
    }

    private func askToSave(_ model: DocumentViewModel, before window: NSWindow) {
        let alert = NSAlert()
        alert.messageText = String(
            localized: "Voulez-vous enregistrer les modifications apportées à « \(model.reference.displayName) » ?"
        )
        alert.informativeText = String(localized: "Sinon, elles seront perdues.")
        alert.addButton(withTitle: String(localized: "Enregistrer…"))
        alert.addButton(withTitle: String(localized: "Annuler"))
        let discard = alert.addButton(withTitle: String(localized: "Ne pas enregistrer"))
        discard.keyEquivalent = "d"
        discard.keyEquivalentModifierMask = .command
        alert.beginSheetModal(for: window) { response in
            MainActor.assumeIsolated {
                switch response {
                case .alertFirstButtonReturn:
                    model.save { window.close() }
                case .alertThirdButtonReturn:
                    model.discardChanges()
                    window.close()
                default:
                    break
                }
            }
        }
    }

    // Transmission au délégué de SwiftUI de tout ce que ce délégué ne traite pas.

    nonisolated override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || swiftUIDelegate?.responds(to: selector) == true
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        swiftUIDelegate?.responds(to: selector) == true ? swiftUIDelegate : super.forwardingTarget(for: selector)
    }
}
