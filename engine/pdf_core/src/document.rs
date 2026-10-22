use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use edit::{FontChoice, PageEdits};
use mupdf::pdf::PdfDocument;

use crate::editing::{self, EmbeddedFonts, FontProvider, ObjectInfo, PageModel, Scratch};
use crate::error::{PdfError, Result};
use crate::outline::{self, OutlineEntry};
use crate::page::{PageContent, PageSize};
use crate::text::PageText;

/// Un document PDF ouvert en mémoire.
///
/// Le fichier source est lu une seule fois puis n'est plus jamais touché : toute
/// la suite travaille sur la copie en mémoire. Ce type n'est pas `Send` (contrainte
/// de MuPDF) ; pour un usage multi-fil, passer par [`crate::DocumentHandle`].
pub struct Document {
    inner: PdfDocument,
    page_sizes: Vec<PageSize>,
    title: Option<String>,
    editing: Editing,
}

/// Nombre de pages dont les objets sont gardés en mémoire.
const MODEL_CACHE_PAGES: usize = 16;

/// État de l'édition du contenu des pages : objets des pages, polices de
/// substitution chargées et ajoutées au document pour l'aperçu.
#[derive(Default)]
struct Editing {
    models: HashMap<u32, Arc<PageModel>>,
    fonts: FontProvider,
    embedded: EmbeddedFonts,
    scratch: Scratch,
}

impl Document {
    /// Ouvre un fichier PDF en lecture seule.
    pub fn open_file(path: &Path, password: Option<&str>) -> Result<Self> {
        Self::from_bytes(&read_file(path)?, password)
    }

    /// Ouvre un PDF à partir de son contenu binaire.
    pub fn from_bytes(bytes: &[u8], password: Option<&str>) -> Result<Self> {
        crate::fonts::install();
        let invalid = |err: mupdf::Error| PdfError::InvalidDocument {
            detail: err.to_string(),
        };
        let mut inner = mupdf::Document::from_bytes(bytes, "application/pdf").map_err(invalid)?;
        if !inner.is_pdf() {
            return Err(PdfError::InvalidDocument {
                detail: "le contenu n'a pas été reconnu comme PDF".into(),
            });
        }
        if inner.needs_password()? {
            let password = password.ok_or(PdfError::PasswordRequired)?;
            if !inner.authenticate(password)? {
                return Err(PdfError::WrongPassword);
            }
        }

        let inner = PdfDocument::try_from(inner).map_err(invalid)?;
        let count = inner.page_count().map_err(invalid)?;
        let page_sizes = (0..count)
            .map(|index| measure_page(&inner, index))
            .collect();
        let title = inner
            .metadata(mupdf::MetadataName::Title)
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());

