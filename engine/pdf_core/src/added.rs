//! Objets ajoutés à une page en mode Édition (textes, images) : emprise,
//! mise en page du texte et dessin par-dessus le contenu de la page, pour
//! l'aperçu comme pour l'enregistrement.
//!
//! Une image ajoutée est un fichier JPEG ou PNG, ou un PDF d'une page : des
//! objets copiés d'une page (cf. [`crate::clip`]) ou collés depuis une autre
//! application. La page de ce PDF est recopiée dans le document en
//! formulaire (`/Form`), dessiné droit dans le cadre de l'image comme elle
//! se montrait.
//!
//! Un objet ajouté est placé dans l'espace de la page PDF et dessiné droit
//! à l'écran : sa matrice compense la rotation propre de la page
//! (`/Rotate`) et les quarts de tour que lui donnait l'organisation quand il
//! a été ajouté (`turns`), comme le texte que l'on tape. Il est dessiné après
//! tout le contenu de la page (retouches d'un scan comprises), dans un flux
//! de plus entouré de `q … Q`.
//!
//! Police d'un texte ajouté : une police standard (Helvetica, Times,
//! Courier) n'est pas intégrée tant que l'encodage WinAnsi suffit à ses
//! caractères ; sinon son clone URW est intégré. Une police du Mac est
//! intégrée si sa licence le permet, sinon la police standard la plus proche
//! la remplace.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use edit::{AddedImage, AddedObject, AddedText, FontChoice};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::{Buffer, Device, DisplayList, Font};

use crate::content::geometry::{Matrix, Point, Rect};
use crate::content::objects::ObjectKind;
use crate::content::writer::{write_hex, write_name, write_op};
use crate::editing::{
    add_font, glyph_advance, page_box, page_contents, win_ansi_code, with_page_content,
    FontProvider, ObjectInfo, Scratch,
};
use crate::error::{PdfError, Result};
use crate::text::PageRect;

/// Interligne d'un texte ajouté, en corps.
const LEADING: f64 = 1.2;

/// Côté maximal d'une image ajoutée, en pixels : au-delà, MuPDF la
/// décoderait très lentement à chaque aperçu.
const MAX_IMAGE_SIDE: u32 = 12_000;

/// Ce que l'application montre d'un objet ajouté.
#[derive(Clone, Debug, PartialEq)]
pub enum AddedInfo {
    /// Texte, avec son style.
    Text {
        /// Police demandée.
        font: FontChoice,
        /// Corps, en points.
        size: f32,
        /// Couleur : rouge, vert, bleu, de 0 à 1.
        color: [f32; 3],
    },
    /// Image.
    Image,
}

/// Polices, images et formulaires déjà ajoutés à un document pour les
/// objets ajoutés.
#[derive(Default)]
pub(crate) struct AddedResources {
    fonts: HashMap<String, PdfObject>,
    images: HashMap<u64, PdfObject>,
    forms: HashMap<u64, PdfForm>,
}

/// Page d'un PDF ajouté comme image, recopiée en formulaire dans un
/// document.
struct PdfForm {
    form: PdfObject,
    /// Cadre de la page, dans son espace.
    media: Rect,
    /// Rotation de la page, en degrés (multiple de 90).
    rotation: i32,
}

/// Vrai si `data` est un PDF (objets copiés) plutôt qu'une image.
pub fn is_pdf(data: &[u8]) -> bool {
    data.windows(5).take(1024).any(|w| w == b"%PDF-")
}

/// Dimensions d'une image ajoutée : en pixels pour une image, en points
/// (page tournée comme elle se montre) pour un PDF ; erreur si MuPDF ne sait
/// pas la lire.
pub fn image_size(data: &[u8]) -> Result<(u32, u32)> {
    let unreadable = |detail: String| PdfError::WriteFailed {
        detail: format!("image illisible : {detail}"),
    };
    if is_pdf(data) {
        let doc = mupdf::Document::from_bytes(data, "application/pdf")
            .map_err(|err| unreadable(err.to_string()))?;
        if doc.page_count()? < 1 {
            return Err(unreadable("PDF sans page".to_owned()));
        }
        let bounds = doc.load_page(0)?.bounds()?;
        let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
        if !(width >= 0.5 && height >= 0.5 && width.is_finite() && height.is_finite()) {
            return Err(unreadable("page vide".to_owned()));
        }
        return Ok((width.ceil() as u32, height.ceil() as u32));
    }
    let image = mupdf::Image::from_bytes(data).map_err(|err| unreadable(err.to_string()))?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return Err(unreadable("image vide".to_owned()));
    }
    if width.max(height) > MAX_IMAGE_SIDE {
        return Err(unreadable(format!("{width} × {height} pixels")));
    }
    Ok((width, height))
}

