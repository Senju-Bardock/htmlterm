//! htmlterm: visor de HTML para terminal.
//!
//! Pipeline del hito 1:
//!
//! ```text
//! texto HTML --parser--> Document (DOM en arena) --render--> líneas con estilo --ansi--> terminal
//! ```
//!
//! Cada fase vive en su propio módulo para poder sustituirla (por ejemplo, el parser
//! por `html5ever`) sin tocar las demás.

pub mod dom;
pub mod parser;
pub mod render;
pub mod style;
pub mod text;

#[cfg(windows)]
pub mod winconsole;
