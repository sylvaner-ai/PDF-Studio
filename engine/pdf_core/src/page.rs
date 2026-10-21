/// Dimensions d'une page en points PDF (1 pt = 1/72 de pouce), rotation `/Rotate` incluse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// Largeur en points.
    pub width: f32,
    /// Hauteur en points.
    pub height: f32,
}

impl PageSize {
    /// Format A4 portrait, utilisé quand une page est illisible.
    pub const A4: PageSize = PageSize {
        width: 595.0,
        height: 842.0,
    };
}

/// Contenu d'une page, prêt à être rastérisé.
///
/// Il s'agit d'une liste d'affichage MuPDF : elle est construite une fois, sur le
/// fil d'exécution propriétaire du document, puis peut être rendue en parallèle
/// depuis n'importe quel fil (`Send + Sync`).
pub struct PageContent {
    list: mupdf::DisplayList,
    size: PageSize,
}

impl PageContent {
    pub(crate) fn new(list: mupdf::DisplayList, size: PageSize) -> Self {
        Self { list, size }
    }

    /// Dimensions de la page en points.
    pub fn size(&self) -> PageSize {
        self.size
    }

    /// Liste d'affichage sous-jacente, pour le module de rendu.
    pub fn display_list(&self) -> &mupdf::DisplayList {
        &self.list
    }
}

impl std::fmt::Debug for PageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageContent")
            .field("size", &self.size)
            .finish()
    }
}
