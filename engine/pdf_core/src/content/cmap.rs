//! Lecture des CMap : `ToUnicode` (code → texte) et CMap d'encodage des
//! polices composites (code → identifiant de glyphe CID, et découpage des
//! chaînes en codes).

use std::collections::HashMap;

use super::lexer::{self, Operand};

/// Plage d'espace de codes : codes de `len` octets entre `low` et `high`,
/// octet par octet.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeRange {
    /// Nombre d'octets des codes de la plage.
    pub len: usize,
    low: Vec<u8>,
    high: Vec<u8>,
}

impl CodeRange {
    /// Plage de codes de `len` octets, de `low` à `high`.
    pub fn new(low: &[u8], high: &[u8]) -> Self {
        Self {
            len: low.len().min(high.len()),
            low: low.to_vec(),
            high: high.to_vec(),
        }
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.len
            && bytes
                .iter()
                .zip(self.low.iter().zip(&self.high))
                .all(|(byte, (low, high))| low <= byte && byte <= high)
    }
}

/// Contenu utile d'une CMap.
#[derive(Clone, Debug, Default)]
pub struct CMap {
    /// Plages d'espace de codes.
    pub ranges: Vec<CodeRange>,
    /// Texte de chaque code (`ToUnicode`).
    pub unicode: HashMap<u32, String>,
    /// Identifiant CID de chaque code (CMap d'encodage).
    pub cids: HashMap<u32, u32>,
    /// CMap de base désignée par `usecmap`.
    pub use_cmap: Option<Vec<u8>>,
}

/// Plus grand nombre de codes lus dans une plage, contre les plages absurdes.
const MAX_RANGE: u32 = 0x10000;

impl CMap {
    /// Lit une CMap. Les erreurs sont tolérées : ce qui est lisible est gardé.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut cmap = CMap::default();
        let mut previous: Option<Vec<Operand>> = None;
        for op in lexer::parse(bytes) {
            match op.operator.as_slice() {
                b"endcodespacerange" => {
                    for pair in op.operands.chunks_exact(2) {
                        if let (Operand::String(low), Operand::String(high)) = (&pair[0], &pair[1])
                        {
                            if !low.is_empty() && low.len() <= 4 {
                                cmap.ranges.push(CodeRange::new(low, high));
                            }
                        }
                    }
                }
                b"endbfchar" => {
                    for pair in op.operands.chunks_exact(2) {
                        if let Operand::String(code) = &pair[0] {
                            if let Some(text) = destination_text(&pair[1]) {
                                cmap.unicode.insert(code_value(code), text);
                            }
                        }
                    }
                }
                b"endbfrange" => {
                    for triple in op.operands.chunks_exact(3) {
                        cmap.bf_range(triple);
                    }
                }
                b"endcidchar" => {
                    for pair in op.operands.chunks_exact(2) {
                        if let (Operand::String(code), Some(cid)) = (&pair[0], pair[1].number()) {
                            cmap.cids.insert(code_value(code), cid.max(0.0) as u32);
                        }
                    }
                }
                b"endcidrange" => {
                    for triple in op.operands.chunks_exact(3) {
                        if let (Operand::String(low), Operand::String(high), Some(cid)) =
                            (&triple[0], &triple[1], triple[2].number())
                        {
                            let (low, high) = (code_value(low), code_value(high));
                            if high >= low && high - low < MAX_RANGE {
                                for (offset, code) in (low..=high).enumerate() {
                                    cmap.cids.insert(code, cid.max(0.0) as u32 + offset as u32);
                                }
                            }
                        }
                    }
                }
                b"usecmap" => {
                    let name = op
                        .operands
                        .last()
                        .or_else(|| previous.as_ref().and_then(|p| p.last()))
                        .and_then(Operand::name);
                    cmap.use_cmap = name.map(<[u8]>::to_vec);
                }
                _ => {}
            }
            previous = Some(op.operands);
        }
        cmap
    }

    fn bf_range(&mut self, triple: &[Operand]) {
        let (Operand::String(low), Operand::String(high)) = (&triple[0], &triple[1]) else {
            return;
        };
        let (low, high) = (code_value(low), code_value(high));
        if high < low || high - low >= MAX_RANGE {
            return;
        }
        match &triple[2] {
            Operand::String(start) => {
                let units = utf16_units(start);
                let Some((&last, prefix)) = units.split_last() else {
                    return;
                };
                for (offset, code) in (low..=high).enumerate() {
                    let mut units = prefix.to_vec();
                    // Le dernier mot de seize bits s'incrémente (sans retenue, comme
                    // l'admettent les lecteurs).
                    units.push(last.wrapping_add(offset as u16));
                    let text: String = char::decode_utf16(units).filter_map(Result::ok).collect();
                    if !text.is_empty() {
                        self.unicode.insert(code, text);
                    }
                }
            }
            Operand::Array(items) => {
                for (code, item) in (low..=high).zip(items) {
                    if let Some(text) = destination_text(item) {
                        self.unicode.insert(code, text);
                    }
                }
            }
            _ => {}
        }
    }

    /// Ajoute ce que définit une CMap de base, sans écraser ce que celle-ci
    /// redéfinit.
    pub fn inherit(&mut self, base: &CMap) {
        if self.ranges.is_empty() {
            self.ranges = base.ranges.clone();
        }
        for (code, text) in &base.unicode {
            self.unicode.entry(*code).or_insert_with(|| text.clone());
        }
        for (code, cid) in &base.cids {
            self.cids.entry(*code).or_insert(*cid);
        }
    }

    /// Découpe une chaîne en codes, selon les plages d'espace de codes ; à
    /// défaut de plages, un code par paire d'octets si `two_bytes`, sinon un par
    /// octet.
    pub fn split(&self, bytes: &[u8], two_bytes: bool) -> Vec<(u32, usize)> {
        let mut codes = Vec::with_capacity(bytes.len());
        let mut pos = 0;
        let shortest = self.ranges.iter().map(|range| range.len).min();
        while pos < bytes.len() {
            let len = if self.ranges.is_empty() {
                if two_bytes {
                    2
                } else {
                    1
                }
            } else {
                (1..=4)
                    .find(|&len| {
                        bytes
                            .get(pos..pos + len)
                            .is_some_and(|code| self.ranges.iter().any(|r| r.matches(code)))
                    })
                    .or(shortest)
                    .unwrap_or(1)
            };
            let end = (pos + len).min(bytes.len());
            codes.push((code_value(&bytes[pos..end]), end - pos));
            pos = end;
        }
        codes
    }
}

