import SwiftUI

/// Affiche un document selon son état : ouverture, mot de passe, erreur ou pages.
struct DocumentView: View {
    @State private var model: DocumentViewModel
    @Environment(\.undoManager) private var undoManager
    /// Le document a changé de fichier (« Enregistrer sous »).
    private let onRelocate: (DocumentReference) -> Void

    init(reference: DocumentReference, onRelocate: @escaping (DocumentReference) -> Void = { _ in }) {
        _model = State(initialValue: DocumentViewModel(reference: reference))
        self.onRelocate = onRelocate
    }

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(nsColor: .underPageBackgroundColor))
            .navigationTitle(model.title)
            .navigationSubtitle(subtitle)
            .focusedSceneValue(\.documentModel, model)
            .background(DocumentWindowBridge(model: model, isModified: model.isModified))
            .alert(
                replaceTitle,
                isPresented: Binding(
                    get: { model.saving.prompt != nil },
                    // Les boutons disent quoi faire ; ce retour ne fait que refermer l'alerte.
                    set: { if !$0 { model.saving.prompt = nil } }
                ),
                presenting: model.saving.prompt,
                actions: { _ in
                    Button("Remplacer") { model.replaceOriginal() }
                    Button("Enregistrer sous…") { model.saveAs() }
                    Button("Annuler", role: .cancel) { model.cancelSave() }
                },
                message: { prompt in Text(replaceMessage(prompt)) }
            )
            .alert(
                recoveryTitle,
                // Les boutons répondent ; l'alerte ne se referme pas autrement.
                isPresented: Binding(get: { model.recoveryOffer != nil }, set: { _ in }),
                presenting: model.recoveryOffer,
                actions: { _ in
                    Button("Restaurer") { model.restoreRecovery() }
                    Button("Ignorer") { model.declineRecovery() }
                },
                message: { record in Text(recoveryMessage(record)) }
            )
            .alert(
                "L'opération n'a pas abouti.",
                isPresented: Binding(
                    get: { model.failureMessage != nil },
                    set: { if !$0 { model.failureMessage = nil } }
                ),
                actions: {},
                message: { Text(model.failureMessage ?? "") }
            )
            .onAppear {
                model.undoManager = undoManager
                model.onRelocate = onRelocate
            }
            .onChange(of: undoManager) { _, manager in model.undoManager = manager }
            .onDisappear { model.close() }
            .task { await model.open() }
    }

    @ViewBuilder
    private var content: some View {
        switch model.phase {
        case .opening:
            ProgressView()
                .controlSize(.small)
        case let .needsPassword(wrongAttempt):
            PasswordPrompt(wrongAttempt: wrongAttempt) { password in
                Task { await model.open(password: password) }
            }
        case let .failed(message):
            Text(message)
                .font(.title3)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(32)
        case .ready:
            if let renderer = model.renderer {
                ReaderView(model: model, renderer: renderer)
            }
        }
    }

    private var replaceTitle: String {
        String(localized: "Remplacer « \(model.reference.displayName) » par la version modifiée ?")
    }

    private func replaceMessage(_ prompt: SavingState.Prompt) -> String {
        let backup = String(localized: "Une copie de l'original est conservée dans les sauvegardes de PDF Studio.")
        guard case .replace(signed: true) = prompt else { return backup }
        let signature = String(localized: "Ce document porte une signature numérique : elle ne sera plus valide.")
        return signature + "\n\n" + backup
    }

    private var recoveryTitle: String {
        String(localized: "Restaurer les modifications non enregistrées de « \(model.reference.displayName) » ?")
    }

    private func recoveryMessage(_ record: RecoveryRecord) -> String {
        let date = record.date.formatted(date: .long, time: .shortened)
        let message = String(localized: "Elles datent du \(date). Si vous les ignorez, elles seront supprimées.")
        guard model.recoveredFileChanged else { return message }
        return message + "\n\n" + String(localized: "Le fichier a été modifié depuis par une autre application.")
    }

    private var subtitle: String {
        guard model.phase == .ready else { return "" }
        if model.workspace == .organizing, model.selection.count > 1 {
            return String(localized: "\(model.selection.count) pages sélectionnées sur \(model.pageCount)")
        }
        return model.pageCount == 1
            ? String(localized: "1 page")
            : String(localized: "Page \(model.canvas.currentPage + 1) sur \(model.pageCount)")
    }
}

/// Lecture d'un document : barre latérale, pages (ou grille d'organisation) et recherche.
struct ReaderView: View {
    @Bindable var model: DocumentViewModel
    let renderer: PageRenderer

    @State private var columnVisibility: NavigationSplitViewVisibility

    init(model: DocumentViewModel, renderer: PageRenderer) {
        self.model = model
        self.renderer = renderer
        // La page est le héros : pas de barre latérale pour un document d'une page.
        _columnVisibility = State(initialValue: model.pageCount > 1 ? .all : .detailOnly)
    }

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(model: model, renderer: renderer)
                .navigationSplitViewColumnWidth(min: 140, ideal: 168, max: 260)
        } detail: {
            switch model.workspace {
            case .reading:
                PageCanvas(renderer: renderer, controller: model.canvas)
                    .accessibilityLabel(Text("Pages du document"))
            case .organizing:
                PageGrid(
                    model: model,
                    renderer: renderer,
                    pages: model.pages,
                    selection: model.selection,
                    display: model.canvas.rotation,
                    thumbnailWidth: model.gridThumbnailWidth
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("Présentation", selection: $model.workspace) {
                    Label("Lecture", systemImage: "doc.text").tag(Workspace.reading)
                    Label("Organisation des pages", systemImage: "square.grid.2x2").tag(Workspace.organizing)
                }
                .pickerStyle(.segmented)
                .labelStyle(.iconOnly)
                .help(Text("Lecture (⌘1) ou organisation des pages (⌘2)"))
            }
            ToolbarItem(placement: .primaryAction) {
                if model.workspace == .reading, let search = model.search {
                    SearchToolbarItem(search: search)
                }
            }
        }
    }
}

/// Champ de recherche et compteur d'occurrences.
struct SearchToolbarItem: View {
    let search: SearchModel

    var body: some View {
        HStack(spacing: 8) {
            if let status = search.status {
                Text(status)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            SearchField(
                text: search.query,
                focusRequest: search.focusRequest,
                onChange: { search.update(query: $0) },
                onNext: { search.next() },
                onPrevious: { search.previous() }
            )
            .frame(minWidth: 140, idealWidth: 200, maxWidth: 240)
        }
    }
}

/// Demande du mot de passe d'un document protégé.
struct PasswordPrompt: View {
    let wrongAttempt: Bool
    let submit: (String) -> Void
    @State private var password = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "lock")
                .font(.system(size: 32, weight: .ultraLight))
                .foregroundStyle(.tertiary)
            Text(wrongAttempt
                 ? LocalizedStringKey("Le mot de passe est incorrect.")
                 : LocalizedStringKey("Ce document est protégé par un mot de passe."))
                .foregroundStyle(.secondary)
            SecureField("Mot de passe", text: $password)
                .textFieldStyle(.roundedBorder)
                .frame(width: 240)
                .focused($focused)
                .onSubmit { submit(password) }
            Button("Ouvrir") { submit(password) }
                .keyboardShortcut(.defaultAction)
                .disabled(password.isEmpty)
        }
        .padding(32)
        .onAppear { focused = true }
    }
}
