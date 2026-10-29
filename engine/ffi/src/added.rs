//! Objets ajoutés aux pages en mode Édition : textes et images, posés
//! par-dessus le contenu de la page, déplacés, modifiés et supprimés comme
//! les autres objets.

use std::collections::BTreeMap;
use std::sync::Arc;

use edit::{is_added_id, AddedImage, AddedObject, AddedText, FontChoice, PageOrigin};
use pdf_core::content::geometry::{Matrix, Point, Rect};
use pdf_core::AddedInfo;

use crate::content::{PageObjectItem, TextFont};
use crate::session::EditSession;
use crate::{EngineError, PageRect, PdfDocument};

/// Style d'un texte ajouté.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AddedTextStyle {
    /// Police : standard (Helvetica, Times, Courier et leurs variantes) ou
    /// installée sur le Mac.
    pub font: TextFont,
    /// Corps, en points.
    pub size: f32,
    /// Couleur : rouge, de 0 à 1.
    pub red: f32,
    /// Vert.
    pub green: f32,
    /// Bleu.
    pub blue: f32,
}

/// Objet ajouté, tel que le conserve la récupération (positions dans
/// l'espace de la page PDF).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum AddedObjectItem {
    /// Texte ajouté.
    Text {
        /// Texte.
        text: String,
        /// Style.
        style: AddedTextStyle,
        /// Coin supérieur gauche à l'affichage : abscisse dans la page PDF.
        x: f32,
        /// Ordonnée de ce coin dans la page PDF.
        y: f32,
    },
    /// Image ajoutée.
    Image {
        /// Octets du fichier image.
        data: Vec<u8>,
        /// Cadre dans la page PDF : bord gauche.
        x0: f32,
        /// Bord inférieur.
        y0: f32,
        /// Bord droit.
        x1: f32,
        /// Bord supérieur.
        y1: f32,
    },
}

impl AddedTextStyle {
    /// Police (une police standard plutôt que la police « d'origine », qui
    /// n'existe pas pour un texte ajouté), corps et couleur, bornés.
    fn parts(&self) -> (FontChoice, f32, [f32; 3]) {
        let style = self;
        let font = match FontChoice::from(style.font.clone()) {
            FontChoice::Original => FontChoice::Standard {
                name: "Helvetica".to_owned(),
            },
            font => font,
        };
        (
            font,
            style.size.clamp(1.0, 500.0),
            [style.red, style.green, style.blue].map(|c| c.clamp(0.0, 1.0)),
        )
    }
}

fn style_item(font: &FontChoice, size: f32, color: [f32; 3]) -> AddedTextStyle {
    let [red, green, blue] = color;
    AddedTextStyle {
        font: TextFont::from(font),
        size,
        red,
        green,
        blue,
    }
}

impl From<&AddedObject> for AddedObjectItem {
    fn from(object: &AddedObject) -> Self {
        match object {
            AddedObject::Text(text) => AddedObjectItem::Text {
                text: text.text.clone(),
                style: style_item(&text.font, text.size, text.color),
                x: text.x,
                y: text.y,
            },
            AddedObject::Image(image) => {
                let [x0, y0, x1, y1] = image.rect;
                AddedObjectItem::Image {
                    data: image.data.as_ref().clone(),
                    x0,
                    y0,
                    x1,
                    y1,
                }
            }
        }
    }
}

impl AddedObjectItem {
    /// Objet retrouvé ; `None` s'il n'est pas valable (texte vide, image
    /// illisible).
    pub(crate) fn object(&self) -> Option<AddedObject> {
        match self {
            AddedObjectItem::Text { text, style, x, y } => {
                if text.trim().is_empty() {
                    return None;
                }
                let (font, size, color) = style.parts();
                Some(AddedObject::Text(AddedText {
                    text: text.clone(),
                    font,
                    size,
                    color,
                    x: *x,
                    y: *y,
                }))
            }
            AddedObjectItem::Image {
                data,
                x0,
                y0,
                x1,
                y1,
            } => {
                pdf_core::image_size(data).ok()?;
                Some(AddedObject::Image(AddedImage::new(
                    data.clone(),
                    [*x0, *y0, *x1, *y1],
                )))
            }
        }
    }
}

/// Style d'un texte ajouté, d'après ce que le moteur en montre.
pub(crate) fn added_text_style(info: &Option<AddedInfo>) -> Option<AddedTextStyle> {
    match info {
        Some(AddedInfo::Text { font, size, color }) => Some(style_item(font, *size, *color)),
        _ => None,
    }
}

