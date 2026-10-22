//! Encodage interne du programme d'une police intégrée (Type 1, CFF, OpenType
//! à contours CFF).
//!
//! Une police simple dont le PDF ne précise pas l'encodage de base (ni
//! `/Encoding` nommé, ni `/BaseEncoding`) suit l'encodage inscrit dans son
//! propre programme. Les polices réduites aux caractères utilisés en portent
//! souvent un : y appliquer StandardEncoding ferait lire « Ø » pour « é ».

use std::collections::HashMap;

/// Format du programme d'une police intégrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramFormat {
    /// Type 1 (`/FontFile`).
    Type1,
    /// CFF nu (`/FontFile3`, sous-type `Type1C`).
    Cff,
    /// OpenType à contours CFF (`/FontFile3`, sous-type `OpenType`).
    OpenType,
}

/// Nom du glyphe de chaque code selon l'encodage interne du programme ;
/// `None` si le programme suit StandardEncoding, ne précise rien ou ne peut
/// pas être lu.
pub fn builtin_encoding(program: &[u8], format: ProgramFormat) -> Option<HashMap<u8, String>> {
    match format {
        ProgramFormat::Type1 => type1_encoding(program),
        ProgramFormat::Cff => cff_encoding(program),
        ProgramFormat::OpenType => cff_encoding(opentype_table(program, b"CFF ")?),
    }
}

// MARK: - Type 1

/// Encodage de la partie en clair d'un programme Type 1 :
/// `/Encoding 256 array … dup 233 /eacute put … readonly def`.
fn type1_encoding(program: &[u8]) -> Option<HashMap<u8, String>> {
    // En-tête de segment PFB éventuel.
    let program = if program.starts_with(&[0x80, 0x01]) && program.len() > 6 {
        &program[6..]
    } else {
        program
    };
    let clear = find(program, b"eexec").map_or(program, |end| &program[..end]);
    let start = find(clear, b"/Encoding")? + b"/Encoding".len();
    let tokens = tokens(&clear[start..]);
    if tokens.first().is_some_and(|t| *t == b"StandardEncoding") {
        return None;
    }
    let mut encoding = HashMap::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == b"def" {
            break;
        }
        if tokens[index] == b"dup" && index + 3 < tokens.len() && tokens[index + 3] == b"put" {
            let code = std::str::from_utf8(tokens[index + 1])
                .ok()
                .and_then(|code| code.parse::<u8>().ok());
            if let (Some(code), Some(name)) = (code, tokens[index + 2].strip_prefix(b"/")) {
                encoding.insert(code, String::from_utf8_lossy(name).into_owned());
            }
            index += 4;
        } else {
            index += 1;
        }
    }
    (!encoding.is_empty()).then_some(encoding)
}

/// Mots PostScript : séparés par les blancs, un nom (`/nom`) commençant un
/// nouveau mot même collé au précédent (`dup 32/space put`).
fn tokens(data: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    for word in data
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
    {
        let mut start = 0;
        for (i, &b) in word.iter().enumerate() {
            if b == b'/' && i > start {
                tokens.push(&word[start..i]);
                start = i;
            }
        }
        tokens.push(&word[start..]);
    }
    tokens
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len())
        .position(|window| window == needle)
}

// MARK: - OpenType

/// Table `tag` d'une police OpenType.
fn opentype_table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = usize::from(read_u16(font, 4)?);
    (0..count).find_map(|i| {
        let record = 12 + 16 * i;
        if font.get(record..record + 4)? != tag {
            return None;
        }
        let offset = read_u32(font, record + 8)? as usize;
        let length = read_u32(font, record + 12)? as usize;
        font.get(offset..offset.checked_add(length)?)
    })
}

// MARK: - CFF

