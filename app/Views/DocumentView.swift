import SwiftUI

/// Affiche un document selon son état : ouverture, mot de passe, erreur ou pages.
struct DocumentView: View {
    @State private var model: DocumentViewModel
    @Environment(\.undoManager) private var undoManager
    /// Le document a changé de fichier (« Enregistrer sous »).
    private let onRelocate: (DocumentReference) -> Void

    init(reference: DocumentReference, onRelocate: @escaping (DocumentReference) -> Void = { _ in }) {
        self.init(model: DocumentViewModel(reference: reference), onRelocate: onRelocate)
    }

    /// Affiche un document dont le modèle existe déjà (captures d'écran des tests).
    init(model: DocumentViewModel, onRelocate: @escaping (DocumentReference) -> Void = { _ in }) {
        _model = State(initialValue: model)
        self.onRelocate = onRelocate
    }

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(nsColor: .underPageBackgroundColor))
            .navigationTitle(model.title)
            .modifier(DocumentSubtitle(model: model))
            .focusedSceneValue(\.documentModel, model)
            .background(DocumentWindowBridge(model: model, isModified: model.isModified))
            .overlay(alignment: .bottom) {
                ZStack {
                    if let activity = model.activity {
                        ActivityBadge(text: activity)
                            .floatingTransition(from: .bottom)
                    } else if let progress = model.recognition {
                        RecognitionBadge(progress: progress) { model.stopRecognition() }
                            .floatingTransition(from: .bottom)
                    } else if let progress = model.enhancement.progress {
                        EnhancementBadge(progress: progress) { model.enhancement.stop() }
                            .floatingTransition(from: .bottom)
                    } else if let notice = model.notice {
                        NoticeBadge(text: notice)
                            .floatingTransition(from: .bottom)
                    }
                }
                .interfaceAnimation(value: bottomBadge)
            }
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
            .task {
                // Un document déjà ouvert (captures d'écran des tests) ne l'est pas deux fois.
                if model.phase == .opening {
                    await model.open()
                }
            }
    }

    /// Bandeau affiché au bas de la fenêtre, pour en animer l'apparition.
    private var bottomBadge: Int {
        if model.activity != nil { return 1 }
        if model.recognition != nil { return 2 }
        if model.enhancement.progress != nil { return 4 }
        return model.notice != nil ? 3 : 0
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
                .padding(Design.Spacing.wide)
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
        var messages: [String] = []
        if case .replace(signed: _, changedOnDisk: true) = prompt {
            messages.append(String(localized: """
                Une autre application a modifié ce fichier depuis son ouverture : \
                ses modifications seront perdues.
                """))
        }
        if case .replace(signed: true, changedOnDisk: _) = prompt {
            messages.append(String(localized: "Ce document porte une signature numérique : elle ne sera plus valide."))
        }
        messages.append(String(localized: "Une copie de l'original est conservée dans les sauvegardes de PDF Studio."))
        return messages.joined(separator: "\n\n")
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

}

/// Sous-titre de la fenêtre : la page lue, ou la sélection de la grille.
///
/// Calculé à part de ``DocumentView`` : la page lue change à chaque page qui
/// défile, et seul ce sous-titre est alors recalculé, pas toute la fenêtre.
private struct DocumentSubtitle: ViewModifier {
    let model: DocumentViewModel

    func body(content: Content) -> some View {
        content.navigationSubtitle(subtitle)
    }

