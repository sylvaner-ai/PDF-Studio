//! Modifications d'un document : organisation des pages (ordre, rotation,
//! suppression, duplication, insertion), contenu de chaque page (objets
//! supprimés, déplacés, texte remplacé), couche de texte des pages
//! reconnues et pages améliorées (mode Amélioration).
//!
//! Le document n'est pas modifié : on manipule une liste de pages qui désignent
//! chacune une page d'un document source (ou une page vierge), et pour chaque
//! page les modifications de son contenu. Le PDF n'est écrit qu'à
//! l'enregistrement. Annuler revient donc à retrouver un état précédent, ce qui
//! est immédiat même pour des centaines de pages. Pages et contenu partagent un
//! même historique : ⌘Z défait la dernière modification, quelle qu'elle soit.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::content::{ObjectEdit, PageEdits, TableEdit, TextReplacement};
use crate::history::{Command, History};
use crate::layer::TextLayer;
use crate::scan::{ScanStyle, ScanText};

/// Origine d'une page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageOrigin {
    /// Page `index` du document source `source` ; 0 désigne le document ouvert.
    Source {
        /// Numéro du document source.
        source: u32,
        /// Indice de la page dans ce document.
        index: u32,
    },
    /// Page vierge.
    Blank {
        /// Largeur en points.
        width: f32,
        /// Hauteur en points.
        height: f32,
    },
}

/// Une page du document en cours d'organisation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageEntry {
    /// Identifiant unique dans la session : il suit la page quand elle se déplace.
    pub id: u64,
    /// Ce que la page affiche.
    pub origin: PageOrigin,
    /// Quarts de tour dans le sens horaire (0 à 3), ajoutés à la rotation propre
    /// de la page source.
    pub quarter_turns: u8,
    /// Page d'origine d'une page améliorée (mode Amélioration) : `origin` est
    /// alors sa version améliorée, et l'enregistrement en garde l'objet
    /// (annotations, liens, signets) avec le nouveau contenu.
    pub replaces: Option<ReplacedPage>,
}

/// Page d'origine d'une page améliorée, telle qu'elle était avant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplacedPage {
    /// Page d'origine.
    pub origin: PageOrigin,
    /// Ses quarts de tour.
    pub quarter_turns: u8,
}

/// Résultat de l'amélioration d'une page, à appliquer par [`Editor::enhance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enhancement {
    /// Identifiant de la page.
    pub page: u64,
    /// Version améliorée de la page (image redressée) ; `None` si ses pixels
    /// ne changent pas.
    pub origin: Option<PageOrigin>,
    /// Quarts de tour (sens horaire) qui remettent à l'endroit la page
    /// d'origine (sans la rotation de l'organisation) ; 0 si elle l'est.
    pub quarter_turns: u8,
}

/// Erreur d'une opération sur les pages ou leur contenu.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// Aucune page ou aucun objet n'est désigné.
    #[error("aucune page n'est sélectionnée")]
    EmptySelection,
    /// Une position désignée n'existe pas.
    #[error("la page {position} n'existe pas (le document en compte {count})")]
    OutOfRange {
        /// Position demandée, à partir de 0.
        position: usize,
        /// Nombre de pages.
        count: usize,
    },
    /// Un PDF doit garder au moins une page.
    #[error("un document doit garder au moins une page")]
    WouldRemoveAllPages,
    /// La page désignée par son identifiant n'existe plus.
    #[error("la page {0} n'existe plus")]
    UnknownPage(u64),
}

/// Résultat d'une opération sur les pages.
pub type Result<T> = std::result::Result<T, EditError>;

/// État du document : ses pages, les modifications du contenu de chacune et
/// leur couche de texte (avec son numéro), par identifiant de page.
#[derive(Clone, Default, PartialEq)]
struct State {
    pages: Vec<PageEntry>,
    contents: HashMap<u64, Arc<PageEdits>>,
    layers: HashMap<u64, (u64, Arc<TextLayer>)>,
}

/// Résultat de la reconnaissance du texte d'une page.
#[derive(Clone, Debug, PartialEq)]
pub struct Recognition {
    /// Identifiant de la page.
    pub page: u64,
    /// Mots reconnus ; une couche vide retire la couche existante.
    pub layer: TextLayer,
    /// Quarts de tour (sens horaire) qui remettent à l'endroit la page
    /// d'origine (sans la rotation de l'organisation) ; 0 si elle l'est déjà.
    pub quarter_turns: u8,
}

/// Pages du document, dans l'ordre, modifications de leur contenu et historique
/// commun de ces modifications.
pub struct Editor {
    state: State,
    next_id: u64,
    next_content_revision: u64,
    history: History<State>,
}

