use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use edit::{FontChoice, PageEdits, ScanCandidate, ScanStyle, TextLayer};
use mupdf::pdf::PdfDocument;

use crate::content::geometry::{Matrix, Point};
use crate::editing::{self, EmbeddedFonts, FontProvider, ObjectInfo, PageModel, Scratch};
use crate::error::{PdfError, Result};
use crate::outline::{self, OutlineEntry};
use crate::page::{PageContent, PageSize};
use crate::scanned::{self, is_scan_id, Retouched, ScanModel};
use crate::text::{PageRect, PageText, TextLine};

/// Ce que la reconnaissance du texte apporterait à une page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecognitionNeed {
    /// La page a du texte visible, ou ne dessine rien : rien à reconnaître.
    None,
    /// Aucun texte, mais un contenu (image d'un scan, texte vectorisé).
    Missing,
    /// Seulement du texte invisible posé sur un scan, laissé par un OCR
    /// précédent : une nouvelle reconnaissance le remplace (la page devient
    /// alors retouchable comme un scan reconnu dans la session).
    Replaceable,
}

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

/// Nombre de pages scannées dont le rendu et les objets sont gardés en
/// mémoire (plusieurs dizaines de mégaoctets chacune).
const SCAN_CACHE_PAGES: usize = 3;

/// Nombre de versions retouchées de pages scannées gardées en mémoire.
const RETOUCH_CACHE: usize = 4;

/// État de l'édition du contenu des pages : objets des pages, polices de
/// substitution chargées et ajoutées au document pour l'aperçu.
#[derive(Default)]
struct Editing {
    models: HashMap<u32, Arc<PageModel>>,
    fonts: FontProvider,
    embedded: EmbeddedFonts,
    scratch: Scratch,
    /// Pages scannées : empreinte de la couche de texte et objets.
    scans: HashMap<u32, (u64, Option<Arc<ScanModel>>)>,
    /// Retouches calculées : (page, version des modifications, empreinte de
    /// la couche).
    retouched: Vec<((u32, u64, u64), Arc<Retouched>)>,
    /// Besoin de reconnaissance de chaque page déjà examinée (le document
    /// source ne change pas).
    recognition: HashMap<u32, RecognitionNeed>,
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
        if !matches!(outline::prepare(&self.inner), Ok(true)) {
            return Vec::new();
        }
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