#[uniffi::export]
impl EditSession {
    /// Ajoute un texte à la page `page`, son coin supérieur gauche au point
    /// (`x`, `y`) de la page droite (en points, origine en haut à gauche),
    /// au-dessus du reste de la page. Une page vierge doit d'abord recevoir
    /// un document d'une page vierge de son format
    /// ([`EditSession::blank_page_document`]), ajouté aux sources sous le
    /// numéro `blank_source`. Renvoie le texte ajouté.
    pub fn add_text(
        &self,
        page: u64,
        x: f32,
        y: f32,
        text: String,
        style: AddedTextStyle,
        blank_source: Option<u32>,
    ) -> Result<PageObjectItem, EngineError> {
        if text.trim().is_empty() {
            return Err(invalid("texte vide"));
        }
        let (document, index, origin) = self.target(page, blank_source)?;
        let to_page = from_display(&document, index)?;
        let anchor = to_page.apply(Point::new(f64::from(x), f64::from(y)));
        let (font, size, color) = style.parts();
        let object = AddedObject::Text(AddedText {
            text,
            font,
            size,
            color,
            x: anchor.x as f32,
            y: anchor.y as f32,
        });
        let id = self.with_editor(|editor| editor.add_object(page, object, origin))?;
        self.object_item(page, id)
    }

    /// Ajoute l'image `data` (fichier JPEG ou PNG) à la page `page`, dans le
    /// cadre `bounds` de la page droite, au-dessus du reste de la page ; pour
    /// une page vierge, comme [`EditSession::add_text`]. Renvoie l'image
    /// ajoutée.
    pub fn add_image(
        &self,
        page: u64,
        data: Vec<u8>,
        bounds: PageRect,
        blank_source: Option<u32>,
    ) -> Result<PageObjectItem, EngineError> {
        pdf_core::image_size(&data).map_err(|err| EngineError::Failed {
            message: "Cette image ne peut pas être lue.".to_owned(),
            detail: err.detail().unwrap_or_default().to_owned(),
        })?;
        let (document, index, origin) = self.target(page, blank_source)?;
        let rect = page_rect(&document, index, bounds)?;
        let object = AddedObject::Image(AddedImage::new(data, rect));
        let id = self.with_editor(|editor| editor.add_object(page, object, origin))?;
        self.object_item(page, id)
    }

    /// Change le style (police, corps, couleur) du texte ajouté `object`.
    /// Renvoie le texte modifié.
    pub fn set_added_text_style(
        &self,
        page: u64,
        object: u32,
        style: AddedTextStyle,
    ) -> Result<PageObjectItem, EngineError> {
        let (font, size, color) = style.parts();
        let mut is_text = false;
        self.with_editor(|editor| {
            editor.edit_added(page, object, "le style du texte", |added| {
                if let AddedObject::Text(text) = added {
                    is_text = true;
                    text.font = font;
                    text.size = size;
                    text.color = color;
                }
            })
        })?;
        if !is_text {
            return Err(invalid("pas un texte ajouté"));
        }
        self.object_item(page, object)
    }

    /// Place l'image ajoutée `object` dans le cadre `bounds` de la page
    /// droite (redimensionnement). Renvoie l'image modifiée.
    pub fn set_object_bounds(
        &self,
        page: u64,
        object: u32,
        bounds: PageRect,
    ) -> Result<PageObjectItem, EngineError> {
        if !is_added_id(object) {
            return Err(invalid("objet d'origine"));
        }
        let (document, index, _) = self.page_source(page)?;
        let document = document.ok_or_else(|| invalid("page vierge"))?;
        let rect = page_rect(&document, index, bounds)?;
        let mut is_image = false;
        self.with_editor(|editor| {
            editor.edit_added(page, object, "le redimensionnement", |added| {
                if let AddedObject::Image(image) = added {
                    is_image = true;
                    image.rect = rect;
                }
            })
        })?;
        if !is_image {
            return Err(invalid("pas une image ajoutée"));
        }
        self.object_item(page, object)
    }

    /// PDF d'une page vierge du format de la page vierge `page`, à ajouter
    /// aux sources avant d'y ajouter un objet.
    pub fn blank_page_document(&self, page: u64) -> Result<Vec<u8>, EngineError> {
        let size = self
            .pages()
            .into_iter()
            .find(|item| item.id == page)
            .and_then(|item| match item.kind {
                crate::PageKind::Blank { size } => Some(size),
                crate::PageKind::Source { .. } => None,
            })
            .ok_or_else(|| invalid("pas une page vierge"))?;
        Ok(pdf_core::blank_document(size.width, size.height)?)
    }
}

