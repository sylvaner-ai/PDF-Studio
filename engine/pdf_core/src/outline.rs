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
