//! Tableaux collés qui restent des tableaux (D75).
//!
//! Des objets d'origine copiés sont collés en image ajoutée : l'extrait PDF
//! qui les contient, dessiné en formulaire (cf. [`crate::clipboard`]). Quand
//! cet extrait contient un tableau, la page le montre par ses objets, comme
//! ceux d'origine : blocs de texte, cellules, lignes et colonnes. Leurs
//! identifiants portent le rang de l'image ajoutée et l'identifiant de l'objet
//! dans l'extrait ; leur emprise est celle de l'extrait, mise à l'échelle de
//! l'image.
//!
//! Une modification d'un de ces objets se fait sur l'extrait, rouvert comme
//! un document (une session à part), puis l'extrait modifié remplace celui de
//! l'image dans la même étape d'annulation : l'extrait garde ainsi toute la
//! modification, et la récupération n'a rien de plus à retenir.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use edit::{AddedImage, AddedObject};

use crate::content::{PageObjectItem, PageObjectKind, TableCellItem, TableGridItem};
use crate::session::EditSession;
use crate::{EngineError, PageRect, PageSize, PdfDocument};

/// Marque des objets d'un extrait collé (ni objet ajouté, ni objet d'un scan).
pub(crate) const CLIP_ID: u32 = 0x1000_0000;
const CLIP_MASK: u32 = 0x7000_0000;
/// Rangs distincts d'images ajoutées.
const SLOTS: u32 = 0xFFF;
/// Les tableaux d'un extrait suivent tous ses autres objets dans l'ordre, et
/// leurs numéros changent quand une modification fait varier le nombre
/// d'objets : ils sont désignés par leur rang à partir de cette base.
const TABLES: u32 = 0xF000;
/// Part de l'extrait qu'un tableau doit couvrir pour le représenter tout entier.
const WHOLE: f32 = 0.85;
/// Extraits lus gardés en mémoire.
const CACHE: usize = 16;

/// L'objet est un objet d'un extrait collé.
pub(crate) fn is_clip_id(id: u32) -> bool {
    id & CLIP_MASK == CLIP_ID
}

fn slot_of(added: u32) -> u32 {
    added & SLOTS
}

fn clip_id(added: u32, inner: u32) -> Option<u32> {
    (inner < TABLES).then(|| CLIP_ID | (slot_of(added) << 16) | inner)
}

/// Identifiants de l'extrait pour des identifiants de la page : les tableaux,
/// désignés par leur rang, retrouvent leur numéro dans la session `session`.
pub(crate) fn inner_ids(session: &EditSession, page: u64, ids: &[u32]) -> Vec<u32> {
    if ids.iter().all(|&id| id < TABLES) {
        return ids.to_vec();
    }
    let tables: Vec<u32> = session
        .page_objects(page)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.kind == PageObjectKind::Table)
        .map(|item| item.id)
        .collect();
    ids.iter()
        .filter_map(|&id| match id.checked_sub(TABLES) {
            Some(rank) => tables.get(rank as usize).copied(),
            None => Some(id),
        })
        .collect()
}

/// Rang de l'image et identifiant dans l'extrait.
fn split(id: u32) -> (u32, u32) {
    ((id >> 16) & SLOTS, id & 0xFFFF)
}

/// Objet de la page `item` qu'une opération désigne par `ids` (objets ajoutés
/// compris : l'extrait d'une image désignée).
pub(crate) fn owned_by(item: u32, ids: &[u32]) -> bool {
    ids.contains(&item)
        || (is_clip_id(item)
            && ids
                .iter()
                .any(|&id| edit::is_added_id(id) && slot_of(id) == split(item).0))
}

/// Un extrait collé, lu comme un document d'une page.
pub(crate) struct ClipInner {
    size: PageSize,
    items: Vec<PageObjectItem>,
    /// Tableaux de l'extrait, dans l'ordre de leurs numéros.
    tables: Vec<u32>,
}

/// Un extrait collé et sa place sur la page.
pub(crate) struct ClipView {
    /// Identifiant de l'image ajoutée.
    pub(crate) added: u32,
    image: AddedImage,
    bounds: PageRect,
    inner: Arc<ClipInner>,
}

