//! Objets copiés, coupés, collés, dupliqués ou déplacés d'une page à une
//! autre, en mode Édition.
//!
//! Une copie ([`ObjectClipboard`]) range les objets tels qu'ils se montrent
//! à l'écran, page tournée par l'organisation : les objets d'origine en un
//! PDF d'une page cadré sur eux (cf. `pdf_core::clip`), les textes et images
//! ajoutés tels quels, pour qu'ils restent modifiables. Collés, ils
//! deviennent des objets ajoutés de la page visée : le PDF des objets
//! d'origine en image ajoutée (dessinée en formulaire), droite sur la page
//! telle qu'elle se montre. La copie porte aussi ce que d'autres
//! applications savent coller : un PDF de tous les objets et leur texte.

use std::sync::Arc;

use edit::{AddedImage, AddedObject, AddedText};
use pdf_core::content::geometry::{Point, Rect};

use crate::added::AddedTextStyle;
use crate::content::{PageObjectItem, PageObjectKind, TextFont};
use crate::session::EditSession;
use crate::{EngineError, PageRect, PageSize, PdfDocument};

/// Décalage d'un objet dupliqué, ou collé sur sa propre page, en points à
/// l'écran (vers la droite et vers le bas).
pub const PASTE_OFFSET: f32 = 12.0;

/// Contenu d'un objet copié.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ClipContent {
    /// Texte ajouté, qui reste modifiable une fois collé.
    Text {
        /// Texte.
        text: String,
        /// Style.
        style: AddedTextStyle,
    },
    /// Image (JPEG, PNG) ou PDF d'une page (objets d'origine copiés).
    Image {
        /// Octets du fichier.
        data: Vec<u8>,
    },
}

/// Un objet copié et son cadre à l'écran, relatif au coin supérieur gauche
/// de la copie (en points, ordonnées vers le bas).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ClipPiece {
    /// Contenu.
    pub content: ClipContent,
    /// Bord gauche.
    pub x: f32,
    /// Bord supérieur.
    pub y: f32,
    /// Largeur.
    pub width: f32,
    /// Hauteur.
    pub height: f32,
}

/// Objets copiés, à coller sur une page de ce document ou d'un autre.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ObjectClipboard {
    /// Objets, de bas en haut.
    pub pieces: Vec<ClipPiece>,
    /// Coin supérieur gauche de la copie sur sa page d'origine, à l'écran
    /// (page tournée par l'organisation), en points.
    pub x: f32,
    /// Ordonnée de ce coin, vers le bas.
    pub y: f32,
    /// Largeur à l'écran.
    pub width: f32,
    /// Hauteur à l'écran.
    pub height: f32,
    /// PDF d'une page de tous les objets, pour les autres applications ; vide
    /// s'il n'a pas pu être produit.
    pub pdf: Vec<u8>,
    /// Texte des objets (blocs de texte, de haut en bas), pour les autres
    /// applications.
    pub text: String,
}

/// Où coller des objets.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum PastePlacement {
    /// À leur place sur leur page d'origine, décalés de (`dx`, `dy`) points à
    /// l'écran.
    InPlace {
        /// Décalage vers la droite.
        dx: f32,
        /// Décalage vers le bas.
        dy: f32,
    },
    /// Centrés sur le point (`x`, `y`) de la page droite (en points, origine
    /// en haut à gauche, sans les quarts de tour de l'organisation).
    Centered {
        /// Abscisse.
        x: f32,
        /// Ordonnée.
        y: f32,
    },
}

/// Copie faite d'un seul fichier (image ou PDF collé depuis une autre
/// application), de taille `width` × `height` points à l'écran.
#[uniffi::export]
pub fn clipboard_from_file(
    data: Vec<u8>,
    width: f32,
    height: f32,
) -> Result<ObjectClipboard, EngineError> {
    pdf_core::image_size(&data).map_err(|err| EngineError::Failed {
        message: "Ce contenu ne peut pas être collé.".to_owned(),
        detail: err.detail().unwrap_or_default().to_owned(),
    })?;
    let (width, height) = (width.max(1.0), height.max(1.0));
    Ok(ObjectClipboard {
        pieces: vec![ClipPiece {
            content: ClipContent::Image { data },
            x: 0.0,
            y: 0.0,
            width,
            height,
        }],
        x: 0.0,
        y: 0.0,
        width,
        height,
        pdf: Vec::new(),
        text: String::new(),
    })
}

