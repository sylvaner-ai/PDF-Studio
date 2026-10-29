use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;

use edit::{FontChoice, PageEdits, ScanCandidate, ScanStyle, TextLayer};

use crate::clip::Clip;
use crate::content::geometry::Matrix;
use crate::document::{self, layer_lines, Document, RecognitionNeed};
use crate::editing::ObjectInfo;
use crate::enhanced::ScanPixels;
use crate::error::{PdfError, Result};
use crate::outline::OutlineEntry;
use crate::page::{PageContent, PageSize};
use crate::qos::Qos;
use crate::text::{GlyphLine, PageRect, PageText, TextLine, TextMatch};
use crate::writer::SourceData;

/// Nombre de pages dont le contenu préparé est gardé en mémoire.
const CONTENT_CACHE_PAGES: usize = 48;

/// Nombre de pages modifiées dont le contenu préparé est gardé en mémoire
/// (une version par page).
const EDITED_CACHE_PAGES: usize = 16;

/// Nombre de pages dont le texte est gardé en mémoire pour la recherche
/// (quelques dizaines de kilo-octets par page).
const TEXT_CACHE_PAGES: usize = 256;

/// Pile du fil d'un document : MuPDF parcourt récursivement certaines
/// structures (plan, arbre des pages, objets imbriqués), plus profondes dans
/// un fichier malformé ; la pile par défaut d'un fil (2 Mio) laisse peu de
/// marge.
const DOCUMENT_STACK: usize = 16 << 20;

/// Origine des données d'un document.
pub enum Source {
    /// Fichier sur le disque, lu une seule fois à l'ouverture.
    File(PathBuf),
    /// Contenu déjà en mémoire.
    Bytes(Vec<u8>),
}

type Job = Box<dyn FnOnce(&mut Worker) + Send>;

/// Requête et qualité de service du fil qui en attend la réponse.
type Request = (Qos, Job);

/// Accès multi-fil à un document.
///
/// MuPDF impose qu'un document ne soit manipulé que depuis un seul fil. Ce type
/// lance un fil dédié qui possède le [`Document`] et exécute les requêtes une à
/// une, à la qualité de service du fil demandeur. Les contenus de page renvoyés
/// ([`PageContent`]) peuvent ensuite être rastérisés en parallèle par l'appelant.
/// Le fil s'arrête quand la poignée est libérée.
///
/// Une requête qui panique (défaut du moteur sur un fichier inattendu) échoue
/// seule, avec une erreur : le fil survit et le document reste utilisable.
pub struct DocumentHandle {
    jobs: mpsc::Sender<Request>,
    page_sizes: Vec<PageSize>,
    title: Option<String>,
    data: SourceData,
}

/// Ce que le fil du document renvoie une fois le document ouvert.
struct Summary {
    page_sizes: Vec<PageSize>,
    title: Option<String>,
    data: SourceData,
}