/// Objets désignés, répartis entre la page et ses extraits collés.
pub(crate) struct Routed {
    /// Objets de la page ; un extrait désigné tout entier y figure par son
    /// image ajoutée.
    pub(crate) page_ids: Vec<u32>,
    /// Objets désignés dans un extrait (identifiants dans l'extrait), quand
    /// ils n'en désignent qu'une partie.
    pub(crate) clips: Vec<(ClipView, Vec<u32>)>,
}

impl ClipView {
    /// Échelle de l'extrait sur la page, en largeur et en hauteur.
    pub(crate) fn scales(&self) -> (f32, f32) {
        (
            self.bounds.width / self.inner.size.width.max(1.0),
            self.bounds.height / self.inner.size.height.max(1.0),
        )
    }

    /// Les objets `ids` de l'extrait le représentent tout entier : tous, ou
    /// un tableau qui l'occupe.
    fn covers(&self, ids: &[u32]) -> bool {
        let size = self.inner.size;
        let ids: Vec<u32> = ids
            .iter()
            .filter_map(|&id| match id.checked_sub(TABLES) {
                Some(rank) => self.inner.tables.get(rank as usize).copied(),
                None => Some(id),
            })
            .collect();
        let ids = ids.as_slice();
        let all = self
            .inner
            .items
            .iter()
            .filter(|item| item.movable)
            .all(|item| ids.contains(&item.id));
        all || self.inner.items.iter().any(|item| {
            item.kind == PageObjectKind::Table
                && ids.contains(&item.id)
                && item.bounds.width >= size.width * WHOLE
                && item.bounds.height >= size.height * WHOLE
        })
    }

    /// Rectangle de l'extrait, sur la page.
    pub(crate) fn rect(&self, rect: &PageRect) -> PageRect {
        let (sx, sy) = self.scales();
        PageRect {
            x: self.bounds.x + rect.x * sx,
            y: self.bounds.y + rect.y * sy,
            width: rect.width * sx,
            height: rect.height * sy,
        }
    }
}

impl ClipInner {
    /// Objets de l'extrait vus de la page : identifiants, emprises et corps à
    /// l'échelle de l'image `added` posée en `bounds`.
    fn on_page(&self, added: u32, bounds: &PageRect) -> Vec<PageObjectItem> {
        let id = |inner: u32| match self.tables.iter().position(|&table| table == inner) {
            Some(rank) => Some(CLIP_ID | (slot_of(added) << 16) | (TABLES + rank as u32)),
            None => clip_id(added, inner),
        };
        let (sx, sy) = (
            bounds.width / self.size.width.max(1.0),
            bounds.height / self.size.height.max(1.0),
        );
        let rect = |rect: &PageRect| PageRect {
            x: bounds.x + rect.x * sx,
            y: bounds.y + rect.y * sy,
            width: rect.width * sx,
            height: rect.height * sy,
        };
        self.items
            .iter()
            .filter_map(|item| {
                let mut shown = item.clone();
                shown.id = id(item.id)?;
                shown.bounds = rect(&item.bounds);
                shown.font_size = item.font_size * sy;
                shown.baseline = item.baseline.map(|line| bounds.y + line * sy);
                shown.table = item.table.and_then(id);
                shown.resizable = false;
                shown.grid = item.grid.as_ref().map(|grid| TableGridItem {
                    rows: grid.rows,
                    columns: grid.columns,
                    cells: grid
                        .cells
                        .iter()
                        .map(|cell| TableCellItem {
                            bounds: rect(&cell.bounds),
                            objects: cell
                                .objects
                                .iter()
                                .filter_map(|&object| id(object))
                                .collect(),
                            ..cell.clone()
                        })
                        .collect(),
                });
                Some(shown)
            })
            .collect()
    }
}

/// Ouvre un extrait comme un document : sa session, sa page et sa taille.
fn open_session(data: &[u8]) -> Result<(Arc<EditSession>, u64, PageSize), EngineError> {
    let document = PdfDocument::from_data(data.to_vec(), None)?;
    let size = document
        .page_sizes()
        .first()
        .copied()
        .ok_or_else(|| invalid("extrait sans page"))?;
    let session = EditSession::new(document);
    let page = session
        .pages()
        .first()
        .map(|page| page.id)
        .ok_or_else(|| invalid("extrait sans page"))?;
    Ok((session, page, size))
}

