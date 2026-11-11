# Avisos de terceros

Botox Optimizer incluye software y fuentes de terceros. Este archivo lista lo que va **dentro del `.exe`** y sus licencias. Hay que distribuirlo junto con el `.exe` (por ejemplo, adjunto en cada release o accesible desde el repo público de versiones).

Lista generada con `cargo metadata` sobre las dependencias que se compilan en el `.exe` para `x86_64-pc-windows-gnu` (156 crates). Para regenerarla, ver el final del archivo.

## Fuentes incrustadas

### Poppins (Regular, Medium, SemiBold, Bold)

`rust/assets/Poppins-*.ttf`, incrustadas con `include_bytes!` en `rust/src/theme.rs`. Licencia: **SIL Open Font License 1.1**. El texto completo está al final de este archivo.

### Fuentes por defecto de egui (`epaint_default_fonts`)

Vienen con la feature `default_fonts` de `eframe`:

- Ubuntu-Light: Ubuntu Font Licence 1.0.
- Hack: MIT y Bitstream Vera.
- Noto Emoji: SIL OFL 1.1.
- emoji-icon-font: SIL OFL 1.1 y MIT.

Ver https://github.com/emilk/egui/tree/master/crates/epaint_default_fonts.

### Íconos Phosphor (`egui-phosphor`)

`egui-phosphor` (MIT OR Apache-2.0) incluye los íconos Phosphor (MIT, https://phosphoricons.com).

## Dependencias directas

| Crate | Versión | Licencia |
|---|---|---|
| eframe / egui | 0.31.1 | MIT OR Apache-2.0 |
| egui-phosphor | 0.9.0 | MIT OR Apache-2.0 |
| image | 0.25.10 | MIT OR Apache-2.0 |
| rfd | 0.15.4 | MIT |
| wmi | 0.18.4 | MIT OR Apache-2.0 |
| arboard | 3.7.0 | MIT OR Apache-2.0 |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| winreg | 0.55.0 | MIT |

## Licencias de todas las dependencias transitivas

| Cantidad | Licencia | Notas |
|---:|---|---|
| 93 | MIT OR Apache-2.0 (y variantes "Apache-2.0 OR MIT", "MIT/Apache-2.0") | la mayoría |
| 18 | Unicode-3.0 | datos ICU (`icu_*`, `zerovec`, etc.) |
| 14 | MIT | |
| 7 | Apache-2.0 | `ab_glyph`, `owned_ttf_parser`, `ab_glyph_rasterizer`, `winit`, `glutin`, `glutin_wgl_sys`, `glutin_egl_sys` |
| 5 | MIT OR Apache-2.0 OR Zlib | `raw-window-handle`, `zune-jpeg`, `zune-core`, `cursor-icon`, `glow` |
| 2 | Unlicense OR MIT | `memchr`, `byteorder-lite` |
| 2 | Zlib | `foldhash`, `zlib-rs` |
| 2 | BSD-2-Clause OR Apache-2.0 OR MIT | `zerocopy` |
| 2 | MIT OR Zlib OR Apache-2.0 | `miniz_oxide` |
| 2 | BSD-3-Clause OR Apache-2.0 | `moxcms`, `pxfm` |
| 2 | Zlib OR Apache-2.0 OR MIT | `bytemuck` |
| 2 | BSL-1.0 | `clipboard-win`, `error-code` |
| 1 | (MIT OR Apache-2.0) AND Unicode-3.0 | `unicode-ident` |
| 1 | 0BSD OR MIT OR Apache-2.0 | `adler2` |
| 1 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | `epaint_default_fonts` |
| 1 | Apache-2.0 AND MIT | `dpi` |
| 1 | ISC | `libloading` |

Ninguna es copyleft (GPL/LGPL/MPL).

**Pendiente:** las licencias MIT, BSD, ISC, Zlib y Apache-2.0 piden reproducir el aviso de copyright de cada crate. La forma práctica es generar un archivo con todos los textos usando [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) o [`cargo-bundle-licenses`](https://github.com/sstadick/cargo-bundle-licenses) y publicarlo junto al `.exe`. No se agregó esa herramienta para no sumar dependencias de build sin aprobación.

Para regenerar la tabla: `cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-gnu`, quedarse con las dependencias normales (no de build ni de dev) a partir del paquete raíz y agrupar por `license`.

---

## Texto de la licencia de Poppins (SIL OFL 1.1)

```
Copyright 2020 The Poppins Project Authors (https://github.com/itfoundry/Poppins)

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
http://scripts.sil.org/OFL


-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded, 
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```
