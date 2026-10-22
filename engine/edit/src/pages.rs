//! Organisation des pages : ordre, rotation, suppression, duplication, insertion.
//!
//! Le document n'est pas modifié pendant l'organisation : on manipule une liste
//! de pages qui désignent chacune une page d'un document source (ou une page
//! vierge). Le PDF n'est écrit qu'à l'enregistrement. Annuler revient donc à
//! retrouver une liste précédente, ce qui est immédiat même pour des centaines
//! de pages.

use std::collections::BTreeSet;

use crate::history::{Command, History};

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
}

/// Erreur d'une opération sur les pages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// Aucune page n'est désignée.
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
}

/// Résultat d'une opération sur les pages.
pub type Result<T> = std::result::Result<T, EditError>;

/// Pages du document, dans l'ordre, et historique de leurs modifications.
pub struct Organizer {
    pages: Vec<PageEntry>,
    next_id: u64,
    history: History<Vec<PageEntry>>,
}

impl Organizer {
    /// Organisation initiale : les pages du document ouvert, dans leur ordre.
    pub fn new(page_count: u32) -> Self {
        let mut organizer = Self {
            pages: Vec::with_capacity(page_count as usize),
            next_id: 1,
            history: History::new(),
        };
        let pages = (0..page_count)
            .map(|index| organizer.new_entry(PageOrigin::Source { source: 0, index }, 0))
            .collect();
        organizer.pages = pages;
        organizer
    }

    /// Pages, dans l'ordre.
    pub fn pages(&self) -> &[PageEntry] {
        &self.pages
    }

    /// Fait pivoter les pages désignées de `quarter_turns` quarts de tour
    /// (positif : sens horaire).
    pub fn rotate(&mut self, positions: &[usize], quarter_turns: i32) -> Result<()> {
        let positions = self.checked(positions)?;
        let mut pages = self.pages.clone();
        for position in positions {
            let page = &mut pages[position];
            page.quarter_turns =
                (i32::from(page.quarter_turns) + quarter_turns).rem_euclid(4) as u8;
        }
        self.commit("la rotation", pages);
        Ok(())
    }

    /// Supprime les pages désignées. Le document doit garder au moins une page.
    pub fn delete(&mut self, positions: &[usize]) -> Result<()> {
        let positions = self.checked(positions)?;
        if positions.len() == self.pages.len() {
            return Err(EditError::WouldRemoveAllPages);
        }
        let pages = self
            .pages
            .iter()
            .enumerate()
            .filter(|(position, _)| !positions.contains(position))
            .map(|(_, page)| *page)
            .collect();
        self.commit("la suppression", pages);
        Ok(())
    }

    /// Duplique les pages désignées, chaque copie juste après son original.
    /// Renvoie les positions des copies.
    pub fn duplicate(&mut self, positions: &[usize]) -> Result<Vec<usize>> {
        let positions = self.checked(positions)?;
        let mut pages = Vec::with_capacity(self.pages.len() + positions.len());
        let mut copies = Vec::with_capacity(positions.len());
        for (position, page) in self.pages.clone().into_iter().enumerate() {
            pages.push(page);
            if positions.contains(&position) {
                copies.push(pages.len());
                pages.push(self.new_entry(page.origin, page.quarter_turns));
            }
        }
        self.commit("la duplication", pages);
        Ok(copies)
    }

    /// Déplace les pages désignées, dans leur ordre, devant la page qui occupe
    /// la position `before` (`before` égal au nombre de pages : à la fin).
    /// Renvoie leurs nouvelles positions.
    pub fn move_pages(&mut self, positions: &[usize], before: usize) -> Result<Vec<usize>> {
        let positions = self.checked(positions)?;
        if before > self.pages.len() {
            return Err(EditError::OutOfRange {
                position: before,
                count: self.pages.len(),
            });
        }
        let moved: Vec<PageEntry> = positions
            .iter()
            .map(|&position| self.pages[position])
            .collect();
        let mut pages: Vec<PageEntry> = self
            .pages
            .iter()
            .enumerate()
            .filter(|(position, _)| !positions.contains(position))
            .map(|(_, page)| *page)
            .collect();
        let at = before - positions.range(..before).count();
        pages.splice(at..at, moved);
        self.commit("le déplacement", pages);
        Ok((at..at + positions.len()).collect())
    }