/// Transformation de l'espace de la page PDF vers l'espace d'affichage, pour
/// une page de zone visible `area` tournée de `rotation` degrés (sens
/// horaire), comme `page_ctm` de MuPDF.
pub(crate) fn display_matrix(area: Rect, rotation: i32) -> Matrix {
    let flip = Matrix::new(1.0, 0.0, 0.0, -1.0, -area.x0, area.y1);
    let (w, h) = (area.width(), area.height());
    let turn = match rotation.rem_euclid(360) {
        90 => Matrix::new(0.0, 1.0, -1.0, 0.0, h, 0.0),
        180 => Matrix::new(-1.0, 0.0, 0.0, -1.0, w, h),
        270 => Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, w),
        _ => Matrix::IDENTITY,
    };
    flip.then(&turn)
}

/// PDF d'une seule page vierge de `width` × `height` points, où dessiner les
/// objets ajoutés à une page vierge.
pub fn blank_document(width: f32, height: f32) -> Result<Vec<u8>> {
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return Err(PdfError::WriteFailed {
            detail: format!("format de page vierge invalide : {width} × {height}"),
        });
    }
    let mut doc = PdfDocument::new();
    doc.new_page(mupdf::Size { width, height })?;
    let mut bytes = Vec::new();
    doc.write_to(&mut bytes)?;
    Ok(bytes)
}

// MARK: - Texte

/// Encodage des codes d'un texte ajouté.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    /// Police standard non intégrée : un octet WinAnsi par caractère.
    WinAnsi,
    /// Police intégrée par MuPDF (`Identity-H`) : identifiant du glyphe.
    GlyphId,
}

/// Police qui dessine un texte ajouté.
struct ResolvedFont {
    /// Clé de la police dans le document (nature et nom).
    key: String,
    font: Rc<Font>,
    encoding: Encoding,
    /// Nom affiché.
    name: String,
    /// Police employée à la place de celle demandée.
    substituted: bool,
}

/// Texte ajouté mis en page.
struct TextPlan {
    font: ResolvedFont,
    /// Codes et largeur (en points) de chaque ligne.
    lines: Vec<(Vec<u8>, f64)>,
    size: f64,
    /// Hauteur des ascendantes et profondeur des descendantes (négative), en
    /// points.
    ascent: f64,
    descent: f64,
    /// Caractères absents de la police, omis.
    missing: String,
}

impl TextPlan {
    fn leading(&self) -> f64 {
        LEADING * self.size
    }

    fn width(&self) -> f64 {
        self.lines.iter().map(|(_, w)| *w).fold(0.0, f64::max)
    }

    fn height(&self) -> f64 {
        self.ascent - self.descent + (self.lines.len().max(1) - 1) as f64 * self.leading()
    }

    /// Ligne de base de la ligne `index`, sous le haut du texte (axe vers le
    /// haut).
    fn baseline(&self, index: usize) -> f64 {
        -self.ascent - index as f64 * self.leading()
    }
}