impl Editor {
    /// État initial : les pages du document ouvert, dans leur ordre, intactes.
    pub fn new(page_count: u32) -> Self {
        let mut editor = Self {
            state: State::default(),
            next_id: 1,
            next_content_revision: 1,
            history: History::new(),
        };
        let pages = (0..page_count)
            .map(|index| editor.new_entry(PageOrigin::Source { source: 0, index }, 0))
            .collect();
        editor.state.pages = pages;
        editor
    }

    /// Pages, dans l'ordre.
    pub fn pages(&self) -> &[PageEntry] {
        &self.state.pages
    }

    /// Modifications du contenu d'une page ; `None` si elle est intacte.
    pub fn content(&self, page: u64) -> Option<&Arc<PageEdits>> {
        self.state.contents.get(&page)
    }

    /// Couche de texte d'une page reconnue ; `None` si elle n'en a pas.
    pub fn text_layer(&self, page: u64) -> Option<&Arc<TextLayer>> {
        self.state.layers.get(&page).map(|(_, layer)| layer)
    }

    /// Numéro de la couche de texte d'une page, unique dans la session
    /// (0 : aucune couche).
    pub fn text_layer_revision(&self, page: u64) -> u64 {
        self.state
            .layers
            .get(&page)
            .map_or(0, |(revision, _)| *revision)
    }

    /// Ajoute aux pages reconnues leur couche de texte et les remet à
    /// l'endroit, en une seule étape d'annulation. Une page qui n'existe plus
    /// est ignorée. La page est lue sans la rotation de l'organisation : si
    /// elle était couchée ou retournée, sa rotation devient celle qui la
    /// remet à l'endroit, quelle qu'elle soit (une rotation déjà corrigée à
    /// la main n'est pas doublée) ; sinon la rotation choisie est gardée.
    pub fn recognize(&mut self, results: Vec<Recognition>) {
        let mut state = self.state.clone();
        for result in results {
            let Some(entry) = state.pages.iter_mut().find(|entry| entry.id == result.page) else {
                continue;
            };
            if result.quarter_turns % 4 != 0 {
                entry.quarter_turns = result.quarter_turns % 4;
            }
            if result.layer.is_empty() {
                state.layers.remove(&result.page);
            } else {
                let revision = self.next_content_revision;
                self.next_content_revision += 1;
                state
                    .layers
                    .insert(result.page, (revision, Arc::new(result.layer)));
            }
        }
        self.commit("la reconnaissance du texte", state);
    }

    /// Remplace les pages améliorées par leur version améliorée et remet à
    /// l'endroit celles qui étaient couchées ou retournées, en une seule
    /// étape d'annulation. Une page dont l'image change perd les
    /// modifications de son contenu et sa couche de texte, établies sur
    /// l'ancienne image. Une page qui n'existe plus est ignorée ; une page
    /// déjà améliorée garde sa page d'origine.
    pub fn enhance(&mut self, results: &[Enhancement]) {
        let mut state = self.state.clone();
        for result in results {
            let Some(entry) = state.pages.iter_mut().find(|entry| entry.id == result.page) else {
                continue;
            };
            let turns = result.quarter_turns % 4;
            // Une rotation déjà corrigée à la main n'est pas une amélioration.
            if result.origin.is_none() && (turns == 0 || turns == entry.quarter_turns) {
                continue;
            }
            let before = ReplacedPage {
                origin: entry.origin,
                quarter_turns: entry.quarter_turns,
            };
            entry.replaces.get_or_insert(before);
            if turns != 0 {
                entry.quarter_turns = turns;
            }
            if let Some(origin) = result.origin {
                entry.origin = origin;
                state.contents.remove(&result.page);
                state.layers.remove(&result.page);
            }
        }
        self.commit("l'amélioration des pages", state);
    }

    /// Rend aux pages améliorées désignées leur page d'origine et sa
    /// rotation, en une étape d'annulation ; les autres sont ignorées.
    pub fn restore_originals(&mut self, pages: &[u64]) -> Result<()> {
        if pages.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let mut state = self.state.clone();
        for &page in pages {
            let Some(entry) = state.pages.iter_mut().find(|entry| entry.id == page) else {
                return Err(EditError::UnknownPage(page));
            };
            let Some(original) = entry.replaces.take() else {
                continue;
            };
            if entry.origin != original.origin {
                entry.origin = original.origin;
                state.contents.remove(&page);
                state.layers.remove(&page);
            }
            entry.quarter_turns = original.quarter_turns;
        }
        self.commit("le retour aux pages d'origine", state);
        Ok(())
    }

    /// Fait pivoter les pages désignées de `quarter_turns` quarts de tour
    /// (positif : sens horaire).
    pub fn rotate(&mut self, positions: &[usize], quarter_turns: i32) -> Result<()> {
        let positions = self.checked(positions)?;
        let mut state = self.state.clone();
        for position in positions {
            let page = &mut state.pages[position];
            page.quarter_turns =
                (i32::from(page.quarter_turns) + quarter_turns).rem_euclid(4) as u8;
        }
        self.commit("la rotation", state);
        Ok(())
    }

