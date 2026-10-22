//! Métriques d'une police de page, telles que les décrit le PDF : découpage des
//! chaînes en codes, chasse de chaque code, texte Unicode et code de chaque
//! caractère.
//!
//! Ces métriques suffisent à placer chaque glyphe exactement comme un lecteur
//! PDF : la chasse déclarée dans le PDF (`/Widths`, `/W`) fait foi pour la
//! position, quelle que soit la police intégrée.

use std::collections::HashMap;

use super::cmap::CMap;

/// Famille technique d'une police.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    /// Police simple (Type 1, TrueType) : un octet par code.
    Simple,
    /// Police Type 3 : glyphes décrits par des procédures de contenu.
    Type3,
    /// Police composite (Type 0) : codes de un à quatre octets, glyphes CID.
    Composite,
}

/// Métriques d'une police.
#[derive(Clone, Debug)]
pub struct FontMetrics {
    /// Nom de la police, sans le préfixe de sous-ensemble (« ABCDEF+ »).
    pub base_name: String,
    /// La police intégrée est un sous-ensemble : seuls certains glyphes y figurent.
    pub subset: bool,
    /// Famille technique.
    pub kind: FontKind,
    /// Écriture verticale (CMap `-V`).
    pub vertical: bool,
    /// Faux si les codes ou les chasses ne peuvent pas être établis (CMap
    /// prédéfinie inconnue) : le texte n'est alors pas modifiable.
    pub supported: bool,
    /// Indicateurs du descripteur de police (`/Flags`).
    pub flags: u32,
    /// Graisse déclarée (`/FontWeight`, 100 à 900), si connue.
    pub weight: Option<f64>,
    /// Angle d'italique déclaré, en degrés.
    pub italic_angle: f64,
    /// Hauteur des ascendantes, en millièmes de corps.
    pub ascent: f64,
    /// Profondeur des descendantes (négative), en millièmes de corps.
    pub descent: f64,
    /// Chasse des codes non listés, en millièmes de corps.
    pub default_width: f64,
    /// Chasse par code (police simple) ou par CID (police composite), en
    /// millièmes de corps (déjà convertie par la matrice d'une police Type 3).
    pub widths: HashMap<u32, f64>,
    /// Texte de chaque code.
    pub unicode: HashMap<u32, String>,
    /// Encodage d'une police composite (`None` : Identity, deux octets par code).
    pub cmap: Option<CMap>,
    /// Code de chaque texte, le plus petit quand plusieurs codes donnent le même.
    reverse: HashMap<String, u32>,
}

impl FontMetrics {
    /// Métriques d'une police simple ; `widths` et `unicode` par code.
    pub fn simple(
        base_name: &str,
        widths: HashMap<u32, f64>,
        unicode: HashMap<u32, String>,
    ) -> Self {
        Self::new(base_name, FontKind::Simple, widths, unicode, None)
    }

    /// Métriques d'une police de famille `kind`.
    pub fn new(
        base_name: &str,
        kind: FontKind,
        widths: HashMap<u32, f64>,
        unicode: HashMap<u32, String>,
        cmap: Option<CMap>,
    ) -> Self {
        let (subset, base_name) = split_subset_prefix(base_name);
        let mut metrics = Self {
            base_name: base_name.to_owned(),
            subset,
            kind,
            vertical: false,
            supported: true,
            flags: 0,
            weight: None,
            italic_angle: 0.0,
            ascent: 800.0,
            descent: -200.0,
            default_width: if kind == FontKind::Composite {
                1000.0
            } else {
                0.0
            },
            widths,
            unicode,
            cmap,
            reverse: HashMap::new(),
        };
        metrics.index_unicode();
        metrics
    }

    /// Recalcule l'index inverse après modification de `unicode`.
    pub fn index_unicode(&mut self) {
        let mut reverse: HashMap<String, u32> = HashMap::new();
        for (&code, text) in &self.unicode {
            reverse
                .entry(text.clone())
                .and_modify(|known| *known = (*known).min(code))
                .or_insert(code);
        }
        self.reverse = reverse;
    }

    /// Découpe une chaîne en codes : `(code, nombre d'octets)`.
    pub fn codes(&self, bytes: &[u8]) -> Vec<(u32, usize)> {
        match self.kind {
            FontKind::Simple | FontKind::Type3 => {
                bytes.iter().map(|&byte| (u32::from(byte), 1)).collect()
            }
            FontKind::Composite => match &self.cmap {
                Some(cmap) => cmap.split(bytes, true),
                None => CMap::default().split(bytes, true),
            },
        }
    }

