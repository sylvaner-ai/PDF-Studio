//! Lecture d'un flux de contenu de page : suite d'opérations (opérandes puis
//! opérateur), chacune avec sa place dans le flux.
//!
//! La lecture est tolérante, comme celle des lecteurs PDF : un jeton illisible
//! est ignoré plutôt que de faire échouer toute la page. Les images en ligne
//! (`BI … ID … EI`) forment une seule opération.

use std::ops::Range;

/// Entrée d'un dictionnaire : clé et valeur.
pub type DictEntry = (Vec<u8>, Operand);

/// Opérande d'une opération.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// Nombre entier ou réel.
    Number(f64),
    /// Nom, sans la barre oblique.
    Name(Vec<u8>),
    /// Chaîne d'octets (littérale ou hexadécimale), décodée.
    String(Vec<u8>),
    /// Tableau.
    Array(Vec<Operand>),
    /// Dictionnaire (propriétés de contenu marqué, image en ligne).
    Dict(Vec<DictEntry>),
    /// Booléen.
    Bool(bool),
    /// Valeur nulle.
    Null,
}

impl Operand {
    /// Valeur numérique, s'il s'agit d'un nombre.
    pub fn number(&self) -> Option<f64> {
        match self {
            Operand::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// Nom, s'il s'agit d'un nom.
    pub fn name(&self) -> Option<&[u8]> {
        match self {
            Operand::Name(name) => Some(name),
            _ => None,
        }
    }
}

/// Une opération du flux.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    /// Opérateur (`Tj`, `re`, `cm`…).
    pub operator: Vec<u8>,
    /// Opérandes, dans l'ordre.
    pub operands: Vec<Operand>,
    /// Octets de l'opération dans le flux, du premier opérande à l'opérateur
    /// (jusqu'à `EI` pour une image en ligne).
    pub span: Range<usize>,
}

impl Operation {
    /// Vrai si l'opérateur est `op`.
    pub fn is(&self, op: &str) -> bool {
        self.operator == op.as_bytes()
    }

    /// Opérande numérique `index`, ou 0.
    pub fn number(&self, index: usize) -> f64 {
        self.operands
            .get(index)
            .and_then(Operand::number)
            .unwrap_or(0.0)
    }