/// Nom PostScript de la police standard la plus proche d'une police du Mac.
fn standard_for(postscript_name: &str) -> &'static str {
    let name = postscript_name.to_ascii_lowercase();
    let bold = ["bold", "black", "heavy", "semibold", "demi"]
        .iter()
        .any(|w| name.contains(w));
    let italic = name.contains("italic") || name.contains("oblique");
    let family = if ["courier", "mono", "menlo", "consol"]
        .iter()
        .any(|w| name.contains(w))
    {
        0
    } else if [
        "times",
        "serif",
        "georgia",
        "garamond",
        "palatino",
        "baskerville",
        "caladea",
        "cambria",
        "minion",
        "didot",
        "bodoni",
    ]
    .iter()
    .any(|w| name.contains(w))
        && !name.contains("sans")
    {
        1
    } else {
        2
    };
    const NAMES: [[&str; 4]; 3] = [
        [
            "Courier",
            "Courier-Bold",
            "Courier-Oblique",
            "Courier-BoldOblique",
        ],
        [
            "Times-Roman",
            "Times-Bold",
            "Times-Italic",
            "Times-BoldItalic",
        ],
        [
            "Helvetica",
            "Helvetica-Bold",
            "Helvetica-Oblique",
            "Helvetica-BoldOblique",
        ],
    ];
    NAMES[family][usize::from(bold) + 2 * usize::from(italic)]
}

/// Police qui dessinera `text` avec la police demandée `choice`.
fn resolve_font(choice: &FontChoice, text: &str, fonts: &mut FontProvider) -> Option<ResolvedFont> {
    let (name, substituted) = match choice {
        FontChoice::Installed { postscript_name } => {
            if let Some(font) = fonts.load(choice) {
                return Some(ResolvedFont {
                    key: format!("mac:{postscript_name}"),
                    font,
                    encoding: Encoding::GlyphId,
                    name: postscript_name.clone(),
                    substituted: false,
                });
            }
            (standard_for(postscript_name).to_owned(), true)
        }
        FontChoice::Standard { name } => (name.clone(), false),
        FontChoice::Original => ("Helvetica".to_owned(), false),
    };
    let standard = FontChoice::Standard { name: name.clone() };
    let font = fonts.load(&standard).or_else(|| {
        fonts.load(&FontChoice::Standard {
            name: "Helvetica".to_owned(),
        })
    })?;
    // Une police standard n'est intégrée que si WinAnsi ne suffit pas.
    let win_ansi = text
        .chars()
        .filter(|c| *c != '\n')
        .all(|c| win_ansi_code(c).is_some());
    let (key, encoding) = if win_ansi {
        (format!("std:{name}"), Encoding::WinAnsi)
    } else {
        (format!("urw:{name}"), Encoding::GlyphId)
    };
    Some(ResolvedFont {
        key,
        font,
        encoding,
        name,
        substituted,
    })
}

/// Met en page un texte ajouté : une ligne par saut de ligne, sans renvoi.
fn plan_text(text: &AddedText, fonts: &mut FontProvider) -> Option<TextPlan> {
    let font = resolve_font(&text.font, &text.text, fonts)?;
    let size = f64::from(text.size.clamp(1.0, 1000.0));
    let mut missing = String::new();
    let mut lines = Vec::new();
    for line in text.text.split('\n') {
        let line = line.trim_end_matches('\r');
        let (mut codes, mut width) = (Vec::new(), 0.0);
        for c in line.chars() {
            let glyph = match font.encoding {
                Encoding::WinAnsi => win_ansi_code(c)
                    .and_then(|code| Some((vec![code], glyph_advance(&font.font, c)?))),
                Encoding::GlyphId => font
                    .font
                    .encode_character(c as i32)
                    .ok()
                    .filter(|&gid| gid > 0)
                    .and_then(|gid| {
                        let advance = f64::from(font.font.advance_glyph(gid).ok()?) * 1000.0;
                        Some((vec![(gid >> 8) as u8, gid as u8], advance))
                    }),
            };
            match glyph {
                Some((code, advance)) => {
                    codes.extend_from_slice(&code);
                    width += advance * size / 1000.0;
                }
                None => {
                    if !missing.contains(c) {
                        missing.push(c);
                    }
                }
            }
        }
        lines.push((codes, width));
    }
    let ascent = f64::from(font.font.ascender());
    let descent = f64::from(font.font.descender());
    let (ascent, descent) = if ascent > 0.0 && descent < 0.0 && ascent - descent < 3.0 {
        (ascent * size, descent * size)
    } else {
        (0.8 * size, -0.2 * size)
    };
    Some(TextPlan {
        font,
        lines,
        size,
        ascent,
        descent,
        missing,
    })
}

// MARK: - Orientation