/// Taille à l'écran, en points, de la première page du PDF `data` : celle
/// d'un PDF collé depuis une autre application.
#[uniffi::export]
pub fn pdf_clip_size(data: Vec<u8>) -> Result<PageSize, EngineError> {
    if !pdf_core::is_pdf(&data) {
        return Err(invalid("pas un PDF"));
    }
    let (width, height) = pdf_core::image_size(&data)?;
    Ok(PageSize {
        width: width as f32,
        height: height as f32,
    })
}

/// Passage de la page droite (espace d'affichage, sans les quarts de tour de
/// l'organisation) à la page telle qu'elle se montre, tournée de `turns`
/// quarts de tour dans le sens horaire.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Upright {
    width: f64,
    height: f64,
    turns: u8,
}

impl Upright {
    pub(crate) fn new(size: PageSize, turns: u8) -> Self {
        Self {
            width: f64::from(size.width),
            height: f64::from(size.height),
            turns: turns & 3,
        }
    }

    /// Taille de la page à l'écran.
    fn size(&self) -> (f64, f64) {
        if self.turns % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    fn point(&self, x: f64, y: f64) -> (f64, f64) {
        let (w, h) = (self.width, self.height);
        match self.turns {
            1 => (h - y, x),
            2 => (w - x, h - y),
            3 => (y, w - x),
            _ => (x, y),
        }
    }

    pub(crate) fn back(&self, u: f64, v: f64) -> (f64, f64) {
        let (w, h) = (self.width, self.height);
        match self.turns {
            1 => (v, h - u),
            2 => (w - u, h - v),
            3 => (w - v, u),
            _ => (u, v),
        }
    }

    /// Rectangle de la page droite, à l'écran.
    pub(crate) fn rect(&self, rect: &PageRect) -> Rect {
        let (x, y) = (f64::from(rect.x), f64::from(rect.y));
        let a = self.point(x, y);
        let b = self.point(x + f64::from(rect.width), y + f64::from(rect.height));
        Rect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }

    /// Rectangle de l'écran, sur la page droite.
    fn back_rect(&self, rect: Rect) -> PageRect {
        let a = self.back(rect.x0, rect.y0);
        let b = self.back(rect.x1, rect.y1);
        PageRect {
            x: a.0.min(b.0) as f32,
            y: a.1.min(b.1) as f32,
            width: (a.0 - b.0).abs() as f32,
            height: (a.1 - b.1).abs() as f32,
        }
    }
}

#[uniffi::export]
impl EditSession {
    /// Copie les objets `objects` de la page `page` (les objets verrouillés
    /// et ceux d'un scan ne se copient pas) ; rien ne change dans le
    /// document.
    pub fn copy_objects(
        &self,
        page: u64,
        objects: Vec<u32>,
    ) -> Result<ObjectClipboard, EngineError> {
        let (document, index, edits) = self.page_source(page)?;
        let document = document.ok_or_else(|| invalid("page vierge"))?;
        let upright = Upright::new(page_size(&document, index)?, self.page_turns(page));
        let items: Vec<PageObjectItem> = self
            .page_objects(page)?
            .into_iter()
            .filter(|item| objects.contains(&item.id) && item.movable && !item.scanned)
            .collect();
        if items.is_empty() {
            return Err(EngineError::Failed {
                message: "Ces objets ne peuvent pas être copiés.".to_owned(),
                detail: "aucun objet copiable".to_owned(),
            });
        }
        let turns = upright.turns;
        let added = |id: u32| edits.as_ref().and_then(|edits| edits.added().get(&id));
        // Un objet ajouté que l'écran montre tourné (posé avant un quart de
        // tour de la page) part dans l'extrait, qui le garde tel qu'il se voit.
        let native = |item: &PageObjectItem| {
            item.added && added(item.id).is_some_and(|object| object.turns() & 3 == turns)
        };
        // Un bloc de texte d'origine copié reste un texte, modifiable, dans le
        // style retrouvé (police, corps, couleur) ; le reste passe dans l'extrait.
        let retyped = |item: &PageObjectItem| original_text_style(item, turns);
        let originals: Vec<u32> = items
            .iter()
            .filter(|item| !native(item) && retyped(item).is_none())
            .map(|item| item.id)
            .collect();
        let mut pieces: Vec<(ClipContent, Rect)> = Vec::new();
        let mut originals_pdf = None;
        if !originals.is_empty() {
            if let Some(clip) =
                document
                    .handle
                    .clip(index, edits.clone(), originals.clone(), turns)?
            {
                let bounds = crate::PageRect {
                    x: clip.bounds.x,
                    y: clip.bounds.y,
                    width: clip.bounds.width,
                    height: clip.bounds.height,
                };
                originals_pdf = Some(clip.pdf.clone());
                pieces.push((ClipContent::Image { data: clip.pdf }, upright.rect(&bounds)));
            }
        }
        for item in items.iter().filter(|item| native(item)) {
            let Some(object) = added(item.id) else {
                continue;
            };
            let content = match object {
                AddedObject::Text(text) => ClipContent::Text {
                    text: text.text.clone(),
                    style: crate::added::style_item(&text.font, text.size, text.color),
                },
                AddedObject::Image(image) => ClipContent::Image {
                    data: image.data.as_ref().clone(),
                },
            };
            pieces.push((content, upright.rect(&item.bounds)));
        }
        for item in items.iter().filter(|item| !native(item)) {
            if let (Some(style), Some(text)) = (retyped(item), item.text.clone()) {
                pieces.push((
                    ClipContent::Text { text, style },
                    upright.rect(&item.bounds),
                ));
            }
        }
        if pieces.is_empty() {
            return Err(invalid("rien à copier"));
        }
        let group = pieces
            .iter()
            .fold(Rect::EMPTY, |rect, (_, piece)| rect.union(piece));
        // Pour les autres applications : un PDF de tous les objets.
        let pdf = match (&originals_pdf, pieces.len()) {
            (Some(pdf), 1) => pdf.clone(),
            _ => {
                let ids: Vec<u32> = items.iter().map(|item| item.id).collect();
                document
                    .handle
                    .clip(index, edits.clone(), ids, turns)
                    .ok()
                    .flatten()
                    .map(|clip| clip.pdf)
                    .unwrap_or_default()
            }
        };
        let mut texts: Vec<(f64, f64, String)> = items
            .iter()
            .filter_map(|item| {
                let text = item.text.clone()?;
                let rect = upright.rect(&item.bounds);
                Some((rect.y0, rect.x0, text))
            })
            .collect();
        texts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        Ok(ObjectClipboard {
            pieces: pieces
                .into_iter()
                .map(|(content, rect)| ClipPiece {
                    content,
                    x: (rect.x0 - group.x0) as f32,
                    y: (rect.y0 - group.y0) as f32,
                    width: rect.width() as f32,
                    height: rect.height() as f32,
                })
                .collect(),
            x: group.x0 as f32,
            y: group.y0 as f32,
            width: group.width() as f32,
            height: group.height() as f32,
            pdf,
            text: texts
                .into_iter()
                .map(|(_, _, text)| text)
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }

    /// Coupe les objets `objects` de la page `page` : les copie, puis les
    /// supprime (⌘Z les rend).
    pub fn cut_objects(
        &self,
        page: u64,
        objects: Vec<u32>,
    ) -> Result<ObjectClipboard, EngineError> {
        let clipboard = self.copy_objects(page, objects.clone())?;
        let movable: Vec<u32> = self
            .page_objects(page)?
            .into_iter()
            .filter(|item| objects.contains(&item.id) && item.movable && !item.scanned)
            .map(|item| item.id)
            .collect();
        self.with_editor(|editor| editor.remove_objects(page, &movable, "la coupe"))?;
        Ok(clipboard)
    }

    /// Colle les objets copiés `clipboard` sur la page `page`, au-dessus des
    /// autres, à la place `placement` ; pour une page vierge, comme
    /// [`EditSession::add_text`]. Renvoie les objets collés.
    pub fn paste_objects(
        &self,
        page: u64,
        clipboard: ObjectClipboard,
        placement: PastePlacement,
        blank_source: Option<u32>,
    ) -> Result<Vec<PageObjectItem>, EngineError> {
        self.place(
            page,
            &clipboard,
            placement,
            blank_source,
            None,
            "le collage",
        )
    }

    /// Duplique les objets `objects` de la page `page`, un peu plus bas à
    /// droite. Renvoie les copies.
    pub fn duplicate_objects(
        &self,
        page: u64,
        objects: Vec<u32>,
    ) -> Result<Vec<PageObjectItem>, EngineError> {
        let clipboard = self.copy_objects(page, objects)?;
        let offset = PastePlacement::InPlace {
            dx: PASTE_OFFSET,
            dy: PASTE_OFFSET,
        };
        self.place(page, &clipboard, offset, None, None, "la duplication")
    }

    /// Déplace (ou copie, si `keep`) les objets `objects` de la page `from`
    /// sur la page `to`, centrés sur le point (`x`, `y`) de sa page droite,
    /// en une seule étape d'annulation. Renvoie les objets posés sur `to`.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer_objects(
        &self,
        from: u64,
        objects: Vec<u32>,
        to: u64,
        x: f32,
        y: f32,
        keep: bool,
        blank_source: Option<u32>,
    ) -> Result<Vec<PageObjectItem>, EngineError> {
        let clipboard = self.copy_objects(from, objects.clone())?;
        let moved: Vec<u32> = self
            .page_objects(from)?
            .into_iter()
            .filter(|item| objects.contains(&item.id) && item.movable && !item.scanned)
            .map(|item| item.id)
            .collect();
        let removed = (!keep).then_some((from, moved.as_slice()));
        let name = if keep { "la copie" } else { "le déplacement" };
        self.place(
            to,
            &clipboard,
            PastePlacement::Centered { x, y },
            blank_source,
            removed,
            name,
        )
    }
}

impl EditSession {
    /// Pose les objets de `clipboard` sur la page `page` et retire
    /// éventuellement des objets d'une page (`removed`), en une étape
    /// d'annulation nommée `name`.
    fn place(
        &self,
        page: u64,
        clipboard: &ObjectClipboard,
        placement: PastePlacement,
        blank_source: Option<u32>,
        removed: Option<(u64, &[u32])>,
        name: &'static str,
    ) -> Result<Vec<PageObjectItem>, EngineError> {
        if clipboard.pieces.is_empty() {
            return Err(invalid("copie vide"));
        }
        let (document, index, origin, turns) = self.target(page, blank_source)?;
        let upright = Upright::new(page_size(&document, index)?, turns);
        let (width, height) = (f64::from(clipboard.width), f64::from(clipboard.height));
        let (x, y) = match placement {
            PastePlacement::InPlace { dx, dy } => (
                f64::from(clipboard.x) + f64::from(dx),
                f64::from(clipboard.y) + f64::from(dy),
            ),
            PastePlacement::Centered { x, y } => {
                let (u, v) = upright.point(f64::from(x), f64::from(y));
                (u - width / 2.0, v - height / 2.0)
            }
        };
        // La copie reste sur la page, si elle y tient.
        let (page_width, page_height) = upright.size();
        let x = x.min(page_width - width).max(0.0);
        let y = y.min(page_height - height).max(0.0);
        let to_page = document
            .handle
            .display_matrix(index)?
            .inverse()
            .ok_or_else(|| invalid("page dégénérée"))?;
        let mut objects = Vec::with_capacity(clipboard.pieces.len());
        for piece in &clipboard.pieces {
            let (px, py) = (x + f64::from(piece.x), y + f64::from(piece.y));
            let shown = Rect::new(
                px,
                py,
                px + f64::from(piece.width.max(0.5)),
                py + f64::from(piece.height.max(0.5)),
            );
            let object = match &piece.content {
                ClipContent::Text { text, style } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    // Le coin supérieur gauche à l'écran, sur la page droite.
                    let (cx, cy) = upright.back(shown.x0, shown.y0);
                    let anchor = to_page.apply(Point::new(cx, cy));
                    let (font, size, color) = style.parts();
                    AddedObject::Text(AddedText {
                        text: text.clone(),
                        font,
                        size,
                        color,
                        x: anchor.x as f32,
                        y: anchor.y as f32,
                        turns,
                    })
                }
                ClipContent::Image { data } => {
                    pdf_core::image_size(data).map_err(|err| EngineError::Failed {
                        message: "Ce contenu ne peut pas être collé.".to_owned(),
                        detail: err.detail().unwrap_or_default().to_owned(),
                    })?;
                    let frame = upright.back_rect(shown);
                    let (fx, fy) = (f64::from(frame.x), f64::from(frame.y));
                    let rect = to_page.apply_rect(Rect::new(
                        fx,
                        fy,
                        fx + f64::from(frame.width),
                        fy + f64::from(frame.height),
                    ));
                    AddedObject::Image(AddedImage::new(
                        data.clone(),
                        [rect.x0, rect.y0, rect.x1, rect.y1].map(|v| v as f32),
                        turns,
                    ))
                }
            };
            objects.push(object);
        }
        let ids =
            self.with_editor(|editor| editor.add_objects(page, objects, origin, removed, name))?;
        let items = self.page_objects(page)?;
        Ok(items
            .into_iter()
            .filter(|item| ids.contains(&item.id))
            .collect())
    }

