//! Parser HTML tolerante (subconjunto práctico de HTML5, sin dependencias).
//!
//! Cubre: etiquetas y atributos (con o sin comillas), comentarios, doctype, elementos
//! vacíos, contenido crudo de `<script>`/`<style>`, entidades, y los cierres implícitos
//! más habituales (`<p>`, `<li>`, `<dt>/<dd>`, `<tr>/<td>`, `<option>`...).
//!
//! No implementa el algoritmo completo de construcción de árbol de la especificación
//! (adopción de elementos de formato, foster parenting...). Si hace falta fidelidad total
//! se puede sustituir este módulo por `html5ever` manteniendo la misma API: `parse(&str) -> Document`.

use crate::dom::{Document, NodeId, NodeKind};
use crate::text::cp1252;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Etiquetas cuyo inicio cierra un `<p>` abierto.
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dir", "div", "dl", "dd",
    "dt", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6",
    "header", "hgroup", "hr", "li", "main", "menu", "nav", "ol", "p", "pre", "section", "summary",
    "table", "ul",
];

/// Elementos que "cortan" la búsqueda de un elemento abierto al procesar etiquetas.
const SCOPE_BOUNDARY: &[&str] = &[
    "td", "th", "table", "caption", "html", "button", "template", "object",
];

/// Profundidad máxima de anidamiento de elementos abiertos (Chrome usa el mismo valor).
const MAX_DEPTH: usize = 512;

/// Analiza un documento HTML y devuelve su DOM.
pub fn parse(html: &str) -> Document {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut b = Builder::new();
    let mut i = 0;
    let mut text_start = 0;

    while i < len {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        if let Some(after) = rest.strip_prefix("<!--") {
            b.flush_text(html, text_start, i);
            i = match after.find("-->") {
                Some(p) => i + 4 + p + 3,
                None => len,
            };
            text_start = i;
            continue;
        }
        match bytes.get(i + 1).copied() {
            // <!DOCTYPE ...>, <?xml ...?>: se descartan.
            Some(b'!') | Some(b'?') => {
                b.flush_text(html, text_start, i);
                i = match bytes[i..].iter().position(|&c| c == b'>') {
                    Some(p) => i + p + 1,
                    None => len,
                };
                text_start = i;
            }
            // </etiqueta>
            Some(b'/') if bytes.get(i + 2).is_some_and(|c| c.is_ascii_alphabetic()) => {
                b.flush_text(html, text_start, i);
                let (name, end) = parse_end_tag(html, i + 2);
                b.end(&name);
                i = end;
                text_start = i;
            }
            // <etiqueta ...>
            Some(c) if c.is_ascii_alphabetic() => {
                b.flush_text(html, text_start, i);
                let (name, attrs, end) = parse_start_tag(html, i + 1);
                i = end;
                text_start = i;
                b.start(&name, attrs);
                let raw = matches!(name.as_str(), "script" | "style");
                let rcdata = matches!(name.as_str(), "title" | "textarea");
                if raw || rcdata {
                    let close = find_ci(bytes, format!("</{name}").as_bytes(), i).unwrap_or(len);
                    let content = &html[i..close];
                    b.add_text(if raw {
                        content.to_string()
                    } else {
                        decode_entities(content)
                    });
                    i = close;
                    text_start = i;
                }
            }
            // Un '<' suelto es texto normal.
            _ => i += 1,
        }
    }
    b.flush_text(html, text_start, len);
    b.doc
}