/// Repère d'un objet ajouté, droit à l'écran (origine en haut à gauche, axe
/// vers le bas), dans l'espace d'affichage d'une page que l'organisation
/// tournait de `turns` quarts de tour (sens horaire) : l'objet est tourné
/// d'autant en sens inverse. `origin` : point d'affichage de son origine.
fn upright(turns: u8, origin: Point) -> Matrix {
    let (a, b, c, d) = match turns & 3 {
        1 => (0.0, -1.0, 1.0, 0.0),
        2 => (-1.0, 0.0, 0.0, -1.0),
        3 => (0.0, 1.0, -1.0, 0.0),
        _ => (1.0, 0.0, 0.0, 1.0),
    };
    Matrix::new(a, b, c, d, origin.x, origin.y)
}

/// Repère droit à l'écran d'un cadre d'affichage `frame` : son origine est
/// le coin du cadre qui se montre en haut à gauche. Renvoie aussi la largeur
/// et la hauteur du cadre à l'écran.
fn upright_frame(turns: u8, frame: Rect) -> (Matrix, f64, f64) {
    let (width, height) = if turns % 2 == 1 {
        (frame.height(), frame.width())
    } else {
        (frame.width(), frame.height())
    };
    let turned =
        upright(turns, Point::new(0.0, 0.0)).apply_rect(Rect::new(0.0, 0.0, width, height));
    let origin = Point::new(frame.x0 - turned.x0, frame.y0 - turned.y0);
    (upright(turns, origin), width, height)
}

// MARK: - Emprise

/// Rectangle de l'espace d'affichage, au format de l'application.
fn page_rect(rect: Rect) -> PageRect {
    PageRect {
        x: rect.x0 as f32,
        y: rect.y0 as f32,
        width: rect.width() as f32,
        height: rect.height() as f32,
    }
}

/// Emprise d'un texte ajouté dans l'espace d'affichage ; `plan` est sa mise
/// en page.
fn text_frame(text: &AddedText, plan: &TextPlan, to_display: &Matrix) -> Rect {
    text_axes(text, to_display).apply_rect(Rect::new(
        0.0,
        0.0,
        plan.width().max(plan.size * 0.25),
        plan.height(),
    ))
}

/// Repère droit à l'écran d'un texte ajouté, dans l'espace d'affichage.
fn text_axes(text: &AddedText, to_display: &Matrix) -> Matrix {
    let anchor = to_display.apply(Point::new(f64::from(text.x), f64::from(text.y)));
    upright(text.turns, anchor)
}

/// Cadre d'une image ajoutée dans l'espace d'affichage.
fn image_frame(image: &AddedImage, to_display: &Matrix) -> Rect {
    let [x0, y0, x1, y1] = image.rect.map(f64::from);
    to_display.apply_rect(Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)))
}

/// Objets ajoutés, tels que les montre l'application, dans l'ordre
/// d'empilement.
pub(crate) fn infos(
    added: &BTreeMap<u32, AddedObject>,
    to_display: &Matrix,
    fonts: &mut FontProvider,
) -> Vec<ObjectInfo> {
    let mut infos = Vec::with_capacity(added.len());
    for (&id, object) in added {
        let mut info = ObjectInfo {
            id,
            kind: ObjectKind::Image,
            bounds: PageRect::default(),
            movable: true,
            resizable: true,
            text_color: None,
            editable: false,
            background: false,
            text: None,
            font_name: None,
            font_size: 0.0,
            substituted: false,
            overflow: false,
            missing: String::new(),
            fingerprint: "ajout".to_owned(),
            table: None,
            grid: None,
            scanned: false,
            scan: None,
            added: Some(AddedInfo::Image),
        };
        match object {
            AddedObject::Text(text) => {
                let Some(plan) = plan_text(text, fonts) else {
                    continue;
                };
                info.kind = ObjectKind::Text;
                info.bounds = page_rect(text_frame(text, &plan, to_display));
                info.editable = true;
                info.text = Some(text.text.clone());
                info.font_name = Some(plan.font.name.clone());
                info.font_size = text.size;
                info.substituted = plan.font.substituted;
                info.missing = plan.missing;
                info.added = Some(AddedInfo::Text {
                    font: text.font.clone(),
                    size: text.size,
                    color: text.color,
                });
            }
            AddedObject::Image(image) => {
                info.bounds = page_rect(image_frame(image, to_display));
            }
        }
        infos.push(info);
    }
    infos
}

