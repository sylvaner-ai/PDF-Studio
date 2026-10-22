//! Couche de texte invisible des pages reconnues (OCR).
//!
//! Chaque mot reconnu est écrit en mode de rendu 3 (ni rempli ni tracé), à
//! l'emplacement exact du mot dans l'image : la page est cherchable et son
//! texte sélectionnable, sans que son aspect change. La police est réduite à
//! un glyphe vide (comme la « GlyphLessFont » de Tesseract) : un code de deux
//! octets par caractère, égal à son point de code Unicode, une chasse fixe de
//! un demi-cadratin, et une table `ToUnicode` qui rend le texte aux lecteurs.
//! Chaque mot est étiré horizontalement (`Tz`) pour couvrir sa largeur ; les
//! mots d'une même ligne partagent une ligne de pied et une taille.

use edit::{LayerWord, TextLayer};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Buffer;

use crate::error::Result;

/// Étiquette du contenu marqué qui entoure la couche, pour la reconnaître.
pub(crate) const LAYER_TAG: &str = "PDFStudioOCR";

/// Chasse de l'unique glyphe, en millièmes de cadratin.
const GLYPH_WIDTH: f32 = 500.0;

/// Part de la hauteur d'un mot sous la ligne de pied (jambages).
const DESCENT: f32 = 0.2;

/// Flux de contenu qui dessine `layer` en texte invisible avec la police de
/// ressource `font`.
///
/// Comme dans les PDF de Tesseract, chaque ligne est un seul alignement : une
/// matrice `Tm` au début de la ligne, une taille et une ligne de pied communes
/// à tous ses mots, puis des déplacements `Td` le long de la ligne. Les
/// lecteurs (Aperçu, Acrobat) y reconnaissent une ligne de texte et
/// sélectionnent le texte ligne après ligne, et non mot par mot en colonnes.
pub(crate) fn layer_content(layer: &TextLayer, font: &str) -> Vec<u8> {
    let mut out = format!("/{LAYER_TAG} BMC\nq\nBT\n3 Tr\n").into_bytes();
    let mut current_size = None;
    for line in layer.words.chunk_by(|a, b| a.line == b.line) {
        write_line(&mut out, line, font, &mut current_size);
    }
    out.extend_from_slice(b"ET\nQ\nEMC\n");
    out
}

/// Codes de deux octets du texte d'un mot (plan multilingue de base).
fn codes(text: &str) -> Vec<u16> {
    text.chars()
        .filter_map(|c| u16::try_from(u32::from(c)).ok())
        .filter(|&code| !(0xD800..0xE000).contains(&code))
        .collect()
}

/// Écrit une ligne : chaque mot est suivi d'une espace étirée jusqu'au mot
/// suivant, pour que les lecteurs y voient une séparation de mots.
fn write_line(out: &mut Vec<u8>, line: &[LayerWord], font: &str, current_size: &mut Option<f32>) {
    let words: Vec<&LayerWord> = line
        .iter()
        .filter(|word| !codes(&word.text).is_empty() && word_size(word).is_some())
        .collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else {
        return;
    };
    // Direction de la ligne : du début du premier mot à la fin du dernier.
    let (bx, by) = first.bottom_left();
    let (ex, ey) = last.bottom_right();
    let length = (ex - bx).hypot(ey - by);
    let (cos, sin) = if length < 0.01 {
        let (rx, ry) = first.bottom_right();
        let own = (rx - bx).hypot(ry - by);
        ((rx - bx) / own, (ry - by) / own)
    } else {
        ((ex - bx) / length, (ey - by) / length)
    };
    // Position d'un point le long de la ligne (x) et au-dessus de son
    // départ (y).
    let along = |(x, y): (f32, f32)| (x - bx) * cos + (y - by) * sin;
    let above = |(x, y): (f32, f32)| (y - by) * cos - (x - bx) * sin;
    // Taille et bas communs : les médianes des mots de la ligne.
    let size = median(words.iter().filter_map(|word| word_size(word)).collect());
    let bottom = median(words.iter().map(|word| above(word.bottom_left())).collect());
    let lift = bottom + size * DESCENT;
    let (ox, oy) = (bx - sin * lift, by + cos * lift);

    if *current_size != Some(size) {
        out.extend_from_slice(format!("/{font} {} Tf\n", number(size)).as_bytes());
        *current_size = Some(size);
    }
    out.extend_from_slice(
        format!(
            "{} {} {} {} {} {} Tm\n",
            number(cos),
            number(sin),
            number(-sin),
            number(cos),
            number(ox),
            number(oy)
        )
        .as_bytes(),
    );
    let mut position = 0.0;
    for (index, word) in words.iter().enumerate() {
        let start = along(word.bottom_left());
        let end = along(word.bottom_right());
        let mut codes = codes(&word.text);
        let mut width = end - start;
        if let Some(next) = words.get(index + 1) {
            let gap = along(next.bottom_left()) - start;
            if gap > width {
                width = gap;
                codes.push(u16::from(b' '));
            }
        }
        if width < 0.01 {
            continue;
        }
        if index > 0 {
            out.extend_from_slice(format!("{} 0 Td\n", number(start - position)).as_bytes());
        }
        position = start;
        let natural = codes.len() as f32 * GLYPH_WIDTH / 1000.0 * size;
        out.extend_from_slice(format!("{} Tz\n<", number(100.0 * width / natural)).as_bytes());
        for code in codes {
            out.extend_from_slice(format!("{code:04X}").as_bytes());
        }
        out.extend_from_slice(b"> Tj\n");
    }
}