/// Nom d'action de la session de l'extrait, tel que l'historique de la page le
/// retient.
fn action_name(session: &EditSession) -> &'static str {
    const NAMES: [&str; 12] = [
        "la suppression",
        "le déplacement",
        "la modification du texte",
        "la saisie",
        "la suppression de la ligne",
        "la suppression des lignes",
        "la suppression de la colonne",
        "la suppression des colonnes",
        "le déplacement de la ligne",
        "le déplacement de la colonne",
        "la coupe",
        "la restauration",
    ];
    let name = session.undo_name();
    NAMES
        .into_iter()
        .find(|known| Some(*known) == name.as_deref())
        .unwrap_or("la modification")
}

impl EditSession {
    /// Extrait lu de l'image `image`, s'il contient un tableau.
    fn clip_inner(&self, image: &AddedImage) -> Option<Arc<ClipInner>> {
        let mut cache = self.clips.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(known) = cache.get(&image.key) {
            return known.clone();
        }
        let inner = (|| {
            let (session, page, size) = open_session(&image.data).ok()?;
            let items = session.page_objects(page).ok()?;
            let tables: Vec<u32> = items
                .iter()
                .filter(|item| item.kind == PageObjectKind::Table)
                .map(|item| item.id)
                .collect();
            (!tables.is_empty()).then(|| {
                Arc::new(ClipInner {
                    size,
                    items,
                    tables,
                })
            })
        })();
        if cache.len() >= CACHE {
            cache.clear();
        }
        cache.insert(image.key, inner.clone());
        inner
    }

    /// Extrait collé que la page montre par ses objets : image ajoutée,
    /// extrait lu et emprise de l'image.
    fn clip_images(
        &self,
        page: u64,
        raw: &[PageObjectItem],
    ) -> Vec<(u32, AddedImage, Arc<ClipInner>, PageRect)> {
        // Une page tournée par l'organisation garde ses extraits en images.
        if self.page_turns(page) != 0 {
            return Vec::new();
        }
        let Some(edits) = self.content(page) else {
            return Vec::new();
        };
        edits
            .added()
            .iter()
            .filter_map(|(&id, object)| {
                let AddedObject::Image(image) = object else {
                    return None;
                };
                if image.turns & 3 != 0 || !pdf_core::is_pdf(&image.data) {
                    return None;
                }
                let bounds = raw.iter().find(|item| item.id == id)?.bounds;
                let inner = self.clip_inner(image)?;
                Some((id, image.clone(), inner, bounds))
            })
            .collect()
    }

    /// Remplace les images d'extraits à tableau par leurs objets.
    pub(crate) fn expand_clips(
        &self,
        page: u64,
        items: Vec<PageObjectItem>,
    ) -> Vec<PageObjectItem> {
        let clips = self.clip_images(page, &items);
        if clips.is_empty() {
            return items;
        }
        let mut shown = Vec::with_capacity(items.len());
        for item in items {
            match clips.iter().find(|(id, ..)| *id == item.id) {
                Some((id, _, inner, bounds)) => shown.extend(inner.on_page(*id, bounds)),
                None => shown.push(item),
            }
        }
        shown
    }

    /// L'extrait de rang `slot` de la page `page`.
    fn clip_view(&self, page: u64, slot: u32) -> Result<ClipView, EngineError> {
        let raw = self.page_objects_raw(page)?;
        self.clip_images(page, &raw)
            .into_iter()
            .find(|(id, ..)| slot_of(*id) == slot)
            .map(|(added, image, inner, bounds)| ClipView {
                added,
                image,
                bounds,
                inner,
            })
            .ok_or_else(|| invalid("objet inexistant"))
    }

    /// Répartit les objets `objects` entre la page et ses extraits collés.
    pub(crate) fn route(&self, page: u64, objects: &[u32]) -> Result<Routed, EngineError> {
        let mut page_ids = Vec::new();
        let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &id in objects {
            if is_clip_id(id) {
                let (slot, inner) = split(id);
                groups.entry(slot).or_default().push(inner);
            } else {
                page_ids.push(id);
            }
        }
        let mut clips = Vec::new();
        for (slot, ids) in groups {
            let view = self.clip_view(page, slot)?;
            if view.covers(&ids) {
                page_ids.push(view.added);
            } else {
                clips.push((view, ids));
            }
        }
        Ok(Routed { page_ids, clips })
    }