// MARK: - Dessin

/// Contenu qui dessine les objets ajoutés et les ressources qu'il emploie.
struct Drawing {
    content: Vec<u8>,
    fonts: Vec<(Vec<u8>, PdfObject)>,
    images: Vec<(Vec<u8>, PdfObject)>,
}

/// Noms des ressources d'une catégorie (`Font`, `XObject`) de la page.
fn resource_names(resources: Option<&PdfObject>, category: &str) -> Result<HashSet<Vec<u8>>> {
    let mut names = HashSet::new();
    let Some(resources) = resources else {
        return Ok(names);
    };
    if !resources.is_dict()? {
        return Ok(names);
    }
    if let Some(dict) = resources.get_dict(category)? {
        if dict.is_dict()? {
            for entry in dict.dict_iter()? {
                let (key, _) = entry?;
                names.insert(key.as_name()?);
            }
        }
    }
    Ok(names)
}

/// Nom de ressource libre, de préfixe `prefix`.
fn free_name(taken: &mut HashSet<Vec<u8>>, prefix: &str) -> Vec<u8> {
    let mut index = 1;
    loop {
        let name = format!("{prefix}{index}").into_bytes();
        if taken.insert(name.clone()) {
            return name;
        }
        index += 1;
    }
}

/// Dictionnaire d'une police standard non intégrée, encodage WinAnsi.
fn standard_font_dict(doc: &mut PdfDocument, name: &str, font: &Font) -> Result<PdfObject> {
    add_font(
        doc,
        &FontChoice::Standard {
            name: name.to_owned(),
        },
        font,
    )
}