    /// Supprime les pages désignées. Le document doit garder au moins une page.
    pub fn delete(&mut self, positions: &[usize]) -> Result<()> {
        let positions = self.checked(positions)?;
        if positions.len() == self.state.pages.len() {
            return Err(EditError::WouldRemoveAllPages);
        }
        let mut state = self.state.clone();
        state.pages = self
            .state
            .pages
            .iter()
            .enumerate()
            .filter(|(position, _)| !positions.contains(position))
            .map(|(_, page)| *page)
            .collect();
        // Le contenu modifié des pages supprimées ne sert plus : l'annulation
        // retrouve l'état précédent tout entier.
        let kept: BTreeSet<u64> = state.pages.iter().map(|page| page.id).collect();
        state.contents.retain(|id, _| kept.contains(id));
        state.layers.retain(|id, _| kept.contains(id));
        self.commit("la suppression", state);
        Ok(())
    }

    /// Duplique les pages désignées, chaque copie juste après son original et
    /// avec ses modifications. Renvoie les positions des copies.
    pub fn duplicate(&mut self, positions: &[usize]) -> Result<Vec<usize>> {
        let positions = self.checked(positions)?;
        let mut state = self.state.clone();
        state.pages = Vec::with_capacity(self.state.pages.len() + positions.len());
        let mut copies = Vec::with_capacity(positions.len());
        for (position, page) in self.state.pages.clone().into_iter().enumerate() {
            state.pages.push(page);
            if positions.contains(&position) {
                copies.push(state.pages.len());
                let copy = PageEntry {
                    replaces: page.replaces,
                    ..self.new_entry(page.origin, page.quarter_turns)
                };
                if let Some(content) = self.state.contents.get(&page.id) {
                    state.contents.insert(copy.id, Arc::clone(content));
                }
                if let Some(layer) = self.state.layers.get(&page.id) {
                    state.layers.insert(copy.id, layer.clone());
                }
                state.pages.push(copy);
            }
        }
        self.commit("la duplication", state);
        Ok(copies)
    }

    /// Déplace les pages désignées, dans leur ordre, devant la page qui occupe
    /// la position `before` (`before` égal au nombre de pages : à la fin).
    /// Renvoie leurs nouvelles positions.
    pub fn move_pages(&mut self, positions: &[usize], before: usize) -> Result<Vec<usize>> {
        let positions = self.checked(positions)?;
        let count = self.state.pages.len();
        if before > count {
            return Err(EditError::OutOfRange {
                position: before,
                count,
            });
        }
        let moved: Vec<PageEntry> = positions
            .iter()
            .map(|&position| self.state.pages[position])
            .collect();
        let mut state = self.state.clone();
        state.pages = self
            .state
            .pages
            .iter()
            .enumerate()
            .filter(|(position, _)| !positions.contains(position))
            .map(|(_, page)| *page)
            .collect();
        let at = before - positions.range(..before).count();
        state.pages.splice(at..at, moved);
        self.commit("le déplacement", state);
        Ok((at..at + positions.len()).collect())
    }

    /// Insère des pages devant la position `at` (`at` égal au nombre de pages :
    /// à la fin). Renvoie leurs positions.
    pub fn insert(&mut self, at: usize, origins: &[PageOrigin]) -> Result<Vec<usize>> {
        if origins.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let count = self.state.pages.len();
        if at > count {
            return Err(EditError::OutOfRange {
                position: at,
                count,
            });
        }
        let mut state = self.state.clone();
        let inserted: Vec<PageEntry> = origins
            .iter()
            .map(|&origin| self.new_entry(origin, 0))
            .collect();
        state.pages.splice(at..at, inserted);
        self.commit("l'insertion", state);
        Ok((at..at + origins.len()).collect())
    }

    /// Remplace toutes les pages, avec les modifications de leur contenu, par
    /// exemple pour retrouver des modifications non enregistrées ; l'opération
    /// s'annule comme les autres.
    pub fn restore(&mut self, pages: &[RestoredPage]) -> Result<()> {
        if pages.is_empty() {
            return Err(EditError::WouldRemoveAllPages);
        }
        let mut state = State::default();
        for page in pages {
            let entry = PageEntry {
                replaces: page.replaces,
                ..self.new_entry(page.origin, page.quarter_turns % 4)
            };
            if !page.content.is_empty() {
                let content = self.new_content(page.content.clone());
                state.contents.insert(entry.id, content);
            }
            if let Some(layer) = page.text_layer.as_ref().filter(|layer| !layer.is_empty()) {
                let revision = self.next_content_revision;
                self.next_content_revision += 1;
                state
                    .layers
                    .insert(entry.id, (revision, Arc::new(layer.clone())));
            }
            state.pages.push(entry);
        }
        self.commit("la restauration", state);
        Ok(())
    }