    /// Lignes de texte de la page (texte invisible compris), avec leur emprise
    /// dans l'espace d'affichage, dans l'ordre établi par MuPDF.
    pub fn text_lines(&self, index: u32) -> Result<Vec<TextLine>> {
        self.page_size(index)?;
        let page = self.inner.load_page(index as i32)?;
        let text = page.to_text_page(mupdf::TextPageFlags::empty())?;
        let mut lines = Vec::new();
        for block in text.blocks() {
            if block.r#type() != mupdf::text_page::TextBlockType::Text {
                continue;
            }
            for line in block.lines() {
                let content: String = line.chars().filter_map(|c| c.char()).collect();
                if content.trim().is_empty() {
                    continue;
                }
                let mut rect: Option<PageRect> = None;
                for c in line.chars() {
                    let glyph = PageRect::from_quad(c.quad());
                    rect = Some(rect.map_or(glyph, |r| r.union(glyph)));
                }
                if let Some(rect) = rect {
                    lines.push(TextLine {
                        text: content,
                        rect,
                    });
                }
            }
        }
        Ok(lines)
    }

    /// Vrai si la page doit passer par la reconnaissance de texte : elle ne
    /// contient aucun texte mais dessine quelque chose (image d'un scan, texte
    /// vectorisé), ou son seul texte est la couche invisible d'un OCR
    /// précédent, posée sur un scan (cf. [`RecognitionNeed`]).
    pub fn needs_recognition(&mut self, index: u32) -> Result<bool> {
        Ok(self.recognition_need(index)? != RecognitionNeed::None)
    }

    /// Ce que la reconnaissance du texte apporterait à la page ; le résultat
    /// est retenu, la page n'est examinée qu'une fois.
    pub fn recognition_need(&mut self, index: u32) -> Result<RecognitionNeed> {
        if let Some(&need) = self.editing.recognition.get(&index) {
            return Ok(need);
        }
        let need = if self.text_lines(index)?.is_empty() {
            if self.page_model(index)?.objects().is_empty() {
                RecognitionNeed::None
            } else {
                RecognitionNeed::Missing
            }
        } else if self.has_replaceable_text(index)? {
            RecognitionNeed::Replaceable
        } else {
            RecognitionNeed::None
        };
        self.editing.recognition.insert(index, need);
        Ok(need)
    }

    /// Vrai si le texte de la page est une couche invisible posée sur un scan
    /// (cf. [`scanned::has_replaceable_text`]).
    fn has_replaceable_text(&mut self, index: u32) -> Result<bool> {
        let size = self.page_size(index)?;
        let page = self.inner.find_page(index as i32)?;
        // Un texte invisible demande un mode de rendu (`3 Tr`) : sans opérateur
        // `Tr`, la page est écartée sans interpréter son contenu.
        if !has_render_mode(&editing::page_contents(&page)?) {
            return Ok(false);
        }
        let model = self.page_model(index)?;
        let area = f64::from(size.width) * f64::from(size.height);
        scanned::has_replaceable_text(&page, &model, area)
    }

    /// Transformation de l'espace de la page PDF vers l'espace d'affichage
    /// (origine en haut à gauche, rotation `/Rotate` comprise).
    pub fn display_matrix(&mut self, index: u32) -> Result<Matrix> {
        Ok(self.page_model(index)?.display_matrix())
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

    /// Objets de la page, modifications `edits` comprises. Une page scannée
    /// reconnue (couche de texte `layer`) montre les objets de son image :
    /// lignes de texte, tableaux, éléments graphiques et filets.
    pub fn page_objects(
        &mut self,
        index: u32,
        edits: Option<&PageEdits>,
        layer: Option<&TextLayer>,
    ) -> Result<Vec<ObjectInfo>> {
        let model = self.page_model(index)?;
        let mut infos = model.infos(edits, &mut self.editing.fonts);
        if let (Some(scan), Some(layer)) = (self.scan_model(index, layer)?, layer) {
            infos.retain(|info| info.id != scan.image.object);
            let retouched = edits
                .filter(|edits| has_scan_edits(edits))
                .map(|edits| self.retouched(index, &scan, layer, edits));
            infos.extend(scanned::infos(&scan, edits, retouched.as_deref()));
        }
        Ok(infos)
    }

    /// Réglages mesurés de la ligne scannée `object` et les polices qui lui
    /// ressemblent le plus ; `None` si ce n'est pas une ligne d'un scan.
    pub fn scan_style(
        &mut self,
        index: u32,
        object: u32,
        layer: Option<&TextLayer>,
    ) -> Result<Option<(ScanStyle, Vec<ScanCandidate>)>> {
        let (Some(scan), Some(layer)) = (self.scan_model(index, layer)?, layer) else {
            return Ok(None);
        };
        Ok(scan.line(object).map(|line| scan.match_style(line, layer)))
    }

    /// Couche de texte de la page avec les retouches de son scan : mots
    /// réécrits, déplacés ou supprimés.
    pub fn effective_layer(
        &mut self,
        index: u32,
        edits: Option<&PageEdits>,
        layer: &TextLayer,
    ) -> Result<TextLayer> {
        let Some(edits) = edits.filter(|edits| has_scan_edits(edits)) else {
            return Ok(layer.clone());
        };
        match self.scan_model(index, Some(layer))? {
            Some(scan) => Ok(self.retouched(index, &scan, layer, edits).layer.clone()),
            None => Ok(layer.clone()),
        }
    }

    /// Objets de la page scannée `index` reconnue avec `layer` ; `None` si la
    /// page n'est pas un scan ou n'est pas reconnue.
    pub(crate) fn scan_model(
        &mut self,
        index: u32,
        layer: Option<&TextLayer>,
    ) -> Result<Option<Arc<ScanModel>>> {
        let Some(layer) = layer.filter(|layer| !layer.is_empty()) else {
            return Ok(None);
        };
        let key = scanned::layer_key(layer);
        if let Some((known, model)) = self.editing.scans.get(&index) {
            if *known == key {
                return Ok(model.clone());
            }
        }
        let size = self.page_size(index)?;
        let model = self.page_model(index)?;
        let page = self.inner.find_page(index as i32)?;
        let area = f64::from(size.width) * f64::from(size.height);
        let built = match scanned::find_scan(&page, &model, area)? {
            Some(image) => {
                let base = scanned::render(&self.inner, index, &image.placement)?;
                Some(Arc::new(ScanModel::build(image, base, layer)))
            }
            None => None,
        };
        if self.editing.scans.len() >= SCAN_CACHE_PAGES {
            self.editing.scans.clear();
        }
        self.editing.scans.insert(index, (key, built.clone()));
        Ok(built)
    }

    /// Effet des modifications `edits` sur les pixels du scan `scan`.
    pub(crate) fn retouched(
        &mut self,
        index: u32,
        scan: &ScanModel,
        layer: &TextLayer,
        edits: &PageEdits,
    ) -> Arc<Retouched> {
        let key = (index, edits.revision(), scanned::layer_key(layer));
        if let Some((_, done)) = self.editing.retouched.iter().find(|(k, _)| *k == key) {
            return Arc::clone(done);
        }
        let done = Arc::new(scanned::retouch(scan, layer, edits));
        if self.editing.retouched.len() >= RETOUCH_CACHE {
            self.editing.retouched.remove(0);
        }
        self.editing.retouched.push((key, Arc::clone(&done)));
        done
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

    /// Contenu affichable de la page avec les modifications `edits` ; pour
    /// une page scannée reconnue (couche `layer`), les pixels retouchés sont
    /// posés par-dessus.
    pub fn edited_page_content(
        &mut self,
        index: u32,
        edits: &PageEdits,
        layer: Option<&TextLayer>,
    ) -> Result<PageContent> {
        let size = self.page_size(index)?;
        let vector = vector_edits(edits);
        let content = if vector.is_empty() {
            self.page_content(index)?
        } else {
            self.vector_page_content(index, &vector)?
        };
        let scan = match layer {
            Some(layer) if has_scan_edits(edits) => {
                self.scan_model(index, Some(layer))?.map(|scan| {
                    (
                        Arc::clone(&scan),
                        self.retouched(index, &scan, layer, edits),
                    )
                })
            }
            _ => None,
        };
        match scan {
            Some((scan, retouched)) => {
                let list =
                    scanned::compose(content.display_list(), &scan.image.placement, &retouched)?;
                Ok(PageContent::new(list, size))
            }
            None => Ok(content),
        }
    }

    /// Contenu affichable de la page avec les modifications `edits` de son
    /// contenu (hors retouches d'un scan).
    fn vector_page_content(&mut self, index: u32, edits: &PageEdits) -> Result<PageContent> {
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

/// Vrai si `edits` touche des objets d'un scan.
pub(crate) fn has_scan_edits(edits: &PageEdits) -> bool {
    edits.iter().any(|(id, _)| is_scan_id(id))
}

/// Modifications du contenu de la page, sans les retouches d'un scan.
pub(crate) fn vector_edits(edits: &PageEdits) -> PageEdits {
    PageEdits::new(
        edits
            .iter()
            .filter(|(id, _)| !is_scan_id(*id))
            .map(|(id, edit)| (id, edit.clone()))
            .collect(),
        edits.revision(),
    )
}

/// Mots d'une couche de texte, ligne par ligne, avec leur emprise dans
/// l'espace d'affichage.
pub(crate) fn layer_lines(layer: &TextLayer, to_display: &Matrix) -> Vec<Vec<(String, PageRect)>> {
    let mut lines: Vec<Vec<(String, PageRect)>> = Vec::new();
    let mut current = None;
    for word in &layer.words {
        let corners = [
            word.bottom_left(),
            word.bottom_right(),
            word.top_right(),
            word.top_left(),
        ]
        .map(|(x, y)| to_display.apply(Point::new(f64::from(x), f64::from(y))));
        let x0 = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let y0 = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let x1 = corners
            .iter()
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let y1 = corners
            .iter()
            .map(|p| p.y)
            .fold(f64::NEG_INFINITY, f64::max);
        let rect = PageRect {
            x: x0 as f32,
            y: y0 as f32,
            width: (x1 - x0) as f32,
            height: (y1 - y0) as f32,
        };
        if current != Some(word.line) || lines.is_empty() {
            lines.push(Vec::new());
            current = Some(word.line);
        }
        if let Some(line) = lines.last_mut() {
            line.push((word.text.clone(), rect));
        }
    }
    lines
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

/// Vrai si le flux de contenu `source` contient l'opérateur `Tr` (mode de
/// rendu du texte), précédé d'un blanc et suivi d'un blanc, d'un délimiteur ou
/// de la fin du flux. Un nom comme `/TrebuchetMS` ne compte pas.
fn has_render_mode(source: &[u8]) -> bool {
    let is_space = |b: u8| matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b'\x0c' | b'\0');
    source.windows(2).enumerate().any(|(start, pair)| {
        pair == b"Tr"
            && start > 0
            && is_space(source[start - 1])
            && source
                .get(start + 2)
                .is_none_or(|&next| is_space(next) || b"/[]()<>{}%".contains(&next))
    })
}

#[cfg(test)]
mod tests {
    use super::has_render_mode;

    #[test]
    fn repere_le_mode_de_rendu() {
        assert!(has_render_mode(b"BT 3 Tr /F1 10 Tf"));
        assert!(has_render_mode(b"BT 3\nTr"));
        assert!(has_render_mode(b"3 Tr/F1 10 Tf"));
        assert!(!has_render_mode(b"BT /TrebuchetMS 10 Tf (Trois) Tj"));
        assert!(!has_render_mode(b"Tr"));
    }
}