/// Contenu qui dessine `added` sur une page dont l'espace d'affichage est
/// donné par `to_display` ; les polices et images sont ajoutées au document
/// (une seule fois, grâce à `cache`).
fn draw(
    doc: &mut PdfDocument,
    page: &PdfObject,
    to_display: &Matrix,
    added: &BTreeMap<u32, AddedObject>,
    fonts: &mut FontProvider,
    cache: &mut AddedResources,
) -> Result<Drawing> {
    let resources = page.get_dict_inheritable("Resources")?;
    let mut font_names = resource_names(resources.as_ref(), "Font")?;
    let mut image_names = resource_names(resources.as_ref(), "XObject")?;
    let from_display = to_display.inverse().unwrap_or(Matrix::IDENTITY);
    let mut drawing = Drawing {
        content: Vec::new(),
        fonts: Vec::new(),
        images: Vec::new(),
    };
    let mut used_fonts: HashMap<String, Vec<u8>> = HashMap::new();
    let mut used_images: HashMap<u64, Vec<u8>> = HashMap::new();
    for object in added.values() {
        match object {
            AddedObject::Text(text) => {
                let Some(plan) = plan_text(text, fonts) else {
                    continue;
                };
                if plan.lines.iter().all(|(codes, _)| codes.is_empty()) {
                    continue;
                }
                let name = match used_fonts.get(&plan.font.key) {
                    Some(name) => name.clone(),
                    None => {
                        let reference = match cache.fonts.get(&plan.font.key) {
                            Some(reference) => reference.try_clone()?,
                            None => {
                                let reference = match plan.font.encoding {
                                    Encoding::WinAnsi => {
                                        standard_font_dict(doc, &plan.font.name, &plan.font.font)?
                                    }
                                    Encoding::GlyphId => doc.add_font(&plan.font.font)?,
                                };
                                cache
                                    .fonts
                                    .insert(plan.font.key.clone(), reference.try_clone()?);
                                reference
                            }
                        };
                        let name = free_name(&mut font_names, "PSAF");
                        drawing.fonts.push((name.clone(), reference));
                        used_fonts.insert(plan.font.key.clone(), name.clone());
                        name
                    }
                };
                // Espace du texte (origine au coin supérieur gauche, axe vers
                // le haut) → repère droit à l'écran → affichage → page.
                let m = Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, 0.0)
                    .then(&text_axes(text, to_display))
                    .then(&from_display);
                let out = &mut drawing.content;
                write_op(out, &[], "q");
                write_op(out, &[m.a, m.b, m.c, m.d, m.e, m.f], "cm");
                let [r, g, b] = text.color.map(|c| f64::from(c.clamp(0.0, 1.0)));
                write_op(out, &[r, g, b], "rg");
                write_op(out, &[], "BT");
                write_name(out, &name);
                out.push(b' ');
                write_op(out, &[plan.size], "Tf");
                write_op(out, &[0.0], "Tr");
                for (index, (codes, _)) in plan.lines.iter().enumerate() {
                    if codes.is_empty() {
                        continue;
                    }
                    write_op(out, &[1.0, 0.0, 0.0, 1.0, 0.0, plan.baseline(index)], "Tm");
                    write_hex(out, codes);
                    out.extend_from_slice(b" Tj\n");
                }
                write_op(out, &[], "ET");
                write_op(out, &[], "Q");
            }
            AddedObject::Image(image) if is_pdf(&image.data) => {
                let frame = image_frame(image, to_display);
                if frame.width() <= 0.0 || frame.height() <= 0.0 {
                    continue;
                }
                let form = match cache.forms.entry(image.key) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(pdf_form(doc, &image.data)?)
                    }
                };
                let form = &*form;
                let name = match used_images.get(&image.key) {
                    Some(name) => name.clone(),
                    None => {
                        let name = free_name(&mut image_names, "PSAI");
                        drawing.images.push((name.clone(), form.form.try_clone()?));
                        used_images.insert(image.key, name.clone());
                        name
                    }
                };
                // Espace de la page copiée → la page telle qu'elle se
                // montrait (origine en haut à gauche) → cadre droit à
                // l'écran → affichage → page.
                let shown = display_matrix(form.media, form.rotation);
                let size = shown.apply_rect(form.media);
                let (axes, width, height) = upright_frame(image.turns, frame);
                let fit = Matrix::new(
                    width / size.width().max(1e-6),
                    0.0,
                    0.0,
                    height / size.height().max(1e-6),
                    0.0,
                    0.0,
                );
                let m = shown.then(&fit).then(&axes).then(&from_display);
                let out = &mut drawing.content;
                write_op(out, &[], "q");
                write_op(out, &[m.a, m.b, m.c, m.d, m.e, m.f], "cm");
                write_name(out, &name);
                out.extend_from_slice(b" Do\n");
                write_op(out, &[], "Q");
            }
            AddedObject::Image(image) => {
                let frame = image_frame(image, to_display);
                if frame.width() <= 0.0 || frame.height() <= 0.0 {
                    continue;
                }
                let name = match used_images.get(&image.key) {
                    Some(name) => name.clone(),
                    None => {
                        let reference = match cache.images.get(&image.key) {
                            Some(reference) => reference.try_clone()?,
                            None => {
                                let decoded = mupdf::Image::from_bytes(&image.data)?;
                                let reference = doc.add_image(&decoded)?;
                                cache.images.insert(image.key, reference.try_clone()?);
                                reference
                            }
                        };
                        let name = free_name(&mut image_names, "PSAI");
                        drawing.images.push((name.clone(), reference));
                        used_images.insert(image.key, name.clone());
                        name
                    }
                };
                // Carré unité de l'image → cadre droit à l'écran (première
                // ligne de pixels en haut) → affichage → page.
                let (axes, width, height) = upright_frame(image.turns, frame);
                let m = Matrix::new(width, 0.0, 0.0, -height, 0.0, height)
                    .then(&axes)
                    .then(&from_display);
                let out = &mut drawing.content;
                write_op(out, &[], "q");
                write_op(out, &[m.a, m.b, m.c, m.d, m.e, m.f], "cm");
                write_name(out, &name);
                out.extend_from_slice(b" Do\n");
                write_op(out, &[], "Q");
            }
        }
    }
    Ok(drawing)
}

