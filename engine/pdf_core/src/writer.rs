//! Écriture d'un PDF réorganisé : ordre des pages, rotation, pages copiées
//! depuis d'autres documents et pages vierges.
//!
//! Le document ouvert sert de base : son catalogue (métadonnées, formulaire,
//! préférences d'affichage…) est conservé tel quel et seules ses pages sont
//! réagencées. Les signets, liens et champs de formulaire qui visaient une page
//! supprimée sont retirés ; ceux des pages conservées continuent de fonctionner,
//! même après un déplacement, puisque la page reste le même objet PDF. Plus rien
//! ne renvoie à une page supprimée : elle disparaît du fichier avec son contenu.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use edit::{PageEdits, TextLayer};
use mupdf::pdf::{Encryption, PdfDocument, PdfGraftMap, PdfObject, PdfWriteOptions};
use mupdf::{Buffer, Size};

use crate::added::{self, AddedResources};
use crate::document::{has_scan_edits, vector_edits};
use crate::editing::{self, EmbeddedFonts, FontProvider, PageModel};
use crate::scanned::{self, ScanModel};
use crate::textlayer;

use crate::error::{PdfError, Result};

/// Contenu d'un document source, tel que lu à l'ouverture.
///
/// L'enregistrement repart de ces octets, et non du fichier sur le disque, qui a
/// pu changer entre-temps.
#[derive(Clone)]
pub struct SourceData {
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) password: Option<String>,
}

impl SourceData {
    /// Source en mémoire (tests, documents générés).
    pub fn new(bytes: impl Into<Arc<[u8]>>, password: Option<String>) -> Self {
        Self {
            bytes: bytes.into(),
            password,
        }
    }

    /// Contenu binaire du document.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Une page du PDF à écrire.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputPage {
    /// Page `index` du document `sources[source]`.
    Source {
        /// Indice du document dans la liste des sources ; 0 est la base.
        source: usize,
        /// Indice de la page dans ce document.
        index: u32,
        /// Quarts de tour ajoutés à la rotation propre de la page.
        quarter_turns: u8,
    },
    /// Page vierge.
    Blank {
        /// Largeur en points.
        width: f32,
        /// Hauteur en points.
        height: f32,
        /// Quarts de tour dans le sens horaire.
        quarter_turns: u8,
    },
    /// Page `index` du document `sources[source]` (son objet, ses
    /// annotations, ses liens, les signets qui la visent), dont le contenu et
    /// les ressources sont remplacés par ceux de la page `content_index` de
    /// `sources[content_source]` : une page améliorée (cf.
    /// [`crate::write_enhanced_pages`]).
    Replaced {
        /// Document de la page d'origine.
        source: usize,
        /// Indice de la page d'origine.
        index: u32,
        /// Document de la page améliorée.
        content_source: usize,
        /// Indice de la page améliorée.
        content_index: u32,
        /// Quarts de tour ajoutés à la rotation propre de la page.
        quarter_turns: u8,
    },
}

/// Écrit le PDF formé de `pages`, à partir de `sources` (la première est la base).
///
/// Le résultat est relu avant d'être renvoyé : un fichier qui ne s'ouvre pas, ou
/// pas avec le bon nombre de pages, n'est jamais livré.
pub fn write_pdf(sources: &[SourceData], pages: &[OutputPage]) -> Result<Vec<u8>> {
    write_edited_pdf(sources, pages, &[])
}

/// Comme [`write_pdf`], en appliquant au contenu de chaque page les
/// modifications de `edits` (même position que dans `pages` ; une liste plus
/// courte laisse les pages suivantes intactes).
pub fn write_edited_pdf(
    sources: &[SourceData],
    pages: &[OutputPage],
    edits: &[Option<Arc<PageEdits>>],
) -> Result<Vec<u8>> {
    write_document(sources, pages, edits, &[])
}

/// Comme [`write_edited_pdf`], en ajoutant aux pages reconnues leur couche de
/// texte invisible `layers` (même position que dans `pages`).
pub fn write_document(
    sources: &[SourceData],
    pages: &[OutputPage],
    edits: &[Option<Arc<PageEdits>>],
    layers: &[Option<Arc<TextLayer>>],
) -> Result<Vec<u8>> {
    let base = sources
        .first()
        .ok_or_else(|| invalid("aucun document source"))?;
    if pages.is_empty() {
        return Err(invalid("aucune page à écrire"));
    }
    let mut doc = open(base)?;
    let original_count = doc.page_count()?.max(0) as usize;

    if is_same_order(pages, original_count) && !has_shared_pages(&doc, original_count)? {
        let mut others: HashMap<usize, (PdfDocument, PdfGraftMap)> = HashMap::new();
        for (position, page) in pages.iter().enumerate() {
            if let OutputPage::Replaced {
                content_source,
                content_index,
                ..
            } = *page
            {
                let (other, map) = other_source(&doc, sources, &mut others, content_source)?;
                replace_content(&mut doc, map, other, content_index, position)?;
            }
            rotate(&doc, position, page_turns(page))?;
        }
    } else {
        rebuild(&mut doc, sources, pages, original_count)?;
    }
    remove_replaced_text(&mut doc, layers)?;
    let layers = apply_edits(&mut doc, pages, edits, layers)?;
    apply_layers(&mut doc, &layers)?;

    let mut options = PdfWriteOptions::default();
    options
        .set_garbage_level(3)
        .set_compress(true)
        .set_compress_images(true)
        .set_encryption(Encryption::Keep);
    let mut output = Vec::new();
    doc.write_to_with_options(&mut output, options)?;
    verify(&output, base.password.as_deref(), pages.len())?;
    Ok(output)
}