/// Hauteur d'un mot, s'il a une emprise.
fn word_size(word: &LayerWord) -> Option<f32> {
    let (bx, by) = word.bottom_left();
    let (rx, ry) = word.bottom_right();
    let (tx, ty) = word.top_left();
    let height = (tx - bx).hypot(ty - by);
    ((rx - bx).hypot(ry - by) >= 0.01 && height >= 0.01).then_some(height)
}

/// Médiane d'une liste non vide.
fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// Nombre écrit avec trois décimales au plus, sans zéros inutiles.
fn number(value: f32) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// Ajoute au document la police de la couche de texte (police composite
/// `Type0` à codage `Identity-H`, programme TrueType intégré).
pub(crate) fn add_layer_font(doc: &mut PdfDocument) -> Result<PdfObject> {
    let program = glyphless_truetype();
    let mut file_dict = doc.new_dict()?;
    file_dict.dict_put("Length1", PdfObject::new_int(program.len() as i32)?)?;
    let file = doc.add_stream(&Buffer::from_bytes(&program)?, Some(&file_dict), false)?;

    let mut descriptor = doc.new_dict()?;
    descriptor.dict_put("Type", PdfObject::new_name("FontDescriptor")?)?;
    descriptor.dict_put("FontName", PdfObject::new_name("GlyphLessFont")?)?;
    descriptor.dict_put("Flags", PdfObject::new_int(5)?)?;
    let mut bbox = doc.new_array()?;
    for v in [0, -200, 500, 800] {
        bbox.array_push(PdfObject::new_int(v)?)?;
    }
    descriptor.dict_put("FontBBox", bbox)?;
    descriptor.dict_put("ItalicAngle", PdfObject::new_int(0)?)?;
    descriptor.dict_put("Ascent", PdfObject::new_int(800)?)?;
    descriptor.dict_put("Descent", PdfObject::new_int(-200)?)?;
    descriptor.dict_put("CapHeight", PdfObject::new_int(700)?)?;
    descriptor.dict_put("StemV", PdfObject::new_int(80)?)?;
    descriptor.dict_put("FontFile2", file)?;
    let descriptor = doc.add_object(&descriptor)?;

    // Tous les codes désignent le glyphe 1.
    let map: Vec<u8> = std::iter::repeat_n([0u8, 1u8], 65536).flatten().collect();
    let map = doc.add_stream(&Buffer::from_bytes(&map)?, None, false)?;

    let mut system = doc.new_dict()?;
    system.dict_put("Registry", PdfObject::new_string("Adobe")?)?;
    system.dict_put("Ordering", PdfObject::new_string("Identity")?)?;
    system.dict_put("Supplement", PdfObject::new_int(0)?)?;

    let mut cid_font = doc.new_dict()?;
    cid_font.dict_put("Type", PdfObject::new_name("Font")?)?;
    cid_font.dict_put("Subtype", PdfObject::new_name("CIDFontType2")?)?;
    cid_font.dict_put("BaseFont", PdfObject::new_name("GlyphLessFont")?)?;
    cid_font.dict_put("CIDSystemInfo", system)?;
    cid_font.dict_put("FontDescriptor", descriptor)?;
    cid_font.dict_put("DW", PdfObject::new_int(GLYPH_WIDTH as i32)?)?;
    cid_font.dict_put("CIDToGIDMap", map)?;
    let cid_font = doc.add_object(&cid_font)?;

    let to_unicode = doc.add_stream(&Buffer::from_bytes(&identity_to_unicode())?, None, false)?;

    let mut font = doc.new_dict()?;
    font.dict_put("Type", PdfObject::new_name("Font")?)?;
    font.dict_put("Subtype", PdfObject::new_name("Type0")?)?;
    font.dict_put("BaseFont", PdfObject::new_name("GlyphLessFont")?)?;
    font.dict_put("Encoding", PdfObject::new_name("Identity-H")?)?;
    let mut descendants = doc.new_array()?;
    descendants.array_push(cid_font)?;
    font.dict_put("DescendantFonts", descendants)?;
    font.dict_put("ToUnicode", to_unicode)?;
    Ok(doc.add_object(&font)?)
}