struct Builder {
    doc: Document,
    /// Pila de elementos abiertos; el índice 0 es siempre la raíz.
    stack: Vec<NodeId>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            doc: Document::new(),
            stack: vec![Document::ROOT],
        }
    }

    fn top(&self) -> NodeId {
        *self.stack.last().unwrap()
    }

    fn tag_at(&self, idx: usize) -> &str {
        self.doc.tag(self.stack[idx]).unwrap_or("")
    }

    fn flush_text(&mut self, html: &str, from: usize, to: usize) {
        if from < to {
            let t = decode_entities(&html[from..to]);
            self.add_text(t);
        }
    }

    fn add_text(&mut self, s: String) {
        if s.is_empty() {
            return;
        }
        let top = self.top();
        self.doc.add(top, NodeKind::Text(s));
    }

    /// Posición en la pila del elemento abierto más cercano con esa etiqueta,
    /// sin atravesar ninguno de los elementos de `stops`.
    fn find_open(&self, tag: &str, stops: &[&str]) -> Option<usize> {
        for i in (1..self.stack.len()).rev() {
            let t = self.tag_at(i);
            if t == tag {
                return Some(i);
            }
            if stops.contains(&t) {
                return None;
            }
        }
        None
    }

    /// Cierra el elemento abierto más cercano que cumpla `pred`, sin pasar de `stops`.
    fn close_nearest(&mut self, pred: impl Fn(&str) -> bool, stops: &[&str]) {
        for i in (1..self.stack.len()).rev() {
            let t = self.tag_at(i);
            if pred(t) {
                self.stack.truncate(i);
                return;
            }
            if stops.contains(&t) {
                return;
            }
        }
    }

    fn start(&mut self, tag: &str, attrs: Vec<(String, String)>) {
        // <html> y <body> duplicados se ignoran.
        if (tag == "html" || tag == "body") && self.doc.find_first(tag).is_some() {
            return;
        }
        if tag == "body"
            && let Some(i) = self.find_open("head", &[]) {
                self.stack.truncate(i);
            }
        if CLOSES_P.contains(&tag)
            && let Some(i) = self.find_open("p", SCOPE_BOUNDARY) {
                self.stack.truncate(i);
            }
        match tag {
            "li" => self.close_nearest(
                |t| t == "li",
                &["ul", "ol", "menu", "dir", "table", "td", "th"],
            ),
            "dt" | "dd" => self.close_nearest(
                |t| t == "dt" || t == "dd",
                &["dl", "table", "td", "th"],
            ),
            "tr" => self.close_nearest(|t| t == "tr", &["table"]),
            "td" | "th" => self.close_nearest(|t| t == "td" || t == "th", &["tr", "table"]),
            "thead" | "tbody" | "tfoot" => self.close_table_section(),
            "option" => self.close_nearest(|t| t == "option", &["select", "datalist"]),
            "optgroup" => self.close_nearest(
                |t| t == "optgroup" || t == "option",
                &["select", "datalist"],
            ),
            "a" => self.close_nearest(|t| t == "a", SCOPE_BOUNDARY),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if matches!(self.tag_at(self.stack.len() - 1), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
                {
                    self.stack.pop();
                }
            }
            _ => {}
        }
        let top = self.top();
        let id = self.doc.add(
            top,
            NodeKind::Element {
                tag: tag.to_string(),
                attrs,
            },
        );
        // Como los navegadores, se limita la profundidad: los elementos más allá del límite
        // no pasan a ser el nodo actual. Evita árboles patológicos y recorridos cuadráticos.
        if !VOID.contains(&tag) && self.stack.len() < MAX_DEPTH {
            self.stack.push(id);
        }
    }

    /// Un nuevo thead/tbody/tfoot cierra la sección anterior (y la fila abierta, si la hay).
    fn close_table_section(&mut self) {
        let mut target = None;
        for i in (1..self.stack.len()).rev() {
            match self.tag_at(i) {
                "thead" | "tbody" | "tfoot" => {
                    target = Some(i);
                    break;
                }
                "tr" => target = Some(i),
                "table" => break,
                _ => {}
            }
        }
        if let Some(i) = target {
            self.stack.truncate(i);
        }
    }

    fn end(&mut self, tag: &str) {
        match tag {
            // Tras </body> o </html> el contenido sigue yendo al cuerpo, como hacen los navegadores.
            "html" | "body" => return,
            "br" => {
                self.start("br", Vec::new());
                return;
            }
            // </p> sin <p> abierto crea un párrafo vacío.
            "p" if self.find_open("p", SCOPE_BOUNDARY).is_none() => {
                self.start("p", Vec::new());
            }
            _ => {}
        }
        let stops: &[&str] = match tag {
            "table" => &[],
            "td" | "th" | "tr" | "thead" | "tbody" | "tfoot" | "caption" | "colgroup" => &["table"],
            _ => SCOPE_BOUNDARY,
        };
        if let Some(i) = self.find_open(tag, stops) {
            self.stack.truncate(i);
        }
    }
}

/// Busca `needle` (en minúsculas) sin distinguir mayúsculas a partir de `from`.
fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn parse_end_tag(html: &str, from: usize) -> (String, usize) {
    let b = html.as_bytes();
    let mut i = from;
    while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'/' || b[i] == b'>') {
        i += 1;
    }
    let name = html[from..i].to_ascii_lowercase();
    while i < b.len() && b[i] != b'>' {
        i += 1;
    }
    (name, (i + 1).min(b.len()))
}