    /// Supprime des objets d'une page.
    pub fn delete_objects(&mut self, page: u64, objects: &[u32]) -> Result<()> {
        self.edit_objects(page, objects, "la suppression", |edit| {
            edit.deleted = true;
        })
    }

    /// Déplace des objets d'une page, en points dans l'espace de la page PDF
    /// (ordonnées vers le haut).
    pub fn move_objects(&mut self, page: u64, objects: &[u32], dx: f32, dy: f32) -> Result<()> {
        self.edit_objects(page, objects, "le déplacement", |edit| {
            edit.dx += dx;
            edit.dy += dy;
        })
    }

    /// Remplace le texte d'un bloc ; `None` rend au bloc son texte d'origine.
    pub fn replace_text(
        &mut self,
        page: u64,
        object: u32,
        text: Option<TextReplacement>,
    ) -> Result<()> {
        self.edit_objects(page, &[object], "la modification du texte", |edit| {
            edit.text = text.clone();
        })
    }

    /// Remplace le texte d'une ligne d'une page scannée ; `None` rend à la
    /// ligne son texte d'origine.
    pub fn replace_scan_text(
        &mut self,
        page: u64,
        object: u32,
        text: Option<ScanText>,
    ) -> Result<()> {
        self.edit_objects(page, &[object], "la modification du texte", |edit| {
            edit.scan = text.clone();
        })
    }

    /// Change les réglages du texte retouché d'une ligne scannée.
    pub fn set_scan_style(&mut self, page: u64, object: u32, style: ScanStyle) -> Result<()> {
        let has_text = self
            .content(page)
            .and_then(|content| content.get(object))
            .is_some_and(|edit| edit.scan.is_some());
        if !has_text {
            return Err(EditError::EmptySelection);
        }
        self.edit_objects(page, &[object], "le réglage du texte", |edit| {
            if let Some(scan) = &mut edit.scan {
                scan.style = style.clone();
            }
        })
    }

    /// Modifie le tableau `table` d'une page : `change` reçoit ses
    /// modifications actuelles ; `name` nomme l'action à annuler.
    pub fn edit_table(
        &mut self,
        page: u64,
        table: u32,
        name: &'static str,
        change: impl Fn(&mut TableEdit),
    ) -> Result<()> {
        self.edit_objects(page, &[table], name, |edit| {
            let mut table = edit.table.clone().unwrap_or_default();
            change(&mut table);
            edit.table = (!table.is_unchanged()).then_some(table);
        })
    }

    /// Défait la dernière modification ; renvoie `false` s'il n'y en a pas.
    pub fn undo(&mut self) -> bool {
        self.history.undo(&mut self.state)
    }

    /// Rétablit la dernière modification annulée ; renvoie `false` s'il n'y en a pas.
    pub fn redo(&mut self) -> bool {
        self.history.redo(&mut self.state)
    }

    /// Nom de l'action que [`Editor::undo`] déferait (« la rotation »).
    pub fn undo_name(&self) -> Option<&str> {
        self.history.undo_name()
    }

    /// Nom de l'action que [`Editor::redo`] rétablirait.
    pub fn redo_name(&self) -> Option<&str> {
        self.history.redo_name()
    }

    /// Vrai si le document a changé depuis l'ouverture ou le dernier enregistrement.
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Révision du document : elle change à chaque modification et se retrouve
    /// en annulant ou en rétablissant.
    pub fn revision(&self) -> u64 {
        self.history.revision()
    }

    /// Retient la révision `revision`, celle de l'état écrit, comme enregistrée.
    pub fn mark_saved(&mut self, revision: u64) {
        self.history.mark_saved(revision);
    }

    /// Oublie l'état enregistré, par exemple quand les pages reposent sur une
    /// version antérieure du fichier : le document reste modifié jusqu'au
    /// prochain enregistrement.
    pub fn forget_saved(&mut self) {
        self.history.forget_saved();
    }

    fn new_entry(&mut self, origin: PageOrigin, quarter_turns: u8) -> PageEntry {
        let id = self.next_id;
        self.next_id += 1;
        PageEntry {
            id,
            origin,
            quarter_turns,
            replaces: None,
        }
    }

    fn new_content(&mut self, objects: BTreeMap<u32, ObjectEdit>) -> Arc<PageEdits> {
        let revision = self.next_content_revision;
        self.next_content_revision += 1;
        Arc::new(PageEdits::new(objects, revision))
    }

