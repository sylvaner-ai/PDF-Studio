//! Choix des polices de remplacement.
//!
//! Les 14 polices standard du PDF (Helvetica, Times, Courier et leurs variantes,
//! Symbol, ZapfDingbats) peuvent être utilisées sans être intégrées au fichier.
//! MuPDF cherche d'abord une police du système portant ce nom, puis se rabat sur
//! ses clones URW intégrés. Sur macOS, cette recherche aboutit pour certaines
//! variantes seulement, ce qui mélange les deux familles dans une même page et
//! dessine un tiret cadratin trop court (cf. `docs/DECISIONS.md`, D8). On impose
//! donc les clones URW pour ces 14 noms : métriques identiques aux originaux,
//! rendu stable d'une machine à l'autre, et polices librement intégrables, ce
//! dont l'édition de texte aura besoin. Les autres polices non intégrées
//! continuent d'être cherchées sur le système.

use std::sync::Once;

/// Noms PostScript des 14 polices standard.
const STANDARD_FONTS: [&str; 14] = [
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
    "Symbol",
    "ZapfDingbats",
];

struct StandardFontLoader;

impl mupdf::FontLoader for StandardFontLoader {
    fn load_font(&self, name: &str, _hints: mupdf::FontHints) -> Option<mupdf::Font> {
        let name = standard_name(name)?;
        mupdf::Font::new(name).ok()
    }
}

/// Reconnaît une police standard, préfixe de sous-ensemble (« ABCDEF+ ») ignoré.
fn standard_name(name: &str) -> Option<&'static str> {
    let name = match name.split_once('+') {
        Some((prefix, rest))
            if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            rest
        }
        _ => name,
    };
    STANDARD_FONTS
        .iter()
        .copied()
        .find(|&standard| standard == name)
}

/// Installe le chargeur de polices, une fois pour tout le processus.
///
/// À appeler avant d'ouvrir le premier document : MuPDF garde en cache les
/// polices déjà résolues.
pub(crate) fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| mupdf::set_font_loader(StandardFontLoader));
}

#[cfg(test)]
mod tests {
    use super::standard_name;

    #[test]
    fn reconnait_les_polices_standard() {
        assert_eq!(standard_name("Helvetica-Bold"), Some("Helvetica-Bold"));
        assert_eq!(standard_name("ABCDEF+Times-Roman"), Some("Times-Roman"));
        assert_eq!(standard_name("Arial"), None);
        assert_eq!(standard_name("Helvetica-Light"), None);
        assert_eq!(standard_name("abc+Courier"), None);
    }
}
