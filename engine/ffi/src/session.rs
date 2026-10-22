//! Session d'organisation des pages d'un document, avec son historique.

use std::sync::{Arc, Mutex, MutexGuard};

use edit::{EditError, Editor, PageEntry, PageOrigin, RestoredPage};
use pdf_core::{OutputPage, SourceData};

use crate::{EngineError, PageSize, PdfDocument};

/// Contenu d'une page de la session.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum PageKind {
    /// Page `index` du document source `source` (0 : le document ouvert).
    Source {
        /// Numéro du document source.
        source: u32,
        /// Indice de la page dans ce document.
        index: u32,
    },
    /// Page vierge.
    Blank {
        /// Format de la page, en points.
        size: PageSize,
    },
}

/// Une page de la session, dans l'ordre du document.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct PageItem {
    /// Identifiant stable : il suit la page quand elle se déplace.
    pub id: u64,
    /// Ce que la page affiche.
    pub kind: PageKind,
    /// Quarts de tour dans le sens horaire (0 à 3), ajoutés à la rotation propre
    /// de la page.
    pub quarter_turns: u8,
}

impl From<&PageEntry> for PageItem {
    fn from(entry: &PageEntry) -> Self {
        let kind = match entry.origin {
            PageOrigin::Source { source, index } => PageKind::Source { source, index },
            PageOrigin::Blank { width, height } => PageKind::Blank {
                size: PageSize { width, height },
            },
        };
        PageItem {
            id: entry.id,
            kind,
            quarter_turns: entry.quarter_turns,
        }
    }
}

impl From<EditError> for EngineError {
    fn from(err: EditError) -> Self {
        let message = match err {
            EditError::WouldRemoveAllPages => {
                "Un document doit garder au moins une page.".to_owned()
            }
            _ => "Cette opération ne s'applique à aucune page.".to_owned(),
        };
        EngineError::Failed {
            message,
            detail: err.to_string(),
        }
    }
}

/// PDF produit pour l'enregistrement, avec la révision des pages qu'il contient.
#[derive(uniffi::Record)]
pub struct ExportedPdf {
    /// Octets du PDF, relus et vérifiés.
    pub bytes: Vec<u8>,
    /// Révision des pages écrites, à passer à [`EditSession::mark_saved`].
    pub revision: u64,
}

/// Un document source et son nombre de pages.
struct Source {
    data: SourceData,
    page_count: u32,
}

struct State {
    editor: Editor,
    sources: Vec<Source>,
}

/// Organisation des pages d'un document ouvert : rotation, suppression,
/// duplication, déplacement et insertion, chacune annulable.
///
/// Le fichier n'est jamais modifié : l'application demande les octets du
/// nouveau PDF ([`EditSession::export_pdf`]) et décide où les écrire.
#[derive(uniffi::Object)]
pub struct EditSession {
    state: Mutex<State>,
}

#[uniffi::export]
impl EditSession {
    /// Session sur les pages du document ouvert, dans leur ordre.
    #[uniffi::constructor]
    pub fn new(document: Arc<PdfDocument>) -> Arc<Self> {
        let page_count = document.page_count();
        Arc::new(Self {
            state: Mutex::new(State {
                editor: Editor::new(page_count),
                sources: vec![Source {
                    data: document.source_data(),
                    page_count,
                }],
            }),
        })
    }

    /// Ajoute un document dont on pourra insérer des pages ; renvoie son numéro.
    pub fn add_source(&self, document: Arc<PdfDocument>) -> u32 {
        let mut state = self.lock();
        state.sources.push(Source {
            data: document.source_data(),
            page_count: document.page_count(),
        });
        (state.sources.len() - 1) as u32
    }

    /// Pages, dans l'ordre.
    pub fn pages(&self) -> Vec<PageItem> {
        self.lock()
            .editor
            .pages()
            .iter()
            .map(PageItem::from)
            .collect()
    }

