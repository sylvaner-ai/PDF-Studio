use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;

use crate::document::Document;
use crate::error::{PdfError, Result};
use crate::outline::OutlineEntry;
use crate::page::{PageContent, PageSize};
use crate::text::{PageText, TextMatch};

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

/// Accès multi-fil à un document.
///
/// MuPDF impose qu'un document ne soit manipulé que depuis un seul fil. Ce type
/// lance un fil dédié qui possède le [`Document`] et exécute les requêtes une à
/// une. Les contenus de page renvoyés ([`PageContent`]) peuvent ensuite être
/// rastérisés en parallèle par l'appelant. Le fil s'arrête quand la poignée est
/// libérée.
pub struct DocumentHandle {
    jobs: mpsc::Sender<Job>,
    page_sizes: Vec<PageSize>,
    title: Option<String>,
}

impl DocumentHandle {
    /// Ouvre un document et attend que ses pages soient mesurées.
    pub fn open(source: Source, password: Option<String>) -> Result<Self> {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        thread::Builder::new()
            .name("pdf-document".into())
            .spawn(move || {
                let opened = match source {
                    Source::File(path) => Document::open_file(&path, password.as_deref()),
                    Source::Bytes(bytes) => Document::from_bytes(&bytes, password.as_deref()),
                };
                let doc = match opened {
                    Ok(doc) => doc,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                let summary = (doc.page_sizes().to_vec(), doc.title().map(str::to_owned));
                let _ = ready_tx.send(Ok(summary));

                let mut worker = Worker {
                    doc,
                    contents: Lru::new(CONTENT_CACHE_PAGES),
                    texts: Lru::new(TEXT_CACHE_PAGES),
                };
                for job in job_rx {
                    job(&mut worker);
                }
            })
            .map_err(|err| PdfError::Engine {
                detail: format!("création du fil du document : {err}"),
            })?;

        let (page_sizes, title) = ready_rx.recv().map_err(|_| PdfError::Closed)??;
        Ok(Self {
            jobs,
            page_sizes,
            title,
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
        self.jobs
            .send(Box::new(move |worker| {
                let _ = reply_tx.send(job(worker));
            }))
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
