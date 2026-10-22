use std::path::PathBuf;

/// Erreurs du moteur PDF.
///
/// Les messages sont rédigés pour être affichés tels quels à l'utilisateur :
/// en français et sans jargon technique.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// Le fichier n'a pas pu être lu sur le disque.
    #[error("Impossible de lire le fichier « {} ».", path.display())]
    Unreadable {
        /// Chemin du fichier concerné.
        path: PathBuf,
        /// Cause système.
        #[source]
        source: std::io::Error,
    },

    /// Le contenu n'est pas un PDF exploitable.
    #[error("Ce fichier n'est pas un PDF valide ou il est trop endommagé pour être ouvert.")]
    InvalidDocument {
        /// Détail technique, destiné au journal et non à l'interface.
        detail: String,
    },

    /// Le document est protégé et aucun mot de passe n'a été fourni.
    #[error("Ce document est protégé par un mot de passe.")]
    PasswordRequired,

    /// Le mot de passe fourni est incorrect.
    #[error("Le mot de passe est incorrect.")]
    WrongPassword,

    /// Une page inexistante a été demandée.
    #[error("La page {} n'existe pas (le document en compte {count}).", index + 1)]
    PageOutOfRange {
        /// Index demandé, à partir de 0.
        index: u32,
        /// Nombre de pages du document.
        count: u32,
    },

    /// Le document a été fermé pendant l'opération.
    #[error("Le document a été fermé.")]
    Closed,

    /// Le PDF modifié n'a pas pu être produit ou n'a pas passé la relecture.
    #[error("Le document n'a pas pu être enregistré.")]
    WriteFailed {
        /// Détail technique, destiné au journal et non à l'interface.
        detail: String,
    },

    /// Erreur interne du moteur de rendu.
    #[error("Une erreur inattendue s'est produite pendant le traitement du document.")]
    Engine {
        /// Détail technique, destiné au journal et non à l'interface.
        detail: String,
    },
}

impl PdfError {
    /// Détail technique éventuel, utile pour le diagnostic.
    pub fn detail(&self) -> Option<&str> {
        match self {
            PdfError::InvalidDocument { detail }
            | PdfError::WriteFailed { detail }
            | PdfError::Engine { detail } => Some(detail),
            _ => None,
        }
    }
}

impl From<mupdf::Error> for PdfError {
    fn from(err: mupdf::Error) -> Self {
        PdfError::Engine {
            detail: err.to_string(),
        }
    }
}

/// Raccourci pour les résultats du moteur.
pub type Result<T, E = PdfError> = std::result::Result<T, E>;
