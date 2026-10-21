import SwiftUI

/// Affiche un document selon son état : ouverture, mot de passe, erreur ou pages.
struct DocumentView: View {
    @State private var model: DocumentViewModel

    init(reference: DocumentReference) {
        _model = State(initialValue: DocumentViewModel(reference: reference))
    }

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(nsColor: .underPageBackgroundColor))
            .navigationTitle(model.title)
            .navigationSubtitle(subtitle)
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
                PageScrollView(pageSizes: model.pageSizes, renderer: renderer)
            }
        }
    }

    private var subtitle: String {
        guard model.phase == .ready else { return "" }
        return model.pageCount == 1
            ? String(localized: "1 page")
            : String(localized: "\(model.pageCount) pages")
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
            Text(wrongAttempt ? "Le mot de passe est incorrect." : "Ce document est protégé par un mot de passe.")
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
