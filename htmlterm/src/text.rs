//! Utilidades de texto: anchura en celdas de terminal, espacios HTML y codificaciones.

/// Espacio en blanco según HTML (el U+00A0 NO cuenta: es un espacio duro).
pub fn is_html_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// Colapsa cualquier secuencia de espacios HTML en uno solo y recorta los extremos.
pub fn collapse_ws(s: &str) -> String {
    s.split(is_html_ws)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Anchura aproximada de un carácter en celdas de terminal (0, 1 o 2).
///
/// Aproximación sin dependencias; se puede sustituir por el crate `unicode-width`.
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x20 || (0x7f..0xa0).contains(&u) {
        return 0;
    }
    let zero = (0x0300..=0x036f).contains(&u)
        || (0x200b..=0x200f).contains(&u)
        || (0xfe00..=0xfe0f).contains(&u)
        || (0x20d0..=0x20ff).contains(&u);
    if zero {
        return 0;
    }
    let wide = (0x1100..=0x115f).contains(&u)
        || ((0x2e80..=0xa4cf).contains(&u) && u != 0x303f)
        || (0xac00..=0xd7a3).contains(&u)
        || (0xf900..=0xfaff).contains(&u)
        || (0xfe30..=0xfe6f).contains(&u)
        || (0xff00..=0xff60).contains(&u)
        || (0xffe0..=0xffe6).contains(&u)
        || (0x1f300..=0x1f64f).contains(&u)
        || (0x1f900..=0x1f9ff).contains(&u)
        || (0x20000..=0x3fffd).contains(&u);
    if wide { 2 } else { 1 }
}

/// Anchura de una cadena en celdas de terminal.
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Recorta una cadena para que ocupe como máximo `max` celdas.
pub fn truncate_width(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

/// Caracteres 0x80..=0x9F de Windows-1252 (los que difieren de Latin-1).
pub fn cp1252(code: u32) -> Option<char> {
    let c = match code {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8a => 'Š',
        0x8b => '‹',
        0x8c => 'Œ',
        0x8e => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9a => 'š',
        0x9b => '›',
        0x9c => 'œ',
        0x9e => 'ž',
        0x9f => 'Ÿ',
        _ => return None,
    };
    Some(c)
}

/// Decodifica bytes: UTF-8 si es válido; si no, Windows-1252 (cubre Latin-1).
pub fn decode_bytes(bytes: &[u8]) -> String {
    // Quita la marca BOM de UTF-8 si existe.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes
            .iter()
            .map(|&b| cp1252(b as u32).unwrap_or(b as char))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(display_width("hola"), 4);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width("e\u{301}"), 1);
    }

    #[test]
    fn collapse() {
        assert_eq!(collapse_ws("  a \n\t b  "), "a b");
        assert_eq!(collapse_ws(""), "");
    }

    #[test]
    fn latin1_fallback() {
        assert_eq!(decode_bytes(&[0x63, 0x61, 0x66, 0xe9]), "café");
        assert_eq!(decode_bytes("ñ".as_bytes()), "ñ");
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_width("abcdef", 3), "abc");
        assert_eq!(truncate_width("日本語", 5), "日本");
    }
}