    /// Applique `change` à la modification de chacun des objets désignés.
    fn edit_objects(
        &mut self,
        page: u64,
        objects: &[u32],
        name: &'static str,
        change: impl Fn(&mut ObjectEdit),
    ) -> Result<()> {
        if objects.is_empty() {
            return Err(EditError::EmptySelection);
        }
        if !self.state.pages.iter().any(|entry| entry.id == page) {
            return Err(EditError::UnknownPage(page));
        }
        let mut edits = self
            .state
            .contents
            .get(&page)
            .map(|content| content.objects())
            .unwrap_or_default();
        for &object in objects {
            change(edits.entry(object).or_default());
        }
        let mut state = self.state.clone();
        let content = self.new_content(edits);
        if content.is_empty() {
            state.contents.remove(&page);
        } else {
            state.contents.insert(page, content);
        }
        self.commit(name, state);
        Ok(())
    }

    /// Positions triées et sans doublon, toutes existantes.
    fn checked(&self, positions: &[usize]) -> Result<BTreeSet<usize>> {
        if positions.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let count = self.state.pages.len();
        if let Some(&position) = positions.iter().find(|&&position| position >= count) {
            return Err(EditError::OutOfRange { position, count });
        }
        Ok(positions.iter().copied().collect())
    }

    /// Enregistre le passage à `state` dans l'historique, sauf s'il ne change rien.
    fn commit(&mut self, name: &'static str, state: State) {
        if same_state(&state, &self.state) {
            return;
        }
        let edit = Snapshot {
            name,
            before: self.state.clone(),
            after: state,
        };
        self.history.execute(&mut self.state, Box::new(edit));
    }
}

/// Deux états identiques, aux numéros de révision du contenu près.
fn same_state(a: &State, b: &State) -> bool {
    a.pages == b.pages
        && a.layers.len() == b.layers.len()
        && a.layers.iter().all(|(id, (_, layer))| {
            b.layers
                .get(id)
                .is_some_and(|(_, other)| Arc::ptr_eq(layer, other) || layer == other)
        })
        && a.contents.len() == b.contents.len()
        && a.contents.iter().all(|(id, content)| {
            b.contents
                .get(id)
                .is_some_and(|other| content.objects() == other.objects())
        })
}

/// Une page à restaurer : son origine, sa rotation et les modifications de son
/// contenu.
#[derive(Clone, Debug, PartialEq)]
pub struct RestoredPage {
    /// Ce que la page affiche.
    pub origin: PageOrigin,
    /// Quarts de tour dans le sens horaire.
    pub quarter_turns: u8,
    /// Modifications du contenu, par numéro d'objet.
    pub content: BTreeMap<u32, ObjectEdit>,
    /// Couche de texte d'une page reconnue.
    pub text_layer: Option<TextLayer>,
    /// Page d'origine d'une page améliorée.
    pub replaces: Option<ReplacedPage>,
}

/// Modification du document, retenue par ses états avant et après.
struct Snapshot {
    name: &'static str,
    before: State,
    after: State,
}

impl Command<State> for Snapshot {
    fn apply(&mut self, target: &mut State) {
        target.clone_from(&self.after);
    }

    fn revert(&mut self, target: &mut State) {
        target.clone_from(&self.before);
    }

    fn name(&self) -> &str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Indices sources des pages, pour lire l'ordre d'un coup d'œil.
    fn order(organizer: &Editor) -> Vec<u32> {
        organizer
            .pages()
            .iter()
            .map(|page| match page.origin {
                PageOrigin::Source { index, .. } => index,
                PageOrigin::Blank { .. } => u32::MAX,
            })
            .collect()
    }

    #[test]
    fn pivote_les_pages_selectionnees() {
        let mut organizer = Editor::new(3);
        organizer.rotate(&[0, 2], 1).unwrap();
        organizer.rotate(&[2], -2).unwrap();
        let turns: Vec<u8> = organizer
            .pages()
            .iter()
            .map(|page| page.quarter_turns)
            .collect();
        assert_eq!(turns, [1, 0, 3]);
        assert_eq!(organizer.undo_name(), Some("la rotation"));
        organizer.undo();
        organizer.undo();
        assert!(organizer.pages().iter().all(|page| page.quarter_turns == 0));
        assert!(!organizer.is_modified());
    }

    #[test]
    fn supprime_mais_garde_une_page() {
        let mut organizer = Editor::new(4);
        organizer.delete(&[1, 3]).unwrap();
        assert_eq!(order(&organizer), [0, 2]);
        assert_eq!(
            organizer.delete(&[0, 1]),
            Err(EditError::WouldRemoveAllPages)
        );
        assert_eq!(
            organizer.delete(&[5]),
            Err(EditError::OutOfRange {
                position: 5,
                count: 2
            })
        );
        assert_eq!(organizer.delete(&[]), Err(EditError::EmptySelection));
        organizer.undo();
        assert_eq!(order(&organizer), [0, 1, 2, 3]);
    }