/// Retire des pages qui reçoivent une couche de texte le texte invisible
/// qu'elles portaient déjà, posé sur leur scan par un OCR précédent (cf.
/// [`scanned::has_replaceable_text`]) : la nouvelle couche le remplace, et le
/// texte ne se trouve pas deux fois. Le texte invisible n'est pas un objet de
/// la page : les numéros des objets, et donc les modifications, ne changent
/// pas.
fn remove_replaced_text(doc: &mut PdfDocument, layers: &[Option<Arc<TextLayer>>]) -> Result<()> {
    for (position, layer) in layers.iter().enumerate() {
        if !layer.as_ref().is_some_and(|layer| !layer.is_empty()) {
            continue;
        }
        let mut page = doc.find_page(position as i32)?;
        let model = PageModel::load(&page)?;
        let bounds = doc.load_page(position as i32)?.bounds()?;
        let area = f64::from(bounds.x1 - bounds.x0) * f64::from(bounds.y1 - bounds.y0);
        if !scanned::has_replaceable_text(&page, &model, area)? {
            continue;
        }
        let stream = doc.add_stream(&Buffer::from_bytes(&model.without_text())?, None, false)?;
        page.dict_put("Contents", stream)?;
        if let Some(resources) = model.resources(doc, &page)? {
            page.dict_put("Resources", resources)?;
        }
    }
    Ok(())
}

/// Remplace le contenu des pages modifiées. Chaque page est alors un objet
/// distinct (les pages répétées ont été copiées) : son nouveau contenu ne
/// s'applique qu'à elle, et les objets partagés (flux, ressources) restent
/// intacts. Sur une page scannée reconnue, les pixels retouchés sont posés
/// en calques par-dessus son contenu, puis les objets ajoutés. Renvoie la
/// couche de texte de chaque page, retouches comprises.
fn apply_edits(
    doc: &mut PdfDocument,
    pages: &[OutputPage],
    edits: &[Option<Arc<PageEdits>>],
    layers: &[Option<Arc<TextLayer>>],
) -> Result<Vec<Option<Arc<TextLayer>>>> {
    let mut fonts = FontProvider::default();
    let mut embedded = EmbeddedFonts::new();
    let mut added_resources = AddedResources::default();
    let mut effective = layers.to_vec();
    for (position, edits) in edits.iter().enumerate() {
        let Some(edits) = edits.as_ref().filter(|edits| !edits.is_empty()) else {
            continue;
        };
        let mut page = doc.find_page(position as i32)?;
        let model = PageModel::load(&page)?;
        let layer = layers.get(position).cloned().flatten();
        // Retouches du scan, calculées sur le contenu d'origine de la page.
        let scan = match &layer {
            Some(layer) if !layer.is_empty() && has_scan_edits(edits) => {
                let bounds = doc.load_page(position as i32)?.bounds()?;
                let area = f64::from(bounds.x1 - bounds.x0) * f64::from(bounds.y1 - bounds.y0);
                match scanned::find_scan(&page, &model, area)? {
                    Some(image) => {
                        let base = scanned::render(doc, position as u32, &image.placement)?;
                        let scan = ScanModel::build(image, base, layer);
                        let retouched = scanned::retouch(&scan, layer, edits);
                        Some((image.placement, retouched))
                    }
                    None => None,
                }
            }
            _ => None,
        };
        let vector = vector_edits(edits);
        if !vector.is_empty() {
            let edited =
                editing::edit_page(doc, &page, &model, &vector, &mut fonts, &mut embedded)?;
            let stream = doc.add_stream(&Buffer::from_bytes(&edited.content)?, None, false)?;
            page.dict_put("Contents", stream)?;
            if let Some(resources) = edited.resources {
                page.dict_put("Resources", resources)?;
            }
        }
        if let Some((placement, retouched)) = scan {
            scanned::write_patches(doc, &mut page, &placement, &retouched)?;
            if let Some(slot) = effective.get_mut(position) {
                *slot = Some(Arc::new(retouched.layer));
            }
        }
        if !edits.added().is_empty() {
            let turns = pages.get(position).map_or(0, page_turns);
            added::append(
                doc,
                position,
                turns,
                edits.added(),
                &mut fonts,
                &mut added_resources,
            )?;
        }
    }
    Ok(effective)
}