    /// Fait pivoter les pages désignées d'un nombre de quarts de tour (positif :
    /// sens horaire).
    pub fn rotate(&self, positions: Vec<u32>, quarter_turns: i32) -> Result<(), EngineError> {
        Ok(self
            .lock()
            .editor
            .rotate(&usizes(&positions), quarter_turns)?)
    }

    /// Supprime les pages désignées ; au moins une page doit rester.
    pub fn delete(&self, positions: Vec<u32>) -> Result<(), EngineError> {
        Ok(self.lock().editor.delete(&usizes(&positions))?)
    }

    /// Duplique les pages désignées, chaque copie juste après son original ;
    /// renvoie les positions des copies.
    pub fn duplicate(&self, positions: Vec<u32>) -> Result<Vec<u32>, EngineError> {
        Ok(u32s(self.lock().editor.duplicate(&usizes(&positions))?))
    }

    /// Déplace les pages désignées devant la page `before` (le nombre de pages :
    /// à la fin), dans leur ordre ; renvoie leurs nouvelles positions.
    pub fn move_pages(&self, positions: Vec<u32>, before: u32) -> Result<Vec<u32>, EngineError> {
        Ok(u32s(
            self.lock()
                .editor
                .move_pages(&usizes(&positions), before as usize)?,
        ))
    }

    /// Insère devant la position `at` des pages du document source `source` ;
    /// renvoie leurs positions.
    pub fn insert_pages(
        &self,
        at: u32,
        source: u32,
        indices: Vec<u32>,
    ) -> Result<Vec<u32>, EngineError> {
        let mut state = self.lock();
        let page_count = state
            .sources
            .get(source as usize)
            .map(|source| source.page_count)
            .ok_or_else(|| invalid("document source inconnu"))?;
        if indices.iter().any(|&index| index >= page_count) {
            return Err(invalid("page source inexistante"));
        }
        let origins: Vec<_> = indices
            .iter()
            .map(|&index| PageOrigin::Source { source, index })
            .collect();
        Ok(u32s(state.editor.insert(at as usize, &origins)?))
    }

    /// Insère une page vierge devant la position `at` ; renvoie sa position.
    pub fn insert_blank(&self, at: u32, size: PageSize) -> Result<u32, EngineError> {
        if !(size.width > 0.0 && size.height > 0.0) {
            return Err(invalid("format de page vierge invalide"));
        }
        let origin = PageOrigin::Blank {
            width: size.width,
            height: size.height,
        };
        let positions = self.lock().editor.insert(at as usize, &[origin])?;
        Ok(positions[0] as u32)
    }

    /// Remplace toutes les pages (modifications non enregistrées retrouvées) ;
    /// les identifiants donnés sont ignorés. Les documents sources doivent avoir
    /// été ajoutés au préalable, dans le même ordre.
    pub fn restore(&self, pages: Vec<PageItem>) -> Result<(), EngineError> {
        let mut state = self.lock();
        let mut entries = Vec::with_capacity(pages.len());
        for page in &pages {
            let origin = match page.kind {
                PageKind::Source { source, index } => {
                    let known = state.sources.get(source as usize);
                    if known.is_none_or(|known| index >= known.page_count) {
                        return Err(invalid("page source inexistante"));
                    }
                    PageOrigin::Source { source, index }
                }
                PageKind::Blank { size } if size.width > 0.0 && size.height > 0.0 => {
                    PageOrigin::Blank {
                        width: size.width,
                        height: size.height,
                    }
                }
                PageKind::Blank { .. } => return Err(invalid("format de page vierge invalide")),
            };
            entries.push(RestoredPage {
                origin,
                quarter_turns: page.quarter_turns,
                content: Default::default(),
            });
        }
        Ok(state.editor.restore(&entries)?)
    }

    /// Défait la dernière modification ; `false` s'il n'y en a pas.
    pub fn undo(&self) -> bool {
        self.lock().editor.undo()
    }