    /// Les `count` derniers opérandes, s'ils sont tous numériques.
    pub fn numbers(&self, count: usize) -> Option<Vec<f64>> {
        if self.operands.len() < count {
            return None;
        }
        self.operands[self.operands.len() - count..]
            .iter()
            .map(Operand::number)
            .collect()
    }
}

/// Lit toutes les opérations d'un flux de contenu.
pub fn parse(bytes: &[u8]) -> Vec<Operation> {
    let mut lexer = Lexer { bytes, pos: 0 };
    let mut operations = Vec::new();
    let mut operands = Vec::new();
    let mut start = None;
    loop {
        lexer.skip_space();
        let token_start = lexer.pos;
        let Some(token) = lexer.token(0) else { break };
        match token {
            Token::Operand(operand) => {
                start.get_or_insert(token_start);
                operands.push(operand);
            }
            Token::Keyword(keyword) if keyword == b"BI" => {
                let span_start = start.take().unwrap_or(token_start);
                operands.clear();
                if let Some((dict, end)) = lexer.inline_image() {
                    operations.push(Operation {
                        operator: keyword,
                        operands: vec![Operand::Dict(dict)],
                        span: span_start..end,
                    });
                }
            }
            Token::Keyword(keyword) => {
                operations.push(Operation {
                    operator: keyword,
                    operands: std::mem::take(&mut operands),
                    span: start.take().unwrap_or(token_start)..lexer.pos,
                });
            }
            // Fin de tableau ou de dictionnaire sans ouverture : ignorée.
            Token::Close => {}
        }
    }
    operations
}

enum Token {
    Operand(Operand),
    Keyword(Vec<u8>),
    Close,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

/// Profondeur d'imbrication maximale des tableaux et dictionnaires.
const MAX_DEPTH: usize = 64;

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0)
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_regular(byte: u8) -> bool {
    !is_space(byte) && !is_delimiter(byte)
}

impl Lexer<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while let Some(byte) = self.peek() {
            if is_space(byte) {
                self.pos += 1;
            } else if byte == b'%' {
                while let Some(byte) = self.peek() {
                    if byte == b'\n' || byte == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn token(&mut self, depth: usize) -> Option<Token> {
        self.skip_space();
        let byte = self.peek()?;
        Some(match byte {
            b'/' => {
                self.pos += 1;
                Token::Operand(Operand::Name(self.name()))
            }
            b'(' => {
                self.pos += 1;
                Token::Operand(Operand::String(self.literal_string()))
            }
            b'<' if self.bytes.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                Token::Operand(Operand::Dict(self.dict(depth)))
            }
            b'<' => {
                self.pos += 1;
                Token::Operand(Operand::String(self.hex_string()))
            }
            b'[' => {
                self.pos += 1;
                Token::Operand(Operand::Array(self.array(depth)))
            }
            b']' | b'}' | b'{' | b')' => {
                self.pos += 1;
                Token::Close
            }
            b'>' => {
                self.pos += if self.bytes.get(self.pos + 1) == Some(&b'>') {
                    2
                } else {
                    1
                };
                Token::Close
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(is_regular) {
                    self.pos += 1;
                }
                let word = &self.bytes[start..self.pos];
                match word {
                    b"true" => Token::Operand(Operand::Bool(true)),
                    b"false" => Token::Operand(Operand::Bool(false)),
                    b"null" => Token::Operand(Operand::Null),
                    _ => match parse_number(word) {
                        Some(value) => Token::Operand(Operand::Number(value)),
                        None => Token::Keyword(word.to_vec()),
                    },
                }
            }
        })
    }

    fn name(&mut self) -> Vec<u8> {
        let mut name = Vec::new();
        while let Some(byte) = self.peek() {
            if !is_regular(byte) {
                break;
            }
            self.pos += 1;
            if byte == b'#' {
                let hex = self.bytes.get(self.pos..self.pos + 2);
                if let Some(value) = hex.and_then(|hex| {
                    std::str::from_utf8(hex)
                        .ok()
                        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                }) {
                    name.push(value);
                    self.pos += 2;
                    continue;
                }
            }
            name.push(byte);
        }
        name
    }

    fn literal_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut depth = 1;
        while let Some(byte) = self.peek() {
            self.pos += 1;
            match byte {
                b'(' => {
                    depth += 1;
                    out.push(byte);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(byte);
                }
                b'\\' => {
                    let Some(next) = self.peek() else { break };
                    self.pos += 1;
                    match next {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut value = u32::from(next - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(digit @ b'0'..=b'7') => {
                                        value = value * 8 + u32::from(digit - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((value & 0xFF) as u8);
                        }
                        other => out.push(other),
                    }
                }
                // Fin de ligne dans une chaîne : toujours un saut de ligne simple.
                b'\r' => {
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                _ => out.push(byte),
            }
        }
        out
    }

    fn hex_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        while let Some(byte) = self.peek() {
            self.pos += 1;
            if byte == b'>' {
                break;
            }
            let Some(digit) = (byte as char).to_digit(16) else {
                continue;
            };
            match high.take() {
                Some(value) => out.push(value << 4 | digit as u8),
                None => high = Some(digit as u8),
            }
        }
        // Chiffre final isolé : complété par 0.
        if let Some(value) = high {
            out.push(value << 4);
        }
        out
    }

    fn array(&mut self, depth: usize) -> Vec<Operand> {
        let mut items = Vec::new();
        if depth > MAX_DEPTH {
            return items;
        }
        loop {
            self.skip_space();
            match self.peek() {
                None => break,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            match self.token(depth + 1) {
                Some(Token::Operand(operand)) => items.push(operand),
                // Un opérateur dans un tableau : tableau mal fermé, l'opérateur
                // est rendu à la suite du flux.
                Some(Token::Keyword(_)) => {
                    self.pos = before;
                    break;
                }
                None => break,
                Some(Token::Close) => {}
            }
        }
        items
    }

    fn dict(&mut self, depth: usize) -> Vec<(Vec<u8>, Operand)> {
        let mut entries = Vec::new();
        if depth > MAX_DEPTH {
            return entries;
        }
        loop {
            self.skip_space();
            match self.peek() {
                None => break,
                Some(b'>') => {
                    self.pos += if self.bytes.get(self.pos + 1) == Some(&b'>') {
                        2
                    } else {
                        1
                    };
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            let key = match self.token(depth + 1) {
                Some(Token::Operand(Operand::Name(key))) => key,
                Some(Token::Keyword(_)) => {
                    self.pos = before;
                    break;
                }
                None => break,
                _ => continue,
            };
            let before = self.pos;
            match self.token(depth + 1) {
                Some(Token::Operand(value)) => entries.push((key, value)),
                Some(Token::Keyword(_)) => {
                    self.pos = before;
                    break;
                }
                None => break,
                Some(Token::Close) => {}
            }
        }
        entries
    }

    /// Lit une image en ligne après `BI` : son dictionnaire jusqu'à `ID`, puis
    /// ses données jusqu'à `EI`. Renvoie le dictionnaire et la fin de `EI`.
    fn inline_image(&mut self) -> Option<(Vec<DictEntry>, usize)> {
        let mut entries = Vec::new();
        loop {
            match self.token(0)? {
                Token::Keyword(keyword) if keyword == b"ID" => break,
                Token::Operand(Operand::Name(key)) => {
                    if let Some(Token::Operand(value)) = self.token(0) {
                        entries.push((key, value));
                    }
                }
                Token::Keyword(_) => return None,
                _ => {}
            }
        }
        // Un seul blanc sépare ID des données.
        if self.peek().is_some_and(is_space) {
            self.pos += 1;
        }
        let data_start = self.pos;
        // Longueur connue (données non compressées) : on saute directement aux
        // données suivantes ; sinon, on cherche « EI » entouré de blancs.
        if let Some(length) = inline_image_length(&entries) {
            let end = data_start.saturating_add(length);
            if let Some(found) = self.find_ei(end.min(self.bytes.len())) {
                if found - end <= 2 {
                    self.pos = found + 2;
                    return Some((entries, self.pos));
                }
            }
        }
        let found = self.find_ei(data_start)?;
        self.pos = found + 2;
        Some((entries, self.pos))
    }

    /// Position du prochain « EI » précédé d'un blanc et suivi d'un blanc ou de
    /// la fin du flux, à partir de `from`.
    fn find_ei(&self, from: usize) -> Option<usize> {
        let bytes = self.bytes;
        let mut index = from;
        while index + 1 < bytes.len() {
            if bytes[index] == b'E'
                && bytes[index + 1] == b'I'
                && (index == 0 || is_space(bytes[index - 1]))
                && bytes.get(index + 2).is_none_or(|&byte| is_space(byte))
            {
                return Some(index);
            }
            index += 1;
        }
        None
    }
}

/// Longueur des données d'une image en ligne non compressée.
fn inline_image_length(entries: &[DictEntry]) -> Option<usize> {
    let get = |long: &[u8], short: &[u8]| {
        entries
            .iter()
            .find(|(key, _)| key == long || key == short)
            .map(|(_, value)| value)
    };
    if get(b"Filter", b"F").is_some() {
        return None;
    }
    let width = get(b"Width", b"W")?.number()? as usize;
    let height = get(b"Height", b"H")?.number()? as usize;
    let is_mask = matches!(get(b"ImageMask", b"IM"), Some(Operand::Bool(true)));
    let bits = if is_mask {
        1
    } else {
        get(b"BitsPerComponent", b"BPC")?.number()? as usize
    };
    let components = if is_mask {
        1
    } else {
        match get(b"ColorSpace", b"CS")? {
            Operand::Name(name) => match name.as_slice() {
                b"DeviceGray" | b"G" | b"Indexed" | b"I" => 1,
                b"DeviceRGB" | b"RGB" => 3,
                b"DeviceCMYK" | b"CMYK" => 4,
                _ => return None,
            },
            Operand::Array(items) if items.first()?.name()? == b"Indexed" => 1,
            Operand::Array(items) if items.first()?.name()? == b"I" => 1,
            _ => return None,
        }
    };
    let row = (width * components * bits).div_ceil(8);
    row.checked_mul(height)
}

/// Lit un nombre PDF ; `None` si le mot n'en est pas un.
fn parse_number(word: &[u8]) -> Option<f64> {
    let first = *word.first()?;
    if !(first.is_ascii_digit() || matches!(first, b'+' | b'-' | b'.')) {
        return None;
    }
    let text = std::str::from_utf8(word).ok()?;
    if let Ok(value) = text.parse::<f64>() {
        return value.is_finite().then_some(value);
    }
    // Formes tolérées par les lecteurs : « --5 », « 1.2.3 », « 5- ».
    let unsigned = text.trim_start_matches(['+', '-']);
    let mut cleaned = String::new();
    if text.len() > unsigned.len() && text[..text.len() - unsigned.len()].contains('-') {
        cleaned.push('-');
    }
    let mut seen_dot = false;
    for c in unsigned.chars() {
        match c {
            '.' if !seen_dot => {
                seen_dot = true;
                cleaned.push('.');
            }
            '0'..='9' => cleaned.push(c),
            _ => break,
        }
    }
    let value = cleaned.parse::<f64>().unwrap_or(0.0);
    Some(if value.is_finite() { value } else { 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operators(bytes: &[u8]) -> Vec<String> {
        parse(bytes)
            .into_iter()
            .map(|op| String::from_utf8(op.operator).unwrap())
            .collect()
    }

    #[test]
    fn lit_les_operations_et_leur_place() {
        let source = b"q 1 0 0 1 72 720 cm\nBT /F1 12 Tf (Bonjour \\(le\\) monde) Tj ET Q";
        let ops = parse(source);
        assert_eq!(operators(source), ["q", "cm", "BT", "Tf", "Tj", "ET", "Q"]);
        assert_eq!(
            ops[1].numbers(6).unwrap(),
            [1.0, 0.0, 0.0, 1.0, 72.0, 720.0]
        );
        assert_eq!(&source[ops[1].span.clone()], b"1 0 0 1 72 720 cm");
        assert_eq!(ops[3].operands[0], Operand::Name(b"F1".to_vec()));
        assert_eq!(
            ops[4].operands[0],
            Operand::String(b"Bonjour (le) monde".to_vec())
        );
    }

    #[test]
    fn chaines_tableaux_et_noms() {
        let ops = parse(
            b"[<48 65 6C6C 6F> -250 (\\101\\102C\\\nD)] TJ /A#20B gs % commentaire\n.5 -.25 Td",
        );
        let Operand::Array(items) = &ops[0].operands[0] else {
            panic!("tableau attendu")
        };
        assert_eq!(items[0], Operand::String(b"Hello".to_vec()));
        assert_eq!(items[1], Operand::Number(-250.0));
        assert_eq!(items[2], Operand::String(b"ABCD".to_vec()));
        assert_eq!(ops[1].operands[0], Operand::Name(b"A B".to_vec()));
        assert_eq!(ops[2].numbers(2).unwrap(), [0.5, -0.25]);
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn image_en_ligne() {
        let source = b"q 10 0 0 10 0 0 cm BI /W 2 /H 2 /CS /G /BPC 8 ID \x00EI\x01\xFF EI Q";
        let ops = parse(source);
        assert_eq!(operators(source), ["q", "cm", "BI", "Q"]);
        assert!(source[ops[2].span.clone()].ends_with(b"EI"));
        assert!(source[ops[2].span.clone()].starts_with(b"BI"));
        // Données compressées : on cherche EI entouré de blancs.
        let source = b"BI /W 1 /H 1 /F /AHx ID 00> EI 1 g";
        assert_eq!(operators(source), ["BI", "g"]);
    }

    #[test]
    fn tolere_les_jetons_illisibles() {
        let ops = parse(b"1 0 0 RG ] >> 0.5 --2 Td [1 2 Tj");
        assert_eq!(ops[0].numbers(3).unwrap(), [1.0, 0.0, 0.0]);
        assert_eq!(ops[1].numbers(2).unwrap(), [0.5, -2.0]);
        // Tableau non fermé : l'opérateur qui suit termine l'opération.
        assert_eq!(ops.len(), 3);
        assert!(ops[2].is("Tj"));
    }
}