/// Ajoute aux pages leur couche de texte invisible : un flux de contenu de
/// plus, après le contenu de la page entouré de `q … Q` (il peut laisser l'état
/// graphique modifié), et la police de la couche dans ses ressources. La police
/// est ajoutée une seule fois au document.
fn apply_layers(doc: &mut PdfDocument, layers: &[Option<Arc<TextLayer>>]) -> Result<()> {
    let mut font: Option<PdfObject> = None;
    for (position, layer) in layers.iter().enumerate() {
        let Some(layer) = layer.as_ref().filter(|layer| !layer.is_empty()) else {
            continue;
        };
        let font = match &font {
            Some(font) => font.try_clone()?,
            None => {
                let added = textlayer::add_layer_font(doc)?;
                font = Some(added.try_clone()?);
                added
            }
        };
        let mut page = doc.find_page(position as i32)?;

        let resources = page.get_dict_inheritable("Resources")?;
        let mut copy = match &resources {
            Some(resources) if resources.is_dict()? => detached_copy(resources)?,
            _ => doc.new_dict()?,
        };
        let mut fonts = match copy.get_dict("Font")? {
            Some(dict) if dict.is_dict()? => detached_copy(&dict)?,
            _ => doc.new_dict()?,
        };
        let mut name = String::from("PSOCR");
        let mut suffix = 1;
        while fonts.get_dict(name.as_str())?.is_some() {
            suffix += 1;
            name = format!("PSOCR{suffix}");
        }
        fonts.dict_put(name.as_str(), font)?;
        copy.dict_put("Font", fonts)?;
        page.dict_put("Resources", copy)?;

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
        let mut layer_stream = b"\nQ\n".to_vec();
        layer_stream.extend_from_slice(&textlayer::layer_content(layer, &name));
        contents.array_push(doc.add_stream(&Buffer::from_bytes(&layer_stream)?, None, false)?)?;
        page.dict_put("Contents", contents)?;
    }
    Ok(())
}

/// Vrai si le document porte une signature numérique, qu'une modification
/// invaliderait.
pub fn is_signed(source: &SourceData) -> Result<bool> {
    let doc = open(source)?;
    let Some(form) = doc.catalog()?.get_dict("AcroForm")? else {
        return Ok(false);
    };
    // Bit 1 de /SigFlags : « SignaturesExist ».
    if form
        .get_dict("SigFlags")?
        .map(|flags| flags.as_int())
        .transpose()?
        .unwrap_or(0)
        & 1
        != 0
    {
        return Ok(true);
    }
    match form.get_dict("Fields")? {
        Some(fields) => has_signed_field(&fields, 0),
        None => Ok(false),
    }
}

