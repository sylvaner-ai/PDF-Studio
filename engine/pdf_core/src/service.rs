use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;

use crate::document::{self, Document};
use crate::error::{PdfError, Result};
use crate::outline::OutlineEntry;
use crate::page::{PageContent, PageSize};
use crate::qos::Qos;
use crate::text::{PageText, TextMatch};
use crate::writer::SourceData;

/// Nombre de pages dont le contenu préparé est gardé en mémoire.
const CONTENT_CACHE_PAGES: usize = 48;

/// Nombre de pages dont le texte est gardé en mémoire pour la recherche
/// (quelques dizaines de kilo-octets par page).
const TEXT_CACHE_PAGES: usize = 256;

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
                    let doc = Document::from_bytes(&bytes, password.as_deref())?;
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

    /// Exécute une requête sur le fil du document et attend sa réponse.
    fn run<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Worker) -> R + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |worker| {
            let _ = reply_tx.send(job(worker));
        });
        self.jobs
            .send((Qos::current(), job))
            .map_err(|_| PdfError::Closed)?;
        reply_rx.recv().map_err(|_| PdfError::Closed)
    }
}

struct Worker {
    doc: Document,
    contents: Lru<Arc<PageContent>>,
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

    fn page_text(&mut self, index: u32) -> Result<Arc<PageText>> {
        if let Some(text) = self.texts.get(index) {
            return Ok(text);
        }
        let text = Arc::new(self.doc.page_text(index)?);
        self.texts.insert(index, Arc::clone(&text));
        Ok(text)
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
}