    /// Rétablit la dernière modification annulée ; `false` s'il n'y en a pas.
    pub fn redo(&self) -> bool {
        self.lock().editor.redo()
    }

    /// Nom de l'action qu'« Annuler » déferait, avec son article (« la rotation »).
    pub fn undo_name(&self) -> Option<String> {
        self.lock().editor.undo_name().map(str::to_owned)
    }

    /// Nom de l'action que « Rétablir » referait.
    pub fn redo_name(&self) -> Option<String> {
        self.lock().editor.redo_name().map(str::to_owned)
    }

    /// Vrai si les pages ont changé depuis l'ouverture ou le dernier enregistrement.
    pub fn is_modified(&self) -> bool {
        self.lock().editor.is_modified()
    }

    /// Révision des pages : elle change à chaque modification et se retrouve
    /// en annulant ou en rétablissant.
    pub fn revision(&self) -> u64 {
        self.lock().editor.revision()
    }

    /// Retient comme enregistrée la révision écrite ([`ExportedPdf::revision`]) ;
    /// une modification faite pendant l'écriture laisse le document modifié.
    pub fn mark_saved(&self, revision: u64) {
        self.lock().editor.mark_saved(revision);
    }

    /// Oublie l'état enregistré : le document reste modifié jusqu'au prochain
    /// enregistrement, même en annulant toutes les modifications.
    pub fn forget_saved(&self) {
        self.lock().editor.forget_saved();
    }

    /// PDF formé de toutes les pages, relu et vérifié, et la révision écrite.
    pub fn export_pdf(&self) -> Result<ExportedPdf, EngineError> {
        let (snapshot, revision) = self.freeze();
        let pages: Vec<OutputPage> = snapshot.pages.iter().map(|&(_, page)| page).collect();
        Ok(ExportedPdf {
            bytes: pdf_core::write_pdf(&snapshot.sources, &pages)?,
            revision,
        })
    }

    /// Octets d'un PDF formé des seules pages désignées par leur identifiant,
    /// dans cet ordre (extraction). Une page qui n'existe plus est une erreur :
    /// les pages ont changé depuis qu'on les a désignées.
    pub fn export_pages(&self, ids: Vec<u64>) -> Result<Vec<u8>, EngineError> {
        self.freeze().0.export_pages(ids)
    }

    /// Pages dans leur état actuel, pour en exporter plusieurs parties (scission)
    /// sans voir les modifications faites pendant l'écriture.
    pub fn snapshot(&self) -> Arc<PageSnapshot> {
        Arc::new(self.freeze().0)
    }

    /// Octets d'un document source tel qu'il a été ouvert (0 : le document,
    /// avant tout enregistrement), pour en conserver une copie de récupération.
    pub fn source_bytes(&self, source: u32) -> Result<Vec<u8>, EngineError> {
        // La copie, longue pour un gros document, se fait hors du verrou.
        let data = self
            .lock()
            .sources
            .get(source as usize)
            .map(|source| source.data.clone())
            .ok_or_else(|| invalid("document source inexistant"))?;
        Ok(data.bytes().to_vec())
    }

    /// Vrai si le document ouvert porte une signature numérique, qu'un
    /// enregistrement invaliderait.
    pub fn has_signature(&self) -> Result<bool, EngineError> {
        let base = self.lock().sources[0].data.clone();
        Ok(pdf_core::is_signed(&base)?)
    }
}

impl EditSession {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Les opérations ne paniquent pas en laissant l'état à moitié modifié :
        // un verrou empoisonné reste utilisable.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sources, pages et révision, copiées pour libérer le verrou pendant
    /// l'écriture (les octets des sources sont partagés, non dupliqués).
    fn freeze(&self) -> (PageSnapshot, u64) {
        let state = self.lock();
        let pages = state
            .editor
            .pages()
            .iter()
            .map(|entry| {
                let page = match entry.origin {
                    PageOrigin::Source { source, index } => OutputPage::Source {
                        source: source as usize,
                        index,
                        quarter_turns: entry.quarter_turns,
                    },
                    PageOrigin::Blank { width, height } => OutputPage::Blank {
                        width,
                        height,
                        quarter_turns: entry.quarter_turns,
                    },
                };
                (entry.id, page)
            })
            .collect();
        let sources = state
            .sources
            .iter()
            .map(|source| source.data.clone())
            .collect();
        let snapshot = PageSnapshot { sources, pages };
        (snapshot, state.editor.revision())
    }
}