/// Encodage d'une police CFF (Adobe Technical Note 5176) : le Top DICT donne
/// l'encodage (codes des glyphes) et le jeu de caractères (noms des glyphes).
fn cff_encoding(data: &[u8]) -> Option<HashMap<u8, String>> {
    let header = usize::from(*data.get(2)?);
    let (_, after_names) = index(data, header)?;
    let (top_dicts, after_tops) = index(data, after_names)?;
    let (strings, _) = index(data, after_tops)?;
    let top = dict(top_dicts.first()?)?;
    // Police CID : pas d'encodage par codes d'un octet.
    if top.contains_key(&ROS) {
        return None;
    }
    let encoding = operand(&top, ENCODING).unwrap_or(0);
    // 0 : StandardEncoding ; 1 : ExpertEncoding (non pris en charge).
    if encoding <= 1 {
        return None;
    }
    let (charstrings, _) = index(data, operand(&top, CHARSTRINGS)?)?;
    let names = glyph_names(
        data,
        operand(&top, CHARSET).unwrap_or(0),
        charstrings.len(),
        &strings,
    )?;

    let mut codes: HashMap<u8, String> = HashMap::new();
    let format = *data.get(encoding)?;
    let mut pos = encoding + 1;
    let mut gid = 1;
    match format & 0x7F {
        0 => {
            let count = usize::from(*data.get(pos)?);
            for &code in data.get(pos + 1..pos + 1 + count)? {
                if let Some(name) = names.get(gid) {
                    codes.insert(code, name.clone());
                }
                gid += 1;
            }
            pos += 1 + count;
        }
        1 => {
            let ranges = usize::from(*data.get(pos)?);
            for range in data.get(pos + 1..pos + 1 + 2 * ranges)?.chunks(2) {
                for code in u16::from(range[0])..=u16::from(range[0]) + u16::from(range[1]) {
                    if let (Ok(code), Some(name)) = (u8::try_from(code), names.get(gid)) {
                        codes.insert(code, name.clone());
                    }
                    gid += 1;
                }
            }
            pos += 1 + 2 * ranges;
        }
        _ => return None,
    }
    // Suppléments : codes supplémentaires désignant un glyphe par son nom.
    if format & 0x80 != 0 {
        let count = usize::from(*data.get(pos)?);
        for supplement in data.get(pos + 1..pos + 1 + 3 * count)?.chunks(3) {
            let sid = u16::from_be_bytes([supplement[1], supplement[2]]);
            if let Some(name) = string(sid, &strings) {
                codes.insert(supplement[0], name);
            }
        }
    }
    Some(codes)
}

const CHARSET: u16 = 15;
const ENCODING: u16 = 16;
const CHARSTRINGS: u16 = 17;
/// `12 30` : police CID.
const ROS: u16 = 1230;

/// Nom de chaque glyphe, par numéro, d'après le jeu de caractères.
fn glyph_names(
    data: &[u8],
    charset: usize,
    count: usize,
    strings: &[&[u8]],
) -> Option<Vec<String>> {
    let mut sids: Vec<u16> = vec![0];
    match charset {
        // ISOAdobe : le glyphe n a le nom standard n.
        0 => sids.extend((1..count).map(|gid| gid as u16)),
        // Jeux experts : non pris en charge.
        1 | 2 => return None,
        _ => {
            let format = *data.get(charset)?;
            let mut pos = charset + 1;
            while sids.len() < count {
                match format {
                    0 => {
                        sids.push(read_u16(data, pos)?);
                        pos += 2;
                    }
                    1 | 2 => {
                        let first = read_u16(data, pos)?;
                        let left = if format == 1 {
                            u16::from(*data.get(pos + 2)?)
                        } else {
                            read_u16(data, pos + 2)?
                        };
                        pos += if format == 1 { 3 } else { 4 };
                        sids.extend((0..=left).map(|i| first.saturating_add(i)));
                    }
                    _ => return None,
                }
            }
        }
    }
    sids.truncate(count);
    Some(
        sids.into_iter()
            .map(|sid| string(sid, strings).unwrap_or_default())
            .collect(),
    )
}

/// Chaîne `sid` : standard, ou de l'index des chaînes de la police.
fn string(sid: u16, strings: &[&[u8]]) -> Option<String> {
    let sid = usize::from(sid);
    match STANDARD_STRINGS.get(sid) {
        Some(name) => Some((*name).to_owned()),
        None => strings
            .get(sid - STANDARD_STRINGS.len())
            .map(|name| String::from_utf8_lossy(name).into_owned()),
    }
}

