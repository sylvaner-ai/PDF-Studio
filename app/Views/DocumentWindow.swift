import AppKit
import SwiftUI

/// Contenu d'une fenêtre, présentée comme un onglet : l'écran d'accueil tant
/// qu'aucun document n'est ouvert, puis le document.
struct DocumentWindow: View {
    @Binding var reference: DocumentReference?
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var windowID = UUID()
    /// Change quand la fenêtre accueille un autre document, et pas quand son
    /// document change seulement de fichier (« Enregistrer sous »).
    @State private var documentID = UUID()
    /// Un fichier est glissé au-dessus de la fenêtre.
    @State private var isDropTargeted = false
    /// Fenêtre AppKit, où s'ouvrent en onglets les fichiers qui y sont déposés.
    @State private var windowHandle = WindowHandle()

    var body: some View {
        Group {
            if let reference {
                DocumentView(reference: reference, onRelocate: { self.reference = $0 })
                    .id(documentID)
            } else {
                WelcomeView(isTargeted: isDropTargeted)
                    .navigationTitle("PDF Studio")
            }
        }
        .frame(minWidth: 480, minHeight: 360)
        .background(WindowTabbing(id: windowID, handle: windowHandle))
        .dropDestination(for: URL.self) { urls, _ in
            let pdfs = urls.filter(DocumentOpener.isPDF)
            DocumentOpener.shared.open(pdfs, besides: windowHandle.window)
            return !pdfs.isEmpty
        } isTargeted: { isDropTargeted = $0 }
        .onAppear {
            updateRegistration()
            DocumentOpener.shared.register(openWindow: openWindow)
        }
        .onChange(of: reference) { updateRegistration() }
        .onDisappear {
            DocumentOpener.shared.unregisterWindow(id: windowID)
        }
    }

    /// Déclare la fenêtre et son document ; vide, elle se propose pour accueillir
    /// le prochain document ouvert.
    private func updateRegistration() {
        DocumentOpener.shared.registerWindow(
            id: windowID,
            reference: reference,
            adopt: {
                documentID = UUID()
                reference = $0
            },
            close: { dismissWindow() }
        )
    }
}

/// Fenêtre AppKit d'une fenêtre SwiftUI, connue une fois sa vue installée.
@MainActor
final class WindowHandle {
    weak var window: NSWindow?
}

/// Vue invisible qui repère la fenêtre AppKit et la confie à ``DocumentOpener``,
/// qui en fait un onglet de la fenêtre d'où vient la demande.
struct WindowTabbing: NSViewRepresentable {
    let id: UUID
    let handle: WindowHandle

    func makeNSView(context: Context) -> TabbingView {
        TabbingView(id: id, handle: handle)
    }

    func updateNSView(_ view: TabbingView, context: Context) {}

    final class TabbingView: NSView {
        private let id: UUID
        private let handle: WindowHandle

        init(id: UUID, handle: WindowHandle) {
            self.id = id
            self.handle = handle
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("init(coder:) n'est pas pris en charge")
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard let window else { return }
            handle.window = window
            DocumentOpener.shared.attach(window: window, id: id)
        }
    }
}

/// Écran d'accueil : un seul message discret ; il se colore de l'accent quand
/// un fichier est glissé au-dessus de la fenêtre.
struct WelcomeView: View {
    var isTargeted = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: Design.Spacing.medium) {
            EmptyStateSymbol(name: "doc", highlighted: isTargeted)
            Text("Déposez un PDF ici")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .underPageBackgroundColor))
        .overlay {
            if isTargeted {
                RoundedRectangle(cornerRadius: Design.Radius.panel, style: .continuous)
                    .strokeBorder(.tint, style: StrokeStyle(lineWidth: 2, dash: [8, 6]))
                    .padding(Design.Spacing.large)
                    .transition(.opacity)
                    .allowsHitTesting(false)
            }
        }
        .animation(Design.Motion.animation(reduceMotion: reduceMotion), value: isTargeted)
        .accessibilityElement(children: .combine)
        .accessibilityHint("Ou choisissez Fichier, Ouvrir.")
    }
}

/// Symbole d'un écran vide (accueil, mot de passe), d'un trait fin et dont la
/// taille suit celle du texte.
struct EmptyStateSymbol: View {
    let name: String
    /// En couleur d'accent (fichier glissé au-dessus de l'accueil).
    var highlighted = false
    @ScaledMetric(relativeTo: .title3) private var size: CGFloat = 40

    var body: some View {
        Image(systemName: name)
            .font(.system(size: size, weight: .light))
            .foregroundStyle(highlighted ? AnyShapeStyle(.tint) : AnyShapeStyle(.tertiary))
            .accessibilityHidden(true)
    }
}