fn has_signed_field(fields: &PdfObject, depth: usize) -> Result<bool> {
    if depth > 32 {
        return Ok(false);
    }
    for field in items(fields)? {
        let is_signature = name_of(&field, "FT")?.as_deref() == Some("Sig");
        if is_signature && field.get_dict("V")?.is_some() {
            return Ok(true);
        }
        if let Some(kids) = field.get_dict("Kids")? {
            if has_signed_field(&kids, depth + 1)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn open(source: &SourceData) -> Result<PdfDocument> {
    let mut doc =
        PdfDocument::from_bytes(&source.bytes).map_err(|err| PdfError::InvalidDocument {
            detail: err.to_string(),
        })?;
    if doc.needs_password()? {
        let password = source
            .password
            .as_deref()
            .ok_or(PdfError::PasswordRequired)?;
        if !doc.authenticate(password)? {
            return Err(PdfError::WrongPassword);
        }
    }
    Ok(doc)
}

fn invalid(detail: &str) -> PdfError {
    PdfError::WriteFailed {
        detail: detail.into(),
    }
}

fn page_turns(page: &OutputPage) -> u8 {
    match *page {
        OutputPage::Source { quarter_turns, .. }
        | OutputPage::Blank { quarter_turns, .. }
        | OutputPage::Replaced { quarter_turns, .. } => quarter_turns,
    }
}

/// Vrai si les pages sont celles de la base, dans leur ordre : seuls la
/// rotation et le contenu des pages améliorées changent, et la structure du
/// document reste intacte (numérotation comprise).
fn is_same_order(pages: &[OutputPage], original_count: usize) -> bool {
    pages.len() == original_count
        && pages
            .iter()
            .enumerate()
            .all(|(position, page)| match *page {
                OutputPage::Source {
                    source: 0, index, ..
                }
                | OutputPage::Replaced {
                    source: 0, index, ..
                } => index as usize == position,
                _ => false,
            })
}

/// Vrai si un même objet page figure plusieurs fois dans l'arbre des pages :
/// pivoter l'une de ses occurrences pivoterait aussi les autres.
fn has_shared_pages(doc: &PdfDocument, count: usize) -> Result<bool> {
    let mut seen = HashSet::new();
    for index in 0..count {
        let page = doc.find_page(index as i32)?;
        if page.is_indirect()? && !seen.insert(page.as_indirect()?) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Ajoute des quarts de tour à la rotation de la page `position`.
fn rotate(doc: &PdfDocument, position: usize, quarter_turns: u8) -> Result<()> {
    if quarter_turns % 4 == 0 {
        return Ok(());
    }
    let mut page = doc.load_pdf_page(position as i32)?;
    // Comme MuPDF à l'affichage, une rotation qui n'est pas un multiple de 90°
    // est arrondie au plus proche.
    let rotation = (page.rotation()?.rem_euclid(360) + 45) / 90 * 90;
    page.set_rotation((rotation + 90 * i32::from(quarter_turns)) % 360)?;
    Ok(())
}

fn rebuild(
    doc: &mut PdfDocument,
    sources: &[SourceData],
    pages: &[OutputPage],
    original_count: usize,
) -> Result<()> {
    let mut originals = Vec::with_capacity(original_count);
    for index in 0..original_count {
        let mut page = doc.find_page(index as i32)?;
        if !page.is_indirect()? {
            // Une page doit être un objet indirect ; certains fichiers l'ignorent.
            page = doc.add_object(&page)?;
        }
        flatten_inherited(&page)?;
        originals.push(page);
    }

    // La numérotation logique (i, ii, 1, 2…) ne correspondrait plus à l'ordre des
    // pages ; la retirer d'abord évite aussi à MuPDF de l'ajuster à chaque page.
    doc.catalog()?.dict_delete("PageLabels")?;

    // Arbre des pages vidé ; les objets des pages restent disponibles, et les
    // nœuds intermédiaires, devenus inaccessibles, disparaîtront à l'écriture.
    // Les attributs hérités de la racine, déjà recopiés dans les pages d'origine,
    // ne doivent pas s'appliquer aux pages insérées.
    let mut tree = doc
        .catalog()?
        .get_dict("Pages")?
        .ok_or_else(|| invalid("arbre des pages absent"))?;
    for key in ["MediaBox", "CropBox", "Rotate", "Resources"] {
        tree.dict_delete(key)?;
    }
    tree.dict_put("Kids", doc.new_array()?)?;
    tree.dict_put("Count", PdfObject::new_int(0)?)?;

    let mut others: HashMap<usize, (PdfDocument, PdfGraftMap)> = HashMap::new();
    let mut placed = Placed::default();
    for (position, page) in pages.iter().enumerate() {
        let at = position as i32;
        match *page {
            OutputPage::Source { source, index, .. } => {
                place_page(
                    doc,
                    sources,
                    &originals,
                    &mut others,
                    &mut placed,
                    source,
                    index,
                    position,
                )?;
            }
            OutputPage::Replaced {
                source,
                index,
                content_source,
                content_index,
                ..
            } => {
                if source == 0 {
                    // L'objet d'origine va changer de contenu : une autre
                    // occurrence de la page, intacte, sera copiée de son état
                    // d'avant.
                    if let Some(original) = originals.get(index as usize) {
                        let number = original.as_indirect()?;
                        if let Entry::Vacant(entry) = placed.pristine.entry(number) {
                            entry.insert(detached_copy(original)?);
                        }
                    }
                }
                place_page(
                    doc,
                    sources,
                    &originals,
                    &mut others,
                    &mut placed,
                    source,
                    index,
                    position,
                )?;
                let (other, map) = other_source(doc, sources, &mut others, content_source)?;
                replace_content(doc, map, other, content_index, position)?;
            }
            OutputPage::Blank { width, height, .. } => {
                doc.new_page_at(at, Size { width, height })?;
            }
        }
    }
    // Une fois toutes les pages en place : une page répétée a été copiée avant
    // que la rotation de sa première occurrence ne s'applique.
    for (position, page) in pages.iter().enumerate() {
        rotate(doc, position, page_turns(page))?;
    }

    let mut retained = HashSet::new();
    for position in 0..pages.len() {
        retained.insert(doc.find_page(position as i32)?.as_indirect()?);
    }
    let kept_annotations = prune_links_and_list_annotations(doc, pages.len(), &retained)?;
    // Ce qui disparaît avec les pages supprimées : elles-mêmes et leurs annotations.
    let mut removed = HashSet::new();
    for page in &originals {
        let number = page.as_indirect()?;
        if retained.contains(&number) || !removed.insert(number) {
            continue;
        }
        if let Some(annots) = page.get_dict("Annots")? {
            for annot in items(&annots)? {
                if annot.is_indirect()? && !kept_annotations.contains(&annot.as_indirect()?) {
                    removed.insert(annot.as_indirect()?);
                }
            }
        }
    }

    let mut catalog = doc.catalog()?;
    if let Some(mut outlines) = catalog.get_dict("Outlines")? {
        let (kept, _) = prune_outline(&mut outlines, &retained, 0)?;
        if kept == 0 {
            catalog.dict_delete("Outlines")?;
        }
    }
    if let Some(form) = catalog.get_dict("AcroForm")? {
        if let Some(mut fields) = form.get_dict("Fields")? {
            prune_fields(&mut fields, &mut removed, 0)?;
        }
        if let Some(mut order) = form.get_dict("CO")? {
            remove_items(&mut order, &removed)?;
        }
    }
    if removed.is_empty() {
        return Ok(());
    }
    if let Some(action) = catalog.get_dict("OpenAction")? {
        if opens_removed_page(&action, &removed)? {
            catalog.dict_delete("OpenAction")?;
        }
    }
    // La structure balisée (accessibilité) décrit aussi les pages supprimées et
    // renvoie à leur contenu : elle est retirée.
    catalog.dict_delete("StructTreeRoot")?;
    catalog.dict_delete("MarkInfo")?;
    forget_references(doc, &removed)?;
    Ok(())
}

/// Pages de la base déjà placées dans l'arbre des pages.
#[derive(Default)]
struct Placed {
    /// Numéros des objets pages déjà placés.
    used: HashSet<i32>,
    /// État d'origine des pages dont l'objet a reçu le contenu d'une page
    /// améliorée, par numéro d'objet.
    pristine: HashMap<i32, PdfObject>,
}

/// Insère à la position `position` la page `index` du document
/// `sources[source]` : l'objet d'origine pour la base (copié s'il y figure
/// déjà), une copie avec ses annotations pour un autre document.
#[allow(clippy::too_many_arguments)]
fn place_page(
    doc: &mut PdfDocument,
    sources: &[SourceData],
    originals: &[PdfObject],
    others: &mut HashMap<usize, (PdfDocument, PdfGraftMap)>,
    placed: &mut Placed,
    source: usize,
    index: u32,
    position: usize,
) -> Result<()> {
    let at = position as i32;
    if source == 0 {
        let original = originals
            .get(index as usize)
            .ok_or_else(|| invalid("page source inexistante"))?;
        let number = original.as_indirect()?;
        // Un objet ne figure qu'une fois dans l'arbre, même s'il y était
        // deux fois dans le fichier : les occurrences suivantes sont copiées.
        let object = if placed.used.insert(number) {
            original.try_clone()?
        } else {
            let model = placed.pristine.get(&number).unwrap_or(original);
            duplicate_page(doc, model)?
        };
        doc.insert_page(at, &object)?;
        return Ok(());
    }
    let (other, map) = other_source(doc, sources, others, source)?;
    if index >= other.page_count()?.max(0) as u32 {
        return Err(invalid("page source inexistante"));
    }
    graft_page(doc, map, other, index, at)?;
    copy_annotations(doc, map, other, index, position)?;
    Ok(())
}

/// Document `sources[source]` (autre que la base), ouvert une fois, et la
/// table des objets déjà copiés depuis lui.
fn other_source<'a>(
    doc: &PdfDocument,
    sources: &[SourceData],
    others: &'a mut HashMap<usize, (PdfDocument, PdfGraftMap)>,
    source: usize,
) -> Result<(&'a PdfDocument, &'a mut PdfGraftMap)> {
    if source == 0 {
        return Err(invalid("contenu de remplacement pris dans la base"));
    }
    let data = sources
        .get(source)
        .ok_or_else(|| invalid("document source inconnu"))?;
    let (other, map) = match others.entry(source) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => entry.insert((open(data)?, doc.new_graft_map()?)),
    };
    Ok((other, map))
}

/// Remplace le contenu, les ressources et les cadres de la page `position` par
/// ceux de la page `index` de `other` (page améliorée). L'objet de la page,
/// ses annotations et ce qui le vise restent.
fn replace_content(
    doc: &mut PdfDocument,
    map: &mut PdfGraftMap,
    other: &PdfDocument,
    index: u32,
    position: usize,
) -> Result<()> {
    if index >= other.page_count()?.max(0) as u32 {
        return Err(invalid("page améliorée inexistante"));
    }
    let source = other.find_page(index as i32)?;
    let mut target = doc.find_page(position as i32)?;
    for key in ["Contents", "Resources", "MediaBox", "CropBox", "Rotate"] {
        // Les attributs absents prennent leur valeur par défaut, et non celle
        // que la page d'origine hériterait de l'arbre des pages.
        let value = match (source.get_dict_inheritable(key)?, key) {
            (Some(value), _) => Some(map.graft_object(&value)?),
            (None, "CropBox") => source
                .get_dict_inheritable("MediaBox")?
                .map(|media| map.graft_object(&media))
                .transpose()?,
            (None, "Rotate") => Some(doc.new_int(0)?),
            (None, "Resources") => Some(doc.new_dict()?),
            (None, _) => None,
        };
        match value {
            Some(value) => target.dict_put(key, value)?,
            None => target.dict_delete(key)?,
        }
    }
    Ok(())
}

/// Recopie dans la page les attributs hérités de l'arbre des pages, qu'elle
/// perdrait en changeant de parent.
fn flatten_inherited(page: &PdfObject) -> Result<()> {
    // Sur un objet indirect, `try_clone` renvoie une référence au même objet.
    let mut page = page.try_clone()?;
    for key in ["MediaBox", "CropBox", "Rotate", "Resources"] {
        if page.get_dict(key)?.is_none() {
            if let Some(value) = page.get_dict_inheritable(key)? {
                page.dict_put(key, value.try_clone()?)?;
            }
        }
    }
    Ok(())
}

/// Copie indépendante d'un dictionnaire : ses objets directs sont dupliqués, ses
/// références indirectes conservées.
///
/// Un objet direct ne doit jamais appartenir à deux dictionnaires : MuPDF
/// renumérote les références à l'enregistrement et traiterait deux fois celles
/// qu'il contient, qui viseraient alors un mauvais objet. C'est pourquoi
/// `copy_dict`, qui partage les valeurs, ne suffit pas.
pub(crate) fn detached_copy(object: &PdfObject) -> Result<PdfObject> {
    // `copy_dict` résout l'objet indirect, `try_clone` copie récursivement.
    Ok(object.copy_dict()?.try_clone()?)
}

/// Copie d'une page du même document : contenu et ressources partagés,
/// annotations recopiées à l'exception des champs de formulaire.
fn duplicate_page(doc: &mut PdfDocument, original: &PdfObject) -> Result<PdfObject> {
    let mut copy = detached_copy(original)?;
    for key in ["Parent", "Annots", "StructParents", "B"] {
        copy.dict_delete(key)?;
    }
    let mut copy = doc.add_object(&copy)?;
    if let Some(annots) = original.get_dict("Annots")? {
        let mut copied = doc.new_array()?;
        for annot in items(&annots)? {
            if matches!(
                name_of(&annot, "Subtype")?.as_deref(),
                Some("Widget" | "Popup")
            ) {
                continue;
            }
            let mut dict = detached_copy(&annot)?;
            for key in ["Popup", "StructParent"] {
                dict.dict_delete(key)?;
            }
            dict.dict_put("P", copy.try_clone()?)?;
            copied.array_push(doc.add_object(&dict)?)?;
        }
        if copied.len()? > 0 {
            copy.dict_put("Annots", copied)?;
        }
    }
    Ok(copy)
}

/// Insère à la position `at` la page `index` d'un autre document, comme
/// `pdf_graft_page`. Le contenu et les ressources passent par `map`, qui ne copie
/// qu'une fois un objet partagé par plusieurs pages (police, image, logo).
fn graft_page(
    doc: &mut PdfDocument,
    map: &mut PdfGraftMap,
    other: &PdfDocument,
    index: u32,
    at: i32,
) -> Result<()> {
    let source = other.find_page(index as i32)?;
    let mut page = doc.new_dict()?;
    page.dict_put("Type", PdfObject::new_name("Page")?)?;
    // Aucune de ces clés ne renvoie à une autre page.
    for key in [
        "Contents",
        "Resources",
        "MediaBox",
        "CropBox",
        "BleedBox",
        "TrimBox",
        "ArtBox",
        "Rotate",
        "UserUnit",
    ] {
        if let Some(value) = source.get_dict_inheritable(key)? {
            page.dict_put(key, map.graft_object(&value)?)?;
        }
    }
    let page = doc.add_object(&page)?;
    doc.insert_page(at, &page)?;
    Ok(())
}

/// Recopie les annotations d'une page venue d'un autre document. Les champs de
/// formulaire et les liens internes à ce document, qui n'auraient plus de sens,
/// ne sont pas repris.
fn copy_annotations(
    doc: &mut PdfDocument,
    map: &mut PdfGraftMap,
    other: &PdfDocument,
    index: u32,
    position: usize,
) -> Result<()> {
    let Some(annots) = other.find_page(index as i32)?.get_dict("Annots")? else {
        return Ok(());
    };
    let mut target = doc.find_page(position as i32)?;
    let mut copied = doc.new_array()?;
    for annot in items(&annots)? {
        let keep = match name_of(&annot, "Subtype")?.as_deref() {
            Some("Widget" | "Popup") | None => false,
            Some("Link") => match annot.get_dict("A")? {
                Some(action) => name_of(&action, "S")?.as_deref() == Some("URI"),
                None => false,
            },
            Some(_) => true,
        };
        if !keep {
            continue;
        }
        let mut dict = annot.copy_dict()?;
        for key in ["P", "Popup", "IRT", "Parent", "StructParent"] {
            dict.dict_delete(key)?;
        }
        let mut grafted = doc.add_object(&map.graft_object(&dict)?)?;
        grafted.dict_put("P", target.try_clone()?)?;
        copied.array_push(grafted)?;
    }
    if copied.len()? > 0 {
        target.dict_put("Annots", copied)?;
    }
    Ok(())
}

/// Où mène une destination ou une action : une page conservée, une page
/// supprimée, ou une cible que l'on ne sait pas vérifier (destination nommée,
/// lien externe…), laissée telle quelle.
#[derive(PartialEq)]
enum Target {
    Kept,
    Removed,
    Unknown,
}

fn target_of(item: &PdfObject, retained: &HashSet<i32>) -> Result<Target> {
    let destination = match item.get_dict("Dest")? {
        Some(dest) => Some(dest),
        None => match item.get_dict("A")? {
            Some(action) if name_of(&action, "S")?.as_deref() == Some("GoTo") => {
                action.get_dict("D")?
            }
            _ => None,
        },
    };
    let Some(destination) = destination else {
        return Ok(Target::Unknown);
    };
    if !destination.is_array()? {
        return Ok(Target::Unknown);
    }
    match destination.get_array(0)? {
        Some(page) if page.is_indirect()? => Ok(if retained.contains(&page.as_indirect()?) {
            Target::Kept
        } else {
            Target::Removed
        }),
        _ => Ok(Target::Unknown),
    }
}

/// Retire du plan les entrées qui visaient une page supprimée ; une entrée qui
/// garde des sous-entrées reste, sans destination. Renvoie le nombre d'entrées
/// filles conservées et le nombre d'entrées visibles sous `parent`.
fn prune_outline(
    parent: &mut PdfObject,
    retained: &HashSet<i32>,
    depth: usize,
) -> Result<(usize, i32)> {
    if depth > 64 {
        return Ok((1, 0));
    }
    let mut children = Vec::new();
    let mut cursor = parent.get_dict("First")?;
    while let Some(item) = cursor {
        if children.len() > 100_000 {
            break; // Chaînage circulaire.
        }
        cursor = item.get_dict("Next")?;
        children.push(item);
    }

    let mut kept = Vec::with_capacity(children.len());
    let mut visible = 0;
    for mut item in children {
        let had_children = item.get_dict("First")?.is_some();
        let (grandchildren, descendants) = prune_outline(&mut item, retained, depth + 1)?;
        match target_of(&item, retained)? {
            Target::Removed if grandchildren == 0 => continue,
            Target::Removed => {
                item.dict_delete("Dest")?;
                item.dict_delete("A")?;
            }
            // Un intitulé sans destination dont toutes les entrées ont disparu.
            Target::Unknown if had_children && grandchildren == 0 && !has_action(&item)? => {
                continue
            }
            _ => {}
        }
        let is_open = item
            .get_dict("Count")?
            .map(|count| count.as_int())
            .transpose()?
            .unwrap_or(0)
            > 0;
        visible += 1 + if is_open { descendants } else { 0 };
        kept.push(item);
    }

    for (position, item) in kept.iter().enumerate() {
        let mut item = item.try_clone()?;
        match position.checked_sub(1) {
            Some(previous) => item.dict_put("Prev", kept[previous].try_clone()?)?,
            None => item.dict_delete("Prev")?,
        }
        match kept.get(position + 1) {
            Some(next) => item.dict_put("Next", next.try_clone()?)?,
            None => item.dict_delete("Next")?,
        }
    }
    match (kept.first(), kept.last()) {
        (Some(first), Some(last)) => {
            parent.dict_put("First", first.try_clone()?)?;
            parent.dict_put("Last", last.try_clone()?)?;
        }
        _ => {
            parent.dict_delete("First")?;
            parent.dict_delete("Last")?;
        }
    }
    if let Some(count) = parent.get_dict("Count")? {
        let sign = if count.as_int()? < 0 { -1 } else { 1 };
        if kept.is_empty() {
            parent.dict_delete("Count")?;
        } else {
            parent.dict_put("Count", PdfObject::new_int(sign * visible)?)?;
        }
    }
    Ok((kept.len(), visible))
}

fn has_action(item: &PdfObject) -> Result<bool> {
    Ok(item.get_dict("Dest")?.is_some() || item.get_dict("A")?.is_some())
}

/// Retire les liens qui menaient à une page supprimée et renvoie les annotations
/// des pages conservées.
fn prune_links_and_list_annotations(
    doc: &PdfDocument,
    count: usize,
    retained: &HashSet<i32>,
) -> Result<HashSet<i32>> {
    let mut kept = HashSet::new();
    for position in 0..count {
        let Some(mut annots) = doc.find_page(position as i32)?.get_dict("Annots")? else {
            continue;
        };
        for index in (0..annots.len()?).rev() {
            let Some(annot) = annots.get_array(index as i32)? else {
                continue;
            };
            if name_of(&annot, "Subtype")?.as_deref() == Some("Link")
                && target_of(&annot, retained)? == Target::Removed
            {
                annots.array_delete(index as i32)?;
            } else if annot.is_indirect()? {
                kept.insert(annot.as_indirect()?);
            }
        }
    }
    Ok(kept)
}

/// Retire du formulaire les champs dont toutes les annotations étaient sur des
/// pages supprimées, et les ajoute à `removed` ; un champ sans annotation reste.
fn prune_fields(fields: &mut PdfObject, removed: &mut HashSet<i32>, depth: usize) -> Result<()> {
    if depth > 32 {
        return Ok(());
    }
    for index in (0..fields.len()?).rev() {
        let Some(field) = fields.get_array(index as i32)? else {
            continue;
        };
        let number = if field.is_indirect()? {
            Some(field.as_indirect()?)
        } else {
            None
        };
        let remove = if number.is_some_and(|number| removed.contains(&number)) {
            true
        } else {
            match field.get_dict("Kids")? {
                Some(mut kids) if kids.is_array()? && kids.len()? > 0 => {
                    prune_fields(&mut kids, removed, depth + 1)?;
                    kids.len()? == 0
                }
                _ => false,
            }
        };
        if remove {
            fields.array_delete(index as i32)?;
            removed.extend(number);
        }
    }
    Ok(())
}

/// Retire d'un tableau les références aux objets `removed`.
fn remove_items(array: &mut PdfObject, removed: &HashSet<i32>) -> Result<()> {
    if !array.is_array()? {
        return Ok(());
    }
    for index in (0..array.len()? as i32).rev() {
        if let Some(item) = array.get_array(index)? {
            if item.is_indirect()? && removed.contains(&item.as_indirect()?) {
                array.array_delete(index)?;
            }
        }
    }
    Ok(())
}

/// Vrai si l'action d'ouverture du document mène à une page supprimée.
fn opens_removed_page(action: &PdfObject, removed: &HashSet<i32>) -> Result<bool> {
    let destination = if action.is_array()? {
        Some(action.try_clone()?)
    } else if name_of(action, "S")?.as_deref() == Some("GoTo") {
        action.get_dict("D")?
    } else {
        None
    };
    let page = match destination {
        Some(destination) if destination.is_array()? => destination.get_array(0)?,
        _ => None,
    };
    match page {
        Some(page) if page.is_indirect()? => Ok(removed.contains(&page.as_indirect()?)),
        _ => Ok(false),
    }
}

/// Efface toute référence aux objets `removed` (pages supprimées, leurs
/// annotations et leurs champs) : n'étant plus accessibles, ils disparaissent à
/// l'écriture avec leur contenu, leurs images et leurs polices. Une destination
/// nommée ou un fil d'article qui y menait ne mène plus nulle part.
fn forget_references(doc: &PdfDocument, removed: &HashSet<i32>) -> Result<()> {
    for number in 1..doc.count_objects()? as i32 {
        if removed.contains(&number) {
            continue;
        }
        // Un objet libre ou illisible est ignoré ; MuPDF l'écarte à l'écriture.
        let Ok(Some(mut object)) = doc.new_indirect(number, 0).and_then(|r| r.resolve()) else {
            continue;
        };
        forget_in(&mut object, removed, 0)?;
    }
    Ok(())
}

fn forget_in(object: &mut PdfObject, removed: &HashSet<i32>, depth: usize) -> Result<()> {
    if depth > 32 {
        return Ok(());
    }
    if object.is_array()? {
        for index in 0..object.len()? as i32 {
            let Some(mut item) = object.get_array(index)? else {
                continue;
            };
            if !item.is_indirect()? {
                forget_in(&mut item, removed, depth + 1)?;
            } else if removed.contains(&item.as_indirect()?) {
                // Une valeur nulle garde la place des éléments suivants (destinations).
                object.array_put(index, PdfObject::new_null())?;
            }
        }
    } else if object.is_dict()? {
        for index in (0..object.dict_len()? as i32).rev() {
            let Some(mut value) = object.get_dict_val(index)? else {
                continue;
            };
            if !value.is_indirect()? {
                forget_in(&mut value, removed, depth + 1)?;
            } else if removed.contains(&value.as_indirect()?) {
                if let Some(key) = object.get_dict_key(index)? {
                    object.dict_delete(key)?;
                }
            }
        }
    }
    Ok(())
}

/// Dictionnaires d'un tableau PDF (annotations, champs) ; un élément nul ou une
/// référence vers un objet disparu sont ignorés, comme le fait MuPDF.
fn items(array: &PdfObject) -> Result<Vec<PdfObject>> {
    let mut items = Vec::new();
    if !array.is_array()? {
        return Ok(items);
    }
    for index in 0..array.len()? as i32 {
        if let Some(item) = array.get_array(index)? {
            if item.is_dict()? {
                items.push(item);
            }
        }
    }
    Ok(items)
}

fn name_of(object: &PdfObject, key: &str) -> Result<Option<String>> {
    match object.get_dict(key)? {
        Some(value) if value.is_name()? => Ok(Some(
            String::from_utf8_lossy(&value.as_name()?).into_owned(),
        )),
        _ => Ok(None),
    }
}

/// Relit le PDF produit.
fn verify(output: &[u8], password: Option<&str>, expected_pages: usize) -> Result<()> {
    let written = open(&SourceData::new(
        output.to_vec(),
        password.map(str::to_owned),
    ))
    .map_err(|err| invalid(&format!("relecture impossible : {err}")))?;
    let count = written.page_count()?.max(0) as usize;
    if count != expected_pages {
        return Err(invalid(&format!(
            "{count} pages écrites au lieu de {expected_pages}"
        )));
    }
    Ok(())
}