/// Recopie dans `doc`, en formulaire, la première page du PDF `data`.
fn pdf_form(doc: &mut PdfDocument, data: &[u8]) -> Result<PdfForm> {
    let clip = PdfDocument::from_bytes(data)?;
    if clip.page_count()? < 1 {
        return Err(PdfError::WriteFailed {
            detail: "PDF copié sans page".to_owned(),
        });
    }
    let page = clip.find_page(0)?;
    let media = page_box(&page)?;
    let rotation = match page.get_dict_inheritable("Rotate")? {
        Some(value) if value.is_number()? => value.as_int()?,
        _ => 0,
    };
    // Comme MuPDF, une rotation qui n'est pas un multiple de 90° est arrondie.
    let rotation = (rotation.rem_euclid(360) + 45) / 90 * 90 % 360;
    let content = page_contents(&page)?;
    let mut map = doc.new_graft_map()?;
    let mut dict = doc.new_dict()?;
    dict.dict_put("Type", PdfObject::new_name("XObject")?)?;
    dict.dict_put("Subtype", PdfObject::new_name("Form")?)?;
    let mut bbox = doc.new_array()?;
    for value in [media.x0, media.y0, media.x1, media.y1] {
        bbox.array_push(PdfObject::new_real(value as f32)?)?;
    }
    dict.dict_put("BBox", bbox)?;
    let resources = match page.get_dict_inheritable("Resources")? {
        Some(resources) => map.graft_object(&resources)?,
        None => doc.new_dict()?,
    };
    dict.dict_put("Resources", resources)?;
    let form = doc.add_stream(&Buffer::from_bytes(&content)?, Some(&dict), false)?;
    Ok(PdfForm {
        form,
        media,
        rotation,
    })
}

/// Ressources de la page augmentées de celles du dessin.
fn resources_with(doc: &mut PdfDocument, page: &PdfObject, drawing: &Drawing) -> Result<PdfObject> {
    let resources = page.get_dict_inheritable("Resources")?;
    let mut copy = match &resources {
        Some(resources) if resources.is_dict()? => crate::writer::detached_copy(resources)?,
        _ => doc.new_dict()?,
    };
    for (category, added) in [("Font", &drawing.fonts), ("XObject", &drawing.images)] {
        if added.is_empty() {
            continue;
        }
        let mut dict = match copy.get_dict(category)? {
            Some(dict) if dict.is_dict()? => crate::writer::detached_copy(&dict)?,
            _ => doc.new_dict()?,
        };
        for (name, reference) in added {
            dict.dict_put(
                String::from_utf8_lossy(name).into_owned(),
                reference.try_clone()?,
            )?;
        }
        copy.dict_put(category, dict)?;
    }
    Ok(copy)
}

/// Liste d'affichage des seuls objets ajoutés de la page `index`, pour
/// l'aperçu ; `None` s'il n'y a rien à dessiner.
pub(crate) fn overlay(
    doc: &mut PdfDocument,
    index: u32,
    to_display: &Matrix,
    added: &BTreeMap<u32, AddedObject>,
    fonts: &mut FontProvider,
    cache: &mut AddedResources,
    scratch: &mut Scratch,
) -> Result<Option<DisplayList>> {
    let page = doc.find_page(index as i32)?;
    let drawing = draw(doc, &page, to_display, added, fonts, cache)?;
    if drawing.content.is_empty() {
        return Ok(None);
    }
    let resources = resources_with(doc, &page, &drawing)?;
    let list = with_page_content(
        doc,
        index,
        &drawing.content,
        Some(&resources),
        scratch,
        |doc| Ok(doc.load_page(index as i32)?.to_display_list(false)?),
    )?;
    Ok(Some(list))
}

/// Liste d'affichage de `top` posée sur `base`.
pub(crate) fn stack(base: &DisplayList, top: &DisplayList) -> Result<DisplayList> {
    let mut out = DisplayList::new(base.bounds())?;
    {
        let device = Device::from_display_list(&mut out)?;
        base.run(&device, &mupdf::Matrix::IDENTITY, mupdf::Rect::INF)?;
        top.run(&device, &mupdf::Matrix::IDENTITY, mupdf::Rect::INF)?;
    }
    Ok(out)
}

