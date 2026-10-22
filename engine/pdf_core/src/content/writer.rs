//! Écriture d'opérations de contenu : nombres, noms, chaînes et tableaux au
//! format du PDF.

use super::lexer::Operand;

/// Écrit un nombre avec au plus cinq décimales, sans zéros inutiles.
pub fn write_number(out: &mut Vec<u8>, value: f64) {
    let value = if value.is_finite() { value } else { 0.0 };
    let rounded = (value * 100_000.0).round() / 100_000.0;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        out.extend_from_slice(format!("{}", rounded as i64).as_bytes());
        return;
    }
    let text = format!("{rounded:.5}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(if text == "-0" { b"0" } else { text.as_bytes() });
}

/// Écrit un nom, avec les caractères spéciaux échappés (`#xx`).
pub fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &byte in name {
        let special = !(0x21..=0x7E).contains(&byte)
            || matches!(
                byte,
                b'#' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
            );
        if special {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
}

/// Écrit une chaîne en hexadécimal.
pub fn write_hex(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'<');
    for byte in bytes {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.push(b'>');
}

/// Écrit un opérande.
pub fn write_operand(out: &mut Vec<u8>, operand: &Operand) {
    match operand {
        Operand::Number(value) => write_number(out, *value),
        Operand::Name(name) => write_name(out, name),
        Operand::String(bytes) => write_hex(out, bytes),
        Operand::Bool(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
        Operand::Null => out.extend_from_slice(b"null"),
        Operand::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                write_operand(out, item);
            }
            out.push(b']');
        }
        Operand::Dict(entries) => {
            out.extend_from_slice(b"<<");
            for (key, value) in entries {
                write_name(out, key);
                out.push(b' ');
                write_operand(out, value);
                out.push(b' ');
            }
            out.extend_from_slice(b">>");
        }
    }
}

/// Écrit une opération : opérandes numériques puis opérateur, et un saut de ligne.
pub fn write_op(out: &mut Vec<u8>, numbers: &[f64], operator: &str) {
    for &value in numbers {
        write_number(out, value);
        out.push(b' ');
    }
    out.extend_from_slice(operator.as_bytes());
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::lexer::parse;

    fn text(f: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut out = Vec::new();
        f(&mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn nombres() {
        assert_eq!(text(|o| write_number(o, 12.0)), "12");
        assert_eq!(text(|o| write_number(o, -0.5)), "-0.5");
        assert_eq!(text(|o| write_number(o, 1.0 / 3.0)), "0.33333");
        assert_eq!(text(|o| write_number(o, -0.000001)), "0");
        assert_eq!(text(|o| write_number(o, f64::NAN)), "0");
    }

    #[test]
    fn aller_retour() {
        let source = b"[<0041> -250 /A#20B (x)] TJ";
        let op = &parse(source)[0];
        let written = text(|o| write_operand(o, &op.operands[0]));
        assert_eq!(written, "[<0041> -250 /A#20B <78>]");
        assert_eq!(
            parse(format!("{written} TJ").as_bytes())[0].operands,
            op.operands
        );
    }
}