    /// Fait `change` sur l'extrait de `view`, rouvert à part, puis remplace
    /// l'extrait de l'image par le résultat, en une étape d'annulation.
    pub(crate) fn on_clip<R>(
        &self,
        page: u64,
        view: &ClipView,
        change: impl FnOnce(&EditSession, u64) -> Result<R, EngineError>,
    ) -> Result<R, EngineError> {
        let (session, inner_page, _) = open_session(&view.image.data)?;
        let result = change(&session, inner_page)?;
        let name = action_name(&session);
        let bytes = session.export_pdf()?.bytes;
        self.with_editor(|editor| {
            editor.edit_added(page, view.added, name, |object| {
                if let AddedObject::Image(image) = object {
                    *image = AddedImage::new(bytes, image.rect, image.turns);
                }
            })
        })?;
        Ok(result)
    }

    /// Fait `change` sur un objet `object` d'un extrait.
    pub(crate) fn on_clip_object<R>(
        &self,
        page: u64,
        object: u32,
        change: impl FnOnce(&EditSession, u64, u32) -> Result<R, EngineError>,
    ) -> Result<R, EngineError> {
        let (slot, inner) = split(object);
        let view = self.clip_view(page, slot)?;
        self.on_clip(page, &view, |session, inner_page| {
            let inner = inner_ids(session, inner_page, &[inner]);
            let inner = inner
                .first()
                .copied()
                .ok_or_else(|| invalid("objet inexistant"))?;
            change(session, inner_page, inner)
        })
    }

    /// Supprime les objets désignés de la page et de ses extraits, sous le
    /// nom d'action `name`.
    pub(crate) fn remove_routed(
        &self,
        page: u64,
        routed: Routed,
        name: &'static str,
    ) -> Result<(), EngineError> {
        if routed.page_ids.is_empty() && routed.clips.is_empty() {
            return Err(invalid("aucun objet désigné"));
        }
        for (view, ids) in &routed.clips {
            self.on_clip(page, view, |session, inner_page| {
                session.delete_objects(inner_page, inner_ids(session, inner_page, ids))
            })?;
        }
        if !routed.page_ids.is_empty() {
            self.with_editor(|editor| editor.remove_objects(page, &routed.page_ids, name))?;
        }
        Ok(())
    }

    /// Copie des objets `ids` d'un extrait, vue de la page.
    pub(crate) fn copy_from_clip(
        &self,
        view: &ClipView,
        ids: Vec<u32>,
    ) -> Result<crate::clipboard::ObjectClipboard, EngineError> {
        let (session, inner_page, _) = open_session(&view.image.data)?;
        let ids = inner_ids(&session, inner_page, &ids);
        let mut copy = session.copy_objects(inner_page, ids)?;
        let (sx, sy) = view.scales();
        for piece in &mut copy.pieces {
            piece.x *= sx;
            piece.y *= sy;
            piece.width *= sx;
            piece.height *= sy;
            piece.baseline = piece.baseline.map(|line| line * sy);
            if let crate::clipboard::ClipContent::Text { style, .. } = &mut piece.content {
                style.size *= sy;
            }
        }
        let group = view.rect(&PageRect {
            x: copy.x,
            y: copy.y,
            width: copy.width,
            height: copy.height,
        });
        copy.x = group.x;
        copy.y = group.y;
        copy.width = group.width;
        copy.height = group.height;
        Ok(copy)
    }

    /// Identifiants des objets de la page qui sont ceux de `ids` ou de
    /// l'extrait d'une image désignée.
    pub(crate) fn pasted(
        &self,
        page: u64,
        ids: &[u32],
    ) -> Result<Vec<PageObjectItem>, EngineError> {
        Ok(self
            .page_objects(page)?
            .into_iter()
            .filter(|item| owned_by(item.id, ids))
            .collect())
    }

    /// Les objets `objects` qui se copient, se coupent ou se déplacent, avec
    /// les extraits dont ils font partie.
    pub(crate) fn movable_routed(&self, page: u64, objects: &[u32]) -> Result<Routed, EngineError> {
        let routed = self.route(page, objects)?;
        let keep: HashSet<u32> = self
            .page_objects_raw(page)?
            .into_iter()
            .filter(|item| item.movable && !item.scanned)
            .map(|item| item.id)
            .collect();
        Ok(Routed {
            page_ids: routed
                .page_ids
                .into_iter()
                .filter(|id| keep.contains(id))
                .collect(),
            clips: routed.clips,
        })
    }
}

fn invalid(detail: &str) -> EngineError {
    EngineError::Failed {
        message: "Cette opération est impossible sur ce document.".to_owned(),
        detail: detail.to_owned(),
    }
}