/// Valeur d'un code de un à quatre octets (gros-boutiste).
pub fn code_value(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |value, &byte| value << 8 | u32::from(byte))
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|pair| match pair {
            [high, low] => u16::from(*high) << 8 | u16::from(*low),
            [single] => u16::from(*single),
            _ => 0,
        })
        .collect()
}

/// Texte d'une destination `ToUnicode` : chaîne UTF-16 gros-boutiste ou nom de
/// glyphe.
fn destination_text(operand: &Operand) -> Option<String> {
    match operand {
        Operand::String(bytes) => {
            let text: String = char::decode_utf16(utf16_units(bytes))
                .filter_map(Result::ok)
                .collect();
            (!text.is_empty()).then_some(text)
        }
        Operand::Name(name) => super::encodings::glyph_name_text(std::str::from_utf8(name).ok()?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TO_UNICODE: &[u8] = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap
        1 begincodespacerange <0000> <FFFF> endcodespacerange
        2 beginbfchar <0003> <0020> <0011> <00660069> endbfchar
        2 beginbfrange <0024> <0026> <0041> <0030> <0031> [<00E9> <20AC>] endbfrange
        endcmap CMapName currentdict /CMap defineresource pop end end";

    #[test]
    fn lit_une_cmap_tounicode() {
        let cmap = CMap::parse(TO_UNICODE);
        assert_eq!(cmap.unicode[&3], " ");
        assert_eq!(cmap.unicode[&0x11], "fi");
        assert_eq!(cmap.unicode[&0x25], "B");
        assert_eq!(cmap.unicode[&0x26], "C");
        assert_eq!(cmap.unicode[&0x30], "é");
        assert_eq!(cmap.unicode[&0x31], "€");
        assert_eq!(cmap.split(&[0, 3, 0, 0x11], false), [(3, 2), (0x11, 2)]);
    }

    #[test]
    fn plages_de_longueurs_variables() {
        let cmap = CMap::parse(
            b"2 begincodespacerange <00> <80> <8140> <9FFC> endcodespacerange
              1 begincidrange <8140> <817E> 633 endcidrange
              1 begincidchar <41> 34 endcidchar",
        );
        assert_eq!(
            cmap.split(&[0x41, 0x81, 0x41, 0x42], false),
            [(0x41, 1), (0x8141, 2), (0x42, 1)]
        );
        assert_eq!(cmap.cids[&0x8141], 634);
        assert_eq!(cmap.cids[&0x41], 34);
        // Sans plage : deux octets par code pour Identity-H.
        assert_eq!(
            CMap::default().split(&[1, 2, 3], true),
            [(0x102, 2), (3, 1)]
        );
    }
}