impl DocumentHandle {
    /// Ouvre un document et attend que ses pages soient mesurées.
    pub fn open(source: Source, password: Option<String>) -> Result<Self> {
        let (jobs, job_rx) = mpsc::channel::<Request>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let opener_qos = Qos::current();

        thread::Builder::new()
            .name("pdf-document".into())
            .stack_size(DOCUMENT_STACK)
            .spawn(move || {
                let mut qos = if opener_qos.apply() {
                    opener_qos
                } else {
                    Qos::current()
                };
                let opened = match source {
                    Source::File(path) => document::read_file(&path),
                    Source::Bytes(bytes) => Ok(bytes),
                }
                .and_then(|bytes| {
                    let doc = panic::catch_unwind(AssertUnwindSafe(|| {
                        Document::from_bytes(&bytes, password.as_deref())
                    }))
                    .map_err(|payload| PdfError::InvalidDocument {
                        detail: panic_detail(payload.as_ref()),
                    })??;
                    Ok((doc, SourceData::new(bytes, password)))
                });
                let (doc, data) = match opened {
                    Ok(opened) => opened,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                let summary = Summary {
                    page_sizes: doc.page_sizes().to_vec(),
                    title: doc.title().map(str::to_owned),
                    data,
                };
                let _ = ready_tx.send(Ok(summary));

                let mut worker = Worker {
                    doc,
                    contents: Lru::new(CONTENT_CACHE_PAGES),
                    edited: Lru::new(EDITED_CACHE_PAGES),
                    texts: Lru::new(TEXT_CACHE_PAGES),
                };
                for (wanted, job) in job_rx {
                    if wanted != qos && wanted.apply() {
                        qos = wanted;
                    }
                    job(&mut worker);
                }
            })
            .map_err(|err| PdfError::Engine {
                detail: format!("création du fil du document : {err}"),
            })?;

        let summary = ready_rx.recv().map_err(|_| PdfError::Closed)??;
        Ok(Self {
            jobs,
            page_sizes: summary.page_sizes,
            title: summary.title,
            data: summary.data,
        })
    }

    /// Nombre de pages.
    pub fn page_count(&self) -> u32 {
        self.page_sizes.len() as u32
    }

    /// Dimensions de toutes les pages, en points.
    pub fn page_sizes(&self) -> &[PageSize] {
        &self.page_sizes
    }

    /// Titre déclaré dans les métadonnées.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Contenu du document tel que lu à l'ouverture, et son mot de passe, à partir
    /// desquels s'écrit une version modifiée.
    pub fn source_data(&self) -> &SourceData {
        &self.data
    }

    /// Contenu préparé d'une page, mis en cache.
    pub fn page_content(&self, index: u32) -> Result<Arc<PageContent>> {
        self.run(move |worker| worker.page_content(index))?
    }

    /// Contenu préparé d'une page avec les modifications `edits` (et, pour une
    /// page scannée reconnue, sa couche de texte `layer`) ; la dernière
    /// version de chaque page est mise en cache.
    pub fn edited_page_content(
        &self,
        index: u32,
        edits: Arc<PageEdits>,
        layer: Option<Arc<TextLayer>>,
    ) -> Result<Arc<PageContent>> {
        if edits.is_empty() {
            return self.page_content(index);
        }
        self.run(move |worker| worker.edited_page_content(index, &edits, layer.as_deref()))?
    }

    /// Objets d'une page (blocs de texte, images, tracés…), modifications
    /// `edits` comprises ; une page scannée reconnue (couche `layer`) montre
    /// les objets de son image.
    pub fn page_objects(
        &self,
        index: u32,
        edits: Option<Arc<PageEdits>>,
        layer: Option<Arc<TextLayer>>,
    ) -> Result<Vec<ObjectInfo>> {
        self.run(move |worker| {
            worker
                .doc
                .page_objects(index, edits.as_deref(), layer.as_deref())
        })?
    }

    /// Réglages mesurés de la ligne scannée `object` et les polices qui lui
    /// ressemblent le plus ; `None` si ce n'est pas une ligne d'un scan.
    pub fn scan_style(
        &self,
        index: u32,
        object: u32,
        layer: Option<Arc<TextLayer>>,
    ) -> Result<Option<(ScanStyle, Vec<ScanCandidate>)>> {
        self.run(move |worker| worker.doc.scan_style(index, object, layer.as_deref()))?
    }

    /// Couche de texte `layer` de la page avec les retouches de son scan.
    pub fn effective_layer(
        &self,
        index: u32,
        edits: Option<Arc<PageEdits>>,
        layer: Arc<TextLayer>,
    ) -> Result<Arc<TextLayer>> {
        if !edits.as_ref().is_some_and(|edits| !edits.is_empty()) {
            return Ok(layer);
        }
        self.run(move |worker| {
            worker
                .doc
                .effective_layer(index, edits.as_deref(), &layer)
                .map(Arc::new)
        })?
    }

    /// Police qui dessinera `text` à la place du bloc de texte `object` de la
    /// page : police d'origine, police du Mac ou police standard.
    pub fn choose_font(&self, index: u32, object: u32, text: &str) -> Result<FontChoice> {
        let text = text.to_owned();
        self.run(move |worker| worker.doc.choose_font(index, object, &text))?
    }

    /// Déplacement exprimé dans l'espace d'affichage d'une page (origine en
    /// haut à gauche), converti dans l'espace de la page PDF.
    pub fn page_offset(&self, index: u32, dx: f32, dy: f32) -> Result<(f32, f32)> {
        self.run(move |worker| {
            let model = worker.doc.page_model(index)?;
            let (x, y) = model.display_to_page_offset(f64::from(dx), f64::from(dy));
            Ok((x as f32, y as f32))
        })?
    }

    /// Copie des objets `objects` de la page en PDF d'une page (cf.
    /// [`Document::clip`]).
    pub fn clip(
        &self,
        index: u32,
        edits: Option<Arc<PageEdits>>,
        objects: Vec<u32>,
        quarter_turns: u8,
    ) -> Result<Option<Clip>> {
        self.run(move |worker| {
            worker
                .doc
                .clip(index, edits.as_deref(), &objects, quarter_turns)
        })?
    }

    /// Agrandissement et déplacement qui placent l'objet d'origine `object`
    /// dans le cadre `target` (cf. [`Document::resize_object`]).
    pub fn resize_object(
        &self,
        index: u32,
        edits: Option<Arc<PageEdits>>,
        object: u32,
        target: PageRect,
    ) -> Result<Option<([f32; 2], f32, f32)>> {
        self.run(move |worker| {
            worker
                .doc
                .resize_object(index, edits.as_deref(), object, &target)
        })?
    }

    /// Plan du document (signets), aplati en parcours en profondeur.
    pub fn outline(&self) -> Result<Vec<OutlineEntry>> {
        self.run(|worker| worker.doc.outline())
    }

    /// Cherche `query` dans une page, sans tenir compte de la casse ni des accents.
    ///
    /// La recherche se fait page par page pour que l'interface puisse afficher les
    /// résultats au fil de l'eau et l'interrompre à tout moment.
    pub fn search_page(&self, index: u32, query: &str) -> Result<Vec<TextMatch>> {
        let query = query.to_owned();
        self.run(move |worker| Ok(worker.page_text(index)?.search(&query)))?
    }

    /// Cherche `query` dans une page et dans sa couche de texte `layer`.
    pub fn search_page_with_layer(
        &self,
        index: u32,
        query: &str,
        layer: Arc<TextLayer>,
    ) -> Result<Vec<TextMatch>> {
        let query = query.to_owned();
        self.run(move |worker| Ok(worker.page_text_with_layer(index, &layer)?.search(&query)))?
    }

    /// Caractères de la page et leur emprise, ligne par ligne, pour la
    /// sélection du texte (cf. [`PageText::glyph_lines`]).
    pub fn glyph_lines(&self, index: u32) -> Result<Vec<GlyphLine>> {
        self.run(move |worker| Ok(worker.page_text(index)?.glyph_lines()))?
    }

    /// Comme [`DocumentHandle::glyph_lines`], suivi des lignes de la couche de
    /// texte `layer`.
    pub fn glyph_lines_with_layer(
        &self,
        index: u32,
        layer: Arc<TextLayer>,
    ) -> Result<Vec<GlyphLine>> {
        self.run(move |worker| Ok(worker.page_text_with_layer(index, &layer)?.glyph_lines()))?
    }

    /// Lignes de texte d'une page (texte invisible compris), avec leur emprise
    /// dans l'espace d'affichage.
    pub fn text_lines(&self, index: u32) -> Result<Vec<TextLine>> {
        self.run(move |worker| worker.doc.text_lines(index))?
    }

    /// Vrai si la page doit passer par la reconnaissance de texte (cf.
    /// [`Document::needs_recognition`]).
    pub fn needs_recognition(&self, index: u32) -> Result<bool> {
        self.run(move |worker| worker.doc.needs_recognition(index))?
    }

    /// Ce que la reconnaissance du texte apporterait à une page.
    pub fn recognition_need(&self, index: u32) -> Result<RecognitionNeed> {
        self.run(move |worker| worker.doc.recognition_need(index))?
    }

    /// Transformation de l'espace de la page PDF vers son espace d'affichage.
    pub fn display_matrix(&self, index: u32) -> Result<Matrix> {
        self.run(move |worker| worker.doc.display_matrix(index))?
    }

    /// Pixels du scan d'une page (cf. [`Document::scan_pixels`]).
    pub fn scan_pixels(&self, index: u32) -> Result<Option<ScanPixels>> {
        self.run(move |worker| worker.doc.scan_pixels(index))?
    }

    /// Texte brut d'une page, pour l'export en texte.
    pub fn plain_text(&self, index: u32) -> Result<String> {
        self.run(move |worker| worker.doc.plain_text(index))?
    }

    /// Exécute une requête sur le fil du document et attend sa réponse.
    fn run<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Worker) -> R + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |worker| {
            // Les caches ne sont remplis qu'une fois le calcul terminé : une
            // requête interrompue ne laisse rien de partiel derrière elle.
            let result = panic::catch_unwind(AssertUnwindSafe(|| job(worker)));
            let _ = reply_tx.send(result.map_err(|payload| panic_detail(payload.as_ref())));
        });
        self.jobs
            .send((Qos::current(), job))
            .map_err(|_| PdfError::Closed)?;
        reply_rx
            .recv()
            .map_err(|_| PdfError::Closed)?
            .map_err(|detail| PdfError::Engine { detail })
    }
}