    #[test]
    fn duplique_avec_de_nouveaux_identifiants() {
        let mut organizer = Editor::new(3);
        organizer.rotate(&[1], 1).unwrap();
        let copies = organizer.duplicate(&[1, 2]).unwrap();
        assert_eq!(copies, [2, 4]);
        assert_eq!(order(&organizer), [0, 1, 1, 2, 2]);
        let pages = organizer.pages();
        assert_eq!(pages[2].quarter_turns, 1);
        let ids: BTreeSet<u64> = pages.iter().map(|page| page.id).collect();
        assert_eq!(ids.len(), pages.len());
    }

    #[test]
    fn deplace_devant_une_position() {
        let mut organizer = Editor::new(5);
        // Pages 0 et 3 devant la page 2.
        assert_eq!(organizer.move_pages(&[3, 0], 2).unwrap(), [1, 2]);
        assert_eq!(order(&organizer), [1, 0, 3, 2, 4]);
        // Vers la fin.
        assert_eq!(organizer.move_pages(&[0], 5).unwrap(), [4]);
        assert_eq!(order(&organizer), [0, 3, 2, 4, 1]);
        // Sans effet : rien n'entre dans l'historique.
        organizer.move_pages(&[0], 1).unwrap();
        organizer.undo();
        assert_eq!(order(&organizer), [1, 0, 3, 2, 4]);
    }

    #[test]
    fn insere_des_pages_d_un_autre_document_et_vierges() {
        let mut organizer = Editor::new(2);
        let origins = [
            PageOrigin::Source {
                source: 1,
                index: 4,
            },
            PageOrigin::Blank {
                width: 595.0,
                height: 842.0,
            },
        ];
        assert_eq!(organizer.insert(1, &origins).unwrap(), [1, 2]);
        assert_eq!(order(&organizer), [0, 4, u32::MAX, 1]);
        assert_eq!(organizer.pages()[1].origin, origins[0]);
        assert_eq!(organizer.redo_name(), None);
        organizer.undo();
        assert_eq!(organizer.redo_name(), Some("l'insertion"));
        organizer.redo();
        assert_eq!(organizer.pages().len(), 4);
    }

    #[test]
    fn restaure_une_organisation() {
        let mut organizer = Editor::new(3);
        let blank = PageOrigin::Blank {
            width: 100.0,
            height: 200.0,
        };
        let mut content = BTreeMap::new();
        content.insert(
            4,
            ObjectEdit {
                deleted: true,
                ..ObjectEdit::default()
            },
        );
        let pages = [
            RestoredPage {
                origin: PageOrigin::Source {
                    source: 0,
                    index: 2,
                },
                quarter_turns: 5,
                content,
                text_layer: None,
                replaces: None,
            },
            RestoredPage {
                origin: blank,
                quarter_turns: 0,
                content: BTreeMap::new(),
                text_layer: None,
                replaces: None,
            },
        ];
        organizer.restore(&pages).unwrap();
        assert_eq!(order(&organizer), [2, u32::MAX]);
        assert_eq!(organizer.pages()[0].quarter_turns, 1);
        let first = organizer.pages()[0].id;
        assert!(organizer.content(first).unwrap().get(4).unwrap().deleted);
        assert!(organizer.content(organizer.pages()[1].id).is_none());
        assert_eq!(organizer.undo_name(), Some("la restauration"));
        assert!(organizer.is_modified());
        assert!(organizer.undo());
        assert_eq!(order(&organizer), [0, 1, 2]);
        assert_eq!(organizer.restore(&[]), Err(EditError::WouldRemoveAllPages));
    }

    #[test]
    fn modifie_le_contenu_d_une_page() {
        let mut editor = Editor::new(2);
        let page = editor.pages()[1].id;
        assert!(editor.content(page).is_none());

        editor.move_objects(page, &[3, 5], 10.0, -2.0).unwrap();
        editor.move_objects(page, &[3], 1.0, 0.0).unwrap();
        let content = editor.content(page).unwrap();
        assert_eq!(
            (content.get(3).unwrap().dx, content.get(3).unwrap().dy),
            (11.0, -2.0)
        );
        assert_eq!(content.get(5).unwrap().dx, 10.0);
        let moved = content.revision();

        editor.delete_objects(page, &[5]).unwrap();
        assert!(editor.content(page).unwrap().get(5).unwrap().deleted);
        assert_eq!(editor.undo_name(), Some("la suppression"));
        assert_ne!(editor.content(page).unwrap().revision(), moved);

        let replacement = TextReplacement {
            text: "Bonjour".into(),
            font: crate::FontChoice::Original,
        };
        editor
            .replace_text(page, 3, Some(replacement.clone()))
            .unwrap();
        assert_eq!(editor.undo_name(), Some("la modification du texte"));
        assert_eq!(
            editor.content(page).unwrap().get(3).unwrap().text,
            Some(replacement)
        );

        // Annuler retrouve les états précédents, avec leurs numéros.
        assert!(editor.undo());
        assert!(editor.undo());
        assert_eq!(editor.content(page).unwrap().revision(), moved);
        assert!(editor.undo() && editor.undo());
        assert!(editor.content(page).is_none());
        assert!(!editor.is_modified());

        assert_eq!(
            editor.delete_objects(999, &[1]),
            Err(EditError::UnknownPage(999))
        );
        assert_eq!(
            editor.delete_objects(page, &[]),
            Err(EditError::EmptySelection)
        );
    }

