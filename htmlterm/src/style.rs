//! Estilos: color y atributos de texto, y los estilos por defecto de cada etiqueta
//! (el equivalente a la hoja de estilos del agente de usuario de un navegador).
//!
//! En el hito 3 (CSS) los valores calculados reemplazarán a estas tablas.

use std::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    #[default]
    Default,
    /// 0..=15 colores ANSI básicos y brillantes; 16..=255 paleta de 256 colores.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TextStyle {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub reverse: bool,
}

impl TextStyle {
    pub fn is_plain(&self) -> bool {
        *self == TextStyle::default()
    }

    /// Secuencia SGR que activa este estilo desde cero (vacía si es el estilo por defecto).
    pub fn sgr(&self) -> String {
        if self.is_plain() {
            return String::new();
        }
        let mut c = String::from("0");
        if self.bold {
            c.push_str(";1");
        }
        if self.dim {
            c.push_str(";2");
        }
        if self.italic {
            c.push_str(";3");
        }
        if self.underline {
            c.push_str(";4");
        }
        if self.reverse {
            c.push_str(";7");
        }
        if self.strike {
            c.push_str(";9");
        }
        push_color(&mut c, self.fg, false);
        push_color(&mut c, self.bg, true);
        format!("\x1b[{c}m")
    }
}

fn push_color(out: &mut String, color: Color, background: bool) {
    let base = if background { 40 } else { 30 };
    match color {
        Color::Default => {}
        Color::Indexed(n) if n < 8 => {
            let _ = write!(out, ";{}", base + n as u32);
        }
        Color::Indexed(n) if n < 16 => {
            let _ = write!(out, ";{}", base + 60 + (n as u32 - 8));
        }
        Color::Indexed(n) => {
            let _ = write!(out, ";{};5;{}", base + 8, n);
        }
        Color::Rgb(r, g, b) => {
            let _ = write!(out, ";{};2;{};{};{}", base + 8, r, g, b);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    None,
    Block,
    Inline,
}

/// Valor `display` por defecto de una etiqueta. Las desconocidas son en línea, como en los navegadores.
pub fn display(tag: &str) -> Display {
    match tag {
        "head" | "title" | "meta" | "link" | "script" | "style" | "template" | "noscript"
        | "base" | "datalist" | "option" | "optgroup" | "param" | "source" | "track" | "svg"
        | "canvas" | "audio" | "video" | "iframe" | "object" | "embed" | "map" | "area"
        | "dialog" | "rp" => Display::None,
        "html" | "body" | "address" | "article" | "aside" | "blockquote" | "center" | "details"
        | "dd" | "div" | "dl" | "dt" | "fieldset" | "figcaption" | "figure" | "footer" | "form"
        | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hgroup" | "hr" | "legend"
        | "main" | "menu" | "nav" | "ol" | "p" | "pre" | "section" | "summary" | "ul" | "li"
        | "tr" | "caption" | "table" | "thead" | "tbody" | "tfoot" | "dir" | "listing" | "xmp"
        | "search" => Display::Block,
        _ => Display::Inline,
    }
}

/// Márgenes verticales (arriba, abajo) en líneas de texto.
pub fn margins(tag: &str) -> (usize, usize) {
    match tag {
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "dl"
        | "fieldset" | "figure" | "table" | "menu" | "dir" => (1, 1),
        _ => (0, 0),
    }
}

/// Aplica el estilo por defecto de una etiqueta sobre el estilo heredado.
pub fn apply(tag: &str, st: &mut TextStyle) {
    match tag {
        "b" | "strong" | "th" | "dt" | "summary" | "legend" => st.bold = true,
        "i" | "em" | "cite" | "var" | "dfn" | "address" => st.italic = true,
        "u" | "ins" => st.underline = true,
        "s" | "strike" | "del" => st.strike = true,
        "small" => st.dim = true,
        "a" => {
            st.underline = true;
            st.fg = Color::Indexed(14);
        }
        "code" | "kbd" | "samp" | "tt" => st.fg = Color::Indexed(3),
        "mark" => {
            st.bg = Color::Indexed(11);
            st.fg = Color::Indexed(0);
        }
        "h1" => {
            st.bold = true;
            st.fg = Color::Indexed(11);
        }
        "h2" => {
            st.bold = true;
            st.fg = Color::Indexed(10);
        }
        "h3" => {
            st.bold = true;
            st.fg = Color::Indexed(13);
        }
        "h4" | "h5" | "h6" => st.bold = true,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_codes() {
        assert_eq!(TextStyle::default().sgr(), "");
        let s = TextStyle {
            bold: true,
            fg: Color::Indexed(14),
            ..Default::default()
        };
        assert_eq!(s.sgr(), "\x1b[0;1;96m");
        let s = TextStyle {
            bg: Color::Indexed(2),
            fg: Color::Rgb(1, 2, 3),
            ..Default::default()
        };
        assert_eq!(s.sgr(), "\x1b[0;38;2;1;2;3;42m");
        let s = TextStyle {
            fg: Color::Indexed(200),
            ..Default::default()
        };
        assert_eq!(s.sgr(), "\x1b[0;38;5;200m");
    }
}