/// Pages d'une session figées à un instant donné.
#[derive(uniffi::Object)]
pub struct PageSnapshot {
    sources: Vec<SourceData>,
    /// Identifiant et contenu de chaque page, dans l'ordre.
    pages: Vec<(u64, OutputPage)>,
}

#[uniffi::export]
impl PageSnapshot {
    /// Octets d'un PDF formé des pages désignées par leur identifiant, dans cet
    /// ordre ; une page absente de l'instantané est une erreur.
    pub fn export_pages(&self, ids: Vec<u64>) -> Result<Vec<u8>, EngineError> {
        if ids.is_empty() {
            return Err(EditError::EmptySelection.into());
        }
        let pages = ids
            .iter()
            .map(|&id| {
                self.pages
                    .iter()
                    .find(|&&(page, _)| page == id)
                    .map(|&(_, page)| page)
                    .ok_or_else(changed)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pdf_core::write_pdf(&self.sources, &pages)?)
    }
}

fn invalid(detail: &str) -> EngineError {
    EngineError::Failed {
        message: "Cette opération est impossible sur ce document.".to_owned(),
        detail: detail.to_owned(),
    }
}

/// Les pages désignées ont changé (supprimées ou annulées) avant d'être lues.
fn changed() -> EngineError {
    EngineError::Failed {
        message: "Les pages ont changé pendant l'opération : recommencez-la.".to_owned(),
        detail: "page inexistante".to_owned(),
    }
}

fn usizes(values: &[u32]) -> Vec<usize> {
    values.iter().map(|&value| value as usize).collect()
}

fn u32s(values: Vec<usize>) -> Vec<u32> {
    values.into_iter().map(|value| value as u32).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn open(name: &str) -> Arc<PdfDocument> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        PdfDocument::open(path.to_string_lossy().into_owned(), None).unwrap()
    }

    fn sources(session: &EditSession) -> Vec<(u32, u32)> {
        session
            .pages()
            .into_iter()
            .map(|page| match page.kind {
                PageKind::Source { source, index } => (source, index),
                PageKind::Blank { .. } => (u32::MAX, 0),
            })
            .collect()
    }

    #[test]
    fn organise_puis_exporte() {
        let session = EditSession::new(open("organisation.pdf"));
        session.rotate(vec![0, 2], 1).unwrap();
        assert_eq!(session.duplicate(vec![1]).unwrap(), [2]);
        assert_eq!(session.move_pages(vec![4], 0).unwrap(), [0]);
        session.delete(vec![5]).unwrap();
        assert_eq!(sources(&session), [(0, 3), (0, 0), (0, 1), (0, 1), (0, 2)]);
        assert_eq!(session.undo_name().as_deref(), Some("la suppression"));
        assert!(session.is_modified());

        let exported = session.export_pdf().unwrap();
        let doc = pdf_core::Document::from_bytes(&exported.bytes, None).unwrap();
        assert_eq!(doc.page_count(), 5);
        let ids: Vec<u64> = session.pages().iter().map(|page| page.id).collect();
        let extract = session.export_pages(vec![ids[4], ids[1]]).unwrap();
        assert_eq!(
            pdf_core::Document::from_bytes(&extract, None)
                .unwrap()
                .page_count(),
            2
        );
        assert!(session.export_pages(vec![9999]).is_err());

        // Un instantané ignore les modifications faites ensuite.
        let snapshot = session.snapshot();
        session.delete(vec![4]).unwrap();
        let part = snapshot.export_pages(vec![ids[4], ids[0]]).unwrap();
        assert_eq!(
            pdf_core::Document::from_bytes(&part, None)
                .unwrap()
                .page_count(),
            2
        );
        assert!(session.export_pages(vec![ids[4]]).is_err());
        assert!(session.undo());

        // Les octets d'ouverture restent ceux du fichier, quelles que soient les modifications.
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/organisation.pdf");
        assert_eq!(
            session.source_bytes(0).unwrap(),
            std::fs::read(path).unwrap()
        );
        assert!(session.source_bytes(1).is_err());

        session.mark_saved(exported.revision);
        assert!(!session.is_modified());
        assert!(session.undo());
        assert!(session.is_modified());
        assert!(session.redo());
        assert!(!session.is_modified());

        // Une rotation faite pendant l'écriture n'est pas dans le fichier.
        let exported = session.export_pdf().unwrap();
        session.rotate(vec![0], 1).unwrap();
        session.mark_saved(exported.revision);
        assert!(session.is_modified());
    }

