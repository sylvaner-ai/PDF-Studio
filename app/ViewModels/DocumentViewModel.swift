import CoreGraphics
import Foundation
import Observation
import os

/// État d'un document affiché dans une fenêtre.
@MainActor
@Observable
final class DocumentViewModel {
    enum Phase: Equatable {
        case opening
        case needsPassword(wrongAttempt: Bool)
        case ready
        case failed(message: String)
    }

    let reference: DocumentReference
    private(set) var phase: Phase = .opening
    private(set) var title: String
    private(set) var pageSizes: [CGSize] = []
    private(set) var renderer: PageRenderer?

    private let logger = Logger(subsystem: "fr.sylvainrobert.pdfstudio", category: "document")

    init(reference: DocumentReference) {
        self.reference = reference
        self.title = reference.displayName
    }

    var pageCount: Int { pageSizes.count }

    /// Ouvre le document hors du fil principal.
    func open(password: String? = nil) async {
        phase = .opening
        let reference = reference
        let result = await Task.detached(priority: .userInitiated) {
            Result {
                try reference.withAccess { url in
                    try PdfDocument.open(path: url.path, password: password)
                }
            }
        }.value

        switch result {
        case let .success(document):
            pageSizes = document.pageSizes().map(\.cgSize)
            if let declared = document.title() {
                title = declared
            }
            renderer = PageRenderer(document: document)
            phase = .ready
        case let .failure(error as EngineError):
            switch error {
            case .PasswordRequired:
                phase = .needsPassword(wrongAttempt: false)
            case .WrongPassword:
                phase = .needsPassword(wrongAttempt: true)
            case .Failed:
                logger.error("Ouverture impossible : \(error.technicalDetail ?? "", privacy: .public)")
                phase = .failed(message: error.userMessage)
            }
        case let .failure(error):
            logger.error("Accès au fichier refusé : \(error.localizedDescription, privacy: .public)")
            phase = .failed(message: String(localized: "PDF Studio n'a pas pu accéder à ce fichier."))
        }
    }
}