/// Lee nombre y atributos de una etiqueta de apertura; devuelve la posición tras el `>`.
fn parse_start_tag(html: &str, from: usize) -> (String, Vec<(String, String)>, usize) {
    let b = html.as_bytes();
    let len = b.len();
    let mut i = from;
    while i < len && !(b[i].is_ascii_whitespace() || b[i] == b'/' || b[i] == b'>') {
        i += 1;
    }
    let name = html[from..i].to_ascii_lowercase();
    let mut attrs: Vec<(String, String)> = Vec::new();

    loop {
        while i < len && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= len {
            break;
        }
        if b[i] == b'>' {
            i += 1;
            break;
        }
        let s = i;
        while i < len && !(b[i].is_ascii_whitespace() || b[i] == b'=' || b[i] == b'>' || b[i] == b'/')
        {
            i += 1;
        }
        if i == s {
            // Carácter inesperado (p. ej. '=' al inicio): avanzar para no bloquearse.
            i += 1;
            continue;
        }
        let attr = html[s..i].to_ascii_lowercase();
        while i < len && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < len && b[i] == b'=' {
            i += 1;
            while i < len && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < len && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&html[vs..i]);
                if i < len {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < len && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = decode_entities(&html[vs..i]);
            }
        }
        if !attrs.iter().any(|(n, _)| *n == attr) {
            attrs.push((attr, value));
        }
    }
    (name, attrs, i)
}

const NAMED: &[(&str, &str)] = &[
    ("amp", "&"), ("lt", "<"), ("gt", ">"), ("quot", "\""), ("apos", "'"),
    ("nbsp", "\u{a0}"), ("copy", "©"), ("reg", "®"), ("trade", "™"), ("hellip", "…"),
    ("mdash", "—"), ("ndash", "–"), ("lsquo", "‘"), ("rsquo", "’"), ("ldquo", "“"),
    ("rdquo", "”"), ("laquo", "«"), ("raquo", "»"), ("bull", "•"), ("middot", "·"),
    ("times", "×"), ("divide", "÷"), ("euro", "€"), ("pound", "£"), ("yen", "¥"),
    ("cent", "¢"), ("sect", "§"), ("deg", "°"), ("plusmn", "±"), ("para", "¶"),
    ("larr", "←"), ("rarr", "→"), ("uarr", "↑"), ("darr", "↓"), ("harr", "↔"),
    ("iexcl", "¡"), ("iquest", "¿"), ("micro", "µ"), ("frac12", "½"), ("frac14", "¼"),
    ("frac34", "¾"), ("sup2", "²"), ("sup3", "³"), ("ordm", "º"), ("ordf", "ª"),
    ("aacute", "á"), ("eacute", "é"), ("iacute", "í"), ("oacute", "ó"), ("uacute", "ú"),
    ("Aacute", "Á"), ("Eacute", "É"), ("Iacute", "Í"), ("Oacute", "Ó"), ("Uacute", "Ú"),
    ("agrave", "à"), ("egrave", "è"), ("igrave", "ì"), ("ograve", "ò"), ("ugrave", "ù"),
    ("acirc", "â"), ("ecirc", "ê"), ("icirc", "î"), ("ocirc", "ô"), ("ucirc", "û"),
    ("auml", "ä"), ("euml", "ë"), ("iuml", "ï"), ("ouml", "ö"), ("uuml", "ü"),
    ("Auml", "Ä"), ("Ouml", "Ö"), ("Uuml", "Ü"), ("szlig", "ß"), ("ntilde", "ñ"),
    ("Ntilde", "Ñ"), ("ccedil", "ç"), ("Ccedil", "Ç"), ("aring", "å"), ("aelig", "æ"),
    ("oslash", "ø"), ("check", "✓"), ("hearts", "♥"), ("infin", "∞"), ("ne", "≠"),
    ("le", "≤"), ("ge", "≥"),
];

/// Entidades antiguas que los navegadores aceptan sin el `;` final.
const LEGACY: &[&str] = &["amp", "lt", "gt", "quot", "nbsp", "copy", "reg"];

