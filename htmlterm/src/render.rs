//! Layout y renderizado sobre una cuadrícula de caracteres.
//!
//! La unidad es la celda de terminal, no el píxel. El motor recorre el DOM y produce
//! `Line`s (listas de `Span` con estilo) ya ajustadas al ancho disponible:
//!
//! * contenido en línea: se acumula en tokens (palabra / espacio / salto) y se ajusta al ancho;
//! * contenido en bloque: márgenes verticales colapsados entre bloques;
//! * prefijos (viñetas, citas, sangrías): una pila de segmentos que se antepone a cada línea;
//! * tablas: columnas calculadas con anchos mínimo/máximo y celdas renderizadas de forma recursiva.

use crate::dom::{Document, NodeId, NodeKind};
use crate::style::{self, Color, Display, TextStyle};
use crate::text::{char_width, collapse_ws, display_width, is_html_ws, truncate_width};

// ---------------------------------------------------------------------------
// Tipos públicos
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: TextStyle,
}

pub type Line = Vec<Span>;

#[derive(Clone, Debug)]
pub struct Link {
    pub href: String,
}

/// Caracteres usados para viñetas, reglas y bordes. Hay una variante ASCII para consolas antiguas.
#[derive(Clone, Copy, Debug)]
pub struct Glyphs {
    pub bullets: [&'static str; 3],
    pub quote: &'static str,
    pub hr: &'static str,
    pub h1: &'static str,
    pub h2: &'static str,
    pub dropdown: &'static str,
    pub radio_on: &'static str,
    pub radio_off: &'static str,
    pub qopen: &'static str,
    pub qclose: &'static str,
    pub tl: &'static str,
    pub tr: &'static str,
    pub bl: &'static str,
    pub br: &'static str,
    pub h: &'static str,
    pub v: &'static str,
    pub tm: &'static str,
    pub bm: &'static str,
    pub lm: &'static str,
    pub rm: &'static str,
    pub x: &'static str,
}

impl Glyphs {
    pub fn unicode() -> Self {
        Glyphs {
            bullets: ["•", "◦", "▪"],
            quote: "│ ",
            hr: "─",
            h1: "═",
            h2: "─",
            dropdown: "▾",
            radio_on: "(•)",
            radio_off: "( )",
            qopen: "“",
            qclose: "”",
            tl: "┌",
            tr: "┐",
            bl: "└",
            br: "┘",
            h: "─",
            v: "│",
            tm: "┬",
            bm: "┴",
            lm: "├",
            rm: "┤",
            x: "┼",
        }
    }