/// Lit un INDEX CFF à partir de `pos` : ses éléments et la position qui le suit.
fn index(data: &[u8], pos: usize) -> Option<(Vec<&[u8]>, usize)> {
    let count = usize::from(read_u16(data, pos)?);
    if count == 0 {
        return Some((Vec::new(), pos + 2));
    }
    let size = usize::from(*data.get(pos + 2)?);
    if !(1..=4).contains(&size) {
        return None;
    }
    let offsets_at = pos + 3;
    let offset = |i: usize| -> Option<usize> {
        let bytes = data.get(offsets_at + i * size..offsets_at + (i + 1) * size)?;
        Some(
            bytes
                .iter()
                .fold(0usize, |value, &b| value << 8 | usize::from(b)),
        )
    };
    // Les décalages comptent à partir de l'octet qui précède les données.
    let base = offsets_at + (count + 1) * size - 1;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let (start, end) = (offset(i)?, offset(i + 1)?);
        items.push(data.get(base + start..base + end)?);
    }
    Some((items, base + offset(count)?))
}

/// Opérandes de chaque opérateur d'un DICT CFF (`12 x` noté `1200 + x`).
fn dict(data: &[u8]) -> Option<HashMap<u16, Vec<f64>>> {
    let mut entries = HashMap::new();
    let mut operands = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let b0 = data[pos];
        pos += 1;
        match b0 {
            12 => {
                let op = 1200 + u16::from(*data.get(pos)?);
                pos += 1;
                entries.insert(op, std::mem::take(&mut operands));
            }
            0..=21 => {
                entries.insert(u16::from(b0), std::mem::take(&mut operands));
            }
            28 => {
                operands.push(f64::from(i16::from_be_bytes([
                    *data.get(pos)?,
                    *data.get(pos + 1)?,
                ])));
                pos += 2;
            }
            29 => {
                let bytes = data.get(pos..pos + 4)?;
                operands.push(f64::from(i32::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])));
                pos += 4;
            }
            30 => {
                // Nombre réel en demi-octets, jusqu'au demi-octet 0xF.
                let mut text = String::new();
                loop {
                    let byte = *data.get(pos)?;
                    pos += 1;
                    let mut done = false;
                    for nibble in [byte >> 4, byte & 0xF] {
                        match nibble {
                            0..=9 => text.push(char::from(b'0' + nibble)),
                            0xA => text.push('.'),
                            0xB => text.push('E'),
                            0xC => text.push_str("E-"),
                            0xE => text.push('-'),
                            0xF => {
                                done = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if done {
                        break;
                    }
                }
                operands.push(text.parse().unwrap_or(0.0));
            }
            32..=246 => operands.push(f64::from(i16::from(b0) - 139)),
            247..=250 => {
                let b1 = *data.get(pos)?;
                pos += 1;
                operands.push(f64::from((i16::from(b0) - 247) * 256 + i16::from(b1) + 108));
            }
            251..=254 => {
                let b1 = *data.get(pos)?;
                pos += 1;
                operands.push(f64::from(
                    -(i16::from(b0) - 251) * 256 - i16::from(b1) - 108,
                ));
            }
            _ => return None,
        }
    }
    Some(entries)
}

fn operand(dict: &HashMap<u16, Vec<f64>>, op: u16) -> Option<usize> {
    let value = *dict.get(&op)?.first()?;
    (value >= 0.0).then_some(value as usize)
}

fn read_u16(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?]))
}