    #[test]
    fn modifie_un_tableau() {
        let mut editor = Editor::new(1);
        let page = editor.pages()[0].id;
        editor
            .edit_table(page, 9, "la suppression de la ligne", |table| {
                table.rows = Some(vec![0, 2]);
            })
            .unwrap();
        let replacement = TextReplacement {
            text: "12".into(),
            font: crate::FontChoice::Original,
        };
        editor
            .edit_table(page, 9, "la saisie dans la cellule", |table| {
                table.cells.insert((2, 1), replacement.clone());
            })
            .unwrap();
        let table = editor.content(page).unwrap().get(9).unwrap().table.clone();
        let table = table.unwrap();
        assert_eq!(table.rows, Some(vec![0, 2]));
        assert_eq!(table.cells[&(2, 1)], replacement);
        assert_eq!(editor.undo_name(), Some("la saisie dans la cellule"));

        // Revenu à l'état d'origine, le tableau n'est plus modifié.
        editor
            .edit_table(page, 9, "la modification du tableau", |table| {
                table.rows = None;
                table.cells.clear();
            })
            .unwrap();
        assert!(editor.content(page).is_none());
        assert!(editor.undo());
        assert!(editor.content(page).unwrap().get(9).is_some());
    }

    #[test]
    fn une_modification_sans_effet_n_entre_pas_dans_l_historique() {
        let mut editor = Editor::new(1);
        let page = editor.pages()[0].id;
        editor.move_objects(page, &[0], 0.0, 0.0).unwrap();
        assert!(editor.undo_name().is_none());
        editor.move_objects(page, &[0], 5.0, 0.0).unwrap();
        editor.move_objects(page, &[0], -5.0, 0.0).unwrap();
        // Revenu à sa place, l'objet n'est plus modifié.
        assert!(editor.content(page).is_none());
        assert_eq!(editor.undo_name(), Some("le déplacement"));
    }

    #[test]
    fn la_copie_d_une_page_garde_ses_modifications() {
        let mut editor = Editor::new(2);
        let page = editor.pages()[0].id;
        editor.delete_objects(page, &[2]).unwrap();
        let copies = editor.duplicate(&[0]).unwrap();
        let copy = editor.pages()[copies[0]].id;
        assert!(editor.content(copy).unwrap().get(2).unwrap().deleted);

        // Les deux pages évoluent ensuite séparément.
        editor.move_objects(copy, &[1], 4.0, 4.0).unwrap();
        assert!(editor.content(page).unwrap().get(1).is_none());

        editor.delete(&[0]).unwrap();
        assert!(editor.content(page).is_none());
        editor.undo();
        assert!(editor.content(page).is_some());
    }

    fn layer(text: &str) -> TextLayer {
        TextLayer {
            words: vec![crate::LayerWord {
                text: text.into(),
                quad: [0.0, 0.0, 10.0, 0.0, 10.0, 5.0, 0.0, 5.0],
                confidence: 0.9,
                line: 0,
            }],
            engine: "test".into(),
        }
    }

    #[test]
    fn reconnaissance_annulable_et_page_remise_a_l_endroit() {
        let mut editor = Editor::new(3);
        let ids: Vec<u64> = editor.pages().iter().map(|page| page.id).collect();
        editor.recognize(vec![
            Recognition {
                page: ids[0],
                layer: layer("Bonjour"),
                quarter_turns: 0,
            },
            Recognition {
                page: ids[2],
                layer: layer("Merci"),
                quarter_turns: 2,
            },
            Recognition {
                page: 999,
                layer: layer("disparue"),
                quarter_turns: 1,
            },
        ]);
        assert_eq!(editor.undo_name(), Some("la reconnaissance du texte"));
        assert_eq!(editor.text_layer(ids[0]).unwrap().words[0].text, "Bonjour");
        assert!(editor.text_layer(ids[1]).is_none());
        assert_eq!(editor.text_layer_revision(ids[1]), 0);
        assert_ne!(
            editor.text_layer_revision(ids[0]),
            editor.text_layer_revision(ids[2])
        );
        assert_eq!(editor.pages()[2].quarter_turns, 2);

        // Une page déjà redressée à la main n'est pas tournée une fois de
        // plus ; une rotation choisie sur une page droite est gardée.
        editor.rotate(&[1], 2).unwrap();
        editor.rotate(&[0], 1).unwrap();
        editor.recognize(vec![
            Recognition {
                page: ids[1],
                layer: layer("Retournée"),
                quarter_turns: 2,
            },
            Recognition {
                page: ids[0],
                layer: layer("Bonjour"),
                quarter_turns: 0,
            },
        ]);
        assert_eq!(editor.pages()[1].quarter_turns, 2);
        assert_eq!(editor.pages()[0].quarter_turns, 1);

        // La copie d'une page garde sa couche ; la suppression la retire.
        let position = editor.duplicate(&[0]).unwrap()[0];
        let copy = editor.pages()[position].id;
        assert!(editor.text_layer(copy).is_some());
        editor.delete(&[0]).unwrap();
        assert!(editor.text_layer(ids[0]).is_none());

        while editor.undo() {}
        assert!(editor.text_layer(ids[0]).is_none());
        assert_eq!(editor.pages()[2].quarter_turns, 0);
        assert!(!editor.is_modified());

        // Une reconnaissance sans effet n'entre pas dans l'historique.
        editor.recognize(vec![Recognition {
            page: ids[1],
            layer: TextLayer::default(),
            quarter_turns: 0,
        }]);
        assert!(editor.undo_name().is_none());
    }