/// Sustituye entidades (`&amp;`, `&#65;`, `&#x41;`...) por sus caracteres.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        match decode_one(rest) {
            Some((text, used)) => {
                out.push_str(&text);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `s` empieza por '&'. Devuelve el texto decodificado y los bytes consumidos.
fn decode_one(s: &str) -> Option<(String, usize)> {
    let b = s.as_bytes();
    if b.get(1) == Some(&b'#') {
        let (hex, start) = match b.get(2) {
            Some(b'x') | Some(b'X') => (true, 3),
            _ => (false, 2),
        };
        let mut end = start;
        while end < b.len() && end - start < 8 && (b[end] as char).is_digit(if hex { 16 } else { 10 }) {
            end += 1;
        }
        if end == start {
            return None;
        }
        let code = u32::from_str_radix(&s[start..end], if hex { 16 } else { 10 }).ok()?;
        let used = if b.get(end) == Some(&b';') { end + 1 } else { end };
        let c = cp1252(code)
            .or_else(|| char::from_u32(code).filter(|_| code != 0))
            .unwrap_or('\u{fffd}');
        return Some((c.to_string(), used));
    }
    let mut end = 1;
    while end < b.len() && end < 12 && b[end].is_ascii_alphanumeric() {
        end += 1;
    }
    let name = &s[1..end];
    let has_semi = b.get(end) == Some(&b';');
    let value = NAMED.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)?;
    if !has_semi && !LEGACY.contains(&name) {
        return None;
    }
    Some((value.to_string(), if has_semi { end + 1 } else { end }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(doc: &Document, id: NodeId) -> Vec<String> {
        doc.node(id)
            .children
            .iter()
            .filter_map(|&c| doc.tag(c).map(str::to_string))
            .collect()
    }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("&lt;b&gt; &amp; &#65;&#x42; &copy"), "<b> & AB ©");
        assert_eq!(decode_entities("a &foo; b & c"), "a &foo; b & c");
        assert_eq!(decode_entities("&aacute;&nbsp;"), "á\u{a0}");
    }

    #[test]
    fn implied_p_and_li() {
        let d = parse("<p>a<p>b<ul><li>x<li>y</ul>");
        assert_eq!(tags(&d, Document::ROOT), ["p", "p", "ul"]);
        let ul = d.find_first("ul").unwrap();
        assert_eq!(tags(&d, ul), ["li", "li"]);
    }

    #[test]
    fn nested_lists_keep_structure() {
        let d = parse("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>");
        let ul = d.find_first("ul").unwrap();
        assert_eq!(tags(&d, ul), ["li", "li"]);
    }

    #[test]
    fn table_rows_and_cells() {
        let d = parse("<table><tr><td>1<td>2<tr><td>3</table>");
        let t = d.find_first("table").unwrap();
        let rows: Vec<NodeId> = d.node(t).children.clone();
        assert_eq!(rows.len(), 2);
        assert_eq!(tags(&d, rows[0]), ["td", "td"]);
        assert_eq!(tags(&d, rows[1]), ["td"]);
    }

    #[test]
    fn raw_script_and_void_elements() {
        let d = parse("<div><script>if (a<b && c) {}</script>x<br>y<img src=a.png alt='q'></div>");
        let s = d.find_first("script").unwrap();
        assert_eq!(d.text_content(s), "if (a<b && c) {}");
        let div = d.find_first("div").unwrap();
        assert_eq!(tags(&d, div), ["script", "br", "img"]);
        let img = d.find_first("img").unwrap();
        assert_eq!(d.attr(img, "src"), Some("a.png"));
        assert_eq!(d.attr(img, "alt"), Some("q"));
    }

    #[test]
    fn attributes_variants() {
        let d = parse("<A HREF=\"x?a=1&amp;b=2\" Class=big disabled>t</A>");
        let a = d.find_first("a").unwrap();
        assert_eq!(d.attr(a, "href"), Some("x?a=1&b=2"));
        assert_eq!(d.attr(a, "class"), Some("big"));
        assert_eq!(d.attr(a, "disabled"), Some(""));
    }

    #[test]
    fn comments_and_doctype_are_dropped() {
        let d = parse("<!DOCTYPE html><!-- hola --><p>x</p>");
        assert_eq!(tags(&d, Document::ROOT), ["p"]);
    }

    #[test]
    fn stray_end_tags_and_lt() {
        let d = parse("<div>a < b</div></span></p>");
        assert!(d.find_first("div").is_some());
        let div = d.find_first("div").unwrap();
        assert_eq!(d.text_content(div), "a < b");
    }

    #[test]
    fn nesting_depth_is_bounded() {
        let html = format!("{}fondo{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
        let d = parse(&html);
        let mut depth = 0;
        let mut cur = d.nodes.len() - 1; // el nodo de texto "fondo"
        while let Some(p) = d.node(cur).parent {
            depth += 1;
            cur = p;
        }
        assert!(depth <= MAX_DEPTH + 1, "profundidad {depth}");
        assert_eq!(d.text_content(Document::ROOT), "fondo");
    }

    #[test]
    fn unterminated_input_does_not_hang() {
        parse("<div class=\"x");
        parse("<!-- sin cerrar");
        parse("<script>sin cerrar");
        parse("<");
        parse("");
    }
}