        Ok(Self {
            inner,
            page_sizes,
            title,
            editing: Editing::default(),
        })
    }

    /// Nombre de pages.
    pub fn page_count(&self) -> u32 {
        self.page_sizes.len() as u32
    }

    /// Dimensions de toutes les pages, dans l'ordre.
    pub fn page_sizes(&self) -> &[PageSize] {
        &self.page_sizes
    }

    /// Titre déclaré dans les métadonnées, s'il existe.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Construit le contenu affichable d'une page (annotations comprises).
    pub fn page_content(&self, index: u32) -> Result<PageContent> {
        let size = self.page_size(index)?;
        let page = self.inner.load_page(index as i32)?;
        let list = page.to_display_list(true)?;
        Ok(PageContent::new(list, size))
    }

    /// Plan du document (signets), aplati ; vide si le document n'en a pas.
    ///
    /// Un plan illisible est traité comme absent : il ne doit pas empêcher la lecture.
    pub fn outline(&self) -> Vec<OutlineEntry> {
        self.inner
            .outlines()
            .map(|tree| outline::flatten(&tree, self.page_count()))
            .unwrap_or_default()
    }

    /// Texte d'une page, préparé pour la recherche.
    pub fn page_text(&self, index: u32) -> Result<PageText> {
        self.page_size(index)?;
        let page = self.inner.load_page(index as i32)?;
        let text = page.to_text_page(mupdf::TextPageFlags::empty())?;
        Ok(PageText::from_text_page(&text))
    }

    /// Texte brut d'une page, dans l'ordre de lecture établi par MuPDF ; les mots
    /// coupés en fin de ligne sont recollés.
    pub fn plain_text(&self, index: u32) -> Result<String> {
        self.page_size(index)?;
        let page = self.inner.load_page(index as i32)?;
        let text = page.to_text_page(mupdf::TextPageFlags::DEHYPHENATE)?;
        Ok(text.to_text()?)
    }

    /// Objets de la page, établis à partir de son contenu d'origine.
    pub fn page_model(&mut self, index: u32) -> Result<Arc<PageModel>> {
        self.page_size(index)?;
        if let Some(model) = self.editing.models.get(&index) {
            return Ok(Arc::clone(model));
        }
        let page = self.inner.find_page(index as i32)?;
        let model = Arc::new(PageModel::load(&page)?);
        if self.editing.models.len() >= MODEL_CACHE_PAGES {
            self.editing.models.clear();
        }
        self.editing.models.insert(index, Arc::clone(&model));
        Ok(model)
    }

    /// Structure masquée du texte de la page (cf. [`PageModel::text_structure`]).
    pub fn text_structure(&mut self, index: u32) -> Result<String> {
        Ok(self.page_model(index)?.text_structure())
    }

    /// Objets de la page, modifications `edits` comprises.
    pub fn page_objects(
        &mut self,
        index: u32,
        edits: Option<&PageEdits>,
    ) -> Result<Vec<ObjectInfo>> {
        let model = self.page_model(index)?;
        Ok(model.infos(edits, &mut self.editing.fonts))
    }

    /// Police qui dessinera `text` à la place du bloc de texte `object`.
    pub fn choose_font(&mut self, index: u32, object: u32, text: &str) -> Result<FontChoice> {
        let model = self.page_model(index)?;
        editing::choose_font(
            &mut self.inner,
            index,
            &model,
            object,
            text,
            &mut self.editing.scratch,
        )
    }

    /// Contenu affichable de la page avec les modifications `edits`.
    pub fn edited_page_content(&mut self, index: u32, edits: &PageEdits) -> Result<PageContent> {
        let size = self.page_size(index)?;
        let model = self.page_model(index)?;
        let page = self.inner.find_page(index as i32)?;
        let editing = &mut self.editing;
        let edited = editing::edit_page(
            &mut self.inner,
            &page,
            &model,
            edits,
            &mut editing.fonts,
            &mut editing.embedded,
        )?;
        editing::with_page_content(
            &mut self.inner,
            index,
            &edited.content,
            edited.resources.as_ref(),
            &mut editing.scratch,
            |doc| {
                let list = doc.load_page(index as i32)?.to_display_list(true)?;
                Ok(PageContent::new(list, size))
            },
        )
    }

    fn page_size(&self, index: u32) -> Result<PageSize> {
        self.page_sizes
            .get(index as usize)
            .copied()
            .ok_or(PdfError::PageOutOfRange {
                index,
                count: self.page_count(),
            })
    }
}

/// Lit le contenu d'un fichier.
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| PdfError::Unreadable {
        path: path.to_owned(),
        source,
    })
}

/// Mesure une page ; une page illisible reçoit le format A4 pour ne pas bloquer
/// l'ouverture du reste du document.
fn measure_page(doc: &mupdf::Document, index: i32) -> PageSize {
    doc.load_page(index)
        .and_then(|page| page.bounds())
        .map(|b| PageSize {
            width: b.x1 - b.x0,
            height: b.y1 - b.y0,
        })
        .unwrap_or(PageSize::A4)
}
