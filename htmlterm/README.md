# htmlterm

Visor de HTML para terminal: muestra una página HTML en una consola de Windows o Linux
sin interfaz gráfica. Un único ejecutable, sin dependencias externas.

> **Estado: hito 1 de 5.** Renderiza HTML con los estilos por defecto de cada etiqueta.
> Todavía no interpreta CSS ni JavaScript (ver [Hoja de ruta](#hoja-de-ruta)).

## Compilar

Necesitas [Rust](https://rustup.rs) 1.88 o superior (el proyecto usa la edición 2024).

```text
cargo build --release
```

El ejecutable queda en `target/release/htmlterm` (`htmlterm.exe` en Windows, unos 500 KB).

```text
cargo test        # 40 pruebas automáticas
```

## Uso

```text
htmlterm pagina.html
htmlterm -w 100 pagina.html       # ancho fijo en columnas
htmlterm --ascii pagina.html      # solo ASCII: viñetas y bordes para consolas antiguas
htmlterm --no-color pagina.html
curl -s https://example.com | htmlterm -
htmlterm pagina.html | less -R    # páginas largas (hasta que exista el desplazamiento propio)
```

Pruébalo con `examples/demo.html`.

## Qué soporta ahora

| Área | Soporte |
|---|---|
| Texto | párrafos, títulos (h1/h2 con regla), negrita, cursiva, subrayado, tachado, `code`, `mark`, `small`, `<br>`, `<hr>`, `<center>` y `align` |
| Listas | `ul`/`ol` anidadas (viñetas por nivel, atributo `start`), `dl`/`dt`/`dd` |
| Bloques | `blockquote` (barra lateral), `pre` (conserva espacios, sin ajuste) |
| Tablas | cabeceras, `caption`, `colspan`, alineación, anchos automáticos, tablas anidadas (no hay `rowspan`) |
| Enlaces | subrayados, numerados `[n]` y listados al final |
| Imágenes | se muestra el texto `alt` (la integración con `imgterm` llegará en el hito 4) |
| Formularios | representación estática de `input`, `select`, `textarea`, `button` |
| Ocultos | `<head>`, `<script>`, `<style>`, atributo `hidden`, `style="display:none"` |
| Entidades | nombradas y numéricas |
| Codificación | UTF-8; si no es válido, Windows-1252/Latin-1 |
| Consola | Windows: activa ANSI y UTF-8 y detecta el ancho; Linux: usa `$COLUMNS` o `stty size` |

El parser es tolerante: cierra implícitamente `p`, `li`, `dt`/`dd`, `tr`/`td`, `option`...,
ignora etiquetas sueltas y limita el anidamiento a 512 niveles, como los navegadores.

## Estructura

```text
HTML --parser--> Document (DOM en arena) --render--> líneas con estilo --to_ansi--> terminal
```

| Archivo | Responsabilidad |
|---|---|
| `src/parser.rs` | tokenizador y constructor de árbol, entidades, cierres implícitos |
| `src/dom.rs` | `Document`: nodos en un `Vec`, referenciados por índice (`NodeId`) |
| `src/style.rs` | color, atributos de texto y estilos por defecto de cada etiqueta |
| `src/render.rs` | layout sobre cuadrícula de caracteres: ajuste de línea, márgenes, listas, tablas |
| `src/text.rs` | anchura en celdas, espacios HTML, decodificación de bytes |
| `src/winconsole.rs` | consola de Windows (ANSI, UTF-8, ancho) por FFI directo a `kernel32` |
| `src/main.rs` | línea de comandos |

Cada fase se puede sustituir sin tocar las demás. Por ejemplo, para cambiar el parser por
`html5ever` basta con que produzca un `Document` (`parser::parse(&str) -> Document`).

## Hoja de ruta

1. **HTML → DOM → terminal** (hecho).
2. **Motor de layout**: modelo de cajas completo (ancho, padding, margin, bordes) sobre celdas.
3. **CSS**: selectores y cascada (`cssparser` + `selectors`, o propio), y las propiedades con
   sentido en una terminal: `color`, `background-color`, `font-weight`, `text-decoration`,
   `display`, `margin`/`padding`, `width`, `text-align`, `border`. Flexbox/grid simplificados al final.
4. **Interacción**: desplazamiento con teclado, saltar entre enlaces, abrir enlaces, formularios
   y imágenes (integrando `imgterm`).
5. **JavaScript**: embeber un motor (QuickJS vía `rquickjs`, o `boa_engine`) y exponerle un DOM propio
   (`querySelector`, `addEventListener`, `innerHTML`, `textContent`, `setTimeout`...). Cada cambio
   del DOM provoca un nuevo layout y repintado.

## Limitaciones conocidas

* Sin CSS ni JavaScript todavía: solo cuentan los estilos por defecto de cada etiqueta.
* No se aplica el algoritmo completo de construcción de árbol de HTML5 (por ejemplo, las
  etiquetas de formato mal anidadas, `<b><i></b></i>`, no se "reconstruyen" como en un navegador).
* La anchura de caracteres es una aproximación (CJK y emojis anchos); se puede cambiar por el crate `unicode-width`.
* El módulo de Windows (`winconsole.rs`) compila y pasa la comprobación de tipos, pero **no se ha
  probado en una consola de Windows real**.