    /// Chasse d'un code, en millièmes de corps.
    pub fn width(&self, code: u32) -> f64 {
        let key = match (self.kind, &self.cmap) {
            (FontKind::Composite, Some(cmap)) => match cmap.cids.get(&code) {
                Some(&cid) => cid,
                None => return self.default_width,
            },
            _ => code,
        };
        self.widths.get(&key).copied().unwrap_or(self.default_width)
    }

    /// Texte d'un code, s'il est connu.
    pub fn text(&self, code: u32) -> Option<&str> {
        self.unicode.get(&code).map(String::as_str)
    }

    /// Code qui donne le caractère `c`, et son nombre d'octets.
    pub fn encode(&self, c: char) -> Option<(u32, usize)> {
        let mut buffer = [0u8; 4];
        let code = *self.reverse.get(c.encode_utf8(&mut buffer) as &str)?;
        Some((code, self.code_len(code)))
    }

    /// Nombre d'octets d'un code.
    pub fn code_len(&self, code: u32) -> usize {
        match (self.kind, &self.cmap) {
            (FontKind::Simple | FontKind::Type3, _) => 1,
            (FontKind::Composite, Some(cmap)) if !cmap.ranges.is_empty() => {
                let bytes = code.to_be_bytes();
                (1..=4)
                    .find(|&len| {
                        let code = &bytes[4 - len..];
                        cmap.split(code, true) == [(super::cmap::code_value(code), len)]
                    })
                    .unwrap_or(2)
            }
            (FontKind::Composite, _) => 2,
        }
    }

    /// Indicateur « police à empattements » du descripteur.
    pub fn is_serif(&self) -> bool {
        self.flags & 0x2 != 0
    }

    /// Indicateur « chasse fixe » du descripteur.
    pub fn is_fixed_pitch(&self) -> bool {
        self.flags & 0x1 != 0
    }

    /// Italique, d'après le descripteur ou le nom.
    pub fn is_italic(&self) -> bool {
        let name = self.base_name.to_ascii_lowercase();
        self.flags & 0x40 != 0
            || self.italic_angle.abs() > 1.0
            || name.contains("italic")
            || name.contains("oblique")
    }

    /// Gras, d'après le descripteur ou le nom.
    pub fn is_bold(&self) -> bool {
        let name = self.base_name.to_ascii_lowercase();
        self.flags & 0x40000 != 0
            || self.weight.is_some_and(|weight| weight >= 600.0)
            || ["bold", "black", "heavy", "semibold", "demi"]
                .iter()
                .any(|word| name.contains(word))
    }
}

/// Octets d'un code de `len` octets.
pub fn code_bytes(code: u32, len: usize) -> Vec<u8> {
    let bytes = code.to_be_bytes();
    bytes[4 - len.clamp(1, 4)..].to_vec()
}

/// Sépare le préfixe de sous-ensemble (six majuscules et « + ») du nom.
fn split_subset_prefix(name: &str) -> (bool, &str) {
    match name.split_once('+') {
        Some((prefix, rest))
            if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            (true, rest)
        }
        _ => (false, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn police_simple() {
        let widths = HashMap::from([(65, 667.0), (32, 278.0)]);
        let unicode = HashMap::from([(65, "A".to_owned()), (32, " ".to_owned())]);
        let font = FontMetrics::simple("ABCDEF+Arial-BoldMT", widths, unicode);
        assert!(font.subset);
        assert_eq!(font.base_name, "Arial-BoldMT");
        assert!(font.is_bold());
        assert_eq!(font.codes(b"AB"), [(65, 1), (66, 1)]);
        assert_eq!(font.width(65), 667.0);
        assert_eq!(font.width(66), 0.0);
        assert_eq!(font.encode('A'), Some((65, 1)));
        assert_eq!(font.encode('Z'), None);
    }

    #[test]
    fn police_composite_identity() {
        let mut cmap = CMap::default();
        cmap.cids.insert(0x0102, 7);
        let widths = HashMap::from([(7, 500.0)]);
        let unicode = HashMap::from([(0x0102, "é".to_owned())]);
        let font = FontMetrics::new("Calibri", FontKind::Composite, widths, unicode, Some(cmap));
        assert_eq!(font.codes(&[1, 2, 0, 9]), [(0x102, 2), (9, 2)]);
        assert_eq!(font.width(0x102), 500.0);
        assert_eq!(font.width(9), 1000.0);
        assert_eq!(font.encode('é'), Some((0x102, 2)));
        assert_eq!(code_bytes(0x102, 2), [1, 2]);
    }
}