    private var subtitle: String {
        guard model.phase == .ready else { return "" }
        if model.workspace == .organizing, model.selectionCount > 1 {
            return String(localized: "\(model.selectionCount) pages sélectionnées sur \(model.pageCount)")
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
            ZStack {
                switch model.workspace {
                case .reading, .editing:
                    // Une seule branche : le canevas reste le même en passant de la
                    // lecture à l'édition.
                    canvas
                        .transition(.opacity)
                case .organizing:
                    PageGrid(
                        model: model,
                        renderer: renderer,
                        pages: model.pages,
                        selection: model.selection,
                        display: model.canvas.rotation,
                        thumbnailWidth: model.gridThumbnailWidth
                    )
                    .transition(.opacity)
                case .enhancing:
                    EnhancementView(model: model, enhancement: model.enhancement)
                        .transition(.opacity)
                }
            }
            .interfaceAnimation(value: model.workspace == .organizing || model.workspace == .enhancing)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("Présentation", selection: $model.workspace) {
                    Label("Lecture", systemImage: "doc.text").tag(Workspace.reading)
                    Label("Pages", systemImage: "square.grid.2x2").tag(Workspace.organizing)
                    Label("Édition", systemImage: "pencil").tag(Workspace.editing)
                    Label("Amélioration", systemImage: "wand.and.stars").tag(Workspace.enhancing)
                }
                .pickerStyle(.segmented)
                .labelStyle(.iconOnly)
                .help(Text("Lecture (⌘1), pages (⌘2), édition (⌘3) ou amélioration des scans (⌘4)"))
            }
            // Comparer et Enregistrer restent visibles dans tous les modes (choix de
            // Sylvain), grisés quand ils ne servent pas.
            ToolbarItemGroup(placement: .primaryAction) {
                Toggle(isOn: $model.isComparing) {
                    Label("Comparer avec l'original", systemImage: "square.split.2x1")
                }
                .help(Text("Montrer les pages sans leurs modifications, pour les comparer (⌥⌘K)"))
                .disabled(!model.canCompare && !model.isComparing)
                Button { model.save() } label: {
                    Label("Enregistrer", systemImage: "square.and.arrow.down")
                }
                .help(Text("Enregistrer les modifications (⌘S)"))
                .disabled(!model.canSave)
            }
        }
    }

    /// Pages du document, avec ce qui se pose dessus : bandeau du mode Édition ou
    /// de la comparaison, panneau de retouche d'un scan, recherche.
    private var canvas: some View {
        PageCanvas(renderer: renderer, controller: model.canvas)
            .accessibilityLabel(Text("Pages du document"))
            .overlay(alignment: .bottom) {
                ZStack {
                    if model.isComparing {
                        ComparisonBadge()
                            .floatingTransition(from: .bottom)
                    } else if showsEditingBadge {
                        EditingBadge(editing: model.editing)
                            .floatingTransition(from: .bottom)
                    }
                }
                .interfaceAnimation(value: model.isComparing ? 2 : showsEditingBadge ? 1 : 0)
            }
            .overlay(alignment: .topLeading) {
                ZStack {
                    if model.isEditingCanvas, let line = model.editing.selectedScanLine, let scan = line.scan {
                        ScanRetouchPanel(editing: model.editing, scan: scan)
                            .id(line.id)
                            .floatingTransition(from: .top)
                    }
                }
                .interfaceAnimation(value: scanPanelKey)
            }
            .overlay(alignment: .topTrailing) {
                ZStack {
                    if let search = model.search, search.isPresented {
                        SearchPanel(search: search)
                            .floatingTransition(from: .top)
                    }
                }
                .interfaceAnimation(value: model.search?.isPresented == true)
            }
    }

    /// Bandeau du mode Édition ; il laisse la place à l'avancement de la
    /// reconnaissance du texte et à son bilan, au même endroit.
    private var showsEditingBadge: Bool {
        model.isEditingCanvas && model.recognition == nil && model.notice == nil
    }

    /// Ligne scannée dont le panneau de retouche est ouvert.
    private var scanPanelKey: UInt32? {
        guard model.isEditingCanvas, let line = model.editing.selectedScanLine, line.scan != nil else { return nil }
        return line.id
    }
}

/// Bandeau de la comparaison avec l'original.
struct ComparisonBadge: View {
    var body: some View {
        Label("Pages d'origine, sans les modifications. ⌥⌘K pour revenir aux pages modifiées.",
              systemImage: "square.split.2x1")
            .padding(.horizontal, Design.Spacing.large)
            .padding(.vertical, Design.Spacing.small)
            .floatingBadge()
            .padding(.bottom, Design.Spacing.large)
            .allowsHitTesting(false)
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

/// Exportation en cours, en bas de la fenêtre.
struct ActivityBadge: View {
    let text: String

    var body: some View {
        HStack(spacing: Design.Spacing.small) {
            ProgressView()
                .controlSize(.small)
            Text(text)
        }
        .padding(.horizontal, Design.Spacing.large)
        .padding(.vertical, Design.Spacing.small)
        .floatingBadge()
        .padding(.bottom, Design.Spacing.large)
        .accessibilityElement(children: .combine)
    }
}

/// Demande du mot de passe d'un document protégé.
struct PasswordPrompt: View {
    let wrongAttempt: Bool
    let submit: (String) -> Void
    @State private var password = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: Design.Spacing.medium) {
            EmptyStateSymbol(name: "lock")
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
        .padding(Design.Spacing.wide)
        .onAppear { focused = true }
    }
}
