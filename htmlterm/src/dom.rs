//! DOM en arena: todos los nodos viven en un `Vec` y se referencian por índice.
//!
//! Evita las referencias cruzadas entre nodos (padre/hijos) que complican el borrow
//! checker, y facilita luego invalidar y recalcular el layout cuando el JS modifique el DOM.

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub nodes: Vec<Node>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// El nodo raíz siempre es el índice 0.
    pub const ROOT: NodeId = 0;

    pub fn new() -> Self {
        Document {
            nodes: vec![Node {
                kind: NodeKind::Document,
                parent: None,
                children: Vec::new(),
            }],
        }
    }

    /// Añade un nodo como último hijo de `parent` y devuelve su id.
    pub fn add(&mut self, parent: NodeId, kind: NodeKind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        id
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// Nombre de la etiqueta (en minúsculas) si el nodo es un elemento.
    pub fn tag(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].kind {
            NodeKind::Element { tag, .. } => Some(tag.as_str()),
            _ => None,
        }
    }

    /// Valor de un atributo (el nombre debe ir en minúsculas).
    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            NodeKind::Element { attrs, .. } => attrs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    /// Texto concatenado de todos los descendientes de texto.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match &self.nodes[id].kind {
            NodeKind::Text(t) => out.push_str(t),
            _ => {
                for &c in &self.nodes[id].children {
                    self.collect_text(c, out);
                }
            }
        }
    }

    /// Primer elemento con esa etiqueta, en orden de documento.
    pub fn find_first(&self, tag: &str) -> Option<NodeId> {
        // Los nodos se crean en orden de aparición, así que un barrido lineal basta.
        (1..self.nodes.len()).find(|&i| self.tag(i) == Some(tag))
    }
}
