use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use htmlterm::{parser, render, text};

const HELP: &str = "\
htmlterm - visor de HTML para terminal

USO:
    htmlterm [OPCIONES] <archivo.html>
    htmlterm [OPCIONES] -          (lee el HTML de la entrada estándar)

OPCIONES:
    -w, --width <N>   Ancho en columnas (por defecto, el de la terminal; 80 si no se puede saber)
        --ascii       Usa solo caracteres ASCII (viñetas y bordes) para consolas antiguas
        --color       Fuerza los colores aunque la salida no sea una terminal
        --no-color    Desactiva los colores y estilos
    -h, --help        Muestra esta ayuda
    -V, --version     Muestra la versión

Para páginas largas:  htmlterm pagina.html | less -R
";

#[derive(Clone, Copy, PartialEq)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[cfg(windows)]
fn enable_ansi() -> bool {
    htmlterm::winconsole::enable_ansi()
}

#[cfg(not(windows))]
fn enable_ansi() -> bool {
    true
}

/// Ancho de la terminal, si se puede averiguar.
#[cfg(windows)]
fn terminal_width() -> Option<usize> {
    // Se deja una columna libre: la consola clásica de Windows duplica el salto de línea
    // cuando una línea llena exactamente el ancho.
    htmlterm::winconsole::width().map(|w| w.saturating_sub(1))
}

#[cfg(not(windows))]
fn terminal_width() -> Option<usize> {
    use std::process::{Command, Stdio};
    if let Some(n) = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
    {
        return Some(n);
    }
    let out = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.split_whitespace()
        .nth(1)?
        .parse::<usize>()
        .ok()
        .filter(|&n| n > 0)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("htmlterm: {msg}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut path: Option<String> = None;
    let mut width: Option<usize> = None;
    let mut ascii = false;
    let mut color = ColorMode::Auto;

    let parse_width = |v: Option<&str>| -> Option<usize> {
        v.and_then(|v| v.parse::<usize>().ok()).filter(|&n| n >= 10)
    };

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("htmlterm {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-w" | "--width" => {
                i += 1;
                match parse_width(args.get(i).map(String::as_str)) {
                    Some(n) => width = Some(n),
                    None => return fail("--width necesita un número (mínimo 10)"),
                }
            }
            "--ascii" => ascii = true,
            "--color" => color = ColorMode::Always,
            "--no-color" => color = ColorMode::Never,
            "-" => path = Some("-".to_string()),
            _ if a.starts_with("--width=") => match parse_width(Some(&a["--width=".len()..])) {
                Some(n) => width = Some(n),
                None => return fail("--width necesita un número (mínimo 10)"),
            },
            _ if a.starts_with('-') => return fail(&format!("opción desconocida: {a} (prueba --help)")),
            _ => {
                if path.is_some() {
                    return fail("solo se admite un archivo de entrada");
                }
                path = Some(a.to_string());
            }
        }
        i += 1;
    }

    // Entrada.
    let bytes = match path.as_deref() {
        Some(p) if p != "-" => match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("htmlterm: no se puede leer '{p}': {e}");
                return ExitCode::FAILURE;
            }
        },
        Some(_) => read_stdin(),
        None if !std::io::stdin().is_terminal() => read_stdin(),
        None => {
            eprint!("{HELP}");
            return ExitCode::from(2);
        }
    };

    // Salida: color y ancho.
    let ansi_ok = enable_ansi();
    let use_color = match color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            std::io::stdout().is_terminal()
                && ansi_ok
                && std::env::var_os("NO_COLOR").is_none()
        }
    };
    let width = width.or_else(terminal_width).unwrap_or(80).max(10);
    let unicode = !ascii;
    let html = text::decode_bytes(&bytes);

    // El renderizado es recursivo: se ejecuta en un hilo con pila grande para soportar
    // documentos con anidamiento muy profundo.
    let worker = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let doc = parser::parse(&html);
            let page = render::render(&doc, &render::Options::new(width, unicode));
            render::to_ansi(&page.lines, use_color)
        });
    let output = match worker.map(|h| h.join()) {
        Ok(Ok(s)) => s,
        _ => {
            eprintln!("htmlterm: error interno al renderizar la página");
            return ExitCode::FAILURE;
        }
    };

    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(output.as_bytes()).and_then(|_| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        // Salida cerrada (p. ej. `| head`): no es un error.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("htmlterm: error al escribir: {e}");
            ExitCode::FAILURE
        }
    }
}

fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    buf
}