    /// Quarts de tour que l'organisation donne à la page `page`.
    pub(crate) fn page_turns(&self, page: u64) -> u8 {
        self.pages()
            .into_iter()
            .find(|item| item.id == page)
            .map_or(0, |item| item.quarter_turns & 3)
    }
}

/// Style d'un bloc de texte d'origine copié comme un texte : police retrouvée
/// (sans le préfixe des polices réduites), corps affiché et couleur ; `None`
/// pour un autre objet, un texte qui ne se réécrit pas, ou une page dont
/// l'affichage est tourné (le texte copié est droit, l'extrait garde l'aspect).
fn original_text_style(item: &PageObjectItem, turns: u8) -> Option<AddedTextStyle> {
    if item.added || !item.editable || item.scanned || item.table.is_some() || turns != 0 {
        return None;
    }
    if item.kind != PageObjectKind::Text || item.text.as_deref().is_none_or(|t| t.trim().is_empty())
    {
        return None;
    }
    let name = item.font_name.as_deref()?;
    let name = match name.split_once('+') {
        Some((prefix, rest)) if prefix.len() == 6 => rest,
        _ => name,
    };
    let [red, green, blue] = match item.text_color.as_slice() {
        [r, g, b] => [*r, *g, *b],
        _ => [0.0; 3],
    };
    Some(AddedTextStyle {
        font: TextFont::Installed {
            postscript_name: name.to_owned(),
        },
        size: item.font_size.max(1.0),
        red,
        green,
        blue,
    })
}

/// Taille de la page droite `index` du document.
pub(crate) fn page_size(document: &Arc<PdfDocument>, index: u32) -> Result<PageSize, EngineError> {
    document
        .page_sizes()
        .get(index as usize)
        .copied()
        .ok_or_else(|| invalid("page inexistante"))
}

fn invalid(detail: &str) -> EngineError {
    EngineError::Failed {
        message: "Cette opération est impossible sur ce document.".to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_tournee_a_l_ecran() {
        let size = PageSize {
            width: 200.0,
            height: 100.0,
        };
        for turns in 0..4 {
            let upright = Upright::new(size, turns);
            let (u, v) = upright.point(30.0, 20.0);
            let (x, y) = upright.back(u, v);
            assert!(
                (x - 30.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9,
                "{turns}"
            );
            let rect = PageRect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            };
            let back = upright.back_rect(upright.rect(&rect));
            assert_eq!(back, rect, "{turns}");
        }
        // Un quart de tour : le coin inférieur gauche passe en haut à gauche.
        let upright = Upright::new(size, 1);
        assert_eq!(upright.point(0.0, 100.0), (0.0, 0.0));
        assert_eq!(upright.size(), (100.0, 200.0));
    }
}