    #[test]
    fn ameliore_puis_rend_les_pages_d_origine() {
        let mut editor = Editor::new(3);
        let ids: Vec<u64> = editor.pages().iter().map(|page| page.id).collect();
        editor.recognize(vec![Recognition {
            page: ids[0],
            layer: layer("Avant"),
            quarter_turns: 0,
        }]);
        editor.delete_objects(ids[0], &[2]).unwrap();
        let enhanced = |index| PageOrigin::Source { source: 1, index };
        editor.enhance(&[
            // Image redressée : la couche et les modifications tombent.
            Enhancement {
                page: ids[0],
                origin: Some(enhanced(0)),
                quarter_turns: 0,
            },
            // Seulement retournée.
            Enhancement {
                page: ids[1],
                origin: None,
                quarter_turns: 2,
            },
            // Déjà droite : rien ne change.
            Enhancement {
                page: ids[2],
                origin: None,
                quarter_turns: 0,
            },
        ]);
        assert_eq!(editor.undo_name(), Some("l'amélioration des pages"));
        let pages = editor.pages();
        assert_eq!(pages[0].origin, enhanced(0));
        assert_eq!(
            pages[0].replaces.unwrap().origin,
            PageOrigin::Source {
                source: 0,
                index: 0
            }
        );
        assert!(editor.text_layer(ids[0]).is_none());
        assert!(editor.content(ids[0]).is_none());
        assert_eq!(pages[1].quarter_turns, 2);
        assert_eq!(pages[1].replaces.unwrap().quarter_turns, 0);
        assert!(pages[2].replaces.is_none());

        // Une deuxième amélioration garde la page d'origine ; la copie d'une
        // page améliorée l'est aussi.
        editor.enhance(&[Enhancement {
            page: ids[0],
            origin: Some(enhanced(1)),
            quarter_turns: 0,
        }]);
        assert_eq!(
            editor.pages()[0].replaces.unwrap().origin,
            PageOrigin::Source {
                source: 0,
                index: 0
            }
        );
        let copy = editor.duplicate(&[0]).unwrap()[0];
        assert!(editor.pages()[copy].replaces.is_some());
        editor.undo();

        // Page par page : retour à l'original, annulable.
        editor.restore_originals(&[ids[1]]).unwrap();
        assert_eq!(editor.undo_name(), Some("le retour aux pages d'origine"));
        assert_eq!(editor.pages()[1].quarter_turns, 0);
        assert!(editor.pages()[1].replaces.is_none());
        assert_eq!(editor.pages()[0].origin, enhanced(1));
        assert_eq!(
            editor.restore_originals(&[999]),
            Err(EditError::UnknownPage(999))
        );

        // En bloc : ⌘Z rend la couche et les modifications d'avant.
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(
            editor.pages()[0].origin,
            PageOrigin::Source {
                source: 0,
                index: 0
            }
        );
        assert!(editor.text_layer(ids[0]).is_some());
        assert!(editor.content(ids[0]).is_some());
        assert!(editor.pages().iter().all(|page| page.replaces.is_none()));
    }

    #[test]
    fn restaure_une_couche_de_texte() {
        let mut editor = Editor::new(1);
        editor
            .restore(&[RestoredPage {
                origin: PageOrigin::Source {
                    source: 0,
                    index: 0,
                },
                quarter_turns: 0,
                content: BTreeMap::new(),
                text_layer: Some(layer("Retrouvé")),
                replaces: None,
            }])
            .unwrap();
        let id = editor.pages()[0].id;
        assert_eq!(editor.text_layer(id).unwrap().words[0].text, "Retrouvé");
    }
}