impl EditSession {
    /// Document et indice de la page où ajouter un objet, et la nouvelle
    /// origine d'une page vierge.
    fn target(
        &self,
        page: u64,
        blank_source: Option<u32>,
    ) -> Result<(Arc<PdfDocument>, u32, Option<PageOrigin>), EngineError> {
        let (document, index, _) = self.page_source(page)?;
        if let Some(document) = document {
            return Ok((document, index, None));
        }
        let source = blank_source.ok_or_else(|| invalid("page vierge sans document"))?;
        let document = self
            .source_document(source)
            .filter(|document| document.page_count() == 1)
            .ok_or_else(|| invalid("document de page vierge inconnu"))?;
        Ok((document, 0, Some(PageOrigin::Source { source, index: 0 })))
    }

    /// L'objet `id` de la page `page`, tel que l'affiche l'application.
    pub(crate) fn object_item(&self, page: u64, id: u32) -> Result<PageObjectItem, EngineError> {
        self.page_objects(page)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| invalid("objet inexistant"))
    }
}

/// Transformation de l'espace d'affichage de la page vers celui de la page
/// PDF.
fn from_display(document: &PdfDocument, index: u32) -> Result<Matrix, EngineError> {
    document
        .handle
        .display_matrix(index)?
        .inverse()
        .ok_or_else(|| invalid("page dégénérée"))
}

/// Cadre `bounds` de la page droite dans l'espace de la page PDF.
fn page_rect(
    document: &PdfDocument,
    index: u32,
    bounds: PageRect,
) -> Result<[f32; 4], EngineError> {
    let valid = |v: f32| v.is_finite();
    if !(bounds.width > 0.5 && bounds.height > 0.5)
        || ![bounds.x, bounds.y, bounds.width, bounds.height]
            .into_iter()
            .all(valid)
    {
        return Err(invalid("cadre vide"));
    }
    let to_page = from_display(document, index)?;
    let (x, y) = (f64::from(bounds.x), f64::from(bounds.y));
    let rect = to_page.apply_rect(Rect::new(
        x,
        y,
        x + f64::from(bounds.width),
        y + f64::from(bounds.height),
    ));
    Ok([rect.x0, rect.y0, rect.x1, rect.y1].map(|v| v as f32))
}

/// Objets ajoutés retrouvés d'une page, par numéro.
pub(crate) fn restored_added(items: &[(u32, AddedObjectItem)]) -> BTreeMap<u32, AddedObject> {
    items
        .iter()
        .filter(|(id, _)| is_added_id(*id))
        .filter_map(|(id, item)| Some((*id, item.object()?)))
        .collect()
}