    pub fn ascii() -> Self {
        Glyphs {
            bullets: ["*", "-", "+"],
            quote: "| ",
            hr: "-",
            h1: "=",
            h2: "-",
            dropdown: "v",
            radio_on: "(*)",
            radio_off: "( )",
            qopen: "\"",
            qclose: "\"",
            tl: "+",
            tr: "+",
            bl: "+",
            br: "+",
            h: "-",
            v: "|",
            tm: "+",
            bm: "+",
            lm: "+",
            rm: "+",
            x: "+",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Ancho total disponible, en columnas.
    pub width: usize,
    pub glyphs: Glyphs,
}

impl Options {
    pub fn new(width: usize, unicode: bool) -> Self {
        Options {
            width: width.max(1),
            glyphs: if unicode { Glyphs::unicode() } else { Glyphs::ascii() },
        }
    }
}

/// Resultado del renderizado: líneas listas para mostrar (incluida la barra de título y la lista de enlaces).
#[derive(Clone, Debug)]
pub struct Page {
    pub title: Option<String>,
    pub lines: Vec<Line>,
    pub links: Vec<Link>,
}

/// Renderiza un documento a líneas con estilo.
pub fn render(doc: &Document, opts: &Options) -> Page {
    let mut lay = Layout::new(doc, opts, opts.width);
    lay.walk(Document::ROOT);
    lay.flush();
    let Layout { lines: body, links, .. } = lay;

    let title = doc
        .find_first("title")
        .map(|t| collapse_ws(&doc.text_content(t)))
        .filter(|t| !t.is_empty());

    let mut lines: Vec<Line> = Vec::new();
    if let Some(t) = &title {
        let text = truncate_width(t, opts.width.saturating_sub(2));
        let pad = opts.width.saturating_sub(display_width(&text) + 1);
        lines.push(vec![Span {
            text: format!(" {text}{}", " ".repeat(pad)),
            style: TextStyle {
                reverse: true,
                bold: true,
                ..Default::default()
            },
        }]);
        lines.push(Vec::new());
    }
    lines.extend(body);

    if !links.is_empty() {
        let dim = TextStyle {
            dim: true,
            ..Default::default()
        };
        lines.push(Vec::new());
        lines.push(vec![Span {
            text: "Enlaces:".to_string(),
            style: TextStyle {
                bold: true,
                ..Default::default()
            },
        }]);
        for (i, l) in links.iter().enumerate() {
            lines.push(vec![
                Span {
                    text: format!("[{}] ", i + 1),
                    style: dim,
                },
                Span {
                    text: l.href.clone(),
                    style: TextStyle {
                        fg: Color::Indexed(14),
                        ..Default::default()
                    },
                },
            ]);
        }
    }
    Page { title, lines, links }
}

/// Convierte líneas con estilo en texto con secuencias ANSI (o texto plano si `color` es falso).
pub fn to_ansi(lines: &[Line], color: bool) -> String {
    let mut out = String::new();
    for line in lines {
        for s in line {
            let text = s.text.replace('\u{a0}', " ");
            let sgr = if color { s.style.sgr() } else { String::new() };
            if sgr.is_empty() {
                out.push_str(&text);
            } else {
                out.push_str(&sgr);
                out.push_str(&text);
                out.push_str("\x1b[0m");
            }
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Internos
// ---------------------------------------------------------------------------

fn line_width(line: &Line) -> usize {
    line.iter().map(|s| display_width(&s.text)).sum()
}

/// Añade texto a una línea fusionándolo con el último tramo si tiene el mismo estilo.
fn push_span(line: &mut Line, text: &str, style: TextStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = line.last_mut()
        && last.style == style {
            last.text.push_str(text);
            return;
        }
    line.push(Span {
        text: text.to_string(),
        style,
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Align {
    Left,
    Center,
    Right,
}

fn parse_align(v: &str) -> Option<Align> {
    match v.trim().to_ascii_lowercase().as_str() {
        "center" | "middle" => Some(Align::Center),
        "right" => Some(Align::Right),
        "left" => Some(Align::Left),
        _ => None,
    }
}

enum Tok {
    Word(String, TextStyle),
    Space(TextStyle),
    Break,
}

/// Palabras pegadas (sin espacio entre ellas, p. ej. `<b>ho</b>la`) que no se pueden separar al ajustar.
struct Unit {
    parts: Vec<(String, TextStyle)>,
    w: usize,
}

enum Item {
    Unit(Unit),
    Space(TextStyle),
    Break,
}

fn build_items(toks: Vec<Tok>) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for t in toks {
        match t {
            Tok::Word(s, st) => {
                let w = display_width(&s);
                match items.last_mut() {
                    Some(Item::Unit(u)) => {
                        u.parts.push((s, st));
                        u.w += w;
                    }
                    _ => items.push(Item::Unit(Unit {
                        parts: vec![(s, st)],
                        w,
                    })),
                }
            }
            Tok::Space(st) => match items.last() {
                None | Some(Item::Space(_)) | Some(Item::Break) => {}
                _ => items.push(Item::Space(st)),
            },
            Tok::Break => {
                if matches!(items.last(), Some(Item::Space(_))) {
                    items.pop();
                }
                items.push(Item::Break);
            }
        }
    }
    if matches!(items.last(), Some(Item::Space(_))) {
        items.pop();
    }
    items
}

/// Prefijo de línea (viñeta, barra de cita, sangría). `first` solo se usa en la primera línea.
struct Seg {
    first: String,
    rest: String,
    style: TextStyle,
    used: bool,
}

struct ListCtx {
    ordered: bool,
    next: i64,
}

struct Layout<'a> {
    doc: &'a Document,
    opts: &'a Options,
    width: usize,
    lines: Vec<Line>,
    toks: Vec<Tok>,
    segs: Vec<Seg>,
    pending_blank: usize,
    style: TextStyle,
    align: Align,
    pre: usize,
    pre_cur: Line,
    pre_first: bool,
    lists: Vec<ListCtx>,
    links: Vec<Link>,
    /// Número de enlaces ya numerados fuera de este layout (para celdas de tabla).
    link_offset: usize,
    /// Modo medición: sin alineación ni reglas a ancho completo (para calcular anchos naturales).
    measure: bool,
    /// Anchura máxima de contenido emitido (sin prefijos); la usan las reglas bajo los títulos.
    content_max: usize,
}

impl<'a> Layout<'a> {
    fn new(doc: &'a Document, opts: &'a Options, width: usize) -> Self {
        Layout {
            doc,
            opts,
            width: width.max(1),
            lines: Vec::new(),
            toks: Vec::new(),
            segs: Vec::new(),
            pending_blank: 0,
            style: TextStyle::default(),
            align: Align::Left,
            pre: 0,
            pre_cur: Vec::new(),
            pre_first: false,
            lists: Vec::new(),
            links: Vec::new(),
            link_offset: 0,
            measure: false,
            content_max: 0,
        }
    }

    // ----- recorrido del DOM -------------------------------------------------

    fn walk(&mut self, id: NodeId) {
        let doc = self.doc;
        match &doc.node(id).kind {
            NodeKind::Text(t) => self.text(t),
            NodeKind::Element { .. } => self.element(id),
            NodeKind::Document => self.walk_children(id),
        }
    }

    fn walk_children(&mut self, id: NodeId) {
        let doc = self.doc;
        for &c in &doc.node(id).children {
            self.walk(c);
        }
    }

    fn element(&mut self, id: NodeId) {
        let doc = self.doc;
        let tag = match doc.tag(id) {
            Some(t) => t,
            None => return,
        };
        if is_hidden(doc, id, tag) {
            return;
        }
        match tag {
            "br" => {
                if self.pre > 0 {
                    let l = std::mem::take(&mut self.pre_cur);
                    self.emit(l, Align::Left);
                } else {
                    self.toks.push(Tok::Break);
                }
            }
            "wbr" => {}
            "hr" => self.hr(),
            "img" => self.img(id),
            "input" => self.input(id),
            "select" => self.select(id),
            "textarea" => self.textarea(id),
            "button" => self.button(id),
            "table" => self.table(id),
            "pre" => self.pre_block(id),
            "ul" => self.list(id, false),
            "ol" => self.list(id, true),
            "li" => self.list_item(id),
            "blockquote" => {
                let seg = Seg {
                    first: self.opts.glyphs.quote.to_string(),
                    rest: self.opts.glyphs.quote.to_string(),
                    style: TextStyle {
                        fg: Color::Indexed(8),
                        ..Default::default()
                    },
                    used: false,
                };
                self.indented(id, seg, 1);
            }
            "dd" => {
                let seg = Seg {
                    first: "    ".to_string(),
                    rest: "    ".to_string(),
                    style: TextStyle::default(),
                    used: false,
                };
                self.indented(id, seg, 0);
            }
            "a" => self.anchor(id),
            "q" => {
                let g = self.opts.glyphs;
                self.push_word(g.qopen);
                self.walk_children(id);
                self.push_word(g.qclose);
            }
            _ => self.generic(id, tag),
        }
    }

    /// Elemento genérico: aplica su estilo por defecto y se comporta como bloque o en línea.
    fn generic(&mut self, id: NodeId, tag: &str) {
        let saved = (self.style, self.align);
        let code_in_pre = self.pre > 0 && matches!(tag, "code" | "kbd" | "samp" | "tt");
        if !code_in_pre {
            style::apply(tag, &mut self.style);
        }
        if tag == "center" {
            self.align = Align::Center;
        }
        if let Some(a) = self.doc.attr(id, "align").and_then(parse_align) {
            self.align = a;
        }
        match style::display(tag) {
            Display::Block => {
                let (top, bottom) = style::margins(tag);
                self.block_start(top);
                self.content_max = 0;
                self.walk_children(id);
                self.flush();
                if tag == "h1" || tag == "h2" {
                    self.heading_rule(tag);
                }
                self.block_end(bottom);
            }
            _ => self.walk_children(id),
        }
        self.style = saved.0;
        self.align = saved.1;
    }

    /// Bloque con un prefijo en cada línea (cita, sangría de definición).
    fn indented(&mut self, id: NodeId, seg: Seg, margin: usize) {
        self.block_start(margin);
        self.materialize_blanks();
        self.segs.push(seg);
        self.walk_children(id);
        self.flush();
        self.segs.pop();
        self.block_end(margin);
    }

    fn heading_rule(&mut self, tag: &str) {
        let w = self.content_max;
        if w == 0 {
            return;
        }
        let g = if tag == "h1" {
            self.opts.glyphs.h1
        } else {
            self.opts.glyphs.h2
        };
        let rule = Span {
            text: g.repeat(w),
            style: self.style,
        };
        let align = self.align;
        self.emit(vec![rule], align);
    }

    // ----- bloques y márgenes ------------------------------------------------

    fn block_start(&mut self, margin: usize) {
        self.flush();
        self.pending_blank = self.pending_blank.max(margin);
    }

    fn block_end(&mut self, margin: usize) {
        self.flush();
        self.pending_blank = self.pending_blank.max(margin);
    }

    fn prefix_width(&self) -> usize {
        self.segs.iter().map(|s| display_width(&s.rest)).sum()
    }

    /// Prefijo de la siguiente línea con contenido (consume el `first` de cada segmento).
    fn take_prefix(&mut self) -> Line {
        let mut line = Vec::new();
        for s in self.segs.iter_mut() {
            let t = if s.used {
                s.rest.clone()
            } else {
                s.used = true;
                s.first.clone()
            };
            push_span(&mut line, &t, s.style);
        }
        line
    }

    /// Línea vacía que conserva las barras de cita pero no las viñetas.
    fn blank_line(&self) -> Line {
        let mut line: Line = Vec::new();
        for s in &self.segs {
            push_span(&mut line, &s.rest, s.style);
        }
        while let Some(last) = line.last_mut() {
            let keep = last.text.trim_end().len();
            last.text.truncate(keep);
            if last.text.is_empty() {
                line.pop();
            } else {
                break;
            }
        }
        line
    }

    /// Emite ya los márgenes pendientes. Se llama antes de añadir un prefijo (cita, viñeta)
    /// para que el margen quede fuera de él.
    fn materialize_blanks(&mut self) {
        if !self.lines.is_empty() {
            for _ in 0..self.pending_blank {
                let l = self.blank_line();
                self.lines.push(l);
            }
        }
        self.pending_blank = 0;
    }

    /// Añade una línea de contenido a la salida, con márgenes pendientes, prefijo y alineación.
    fn emit(&mut self, spans: Line, align: Align) {
        self.materialize_blanks();
        let mut line = self.take_prefix();
        let pw = line_width(&line);
        let w = line_width(&spans);
        self.content_max = self.content_max.max(w);
        let avail = self.width.saturating_sub(pw);
        if !self.measure && w < avail {
            let pad = match align {
                Align::Left => 0,
                Align::Center => (avail - w) / 2,
                Align::Right => avail - w,
            };
            push_span(&mut line, &" ".repeat(pad), TextStyle::default());
        }
        for s in spans {
            push_span(&mut line, &s.text, s.style);
        }
        self.lines.push(line);
    }

    // ----- texto en línea ----------------------------------------------------

    fn text(&mut self, t: &str) {
        if self.pre > 0 {
            self.pre_text(t);
            return;
        }
        let style = self.style;
        let mut word = String::new();
        for ch in t.chars() {
            if is_html_ws(ch) {
                if !word.is_empty() {
                    self.toks.push(Tok::Word(std::mem::take(&mut word), style));
                }
                if !matches!(self.toks.last(), Some(Tok::Space(_))) {
                    self.toks.push(Tok::Space(style));
                }
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            self.toks.push(Tok::Word(word, style));
        }
    }

    fn pre_text(&mut self, t: &str) {
        let mut t = t;
        if self.pre_first {
            self.pre_first = false;
            t = t.strip_prefix('\n').unwrap_or(t);
        }
        let style = self.style;
        for (i, part) in t.split('\n').enumerate() {
            if i > 0 {
                let l = std::mem::take(&mut self.pre_cur);
                self.emit(l, Align::Left);
            }
            let part = part.trim_end_matches('\r');
            let mut col = line_width(&self.pre_cur);
            let mut buf = String::new();
            for ch in part.chars() {
                if ch == '\t' {
                    let n = 8 - col % 8;
                    buf.push_str(&" ".repeat(n));
                    col += n;
                } else {
                    buf.push(ch);
                    col += char_width(ch);
                }
            }
            push_span(&mut self.pre_cur, &buf, style);
        }
    }

    /// Añade una palabra indivisible con el estilo actual.
    fn push_word(&mut self, s: &str) {
        if self.pre > 0 {
            push_span(&mut self.pre_cur, s, self.style);
        } else {
            self.toks.push(Tok::Word(s.to_string(), self.style));
        }
    }

    fn push_space(&mut self) {
        if self.pre > 0 {
            push_span(&mut self.pre_cur, " ", self.style);
        } else if !matches!(self.toks.last(), Some(Tok::Space(_))) {
            self.toks.push(Tok::Space(self.style));
        }
    }

    /// Cierra el párrafo en curso: ajusta los tokens al ancho disponible y emite las líneas.
    fn flush(&mut self) {
        if !self.pre_cur.is_empty() {
            let l = std::mem::take(&mut self.pre_cur);
            self.emit(l, Align::Left);
        }
        if self.toks.is_empty() {
            return;
        }
        let items = build_items(std::mem::take(&mut self.toks));
        let avail = self.width.saturating_sub(self.prefix_width()).max(1);
        let align = self.align;
        let mut cur: Line = Vec::new();
        let mut cur_w = 0usize;
        let mut pend: Option<TextStyle> = None;

        for item in items {
            match item {
                Item::Space(s) => {
                    if cur_w > 0 {
                        pend = Some(s);
                    }
                }
                Item::Break => {
                    let l = std::mem::take(&mut cur);
                    self.emit(l, align);
                    cur_w = 0;
                    pend = None;
                }
                Item::Unit(u) => {
                    let sp = usize::from(pend.is_some() && cur_w > 0);
                    if cur_w + sp + u.w <= avail {
                        if let (1, Some(s)) = (sp, pend) {
                            push_span(&mut cur, " ", s);
                        }
                        for (t, s) in &u.parts {
                            push_span(&mut cur, t, *s);
                        }
                        cur_w += sp + u.w;
                    } else {
                        if cur_w > 0 {
                            let l = std::mem::take(&mut cur);
                            self.emit(l, align);
                            cur_w = 0;
                        }
                        if u.w <= avail {
                            for (t, s) in &u.parts {
                                push_span(&mut cur, t, *s);
                            }
                            cur_w = u.w;
                        } else {
                            // Palabra más ancha que la línea: se parte por caracteres.
                            for (t, s) in &u.parts {
                                for ch in t.chars() {
                                    let cw = char_width(ch);
                                    if cur_w + cw > avail && cur_w > 0 {
                                        let l = std::mem::take(&mut cur);
                                        self.emit(l, align);
                                        cur_w = 0;
                                    }
                                    let mut buf = [0u8; 4];
                                    push_span(&mut cur, ch.encode_utf8(&mut buf), *s);
                                    cur_w += cw;
                                }
                            }
                        }
                    }
                    pend = None;
                }
            }
        }
        if !cur.is_empty() {
            self.emit(cur, align);
        }
    }

    // ----- elementos concretos -----------------------------------------------

    fn hr(&mut self) {
        self.block_start(0);
        let avail = if self.measure {
            3
        } else {
            self.width.saturating_sub(self.prefix_width()).max(1)
        };
        let text = self.opts.glyphs.hr.repeat(avail);
        let style = TextStyle {
            dim: true,
            ..Default::default()
        };
        self.emit(vec![Span { text, style }], Align::Left);
        self.block_end(0);
    }

    fn img(&mut self, id: NodeId) {
        let alt = self.doc.attr(id, "alt").map(collapse_ws).unwrap_or_default();
        let label = if alt.is_empty() {
            "[imagen]".to_string()
        } else {
            format!("[{alt}]")
        };
        let saved = self.style;
        self.style.dim = true;
        self.style.italic = true;
        self.text(&label);
        self.style = saved;
    }

    fn anchor(&mut self, id: NodeId) {
        let doc = self.doc;
        let href = doc
            .attr(id, "href")
            .map(str::trim)
            .filter(|h| !h.is_empty());
        let saved = self.style;
        if href.is_some() {
            style::apply("a", &mut self.style);
        }
        self.walk_children(id);
        self.style = saved;
        if let Some(h) = href {
            let lower = h.to_ascii_lowercase();
            if !h.starts_with('#') && !lower.starts_with("javascript:") {
                let n = self.link_offset + self.links.len() + 1;
                self.links.push(Link {
                    href: h.to_string(),
                });
                let marker = TextStyle {
                    dim: true,
                    ..Default::default()
                };
                let text = format!("[{n}]");
                if self.pre > 0 {
                    push_span(&mut self.pre_cur, &text, marker);
                } else {
                    self.toks.push(Tok::Word(text, marker));
                }
            }
        }
    }

    fn pre_block(&mut self, id: NodeId) {
        self.block_start(1);
        self.pre += 1;
        self.pre_first = true;
        self.walk_children(id);
        self.flush();
        self.pre -= 1;
        self.block_end(1);
    }

    fn list(&mut self, id: NodeId, ordered: bool) {
        let margin = usize::from(self.lists.is_empty());
        self.block_start(margin);
        let start = if ordered {
            self.doc
                .attr(id, "start")
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(1)
        } else {
            1
        };
        self.lists.push(ListCtx {
            ordered,
            next: start,
        });
        self.walk_children(id);
        self.flush();
        self.lists.pop();
        self.block_end(margin);
    }

    fn list_item(&mut self, id: NodeId) {
        self.block_start(0);
        let depth = self.lists.len().saturating_sub(1) % 3;
        let bullet = self.opts.glyphs.bullets[depth];
        let marker = match self.lists.last_mut() {
            Some(l) if l.ordered => {
                let n = l.next;
                l.next += 1;
                format!("{n}. ")
            }
            _ => format!("{bullet} "),
        };
        let rest = " ".repeat(display_width(&marker));
        self.materialize_blanks();
        self.segs.push(Seg {
            first: marker,
            rest,
            style: TextStyle::default(),
            used: false,
        });
        self.walk_children(id);
        self.flush();
        if self.segs.last().is_some_and(|s| !s.used) {
            // <li> vacío: se muestra igualmente su viñeta.
            self.emit(Vec::new(), Align::Left);
        }
        self.segs.pop();
        self.block_end(0);
    }

    // ----- controles de formulario (solo representación; sin interacción todavía) ---

    /// Campo de texto como una palabra indivisible: `[contenido____]`.
    fn field(&mut self, content: &str, placeholder: bool, min: usize) {
        let pad = min.saturating_sub(display_width(content));
        let inner = format!("{}{}", content.replace(' ', "\u{a0}"), "_".repeat(pad));
        let saved = self.style;
        if placeholder {
            self.style.dim = true;
        }
        self.push_word(&format!("[{inner}]"));
        self.style = saved;
    }

    fn input(&mut self, id: NodeId) {
        let doc = self.doc;
        let ty = doc.attr(id, "type").unwrap_or("text").to_ascii_lowercase();
        let g = self.opts.glyphs;
        match ty.as_str() {
            "hidden" => {}
            "submit" | "button" | "reset" | "image" => {
                let default = match ty.as_str() {
                    "submit" => "Enviar",
                    "reset" => "Restablecer",
                    _ => "Botón",
                };
                let label = doc
                    .attr(id, "value")
                    .map(collapse_ws)
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| default.to_string());
                let saved = self.style;
                self.style.reverse = true;
                self.push_word(&format!("[ {label} ]").replace(' ', "\u{a0}"));
                self.style = saved;
            }
            "checkbox" => {
                let on = doc.attr(id, "checked").is_some();
                self.push_word(if on { "[x]" } else { "[ ]" }.replace(' ', "\u{a0}").as_str());
            }
            "radio" => {
                let on = doc.attr(id, "checked").is_some();
                let s = if on { g.radio_on } else { g.radio_off };
                self.push_word(&s.replace(' ', "\u{a0}"));
            }
            _ => {
                let value = doc.attr(id, "value").map(collapse_ws).unwrap_or_default();
                if !value.is_empty() {
                    let shown = if ty == "password" {
                        "*".repeat(value.chars().count())
                    } else {
                        value
                    };
                    self.field(&shown, false, 12);
                } else {
                    let ph = doc
                        .attr(id, "placeholder")
                        .map(collapse_ws)
                        .unwrap_or_default();
                    self.field(&ph, true, 12);
                }
            }
        }
    }

    fn textarea(&mut self, id: NodeId) {
        let doc = self.doc;
        let content = collapse_ws(&doc.text_content(id));
        if content.is_empty() {
            let ph = doc
                .attr(id, "placeholder")
                .map(collapse_ws)
                .unwrap_or_default();
            self.field(&ph, true, 20);
        } else {
            self.field(&content, false, 20);
        }
    }

    fn select(&mut self, id: NodeId) {
        let doc = self.doc;
        let mut options = Vec::new();
        collect_options(doc, id, &mut options);
        let chosen = options
            .iter()
            .find(|&&o| doc.attr(o, "selected").is_some())
            .or(options.first())
            .copied();
        let label = chosen
            .map(|o| collapse_ws(&doc.text_content(o)))
            .unwrap_or_default();
        let g = self.opts.glyphs;
        self.push_word(&format!("[{}\u{a0}{}]", label.replace(' ', "\u{a0}"), g.dropdown));
    }

    fn button(&mut self, id: NodeId) {
        let saved = self.style;
        self.style.reverse = true;
        self.push_word("[");
        self.push_space();
        self.walk_children(id);
        self.push_space();
        self.push_word("]");
        self.style = saved;
    }

    // ----- tablas ------------------------------------------------------------

    /// Renderiza el contenido de una celda con un ancho dado en un layout independiente.
    fn render_cell(&self, c: &CellInfo, w: usize, measure: bool) -> (Vec<Line>, Vec<Link>) {
        let mut sub = Layout::new(self.doc, self.opts, w);
        sub.measure = measure || self.measure;
        sub.link_offset = self.link_offset + self.links.len();
        sub.style = self.style;
        if c.header {
            style::apply("th", &mut sub.style);
        }
        sub.walk_children(c.id);
        sub.flush();
        (sub.lines, sub.links)
    }

    fn table(&mut self, id: NodeId) {
        let doc = self.doc;
        let g = self.opts.glyphs;
        let mut rows: Vec<Row> = Vec::new();
        let mut caption: Option<NodeId> = None;
        collect_table(doc, id, &mut rows, &mut caption, false);

        self.block_start(1);
        if let Some(c) = caption {
            let saved = (self.style, self.align);
            self.align = Align::Center;
            self.walk_children(c);
            self.flush();
            self.style = saved.0;
            self.align = saved.1;
        }
        let ncols = rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.colspan).sum::<usize>())
            .max()
            .unwrap_or(0);
        if ncols == 0 {
            self.block_end(1);
            return;
        }

        // 1) Anchos naturales (máximo) y mínimos (palabra más larga) por columna.
        let mut nat = vec![0usize; ncols];
        let mut minw = vec![0usize; ncols];
        for r in &rows {
            let mut col = 0;
            for c in &r.cells {
                let n = c.colspan;
                let (lines, _) = self.render_cell(c, 4000, true);
                let nw = lines.iter().map(line_width).max().unwrap_or(0);
                let mw = longest_word(doc, c.id);
                let gap = 3 * (n - 1);
                let share_n = nw.saturating_sub(gap).div_ceil(n);
                let share_m = mw.saturating_sub(gap).div_ceil(n);
                for k in col..col + n {
                    nat[k] = nat[k].max(share_n);
                    minw[k] = minw[k].max(share_m);
                }
                col += n;
            }
        }

        // 2) Reparto del ancho disponible (cada columna añade 3 celdas de borde/relleno, más 1 final).
        let overhead = ncols * 3 + 1;
        let avail = self
            .width
            .saturating_sub(self.prefix_width())
            .saturating_sub(overhead);
        let widths = distribute(&nat, &minw, avail);

        // 3) Construcción de las líneas de la tabla.
        let border_style = TextStyle {
            dim: true,
            ..Default::default()
        };
        let border = |l: &str, m: &str, r: &str| -> Line {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    s.push_str(m);
                }
                s.push_str(&g.h.repeat(w + 2));
            }
            s.push_str(r);
            vec![Span {
                text: s,
                style: border_style,
            }]
        };

        let mut out: Vec<Line> = vec![border(g.tl, g.tm, g.tr)];
        for (ri, row) in rows.iter().enumerate() {
            let mut cells: Vec<(Vec<Line>, usize, Align)> = Vec::new();
            let mut col = 0;
            for c in &row.cells {
                let n = c.colspan;
                let w: usize = widths[col..col + n].iter().sum::<usize>() + 3 * (n - 1);
                let (lines, links) = self.render_cell(c, w, false);
                self.links.extend(links);
                cells.push((lines, w, c.align));
                col += n;
            }
            while col < ncols {
                cells.push((Vec::new(), widths[col], Align::Left));
                col += 1;
            }
            let height = cells.iter().map(|(l, _, _)| l.len()).max().unwrap_or(1).max(1);
            for h in 0..height {
                let mut line: Line = Vec::new();
                push_span(&mut line, g.v, border_style);
                for (lines, w, al) in &cells {
                    push_span(&mut line, " ", TextStyle::default());
                    let content = lines.get(h);
                    let cw = content.map(line_width).unwrap_or(0);
                    let pad = w.saturating_sub(cw);
                    let (l, r) = match al {
                        Align::Left => (0, pad),
                        Align::Right => (pad, 0),
                        Align::Center => (pad / 2, pad - pad / 2),
                    };
                    push_span(&mut line, &" ".repeat(l), TextStyle::default());
                    if let Some(c) = content {
                        for s in c {
                            push_span(&mut line, &s.text, s.style);
                        }
                    }
                    push_span(&mut line, &" ".repeat(r + 1), TextStyle::default());
                    push_span(&mut line, g.v, border_style);
                }
                out.push(line);
            }
            let next_is_body = rows.get(ri + 1).is_some_and(|n| !n.header);
            if row.header && next_is_body {
                out.push(border(g.lm, g.x, g.rm));
            }
        }
        out.push(border(g.bl, g.bm, g.br));

        let align = self.align;
        for l in out {
            self.emit(l, align);
        }
        self.block_end(1);
    }
}

// ---------------------------------------------------------------------------
// Tablas: estructura y reparto de anchos
// ---------------------------------------------------------------------------

struct CellInfo {
    id: NodeId,
    colspan: usize,
    align: Align,
    header: bool,
}

struct Row {
    cells: Vec<CellInfo>,
    header: bool,
}

fn collect_table(
    doc: &Document,
    id: NodeId,
    rows: &mut Vec<Row>,
    caption: &mut Option<NodeId>,
    in_head: bool,
) {
    for &c in &doc.node(id).children {
        let Some(tag) = doc.tag(c) else { continue };
        if is_hidden(doc, c, tag) {
            continue;
        }
        match tag {
            "caption" => {
                if caption.is_none() {
                    *caption = Some(c);
                }
            }
            "thead" => collect_table(doc, c, rows, caption, true),
            "tbody" | "tfoot" => collect_table(doc, c, rows, caption, in_head),
            "tr" => {
                let mut cells = Vec::new();
                for &d in &doc.node(c).children {
                    let Some(t) = doc.tag(d) else { continue };
                    if (t != "td" && t != "th") || is_hidden(doc, d, t) {
                        continue;
                    }
                    let colspan = doc
                        .attr(d, "colspan")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 50);
                    let default_align = if t == "th" { Align::Center } else { Align::Left };
                    let align = doc
                        .attr(d, "align")
                        .and_then(parse_align)
                        .unwrap_or(default_align);
                    cells.push(CellInfo {
                        id: d,
                        colspan,
                        align,
                        header: t == "th",
                    });
                }
                if !cells.is_empty() {
                    let header = in_head || cells.iter().all(|c| c.header);
                    rows.push(Row { cells, header });
                }
            }
            _ => {}
        }
    }
}

/// Reparte `avail` celdas entre columnas con ancho natural `nat` y mínimo `min`.
fn distribute(nat: &[usize], min: &[usize], avail: usize) -> Vec<usize> {
    let n = nat.len();
    let nat: Vec<usize> = nat.iter().map(|&w| w.max(1)).collect();
    let min: Vec<usize> = min
        .iter()
        .zip(&nat)
        .map(|(&m, &w)| m.max(1).min(w))
        .collect();

    if nat.iter().sum::<usize>() <= avail {
        return nat;
    }
    let total_min: usize = min.iter().sum();
    if total_min >= avail {
        // Ni las palabras más largas caben: se recortan las columnas más anchas.
        let mut w = min;
        let mut total = total_min;
        while total > avail.max(n) {
            let idx = (0..n).max_by_key(|&i| w[i]).unwrap();
            if w[idx] <= 1 {
                break;
            }
            w[idx] -= 1;
            total -= 1;
        }
        return w;
    }
    let extra = avail - total_min;
    let flex: Vec<usize> = nat.iter().zip(&min).map(|(a, b)| a - b).collect();
    let total_flex: usize = flex.iter().sum();
    let mut w: Vec<usize> = min
        .iter()
        .zip(&flex)
        .map(|(m, f)| m + extra * f / total_flex)
        .collect();
    let mut left = avail - w.iter().sum::<usize>();
    while left > 0 {
        let mut moved = false;
        for i in 0..n {
            if left == 0 {
                break;
            }
            if w[i] < nat[i] {
                w[i] += 1;
                left -= 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    w
}

// ---------------------------------------------------------------------------
// Auxiliares sobre el DOM
// ---------------------------------------------------------------------------

/// Elementos que no se muestran: `display: none` por defecto, atributo `hidden`,
/// o un `style="display:none"` en línea (hasta que exista el motor de CSS).
fn is_hidden(doc: &Document, id: NodeId, tag: &str) -> bool {
    if style::display(tag) == Display::None {
        return true;
    }
    if doc.attr(id, "hidden").is_some() {
        return true;
    }
    if let Some(s) = doc.attr(id, "style") {
        let compact: String = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if compact.contains("display:none") {
            return true;
        }
    }
    false
}

fn visible_text(doc: &Document, id: NodeId, out: &mut String) {
    match &doc.node(id).kind {
        NodeKind::Text(t) => out.push_str(t),
        NodeKind::Element { tag, .. } => {
            if is_hidden(doc, id, tag) {
                return;
            }
            for &c in &doc.node(id).children {
                visible_text(doc, c, out);
            }
        }
        NodeKind::Document => {}
    }
}

/// Anchura de la palabra más larga del contenido visible (ancho mínimo de una celda).
fn longest_word(doc: &Document, id: NodeId) -> usize {
    let mut s = String::new();
    visible_text(doc, id, &mut s);
    s.split(is_html_ws).map(display_width).max().unwrap_or(0)
}

fn collect_options(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
    for &c in &doc.node(id).children {
        match doc.tag(c) {
            Some("option") => out.push(c),
            Some("optgroup") => collect_options(doc, c, out),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Pruebas
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn lines_of(html: &str, width: usize) -> Vec<String> {
        let doc = parse(html);
        let page = render(&doc, &Options::new(width, true));
        to_ansi(&page.lines, false)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn text_of(html: &str, width: usize) -> String {
        lines_of(html, width).join("\n")
    }

    #[test]
    fn wraps_to_width() {
        let html = "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor</p>";
        for l in lines_of(html, 20) {
            assert!(display_width(&l) <= 20, "línea demasiado ancha: {l:?}");
        }
    }

    #[test]
    fn collapses_whitespace_and_keeps_inline_glue() {
        assert_eq!(text_of("<p>a \n\n   b  <b>ho</b>la</p>", 40), "a b hola");
    }

    #[test]
    fn long_word_is_split() {
        let l = lines_of("<p>aaaaaaaaaaaaaaaaaaaaaaaaa</p>", 10);
        assert_eq!(l.len(), 3);
        assert!(l.iter().all(|x| display_width(x) <= 10));
    }

    #[test]
    fn paragraphs_are_separated_by_one_blank_line() {
        assert_eq!(text_of("<p>uno</p><p>dos</p>", 40), "uno\n\ndos");
    }

    #[test]
    fn br_breaks_lines() {
        assert_eq!(text_of("<p>a<br>b<br><br>c</p>", 40), "a\nb\n\nc");
    }

    #[test]
    fn unordered_and_ordered_lists() {
        assert_eq!(text_of("<ul><li>a<li>b</ul>", 40), "• a\n• b");
        assert_eq!(text_of("<ol><li>x<li>y</ol>", 40), "1. x\n2. y");
        assert_eq!(text_of("<ol start=5><li>x</ol>", 40), "5. x");
    }

    #[test]
    fn nested_list_is_indented() {
        let t = text_of("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>", 40);
        assert_eq!(t, "• a\n  ◦ b\n• c");
    }

    #[test]
    fn list_item_continuation_is_aligned() {
        let l = lines_of("<ul><li>uno dos tres cuatro</li></ul>", 10);
        assert_eq!(l, ["• uno dos", "  tres", "  cuatro"]);
    }

    #[test]
    fn blockquote_prefix() {
        assert_eq!(text_of("<blockquote>hola</blockquote>", 40), "│ hola");
    }

    #[test]
    fn blockquote_margin_is_outside_the_bar() {
        let t = text_of("<p>antes</p><blockquote>cita</blockquote><p>después</p>", 40);
        assert_eq!(t, "antes\n\n│ cita\n\ndespués");
    }

    #[test]
    fn blockquote_with_paragraphs_keeps_the_bar_between_them() {
        let t = text_of("<blockquote><p>uno</p><p>dos</p></blockquote>", 40);
        assert_eq!(t, "│ uno\n│\n│ dos");
    }

    #[test]
    fn pre_preserves_whitespace() {
        assert_eq!(text_of("<pre>\na  b\n  c\n</pre>", 40), "a  b\n  c");
    }

    #[test]
    fn hidden_content_is_not_shown() {
        let t = text_of(
            "<head><title>T</title><style>p{}</style></head><body><script>x()</script><p hidden>no</p><p style='display: none'>no</p>si</body>",
            40,
        );
        assert!(t.contains("si"));
        assert!(!t.contains("no"));
        assert!(!t.contains("x()"));
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(text_of("<p>&lt;b&gt; &amp; &#65; &nbsp;x</p>", 40), "<b> & A  x");
    }

    #[test]
    fn links_are_numbered_and_listed() {
        let t = text_of("<p><a href='http://x.org'>hola</a> y <a href='#a'>ancla</a></p>", 40);
        assert!(t.starts_with("hola[1] y ancla"));
        assert!(t.contains("Enlaces:"));
        assert!(t.contains("[1] http://x.org"));
        assert!(!t.contains("[2]"));
    }

    #[test]
    fn headings_get_rules() {
        let t = text_of("<h1>Hola</h1>", 40);
        assert_eq!(t, "Hola\n════");
    }

    #[test]
    fn center_alignment() {
        assert_eq!(text_of("<center>ab</center>", 10), "    ab");
        assert_eq!(text_of("<p align=right>ab</p>", 10), "        ab");
    }

    #[test]
    fn title_bar_is_rendered() {
        let t = text_of("<title>Mi página</title><p>x</p>", 20);
        assert!(t.starts_with(" Mi página"));
    }

    #[test]
    fn table_lines_have_equal_width() {
        let html = "<table><tr><th>Nombre<th>Edad<tr><td>Ana<td>30<tr><td>Luis Alberto<td>5</table>";
        let l = lines_of(html, 40);
        let w = display_width(&l[0]);
        assert!(l.iter().all(|x| display_width(x) == w), "{l:#?}");
        assert!(l[0].starts_with('┌') && l.last().unwrap().starts_with('└'));
    }

    #[test]
    fn table_shrinks_to_width() {
        let html = "<table><tr><td>una frase bastante larga para forzar el ajuste<td>otra celda con bastante texto también</table>";
        for l in lines_of(html, 30) {
            assert!(display_width(&l) <= 30, "{l:?}");
        }
    }

    #[test]
    fn table_with_colspan() {
        let html = "<table><tr><td colspan=2>cabecera ancha<tr><td>a<td>b</table>";
        let l = lines_of(html, 40);
        let w = display_width(&l[0]);
        assert!(l.iter().all(|x| display_width(x) == w), "{l:#?}");
    }

    #[test]
    fn ascii_mode_has_no_unicode_boxes() {
        let doc = parse("<ul><li>a</ul><table><tr><td>x</table><hr>");
        let page = render(&doc, &Options::new(20, false));
        let t = to_ansi(&page.lines, false);
        assert!(t.is_ascii(), "{t}");
    }

    #[test]
    fn form_controls() {
        let t = text_of(
            "<input type=checkbox checked> <input type=radio> <input type=submit value='Ir'> <input placeholder='tu nombre'>",
            80,
        );
        assert!(t.contains("[x]"));
        assert!(t.contains("( )"));
        assert!(t.contains("[ Ir ]"));
        assert!(t.contains("[tu nombre"));
    }

    #[test]
    fn distribute_respects_bounds() {
        let w = distribute(&[40, 40], &[5, 5], 30);
        assert_eq!(w.iter().sum::<usize>(), 30);
        let w = distribute(&[10, 10], &[5, 5], 100);
        assert_eq!(w, [10, 10]);
        let w = distribute(&[30, 30], &[20, 20], 10);
        assert!(w.iter().all(|&x| x >= 1));
    }

    #[test]
    fn empty_and_garbage_input() {
        assert_eq!(text_of("", 40), "");
        assert_eq!(text_of("   \n  ", 40), "");
        let _ = text_of("<table><tr></tr></table><ul></ul><ol><li></ol>", 40);
    }
}