    /// Insère des pages devant la position `at` (`at` égal au nombre de pages :
    /// à la fin). Renvoie leurs positions.
    pub fn insert(&mut self, at: usize, origins: &[PageOrigin]) -> Result<Vec<usize>> {
        if origins.is_empty() {
            return Err(EditError::EmptySelection);
        }
        if at > self.pages.len() {
            return Err(EditError::OutOfRange {
                position: at,
                count: self.pages.len(),
            });
        }
        let mut pages = self.pages.clone();
        let inserted: Vec<PageEntry> = origins
            .iter()
            .map(|&origin| self.new_entry(origin, 0))
            .collect();
        pages.splice(at..at, inserted);
        self.commit("l'insertion", pages);
        Ok((at..at + origins.len()).collect())
    }

    /// Remplace toutes les pages, par exemple pour retrouver des modifications
    /// non enregistrées ; l'opération s'annule comme les autres.
    pub fn restore(&mut self, pages: &[(PageOrigin, u8)]) -> Result<()> {
        if pages.is_empty() {
            return Err(EditError::WouldRemoveAllPages);
        }
        let entries = pages
            .iter()
            .map(|&(origin, quarter_turns)| self.new_entry(origin, quarter_turns % 4))
            .collect();
        self.commit("la restauration", entries);
        Ok(())
    }

    /// Défait la dernière modification ; renvoie `false` s'il n'y en a pas.
    pub fn undo(&mut self) -> bool {
        self.history.undo(&mut self.pages)
    }

    /// Rétablit la dernière modification annulée ; renvoie `false` s'il n'y en a pas.
    pub fn redo(&mut self) -> bool {
        self.history.redo(&mut self.pages)
    }

    /// Nom de l'action que [`Organizer::undo`] déferait (« la rotation »).
    pub fn undo_name(&self) -> Option<&str> {
        self.history.undo_name()
    }

    /// Nom de l'action que [`Organizer::redo`] rétablirait.
    pub fn redo_name(&self) -> Option<&str> {
        self.history.redo_name()
    }

    /// Vrai si les pages ont changé depuis l'ouverture ou le dernier enregistrement.
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Révision des pages : elle change à chaque modification et se retrouve
    /// en annulant ou en rétablissant.
    pub fn revision(&self) -> u64 {
        self.history.revision()
    }

    /// Retient la révision `revision`, celle des pages écrites, comme enregistrée.
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
        }
    }

    /// Positions triées et sans doublon, toutes existantes.
    fn checked(&self, positions: &[usize]) -> Result<BTreeSet<usize>> {
        if positions.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let count = self.pages.len();
        if let Some(&position) = positions.iter().find(|&&position| position >= count) {
            return Err(EditError::OutOfRange { position, count });
        }
        Ok(positions.iter().copied().collect())
    }

    /// Enregistre le passage à `pages` dans l'historique, sauf s'il ne change rien.
    fn commit(&mut self, name: &'static str, pages: Vec<PageEntry>) {
        if pages == self.pages {
            return;
        }
        let edit = PageEdit {
            name,
            before: self.pages.clone(),
            after: pages,
        };
        self.history.execute(&mut self.pages, Box::new(edit));
    }
}

/// Modification de la liste des pages, retenue par ses états avant et après.
struct PageEdit {
    name: &'static str,
    before: Vec<PageEntry>,
    after: Vec<PageEntry>,
}

impl Command<Vec<PageEntry>> for PageEdit {
    fn apply(&mut self, target: &mut Vec<PageEntry>) {
        target.clone_from(&self.after);
    }

    fn revert(&mut self, target: &mut Vec<PageEntry>) {
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
    fn order(organizer: &Organizer) -> Vec<u32> {
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
        let mut organizer = Organizer::new(3);
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
        let mut organizer = Organizer::new(4);
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
        let mut organizer = Organizer::new(3);
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
        let mut organizer = Organizer::new(5);
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
        let mut organizer = Organizer::new(2);
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
        let mut organizer = Organizer::new(3);
        let blank = PageOrigin::Blank {
            width: 100.0,
            height: 200.0,
        };
        let pages = [
            (
                PageOrigin::Source {
                    source: 0,
                    index: 2,
                },
                5,
            ),
            (blank, 0),
        ];
        organizer.restore(&pages).unwrap();
        assert_eq!(order(&organizer), [2, u32::MAX]);
        assert_eq!(organizer.pages()[0].quarter_turns, 1);
        assert_eq!(organizer.undo_name(), Some("la restauration"));
        assert!(organizer.is_modified());
        assert!(organizer.undo());
        assert_eq!(order(&organizer), [0, 1, 2]);
        assert_eq!(organizer.restore(&[]), Err(EditError::WouldRemoveAllPages));
    }
}
