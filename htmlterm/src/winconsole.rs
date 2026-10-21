//! Soporte mínimo de la consola de Windows, sin dependencias externas (FFI directo a kernel32).
//!
//! * activa el procesamiento de secuencias ANSI (colores/estilos) en la consola;
//! * cambia la página de códigos de salida a UTF-8;
//! * lee el ancho de la ventana de la consola.
#![allow(non_snake_case, non_camel_case_types, dead_code)]

type HANDLE = isize;

const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
const CP_UTF8: u32 = 65001;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct COORD {
    x: i16,
    y: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SMALL_RECT {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CONSOLE_SCREEN_BUFFER_INFO {
    size: COORD,
    cursor: COORD,
    attributes: u16,
    window: SMALL_RECT,
    max_size: COORD,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(n_std_handle: u32) -> HANDLE;
    fn GetConsoleMode(h: HANDLE, mode: *mut u32) -> i32;
    fn SetConsoleMode(h: HANDLE, mode: u32) -> i32;
    fn SetConsoleOutputCP(code_page: u32) -> i32;
    fn GetConsoleScreenBufferInfo(h: HANDLE, info: *mut CONSOLE_SCREEN_BUFFER_INFO) -> i32;
}

/// Activa ANSI y UTF-8 en la consola. Devuelve `false` si la salida no es una consola
/// o la consola no soporta secuencias de escape (Windows anterior a 10).
pub fn enable_ansi() -> bool {
    unsafe {
        SetConsoleOutputCP(CP_UTF8);
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: u32 = 0;
        if GetConsoleMode(h, &mut mode) == 0 {
            return false;
        }
        SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

/// Ancho de la ventana de la consola en columnas.
pub fn width() -> Option<usize> {
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        if GetConsoleScreenBufferInfo(h, &mut info) == 0 {
            return None;
        }
        let w = info.window.right as i32 - info.window.left as i32 + 1;
        if w > 0 { Some(w as usize) } else { None }
    }
}
