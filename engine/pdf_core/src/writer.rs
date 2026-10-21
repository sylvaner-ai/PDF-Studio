//! Écriture d'un PDF réorganisé : ordre des pages, rotation, pages copiées
//! depuis d'autres documents et pages vierges.
//!
//! Le document ouvert sert de base : son catalogue (métadonnées, formulaire,
//! préférences d'affichage…) est conservé tel quel et seules ses pages sont
//! réagencées. Les signets, liens et champs de formulaire qui visaient une page
//! supprimée sont retirés ; ceux des pages conservées continuent de fonctionner,
//! même après un déplacement, puisque la page reste le même objet PDF.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mupdf::pdf::{
    Encryption, InsertPdfOptions, InsertPosition, PageSelection, PdfDocument, PdfObject,
    PdfWriteOptions,
};
use mupdf::Size;

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
}

/// Écrit le PDF formé de `pages`, à partir de `sources` (la première est la base).
///
/// Le résultat est relu avant d'être renvoyé : un fichier qui ne s'ouvre pas, ou
/// pas avec le bon nombre de pages, n'est jamais livré.
pub fn write_pdf(sources: &[SourceData], pages: &[OutputPage]) -> Result<Vec<u8>> {
    let base = sources
        .first()
        .ok_or_else(|| invalid("aucun document source"))?;
    if pages.is_empty() {
        return Err(invalid("aucune page à écrire"));
    }
    let mut doc = open(base)?;
    let original_count = doc.page_count()?.max(0) as usize;

    if is_same_order(pages, original_count) {
        for (position, page) in pages.iter().enumerate() {
            rotate(&doc, position, page_turns(page))?;
        }
    } else {
        rebuild(&mut doc, sources, pages, original_count)?;
    }

    let mut options = PdfWriteOptions::default();
    options
        .set_garbage_level(3)
        .set_compress(true)
        .set_encryption(Encryption::Keep);
    let mut output = Vec::new();
    doc.write_to_with_options(&mut output, options)?;
    verify(&output, base.password.as_deref(), pages.len())?;
    Ok(output)
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
        OutputPage::Source { quarter_turns, .. } | OutputPage::Blank { quarter_turns, .. } => {
            quarter_turns
        }
    }
}

/// Vrai si les pages sont celles de la base, dans leur ordre : seule la rotation
/// change, et la structure du document reste intacte (numérotation comprise).
fn is_same_order(pages: &[OutputPage], original_count: usize) -> bool {
    pages.len() == original_count
        && pages.iter().enumerate().all(|(position, page)| {
            matches!(*page, OutputPage::Source { source: 0, index, .. } if index as usize == position)
        })
}