/// Préfixe du détail technique d'une erreur due à une panique du moteur.
pub const PANIC_DETAIL: &str = "panique du moteur";

/// Détail technique d'une panique, pour le journal.
fn panic_detail(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    format!("{PANIC_DETAIL} : {message}")
}

struct Worker {
    doc: Document,
    contents: Lru<Arc<PageContent>>,
    /// Contenu modifié : version des modifications, empreinte de la couche
    /// de texte.
    edited: Lru<((u64, u64), Arc<PageContent>)>,
    texts: Lru<Arc<PageText>>,
}

impl Worker {
    fn page_content(&mut self, index: u32) -> Result<Arc<PageContent>> {
        if let Some(content) = self.contents.get(index) {
            return Ok(content);
        }
        let content = Arc::new(self.doc.page_content(index)?);
        self.contents.insert(index, Arc::clone(&content));
        Ok(content)
    }

    fn edited_page_content(
        &mut self,
        index: u32,
        edits: &PageEdits,
        layer: Option<&TextLayer>,
    ) -> Result<Arc<PageContent>> {
        let key = (edits.revision(), layer.map_or(0, crate::scanned::layer_key));
        if let Some((known, content)) = self.edited.get(index) {
            if known == key {
                return Ok(content);
            }
        }
        let content = Arc::new(self.doc.edited_page_content(index, edits, layer)?);
        self.edited.insert(index, (key, Arc::clone(&content)));
        Ok(content)
    }

