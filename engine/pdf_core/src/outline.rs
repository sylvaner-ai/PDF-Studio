use std::collections::HashSet;

use mupdf::pdf::{PdfDocument, PdfObject};

/// Entrée du plan du document (signet).
///
/// Le plan est aplati en parcours en profondeur : l'arborescence se reconstruit
/// à partir de [`OutlineEntry::level`]. Cette forme simple traverse le pont vers
/// Swift sans type récursif.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    /// Titre affiché.
    pub title: String,
    /// Profondeur dans l'arborescence, 0 pour le premier niveau.
    pub level: u32,
    /// Page visée, à partir de 0 ; absente pour un lien externe ou une cible introuvable.
    pub page: Option<u32>,
    /// Position verticale visée, en points depuis le haut de la page, si le signet la précise.
    pub top: Option<f32>,
}

/// Nombre d'entrées du plan au-delà duquel il est ignoré.
const MAX_ITEMS: usize = 100_000;

/// Profondeur du plan au-delà de laquelle il est ignoré : MuPDF le lit
/// récursivement, et un plan malformé très profond épuiserait la pile.
const MAX_DEPTH: usize = 64;

/// Prépare le plan du document à sa lecture par MuPDF ; renvoie faux s'il
/// ne peut pas être lu sans risque (trop profond ou trop grand).
///
/// Une entrée sans titre (`/Title` absent, cas d'un fichier endommagé ou mal
/// produit) en reçoit un vide : le crate `mupdf` lit sinon un pointeur nul et
/// l'application s'arrête. La modification ne touche que la copie en mémoire,
/// jamais le fichier ni ce qui est enregistré (qui repart des octets lus).
/// Une entrée illisible (objet endommagé) ne peut pas recevoir de titre : le
/// plan est alors ignoré. Une boucle dans le chaînage des entrées est coupée
/// au premier retour.
pub(crate) fn prepare(doc: &PdfDocument) -> Result<bool, mupdf::Error> {
    let Some(root) = doc.catalog()?.get_dict("Outlines")? else {
        return Ok(true);
    };
    let mut seen = HashSet::new();
    let mut count = 0;
    let mut levels = vec![(root.get_dict("First")?, 0)];
    while let Some((first, depth)) = levels.pop() {
        let mut next = first;
        while let Some(mut entry) = next {
            if !entry.is_dict()? {
                return Ok(false);
            }
            if entry.is_indirect()? && !seen.insert(entry.as_indirect()?) {
                break;
            }
            count += 1;
            if count > MAX_ITEMS || depth > MAX_DEPTH {
                return Ok(false);
            }
            if entry.get_dict("Title")?.is_none() {
                entry.dict_put("Title", PdfObject::new_string("")?)?;
            }
            if let Some(child) = entry.get_dict("First")? {
                levels.push((Some(child), depth + 1));
            }
            next = entry.get_dict("Next")?;
        }
    }
    Ok(true)
}

/// Aplatit l'arborescence de MuPDF.
pub(crate) fn flatten(outlines: &[mupdf::Outline], page_count: u32) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    push_level(outlines, 0, page_count, &mut entries);
    entries
}

fn push_level(
    outlines: &[mupdf::Outline],
    level: u32,
    page_count: u32,
    entries: &mut Vec<OutlineEntry>,
) {
    for outline in outlines {
        let dest = outline.dest.filter(|d| d.loc.page_number < page_count);
        entries.push(OutlineEntry {
            title: clean_title(&outline.title),
            level,
            page: dest.map(|d| d.loc.page_number),
            top: dest.and_then(|d| top_of(d.kind)),
        });
        push_level(&outline.down, level + 1, page_count, entries);
    }
}

/// MuPDF exprime déjà les destinations dans l'espace de la page affichée
/// (origine en haut à gauche, rotation comprise).
fn top_of(kind: mupdf::DestinationKind) -> Option<f32> {
    use mupdf::DestinationKind as Kind;
    let top = match kind {
        Kind::XYZ { top, .. } | Kind::FitH { top } | Kind::FitBH { top } => top,
        Kind::FitR { top, bottom, .. } => Some(top.min(bottom)),
        Kind::Fit | Kind::FitV { .. } | Kind::FitB | Kind::FitBV { .. } => None,
    };
    top.filter(|t| t.is_finite())
}

/// Les titres contiennent parfois des retours à la ligne ou des espaces superflues.
fn clean_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}