/// Ajoute à la page `page` du document écrit le dessin de ses objets
/// ajoutés, dans un flux de contenu de plus, après le reste de son contenu
/// entouré de `q … Q`. `quarter_turns` : quarts de tour ajoutés à la page par
/// l'organisation, déjà appliqués à son `/Rotate`, que les objets suivent
/// comme le reste de la page.
pub(crate) fn append(
    doc: &mut PdfDocument,
    position: usize,
    quarter_turns: u8,
    added: &BTreeMap<u32, AddedObject>,
    fonts: &mut FontProvider,
    cache: &mut AddedResources,
) -> Result<()> {
    let mut page = doc.find_page(position as i32)?;
    let rotation = doc.load_pdf_page(position as i32)?.rotation()?;
    // Comme MuPDF, une rotation qui n'est pas un multiple de 90° est arrondie.
    let rotation = (rotation.rem_euclid(360) + 45) / 90 * 90 - 90 * i32::from(quarter_turns);
    let to_display = display_matrix(page_box(&page)?, rotation);
    let drawing = draw(doc, &page, &to_display, added, fonts, cache)?;
    if drawing.content.is_empty() {
        return Ok(());
    }
    let resources = resources_with(doc, &page, &drawing)?;
    page.dict_put("Resources", resources)?;
    let mut contents = doc.new_array()?;
    contents.array_push(doc.add_stream(&Buffer::from_bytes(b"q\n")?, None, false)?)?;
    match page.get_dict("Contents")? {
        Some(existing) if existing.is_array()? => {
            for item in existing.array_iter()? {
                contents.array_push(item?)?;
            }
        }
        Some(existing) => contents.array_push(existing)?,
        None => {}
    }
    let mut content = b"\nQ\n".to_vec();
    content.extend_from_slice(&drawing.content);
    contents.array_push(doc.add_stream(&Buffer::from_bytes(&content)?, None, false)?)?;
    page.dict_put("Contents", contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn police_standard_la_plus_proche_d_une_police_du_mac() {
        assert_eq!(standard_for("Avenir-Book"), "Helvetica");
        assert_eq!(standard_for("Georgia-BoldItalic"), "Times-BoldItalic");
        assert_eq!(standard_for("Menlo-Bold"), "Courier-Bold");
        assert_eq!(standard_for("PTSans-Italic"), "Helvetica-Oblique");
    }

    #[test]
    fn matrice_d_affichage_selon_la_rotation() {
        let area = Rect::new(0.0, 0.0, 200.0, 100.0);
        let top_left = Point::new(0.0, 100.0);
        let p = display_matrix(area, 0).apply(top_left);
        assert_eq!((p.x, p.y), (0.0, 0.0));
        // Tournée d'un quart de tour : le haut de la page passe à droite.
        let p = display_matrix(area, 90).apply(top_left);
        assert_eq!((p.x, p.y), (100.0, 0.0));
        let p = display_matrix(area, 180).apply(top_left);
        assert_eq!((p.x, p.y), (200.0, 100.0));
        let p = display_matrix(area, 270).apply(top_left);
        assert_eq!((p.x, p.y), (0.0, 200.0));
    }

    #[test]
    fn repere_droit_sur_une_page_tournee() {
        // Page tournée d'un quart de tour à l'écran : la droite de l'écran
        // est le haut de l'affichage, le bas de l'écran sa droite.
        let m = upright(1, Point::new(10.0, 50.0));
        let right = m.apply(Point::new(1.0, 0.0));
        assert_eq!((right.x, right.y), (10.0, 49.0));
        let down = m.apply(Point::new(0.0, 1.0));
        assert_eq!((down.x, down.y), (11.0, 50.0));
        // Le cadre garde sa place ; ses côtés à l'écran sont échangés.
        let frame = Rect::new(100.0, 200.0, 140.0, 280.0);
        for turns in 0..4 {
            let (axes, width, height) = upright_frame(turns, frame);
            let back = axes.apply_rect(Rect::new(0.0, 0.0, width, height));
            assert_eq!(
                (back.x0, back.y0, back.x1, back.y1),
                (100.0, 200.0, 140.0, 280.0),
                "{turns}"
            );
            assert_eq!(width, if turns % 2 == 1 { 80.0 } else { 40.0 });
        }
    }
}