    fn page_text(&mut self, index: u32) -> Result<Arc<PageText>> {
        if let Some(text) = self.texts.get(index) {
            return Ok(text);
        }
        let text = Arc::new(self.doc.page_text(index)?);
        self.texts.insert(index, Arc::clone(&text));
        Ok(text)
    }

    /// Texte d'une page suivi de sa couche de texte `layer`. Le texte
    /// invisible d'un OCR précédent, que `layer` remplace, n'en fait plus
    /// partie.
    fn page_text_with_layer(&mut self, index: u32, layer: &TextLayer) -> Result<PageText> {
        let replaced =
            !layer.is_empty() && self.doc.recognition_need(index)? == RecognitionNeed::Replaceable;
        let native = if replaced {
            Arc::new(PageText::default())
        } else {
            self.page_text(index)?
        };
        let to_display = self.doc.display_matrix(index)?;
        Ok(native.with_lines(&layer_lines(layer, &to_display)))
    }
}

/// Cache LRU minimal, indexé par numéro de page.
struct Lru<V> {
    capacity: usize,
    entries: HashMap<u32, V>,
    order: VecDeque<u32>,
}

impl<V: Clone> Lru<V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    fn get(&mut self, index: u32) -> Option<V> {
        let value = self.entries.get(&index).cloned()?;
        self.touch(index);
        Some(value)
    }

    fn insert(&mut self, index: u32, value: V) {
        if self.entries.insert(index, value).is_some() {
            self.touch(index);
            return;
        }
        self.order.push_back(index);
        while self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    fn touch(&mut self, index: u32) {
        if let Some(pos) = self.order.iter().position(|&i| i == index) {
            self.order.remove(pos);
        }
        self.order.push_back(index);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::*;
    use crate::qos::{install_qos_hooks, QosHooks};

    thread_local! {
        /// Qualité de service simulée, propre à chaque fil.
        static FAKE_QOS: Cell<u32> = const { Cell::new(0) };
    }

    fn worker_qos(handle: &DocumentHandle) -> u32 {
        handle.run(|_| FAKE_QOS.with(Cell::get)).unwrap()
    }

    #[test]
    fn le_fil_du_document_adopte_la_priorite_du_demandeur() {
        install_qos_hooks(QosHooks {
            current: || FAKE_QOS.with(Cell::get),
            apply: |class| {
                FAKE_QOS.with(|qos| qos.set(class));
                true
            },
        });
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texte_simple.pdf");

        FAKE_QOS.with(|qos| qos.set(0x19));
        let handle = DocumentHandle::open(Source::File(path), None).unwrap();
        assert_eq!(worker_qos(&handle), 0x19);

        FAKE_QOS.with(|qos| qos.set(0x21));
        assert_eq!(worker_qos(&handle), 0x21);

        // Priorité non précisée : le fil garde la précédente.
        FAKE_QOS.with(|qos| qos.set(0));
        assert_eq!(worker_qos(&handle), 0x21);
    }

    #[test]
    fn une_requete_qui_panique_echoue_seule() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texte_simple.pdf");
        let handle = DocumentHandle::open(Source::File(path), None).unwrap();
        let failed = handle.run(|_| -> u32 { panic!("défaut simulé") });
        match failed {
            Err(PdfError::Engine { detail }) => {
                assert!(detail.starts_with(PANIC_DETAIL), "{detail}");
                assert!(detail.contains("défaut simulé"), "{detail}");
            }
            other => panic!("erreur attendue, obtenu {other:?}"),
        }
        // Le fil du document a survécu : la page se lit toujours.
        assert!(handle.page_content(0).is_ok());
        assert!(!handle.plain_text(0).unwrap().is_empty());
    }
}