/// Table `ToUnicode` : chaque code de deux octets vaut son point de code
/// (plan multilingue de base, hors demi-codets d'indirection).
fn identity_to_unicode() -> Vec<u8> {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    // Une plage ne peut changer que le dernier octet : une plage par octet
    // de poids fort, par blocs de cent au plus.
    let highs: Vec<u16> = (0u16..=0xFF)
        .filter(|high| !(0xD8..0xE0).contains(high))
        .collect();
    for chunk in highs.chunks(100) {
        out.push_str(&format!("{} beginbfrange\n", chunk.len()));
        for high in chunk {
            let start = high << 8;
            out.push_str(&format!(
                "<{start:04X}> <{:04X}> <{start:04X}>\n",
                start | 0xFF
            ));
        }
        out.push_str("endbfrange\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

// MARK: - Programme TrueType à un glyphe vide

/// Police TrueType minimale : deux glyphes vides (`.notdef` et le glyphe
/// unique de la couche), d'un demi-cadratin de chasse, cadratin de 1 000
/// unités, hauteur 800 au-dessus de la ligne de pied et 200 au-dessous.
pub(crate) fn glyphless_truetype() -> Vec<u8> {
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();

    let mut os2 = Writer::default();
    os2.u16(1); // version
    os2.i16(250); // xAvgCharWidth
    os2.u16(400); // usWeightClass
    os2.u16(5); // usWidthClass
    os2.u16(0); // fsType : intégration libre
    for v in [650, 600, 0, 75, 650, 600, 0, 350, 50, 300] {
        os2.i16(v); // indices et soulignement
    }
    os2.i16(0); // sFamilyClass
    os2.bytes(&[0; 10]); // panose
    os2.u32(0);
    os2.u32(0);
    os2.u32(0);
    os2.u32(0); // ulUnicodeRange
    os2.bytes(b"PDFS"); // achVendID
    os2.u16(0x40); // fsSelection : regular
    os2.u16(0); // usFirstCharIndex
    os2.u16(0xFFFF); // usLastCharIndex
    os2.i16(800); // sTypoAscender
    os2.i16(-200); // sTypoDescender
    os2.i16(0); // sTypoLineGap
    os2.u16(800); // usWinAscent
    os2.u16(200); // usWinDescent
    os2.u32(1);
    os2.u32(0); // ulCodePageRange
    tables.push((*b"OS/2", os2.0));

    // Table cmap : un format 4 vide (le codage passe par CIDToGIDMap).
    let mut cmap = Writer::default();
    cmap.u16(0);
    cmap.u16(1);
    cmap.u16(3);
    cmap.u16(1);
    cmap.u32(12);
    for v in [4, 24, 0, 2, 2, 0, 0, 0xFFFF, 0, 0xFFFF, 1, 0] {
        cmap.u16(v);
    }
    tables.push((*b"cmap", cmap.0));

    tables.push((*b"glyf", Vec::new()));

    let mut head = Writer::default();
    head.u32(0x0001_0000);
    head.u32(0x0001_0000);
    head.u32(0); // checkSumAdjustment, calculé à la fin
    head.u32(0x5F0F_3CF5);
    head.u16(0x000B);
    head.u16(1000);
    head.bytes(&[0; 16]); // dates
    for v in [0, -200, 500, 800] {
        head.i16(v);
    }
    head.u16(0); // macStyle
    head.u16(3); // lowestRecPPEM
    head.i16(2); // fontDirectionHint
    head.i16(0); // indexToLocFormat : décalages courts
    head.i16(0);
    tables.push((*b"head", head.0));

    let mut hhea = Writer::default();
    hhea.u32(0x0001_0000);
    hhea.i16(800);
    hhea.i16(-200);
    hhea.i16(0);
    hhea.u16(500);
    for v in [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0] {
        hhea.i16(v);
    }
    hhea.u16(2); // numberOfHMetrics
    tables.push((*b"hhea", hhea.0));

    let mut hmtx = Writer::default();
    for _ in 0..2 {
        hmtx.u16(500);
        hmtx.i16(0);
    }
    tables.push((*b"hmtx", hmtx.0));

    let mut loca = Writer::default();
    for _ in 0..3 {
        loca.u16(0);
    }
    tables.push((*b"loca", loca.0));

    let mut maxp = Writer::default();
    maxp.u32(0x0001_0000);
    maxp.u16(2); // numGlyphs
    for v in [0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        maxp.u16(v);
    }
    tables.push((*b"maxp", maxp.0));

    let mut name = Writer::default();
    let family: Vec<u8> = "GlyphLessFont"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let records = [1u16, 2, 4, 6];
    name.u16(0);
    name.u16(records.len() as u16);
    name.u16(6 + 12 * records.len() as u16);
    let style: Vec<u8> = "Regular"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut strings = Vec::new();
    for id in records {
        let value = if id == 2 { &style } else { &family };
        name.u16(3);
        name.u16(1);
        name.u16(0x0409);
        name.u16(id);
        name.u16(value.len() as u16);
        name.u16(strings.len() as u16);
        strings.extend_from_slice(value);
    }
    name.bytes(&strings);
    tables.push((*b"name", name.0));

    let mut post = Writer::default();
    post.u32(0x0003_0000);
    post.u32(0);
    post.i16(-100);
    post.i16(50);
    post.u32(1); // isFixedPitch
    post.bytes(&[0; 16]);
    tables.push((*b"post", post.0));

    // Répertoire des tables, triées par étiquette.
    tables.sort_by_key(|table| table.0);
    let count = tables.len() as u16;
    let entry_selector = 15 - count.leading_zeros() as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let mut font = Writer::default();
    font.u32(0x0001_0000);
    font.u16(count);
    font.u16(search_range);
    font.u16(entry_selector);
    font.u16(count * 16 - search_range);
    let mut offset = 12 + 16 * tables.len();
    let mut head_offset = 0;
    for (tag, data) in &tables {
        if tag == b"head" {
            head_offset = offset;
        }
        font.bytes(tag);
        font.u32(checksum(data));
        font.u32(offset as u32);
        font.u32(data.len() as u32);
        offset += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        font.bytes(data);
        font.bytes(&vec![0; data.len().div_ceil(4) * 4 - data.len()]);
    }
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&font.0));
    font.0[head_offset + 8..head_offset + 12].copy_from_slice(&adjustment.to_be_bytes());
    font.0
}

/// Somme de contrôle TrueType : mots de 32 bits, gros-boutistes, complétés.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, line: u32) -> LayerWord {
        LayerWord {
            text: text.into(),
            quad: [x, 100.0, x + 40.0, 100.0, x + 40.0, 110.0, x, 110.0],
            confidence: 0.9,
            line,
        }
    }

    #[test]
    fn contenu_de_la_couche() {
        let layer = TextLayer {
            words: vec![
                word("Été", 10.0, 0),
                word("2026", 60.0, 0),
                word("x", 10.0, 1),
            ],
            engine: "test".into(),
        };
        let content = String::from_utf8(layer_content(&layer, "OCR1")).unwrap();
        assert!(content.starts_with("/PDFStudioOCR BMC\nq\nBT\n3 Tr\n/OCR1 10 Tf\n"));
        // « Été » + espace jusqu'au mot suivant : 4 glyphes de 5 points,
        // étirés sur 50 points.
        assert!(content.contains("1 0 0 1 10 102 Tm\n250 Tz\n<00C9007400E90020> Tj"));
        // Mot suivant : déplacé le long de la ligne, sans nouvelle matrice ;
        // dernier mot de la ligne : sans espace.
        assert!(content.contains("<00C9007400E90020> Tj\n50 0 Td\n200 Tz\n<0032003000320036> Tj\n"));
        assert_eq!(content.matches(" Tm\n").count(), 2);
        assert!(content.ends_with("ET\nQ\nEMC\n"));
    }

    #[test]
    fn ligne_commune_aux_mots() {
        // Mots de hauteurs et de bas différents (jambages) : une seule ligne
        // de pied et une seule taille, les médianes.
        let mut low = word("joli", 60.0, 0);
        low.quad = [60.0, 97.0, 100.0, 97.0, 100.0, 110.0, 60.0, 110.0];
        let layer = TextLayer {
            words: vec![word("Le", 10.0, 0), low, word("chat", 110.0, 0)],
            engine: "test".into(),
        };
        let content = String::from_utf8(layer_content(&layer, "OCR1")).unwrap();
        assert_eq!(content.matches(" Tf\n").count(), 1);
        assert!(content.contains("/OCR1 10 Tf\n1 0 0 1 10 102 Tm\n"));
        assert!(content.contains("50 0 Td\n"));
    }

    #[test]
    fn police_truetype_valide() {
        let program = glyphless_truetype();
        assert_eq!(program.len() % 4, 0);
        assert_eq!(checksum(&program), 0xB1B0_AFBA);
        assert!(mupdf::Font::from_bytes("GlyphLessFont", &program).is_ok());
    }

    #[test]
    fn nombres_compacts() {
        assert_eq!(number(10.0), "10");
        assert_eq!(number(-0.0001), "0");
        assert_eq!(number(0.12345), "0.123");
    }
}