fn invalid(detail: &str) -> EngineError {
    EngineError::Failed {
        message: "Cette opération est impossible sur ce document.".to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{PageKind, PageObjectKind, PageSize};

    fn open(name: &str) -> Arc<PdfDocument> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        PdfDocument::open(path.to_string_lossy().into_owned(), None).unwrap()
    }

    /// PNG non compressé de `width` × `height` pixels RVB gris.
    fn png(width: u32, height: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        }
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.push(0);
            raw.extend(std::iter::repeat_n(128u8, 3 * width as usize));
        }
        let mut zlib = vec![0x78, 0x01, 1];
        let len = raw.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&raw);
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &zlib);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    fn style(size: f32) -> AddedTextStyle {
        AddedTextStyle {
            font: TextFont::Standard {
                name: "Times-Roman".to_owned(),
            },
            size,
            red: 0.8,
            green: 0.0,
            blue: 0.0,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> PageRect {
        PageRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn ajoute_modifie_retrouve_et_enregistre() {
        let session = EditSession::new(open("edition.pdf"));
        let page = session.pages()[1].id;
        let before = session.page_objects(page).unwrap().len();

        let text = session
            .add_text(page, 50.0, 60.0, "Note ajoutée".into(), style(18.0), None)
            .unwrap();
        assert!(text.added && text.editable && text.movable);
        assert_eq!(text.kind, PageObjectKind::Text);
        assert_eq!(text.added_text, Some(style(18.0)));
        assert!((text.bounds.x - 50.0).abs() < 0.01 && (text.bounds.y - 60.0).abs() < 0.01);
        assert_eq!(session.undo_name().as_deref(), Some("l'ajout du texte"));
        assert!(session
            .add_text(page, 0.0, 0.0, "  ".into(), style(12.0), None)
            .is_err());

        let image = session
            .add_image(page, png(4, 2), rect(100.0, 200.0, 80.0, 40.0), None)
            .unwrap();
        assert!(image.added && image.added_text.is_none());
        assert_eq!(image.kind, PageObjectKind::Image);
        assert!(session
            .add_image(page, b"rien".to_vec(), rect(0.0, 0.0, 10.0, 10.0), None)
            .is_err());

        // Déplacé (page tournée : à l'écran), redimensionné, restylé, réécrit.
        session
            .move_objects(page, vec![text.id, image.id], 10.0, 5.0)
            .unwrap();
        let moved = session.object_item(page, image.id).unwrap();
        assert!((moved.bounds.x - 110.0).abs() < 0.01 && (moved.bounds.y - 205.0).abs() < 0.01);
        let resized = session
            .set_object_bounds(page, image.id, rect(110.0, 205.0, 40.0, 20.0))
            .unwrap();
        assert!((resized.bounds.width - 40.0).abs() < 0.01);
        assert!(session
            .set_object_bounds(page, text.id, rect(0.0, 0.0, 5.0, 5.0))
            .is_err());
        let restyled = session
            .set_added_text_style(page, text.id, style(30.0))
            .unwrap();
        assert!(restyled.bounds.height > text.bounds.height);
        let rewritten = session
            .replace_text(page, text.id, "Autre note".into())
            .unwrap();
        assert_eq!(rewritten.text.as_deref(), Some("Autre note"));
        assert_eq!(rewritten.font_size, 30.0);
        assert_eq!(session.page_objects(page).unwrap().len(), before + 2);

        // Récupération : les objets ajoutés reviennent tels quels.
        let record = session.content_record(page).unwrap();
        assert_eq!(record.iter().filter(|item| item.added.is_some()).count(), 2);
        let restored = EditSession::new(open("edition.pdf"));
        let pages = session.pages();
        let contents = pages
            .iter()
            .map(|item| session.content_record(item.id).unwrap())
            .collect();
        restored.restore(pages, contents, Vec::new()).unwrap();
        let again = restored.page_objects(restored.pages()[1].id).unwrap();
        let mine = session.page_objects(page).unwrap();
        let bounds = |items: &[PageObjectItem]| -> Vec<(u32, i32, i32)> {
            items
                .iter()
                .filter(|item| item.added)
                .map(|item| (item.id, item.bounds.x as i32, item.bounds.height as i32))
                .collect()
        };
        assert_eq!(bounds(&again), bounds(&mine));

        // Enregistré, le texte ajouté se lit.
        let exported = session.export_pdf().unwrap();
        let reopened = PdfDocument::from_data(exported.bytes, None).unwrap();
        assert_eq!(
            reopened.search_page(1, "Autre note".into()).unwrap().len(),
            1
        );

        // Supprimés, puis tout s'annule.
        session
            .delete_objects(page, vec![text.id, image.id])
            .unwrap();
        assert_eq!(session.page_objects(page).unwrap().len(), before);
        while session.undo() {}
        assert!(session.content_edits(page).is_none());
    }

    #[test]
    fn ajoute_un_texte_sur_une_page_vierge() {
        let session = EditSession::new(open("texte_simple.pdf"));
        let size = PageSize {
            width: 300.0,
            height: 400.0,
        };
        let position = session.insert_blank(1, size).unwrap();
        let page = session.pages()[position as usize].id;
        assert!(session
            .add_text(page, 20.0, 20.0, "Seul".into(), style(12.0), None)
            .is_err());
        let bytes = session.blank_page_document(page).unwrap();
        let blank = PdfDocument::from_data(bytes, None).unwrap();
        assert_eq!(blank.page_count(), 1);
        let source = session.add_source(blank);
        let text = session
            .add_text(page, 20.0, 20.0, "Seul".into(), style(12.0), Some(source))
            .unwrap();
        assert!((text.bounds.x - 20.0).abs() < 0.01);
        assert_eq!(
            session.pages()[position as usize].kind,
            PageKind::Source { source, index: 0 }
        );
        let exported = session.export_pdf().unwrap();
        let reopened = PdfDocument::from_data(exported.bytes, None).unwrap();
        assert_eq!(reopened.page_sizes()[1].width, 300.0);
        assert_eq!(reopened.search_page(1, "Seul".into()).unwrap().len(), 1);
        // Annuler rend la page vierge.
        assert!(session.undo());
        assert!(matches!(
            session.pages()[position as usize].kind,
            PageKind::Blank { .. }
        ));
    }
}