/// Ajoute des quarts de tour à la rotation de la page `position`.
fn rotate(doc: &PdfDocument, position: usize, quarter_turns: u8) -> Result<()> {
    if quarter_turns % 4 == 0 {
        return Ok(());
    }
    let mut page = doc.load_pdf_page(position as i32)?;
    let rotation = page.rotation()?;
    page.set_rotation((rotation + 90 * i32::from(quarter_turns)).rem_euclid(360))?;
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

    // Arbre des pages vidé ; les objets des pages restent disponibles.
    if original_count > 0 {
        doc.delete_pages(PageSelection::All)?;
    }
    let mut tree = doc
        .catalog()?
        .get_dict("Pages")?
        .ok_or_else(|| invalid("arbre des pages absent"))?;
    tree.dict_put("Kids", doc.new_array()?)?;
    tree.dict_put("Count", PdfObject::new_int(0)?)?;

    let mut others: HashMap<usize, PdfDocument> = HashMap::new();
    let mut used = vec![false; original_count];
    for (position, page) in pages.iter().enumerate() {
        let at = position as i32;
        match *page {
            OutputPage::Source {
                source: 0, index, ..
            } => {
                let index = index as usize;
                let original = originals
                    .get(index)
                    .ok_or_else(|| invalid("page source inexistante"))?;
                let object = if used[index] {
                    duplicate_page(doc, original)?
                } else {
                    used[index] = true;
                    original.try_clone()?
                };
                doc.insert_page(at, &object)?;
            }
            OutputPage::Source { source, index, .. } => {
                let data = sources
                    .get(source)
                    .ok_or_else(|| invalid("document source inconnu"))?;
                let other = match others.entry(source) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(open(data)?),
                };
                if index >= other.page_count()?.max(0) as u32 {
                    return Err(invalid("page source inexistante"));
                }
                doc.insert_pdf(
                    other,
                    InsertPdfOptions {
                        source_pages: PageSelection::Pages(vec![index as usize]),
                        target: InsertPosition::Append,
                        ..InsertPdfOptions::default()
                    },
                )?;
                copy_annotations(doc, other, index, position)?;
            }
            OutputPage::Blank { width, height, .. } => {
                doc.new_page_at(at, Size { width, height })?;
            }
        }
        rotate(doc, position, page_turns(page))?;
    }

    let mut catalog = doc.catalog()?;

    let mut retained = HashSet::new();
    for position in 0..pages.len() {
        retained.insert(doc.find_page(position as i32)?.as_indirect()?);
    }
    if let Some(mut outlines) = catalog.get_dict("Outlines")? {
        let (kept, _) = prune_outline(&mut outlines, &retained, 0)?;
        if kept == 0 {
            catalog.dict_delete("Outlines")?;
        }
    }
    let widgets = prune_links_and_list_widgets(doc, pages.len(), &retained)?;
    if let Some(form) = catalog.get_dict("AcroForm")? {
        if let Some(mut fields) = form.get_dict("Fields")? {
            prune_fields(&mut fields, &widgets, 0)?;
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
fn detached_copy(object: &PdfObject) -> Result<PdfObject> {
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

/// Recopie les annotations d'une page venue d'un autre document. Les champs de
/// formulaire et les liens internes à ce document, qui n'auraient plus de sens,
/// ne sont pas repris.
fn copy_annotations(
    doc: &mut PdfDocument,
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
        let mut grafted = doc.add_object(&doc.graft_object(&dict)?)?;
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

/// Retire les liens qui menaient à une page supprimée et renvoie les champs de
/// formulaire (annotations « Widget ») des pages conservées.
fn prune_links_and_list_widgets(
    doc: &PdfDocument,
    count: usize,
    retained: &HashSet<i32>,
) -> Result<HashSet<i32>> {
    let mut widgets = HashSet::new();
    for position in 0..count {
        let Some(mut annots) = doc.find_page(position as i32)?.get_dict("Annots")? else {
            continue;
        };
        for index in (0..annots.len()?).rev() {
            let Some(annot) = annots.get_array(index as i32)? else {
                continue;
            };
            match name_of(&annot, "Subtype")?.as_deref() {
                Some("Link") if target_of(&annot, retained)? == Target::Removed => {
                    annots.array_delete(index as i32)?;
                }
                Some("Widget") if annot.is_indirect()? => {
                    widgets.insert(annot.as_indirect()?);
                }
                _ => {}
            }
        }
    }
    Ok(widgets)
}

/// Retire du formulaire les champs dont aucune annotation ne reste sur une page
/// conservée. Renvoie le nombre de champs gardés.
fn prune_fields(fields: &mut PdfObject, widgets: &HashSet<i32>, depth: usize) -> Result<usize> {
    if depth > 32 {
        return fields.len().map_err(Into::into);
    }
    for index in (0..fields.len()?).rev() {
        let Some(field) = fields.get_array(index as i32)? else {
            continue;
        };
        let keep = match field.get_dict("Kids")? {
            Some(mut kids) if kids.is_array()? => prune_fields(&mut kids, widgets, depth + 1)? > 0,
            _ => !field.is_indirect()? || widgets.contains(&field.as_indirect()?),
        };
        if !keep {
            fields.array_delete(index as i32)?;
        }
    }
    fields.len().map_err(Into::into)
}

/// Éléments d'un tableau PDF ; un objet qui n'est pas un tableau n'en a aucun.
fn items(array: &PdfObject) -> Result<Vec<PdfObject>> {
    if !array.is_array()? {
        return Ok(Vec::new());
    }
    array
        .array_iter()?
        .map(|item| item.map_err(Into::into))
        .collect()
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