    #[test]
    fn insere_des_pages_d_un_autre_document() {
        let session = EditSession::new(open("organisation.pdf"));
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/signets.pdf");
        let copy = PdfDocument::from_data(std::fs::read(path).unwrap(), None).unwrap();
        let other = session.add_source(copy);
        assert_eq!(other, 1);
        assert_eq!(session.insert_pages(5, other, vec![0, 5]).unwrap(), [5, 6]);
        let blank = PageSize {
            width: 595.0,
            height: 842.0,
        };
        assert_eq!(session.insert_blank(0, blank).unwrap(), 0);
        assert!(matches!(
            session.pages()[0].kind,
            PageKind::Blank { size } if size.width == 595.0
        ));
        assert!(session.insert_pages(0, other, vec![6]).is_err());
        assert!(session.insert_pages(0, 7, vec![0]).is_err());

        let bytes = session.export_pdf().unwrap().bytes;
        let doc = pdf_core::Document::from_bytes(&bytes, None).unwrap();
        assert_eq!(doc.page_count(), 8);
    }

    #[test]
    fn restaure_des_pages() {
        let session = EditSession::new(open("organisation.pdf"));
        session.add_source(open("texte_simple.pdf"));
        let mut pages = session.pages();
        pages.reverse();
        pages[0].quarter_turns = 2;
        pages.push(PageItem {
            id: 0,
            kind: PageKind::Source {
                source: 1,
                index: 0,
            },
            quarter_turns: 0,
        });
        session.restore(pages).unwrap();
        assert_eq!(
            sources(&session),
            [(0, 4), (0, 3), (0, 2), (0, 1), (0, 0), (1, 0)]
        );
        assert_eq!(session.pages()[0].quarter_turns, 2);
        assert_eq!(session.undo_name().as_deref(), Some("la restauration"));

        let missing = PageItem {
            id: 0,
            kind: PageKind::Source {
                source: 1,
                index: 1,
            },
            quarter_turns: 0,
        };
        assert!(session.restore(vec![missing]).is_err());
        assert_eq!(session.pages().len(), 6);

        // Restaurées sur une version antérieure du fichier, les pages ne
        // redeviennent pas « enregistrées » en annulant la restauration.
        session.forget_saved();
        assert!(session.undo());
        assert!(session.is_modified());
    }

    #[test]
    fn refuse_de_vider_le_document() {
        let session = EditSession::new(open("texte_simple.pdf"));
        let err = session.delete(vec![0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Un document doit garder au moins une page."
        );
        assert!(!session.is_modified());
        assert!(session.undo_name().is_none());
    }

    #[test]
    fn signale_une_signature() {
        assert!(EditSession::new(open("signature_factice.pdf"))
            .has_signature()
            .unwrap());
        assert!(!EditSession::new(open("organisation.pdf"))
            .has_signature()
            .unwrap());
    }
}