fn read_u32(data: &[u8], pos: usize) -> Option<u32> {
    let bytes = data.get(pos..pos + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Chaînes standard du format CFF (annexe A de la note 5176).
#[rustfmt::skip]
const STANDARD_STRINGS: [&str; 391] = [
    ".notdef", "space", "exclam", "quotedbl", "numbersign", "dollar", "percent", "ampersand",
    "quoteright", "parenleft", "parenright", "asterisk", "plus", "comma", "hyphen", "period",
    "slash", "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "colon", "semicolon", "less", "equal", "greater", "question", "at", "A", "B", "C", "D", "E",
    "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X",
    "Y", "Z", "bracketleft", "backslash", "bracketright", "asciicircum", "underscore", "quoteleft",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z", "braceleft", "bar", "braceright", "asciitilde",
    "exclamdown", "cent", "sterling", "fraction", "yen", "florin", "section", "currency",
    "quotesingle", "quotedblleft", "guillemotleft", "guilsinglleft", "guilsinglright", "fi", "fl",
    "endash", "dagger", "daggerdbl", "periodcentered", "paragraph", "bullet", "quotesinglbase",
    "quotedblbase", "quotedblright", "guillemotright", "ellipsis", "perthousand", "questiondown",
    "grave", "acute", "circumflex", "tilde", "macron", "breve", "dotaccent", "dieresis", "ring",
    "cedilla", "hungarumlaut", "ogonek", "caron", "emdash", "AE", "ordfeminine", "Lslash",
    "Oslash", "OE", "ordmasculine", "ae", "dotlessi", "lslash", "oslash", "oe", "germandbls",
    "onesuperior", "logicalnot", "mu", "trademark", "Eth", "onehalf", "plusminus", "Thorn",
    "onequarter", "divide", "brokenbar", "degree", "thorn", "threequarters", "twosuperior",
    "registered", "minus", "eth", "multiply", "threesuperior", "copyright", "Aacute",
    "Acircumflex", "Adieresis", "Agrave", "Aring", "Atilde", "Ccedilla", "Eacute", "Ecircumflex",
    "Edieresis", "Egrave", "Iacute", "Icircumflex", "Idieresis", "Igrave", "Ntilde", "Oacute",
    "Ocircumflex", "Odieresis", "Ograve", "Otilde", "Scaron", "Uacute", "Ucircumflex", "Udieresis",
    "Ugrave", "Yacute", "Ydieresis", "Zcaron", "aacute", "acircumflex", "adieresis", "agrave",
    "aring", "atilde", "ccedilla", "eacute", "ecircumflex", "edieresis", "egrave", "iacute",
    "icircumflex", "idieresis", "igrave", "ntilde", "oacute", "ocircumflex", "odieresis", "ograve",
    "otilde", "scaron", "uacute", "ucircumflex", "udieresis", "ugrave", "yacute", "ydieresis",
    "zcaron", "exclamsmall", "Hungarumlautsmall", "dollaroldstyle", "dollarsuperior",
    "ampersandsmall", "Acutesmall", "parenleftsuperior", "parenrightsuperior", "twodotenleader",
    "onedotenleader", "zerooldstyle", "oneoldstyle", "twooldstyle", "threeoldstyle",
    "fouroldstyle", "fiveoldstyle", "sixoldstyle", "sevenoldstyle", "eightoldstyle",
    "nineoldstyle", "commasuperior", "threequartersemdash", "periodsuperior", "questionsmall",
    "asuperior", "bsuperior", "centsuperior", "dsuperior", "esuperior", "isuperior", "lsuperior",
    "msuperior", "nsuperior", "osuperior", "rsuperior", "ssuperior", "tsuperior", "ff", "ffi",
    "ffl", "parenleftinferior", "parenrightinferior", "Circumflexsmall", "hyphensuperior",
    "Gravesmall", "Asmall", "Bsmall", "Csmall", "Dsmall", "Esmall", "Fsmall", "Gsmall", "Hsmall",
    "Ismall", "Jsmall", "Ksmall", "Lsmall", "Msmall", "Nsmall", "Osmall", "Psmall", "Qsmall",
    "Rsmall", "Ssmall", "Tsmall", "Usmall", "Vsmall", "Wsmall", "Xsmall", "Ysmall", "Zsmall",
    "colonmonetary", "onefitted", "rupiah", "Tildesmall", "exclamdownsmall", "centoldstyle",
    "Lslashsmall", "Scaronsmall", "Zcaronsmall", "Dieresissmall", "Brevesmall", "Caronsmall",
    "Dotaccentsmall", "Macronsmall", "figuredash", "hypheninferior", "Ogoneksmall", "Ringsmall",
    "Cedillasmall", "questiondownsmall", "oneeighth", "threeeighths", "fiveeighths",
    "seveneighths", "onethird", "twothirds", "zerosuperior", "foursuperior", "fivesuperior",
    "sixsuperior", "sevensuperior", "eightsuperior", "ninesuperior", "zeroinferior", "oneinferior",
    "twoinferior", "threeinferior", "fourinferior", "fiveinferior", "sixinferior", "seveninferior",
    "eightinferior", "nineinferior", "centinferior", "dollarinferior", "periodinferior",
    "commainferior", "Agravesmall", "Aacutesmall", "Acircumflexsmall", "Atildesmall",
    "Adieresissmall", "Aringsmall", "AEsmall", "Ccedillasmall", "Egravesmall", "Eacutesmall",
    "Ecircumflexsmall", "Edieresissmall", "Igravesmall", "Iacutesmall", "Icircumflexsmall",
    "Idieresissmall", "Ethsmall", "Ntildesmall", "Ogravesmall", "Oacutesmall", "Ocircumflexsmall",
    "Otildesmall", "Odieresissmall", "OEsmall", "Oslashsmall", "Ugravesmall", "Uacutesmall",
    "Ucircumflexsmall", "Udieresissmall", "Yacutesmall", "Thornsmall", "Ydieresissmall", "001.000",
    "001.001", "001.002", "001.003", "Black", "Bold", "Book", "Light", "Medium", "Regular",
    "Roman", "Semibold",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaines_standard() {
        assert_eq!(STANDARD_STRINGS[228], "zcaron");
        assert_eq!(STANDARD_STRINGS[229], "exclamsmall");
        assert_eq!(STANDARD_STRINGS[390], "Semibold");
    }

    #[test]
    fn encodage_type1() {
        let program = b"%!PS-AdobeFont-1.0: Test\n/FontName /Test def\n\
            /Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\n\
            dup 32 /space put\ndup 101/e put\ndup 233 /eacute put\nreadonly def\n\
            currentfile eexec \x00\x01dup 66 /B put";
        let encoding = builtin_encoding(program, ProgramFormat::Type1).unwrap();
        assert_eq!(encoding.len(), 3);
        assert_eq!(encoding[&233], "eacute");
        assert_eq!(encoding[&101], "e");
        assert!(!encoding.contains_key(&66));

        let standard = b"/FontName /Test def /Encoding StandardEncoding def currentfile eexec";
        assert_eq!(builtin_encoding(standard, ProgramFormat::Type1), None);
    }

    /// INDEX CFF de décalages sur un octet.
    fn cff_index(items: &[&[u8]]) -> Vec<u8> {
        let mut out = (items.len() as u16).to_be_bytes().to_vec();
        if items.is_empty() {
            return out;
        }
        out.push(1);
        let mut offset = 1u8;
        out.push(offset);
        for item in items {
            offset += item.len() as u8;
            out.push(offset);
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    /// Entier de DICT sur cinq octets (opérateur 29).
    fn int(value: usize) -> Vec<u8> {
        let mut out = vec![29];
        out.extend_from_slice(&(value as i32).to_be_bytes());
        out
    }

    #[test]
    fn encodage_cff() {
        // Glyphes : .notdef, e (SID 70), eacute (SID 207 : « Ø » en
        // StandardEncoding pour le code 0xE9), « custom » (chaîne propre 391).
        let names = cff_index(&[b"Test"]);
        let strings = cff_index(&[b"custom"]);
        let gsubrs = cff_index(&[]);
        let charstrings = cff_index(&[&[14], &[14], &[14], &[14]]);
        let charset: Vec<u8> = [0u8].into_iter().chain([0, 70, 0, 207, 1, 135]).collect();
        // Format 0 avec suppléments : e → 0x65, eacute → 0xE9, custom → 0x80 ;
        // supplément : 0x41 → « custom ».
        let encoding = [0x80, 3, 0x65, 0xE9, 0x80, 1, 0x41, 1, 135];
        let top_len = 3 * 6 + 2;
        let start = 4 + names.len() + (2 + 1 + 2 + top_len) + strings.len() + gsubrs.len();
        let (charset_at, encoding_at) = (start, start + charset.len());
        let charstrings_at = encoding_at + encoding.len();
        let mut top = Vec::new();
        top.extend(int(charset_at));
        top.push(15);
        top.extend(int(encoding_at));
        top.push(16);
        top.extend(int(charstrings_at));
        top.push(17);
        top.extend([139, 139]); // FontBBox tronqué, ignoré.
        assert_eq!(top.len(), top_len);
        let mut font = vec![1, 0, 4, 1];
        font.extend(names);
        font.extend(cff_index(&[&top]));
        font.extend(strings);
        font.extend(gsubrs);
        assert_eq!(font.len(), start);
        font.extend(charset);
        font.extend(encoding);
        font.extend(charstrings);

        let encoding = builtin_encoding(&font, ProgramFormat::Cff).unwrap();
        assert_eq!(encoding[&0x65], "e");
        assert_eq!(encoding[&0xE9], "eacute");
        assert_eq!(encoding[&0x80], "custom");
        assert_eq!(encoding[&0x41], "custom");
        assert_eq!(encoding.len(), 4);
    }
}
