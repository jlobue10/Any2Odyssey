# Third-party licenses

Any2Odyssey itself is GPL-3.0-only (see `LICENSE` and `NOTICE`). This file lists the other works the Windows package contains: the Rust crates compiled into `Any2Odyssey.exe` and the fonts embedded in it. Samsung's and Leia's software is not included (see below).

## Rust crates

The 157 crates below are the normal (non-build, non-test) dependencies of the `any2odyssey` binary for the `x86_64-pc-windows-msvc` target, as `Cargo.lock` resolves them. Their licenses are permissive (MIT, Apache-2.0, Zlib, BSD, ISC, BSL-1.0, Unicode-3.0, 0BSD, Unlicense) or file-level copyleft compatible with the GPL (MPL-2.0 for `option-ext`); the `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` entry is `epaint_default_fonts`, whose embedded fonts are listed in the next section. Each crate's own license text is in its source package on crates.io.

Regenerate the table after changing dependencies:

```sh
cargo tree -p any2odyssey --target x86_64-pc-windows-msvc -e normal --prefix none --format '{p}|{l}' | sort -u
```

| Crate | Version | License |
| --- | --- | --- |
| `ab_glyph` | 0.2.32 | Apache-2.0 |
| `ab_glyph_rasterizer` | 0.1.10 | Apache-2.0 |
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| `ahash` | 0.8.12 | MIT OR Apache-2.0 |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 |
| `arboard` | 3.7.0 | MIT OR Apache-2.0 |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 |
| `block-buffer` | 0.10.4 | MIT OR Apache-2.0 |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| `bytemuck_derive` | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| `byteorder-lite` | 0.1.0 | Unlicense OR MIT |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 |
| `clipboard-win` | 5.4.3 | BSL-1.0 |
| `cpufeatures` | 0.2.17 | MIT OR Apache-2.0 |
| `crc32fast` | 1.5.4 | MIT OR Apache-2.0 |
| `crossbeam-channel` | 0.5.17 | MIT OR Apache-2.0 |
| `crossbeam-utils` | 0.8.23 | MIT OR Apache-2.0 |
| `crypto-common` | 0.1.7 | MIT OR Apache-2.0 |
| `cursor-icon` | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| `deranged` | 0.5.9 | MIT OR Apache-2.0 |
| `digest` | 0.10.7 | MIT OR Apache-2.0 |
| `dirs` | 6.0.0 | MIT OR Apache-2.0 |
| `dirs-sys` | 0.5.0 | MIT OR Apache-2.0 |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 |
| `document-features` | 0.2.12 | MIT OR Apache-2.0 |
| `dpi` | 0.1.2 | Apache-2.0 AND MIT |
| `ecolor` | 0.33.3 | MIT OR Apache-2.0 |
| `eframe` | 0.33.3 | MIT OR Apache-2.0 |
| `egui` | 0.33.3 | MIT OR Apache-2.0 |
| `egui-winit` | 0.33.3 | MIT OR Apache-2.0 |
| `egui_glow` | 0.33.3 | MIT OR Apache-2.0 |
| `emath` | 0.33.3 | MIT OR Apache-2.0 |
| `epaint` | 0.33.3 | MIT OR Apache-2.0 |
| `epaint_default_fonts` | 0.33.3 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 |
| `error-code` | 3.4.0 | BSL-1.0 |
| `fastrand` | 2.5.0 | Apache-2.0 OR MIT |
| `fdeflate` | 0.3.7 | MIT OR Apache-2.0 |
| `flate2` | 1.1.11 | MIT OR Apache-2.0 |
| `form_urlencoded` | 1.2.2 | MIT OR Apache-2.0 |
| `generic-array` | 0.14.7 | MIT |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 |
| `global-hotkey` | 0.7.0 | Apache-2.0 OR MIT |
| `glow` | 0.16.0 | MIT OR Apache-2.0 OR Zlib |
| `glutin` | 0.32.3 | Apache-2.0 |
| `glutin-winit` | 0.5.0 | MIT |
| `glutin_egl_sys` | 0.7.1 | Apache-2.0 |
| `glutin_wgl_sys` | 0.6.1 | Apache-2.0 |
| `html-escape` | 0.2.15 | MIT |
| `icu_collections` | 2.4.0 | Unicode-3.0 |
| `icu_locale_core` | 2.4.0 | Unicode-3.0 |
| `icu_normalizer` | 2.4.0 | Unicode-3.0 |
| `icu_normalizer_data` | 2.4.0 | Unicode-3.0 |
| `icu_properties` | 2.4.0 | Unicode-3.0 |
| `icu_properties_data` | 2.4.0 | Unicode-3.0 |
| `icu_provider` | 2.4.0 | Unicode-3.0 |
| `idna` | 1.1.0 | MIT OR Apache-2.0 |
| `idna_adapter` | 1.2.2 | Apache-2.0 OR MIT |
| `image` | 0.25.10 | MIT OR Apache-2.0 |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 |
| `keyboard-types` | 0.7.0 | MIT OR Apache-2.0 |
| `libloading` | 0.8.9 | ISC |
| `litemap` | 0.8.3 | Unicode-3.0 |
| `litrs` | 1.0.1 | MIT OR Apache-2.0 |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 |
| `log` | 0.4.35 | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `memoffset` | 0.9.1 | MIT |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| `muda` | 0.17.2 | Apache-2.0 OR MIT |
| `nohash-hasher` | 0.2.0 | Apache-2.0 OR MIT |
| `num-conv` | 0.2.2 | MIT OR Apache-2.0 |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 |
| `open` | 5.4.5 | MIT |
| `option-ext` | 0.2.0 | MPL-2.0 |
| `owned_ttf_parser` | 0.25.1 | Apache-2.0 |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 |
| `percent-encoding` | 2.3.2 | MIT OR Apache-2.0 |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT |
| `png` | 0.18.1 | MIT OR Apache-2.0 |
| `potential_utf` | 0.1.6 | Unicode-3.0 |
| `powerfmt` | 0.2.1 | MIT OR Apache-2.0 |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 |
| `profiling` | 1.0.18 | MIT OR Apache-2.0 |
| `pxfm` | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| `quote` | 1.0.47 | MIT OR Apache-2.0 |
| `raw-window-handle` | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| `same-file` | 1.0.6 | Unlicense/MIT |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 |
| `sha2` | 0.10.9 | MIT OR Apache-2.0 |
| `simd-adler32` | 0.3.10 | MIT |
| `smallvec` | 1.16.5 | MIT OR Apache-2.0 |
| `smol_str` | 0.2.2 | MIT OR Apache-2.0 |
| `stable_deref_trait` | 1.2.1 | MIT OR Apache-2.0 |
| `static_assertions` | 1.1.0 | MIT OR Apache-2.0 |
| `syn` | 2.0.119 | MIT OR Apache-2.0 |
| `syn` | 3.0.9 | MIT OR Apache-2.0 |
| `synstructure` | 0.14.0 | MIT |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 |
| `thiserror-impl` | 2.0.21 | MIT OR Apache-2.0 |
| `time` | 0.3.55 | MIT OR Apache-2.0 |
| `time-core` | 0.1.9 | MIT OR Apache-2.0 |
| `time-macros` | 0.2.32 | MIT OR Apache-2.0 |
| `tinystr` | 0.8.5 | Unicode-3.0 |
| `tracing` | 0.1.44 | MIT |
| `tracing-core` | 0.1.36 | MIT |
| `tray-icon` | 0.21.3 | MIT OR Apache-2.0 |
| `ttf-parser` | 0.25.1 | MIT OR Apache-2.0 |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 |
| `url` | 2.5.8 | MIT OR Apache-2.0 |
| `utf8_iter` | 1.1.0 | Apache-2.0 OR MIT |
| `utf_types` | 1.0.0 | Apache-2.0 OR MIT |
| `web-time` | 1.1.0 | MIT OR Apache-2.0 |
| `webbrowser` | 1.2.6 | MIT OR Apache-2.0 |
| `winapi-util` | 0.1.11 | Unlicense OR MIT |
| `windows` | 0.62.2 | MIT OR Apache-2.0 |
| `windows-collections` | 0.3.2 | MIT OR Apache-2.0 |
| `windows-core` | 0.62.2 | MIT OR Apache-2.0 |
| `windows-future` | 0.3.2 | MIT OR Apache-2.0 |
| `windows-implement` | 0.60.2 | MIT OR Apache-2.0 |
| `windows-interface` | 0.59.3 | MIT OR Apache-2.0 |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 |
| `windows-numerics` | 0.3.1 | MIT OR Apache-2.0 |
| `windows-result` | 0.4.1 | MIT OR Apache-2.0 |
| `windows-strings` | 0.5.1 | MIT OR Apache-2.0 |
| `windows-sys` | 0.52.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.60.2 | MIT OR Apache-2.0 |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 |
| `windows-targets` | 0.53.5 | MIT OR Apache-2.0 |
| `windows-threading` | 0.2.1 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `winit` | 0.30.13 | Apache-2.0 |
| `writeable` | 0.6.4 | Unicode-3.0 |
| `yoke` | 0.8.4 | Unicode-3.0 |
| `yoke-derive` | 0.8.4 | Unicode-3.0 |
| `zerocopy` | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerofrom` | 0.1.8 | Unicode-3.0 |
| `zerofrom-derive` | 0.1.8 | Unicode-3.0 |
| `zerotrie` | 0.2.5 | Unicode-3.0 |
| `zerovec` | 0.11.9 | Unicode-3.0 |
| `zerovec-derive` | 0.11.7 | Unicode-3.0 |
| `zmij` | 1.0.23 | MIT |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

## Samsung / Leia

Samsung's Odyssey 3D Hub, its `ConversionWrapper.dll`, the LeiaSR runtime, AI models and calibration data are **not** included. They are obtained and licensed separately from Samsung and are loaded from their own installation folders at run time.

## Fonts embedded in the application

The controls window embeds five typefaces. Their license texts follow.

### Fira Sans (`assets/fonts/FiraSans-Regular.ttf`)

The UI text face; it also supplies the arrows and triangles egui's default font lacks. Digitized data copyright 2012-2018 The Mozilla Foundation, Telefonica S.A., Carrois Corporate GbR and bBox Type GmbH, with Reserved Font Name Fira Sans. Licensed under the SIL Open Font License 1.1; the full text is in `assets/fonts/OFL-FiraSans.txt` (the OFL text is reproduced once, under Libre Baskerville below).

### Libre Baskerville (`tools/iconmaker/fonts/LibreBaskerville-Variable.ttf`)

Used by the icon generator for the emblem and wordmark lettering, **and embedded in the application** as the display face for headings and the title bar. Copyright 2012 The Libre Baskerville Project Authors (https://github.com/impallari/Libre-Baskerville), with Reserved Font Name Libre Baskerville. Licensed under the SIL Open Font License 1.1; the full text is in `tools/iconmaker/fonts/OFL.txt` and reproduced here:

    Copyright 2012 The Libre Baskerville Project Authors (https://github.com/impallari/Libre-Baskerville) with Reserved Font Name Libre Baskerville.
    
    This Font Software is licensed under the SIL Open Font License, Version 1.1.
    This license is copied below, and is also available with a FAQ at:
    https://openfontlicense.org
    
    
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

### Ubuntu Light, Hack, Noto Emoji and emoji-icon-font (egui's default fonts)

The `egui` user-interface library is built with its `default_fonts` feature, which embeds four fonts from the `epaint_default_fonts` crate. Ubuntu Light is the fallback for the display family; the others serve monospace text and emoji.

- **Ubuntu Light** — Copyright 2010, 2011 Canonical Ltd. Licensed under the Ubuntu Font Licence 1.0 (reproduced below).
- **Hack** — Copyright 2018 Source Foundry Authors, MIT License; derived from DejaVu (public domain) and Bitstream Vera Sans Mono, Copyright 2003 Bitstream Inc., Bitstream Vera License (reproduced below).
- **Noto Emoji** — Copyright Google Inc. Licensed under the SIL Open Font License 1.1 (text as above).
- **emoji-icon-font** — Copyright 2014 John Slegers, MIT License (reproduced below).

Ubuntu Font Licence 1.0:

    -------------------------------
    UBUNTU FONT LICENCE Version 1.0
    -------------------------------
    
    PREAMBLE
    This licence allows the licensed fonts to be used, studied, modified and
    redistributed freely. The fonts, including any derivative works, can be
    bundled, embedded, and redistributed provided the terms of this licence
    are met. The fonts and derivatives, however, cannot be released under
    any other licence. The requirement for fonts to remain under this
    licence does not require any document created using the fonts or their
    derivatives to be published under this licence, as long as the primary
    purpose of the document is not to be a vehicle for the distribution of
    the fonts.
    
    DEFINITIONS
    "Font Software" refers to the set of files released by the Copyright
    Holder(s) under this licence and clearly marked as such. This may
    include source files, build scripts and documentation.
    
    "Original Version" refers to the collection of Font Software components
    as received under this licence.
    
    "Modified Version" refers to any derivative made by adding to, deleting,
    or substituting -- in part or in whole -- any of the components of the
    Original Version, by changing formats or by porting the Font Software to
    a new environment.
    
    "Copyright Holder(s)" refers to all individuals and companies who have a
    copyright ownership of the Font Software.
    
    "Substantially Changed" refers to Modified Versions which can be easily
    identified as dissimilar to the Font Software by users of the Font
    Software comparing the Original Version with the Modified Version.
    
    To "Propagate" a work means to do anything with it that, without
    permission, would make you directly or secondarily liable for
    infringement under applicable copyright law, except executing it on a
    computer or modifying a private copy. Propagation includes copying,
    distribution (with or without modification and with or without charging
    a redistribution fee), making available to the public, and in some
    countries other activities as well.
    
    PERMISSION & CONDITIONS
    This licence does not grant any rights under trademark law and all such
    rights are reserved.
    
    Permission is hereby granted, free of charge, to any person obtaining a
    copy of the Font Software, to propagate the Font Software, subject to
    the below conditions:
    
    1) Each copy of the Font Software must contain the above copyright
    notice and this licence. These can be included either as stand-alone
    text files, human-readable headers or in the appropriate machine-
    readable metadata fields within text or binary files as long as those
    fields can be easily viewed by the user.
    
    2) The font name complies with the following:
    (a) The Original Version must retain its name, unmodified.
    (b) Modified Versions which are Substantially Changed must be renamed to
    avoid use of the name of the Original Version or similar names entirely.
    (c) Modified Versions which are not Substantially Changed must be
    renamed to both (i) retain the name of the Original Version and (ii) add
    additional naming elements to distinguish the Modified Version from the
    Original Version. The name of such Modified Versions must be the name of
    the Original Version, with "derivative X" where X represents the name of
    the new work, appended to that name.
    
    3) The name(s) of the Copyright Holder(s) and any contributor to the
    Font Software shall not be used to promote, endorse or advertise any
    Modified Version, except (i) as required by this licence, (ii) to
    acknowledge the contribution(s) of the Copyright Holder(s) or (iii) with
    their explicit written permission.
    
    4) The Font Software, modified or unmodified, in part or in whole, must
    be distributed entirely under this licence, and must not be distributed
    under any other licence. The requirement for fonts to remain under this
    licence does not affect any document created using the Font Software,
    except any version of the Font Software extracted from a document
    created using the Font Software may only be distributed under this
    licence.
    
    TERMINATION
    This licence becomes null and void if any of the above conditions are
    not met.
    
    DISCLAIMER
    THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
    EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
    MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
    COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
    COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
    INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
    DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
    FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER
    DEALINGS IN THE FONT SOFTWARE.

Hack license (MIT, with the Bitstream Vera notice):

    The work in the Hack project is Copyright 2018 Source Foundry Authors and licensed under the MIT License
    
    The work in the DejaVu project was committed to the public domain.
    
    Bitstream Vera Sans Mono Copyright 2003 Bitstream Inc. and licensed under the Bitstream Vera License with Reserved Font Names "Bitstream" and "Vera"
    MIT License
    
    Copyright (c) 2018 Source Foundry Authors
    
    Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
    
    The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
    
    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
    BITSTREAM VERA LICENSE
    
    Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream Vera is a trademark of Bitstream, Inc.
    
    Permission is hereby granted, free of charge, to any person obtaining a copy of the fonts accompanying this license ("Fonts") and associated documentation files (the "Font Software"), to reproduce and distribute the Font Software, including without limitation the rights to use, copy, merge, publish, distribute, and/or sell copies of the Font Software, and to permit persons to whom the Font Software is furnished to do so, subject to the following conditions:
    
    The above copyright and trademark notices and this permission notice shall be included in all copies of one or more of the Font Software typefaces.
    
    The Font Software may be modified, altered, or added to, and in particular the designs of glyphs or characters in the Fonts may be modified and additional glyphs or characters may be added to the Fonts, only if the fonts are renamed to names not containing either the words "Bitstream" or the word "Vera".
    
    This License becomes null and void to the extent applicable to Fonts or Font Software that has been modified and is distributed under the "Bitstream Vera" names.
    
    The Font Software may be sold as part of a larger software package but no copy of one or more of the Font Software typefaces may be sold by itself.
    
    THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL BITSTREAM OR THE GNOME FOUNDATION BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.
    
    Except as contained in this notice, the names of Gnome, the Gnome Foundation, and Bitstream Inc., shall not be used in advertising or otherwise to promote the sale, use or other dealings in this Font Software without prior written authorization from the Gnome Foundation or Bitstream Inc., respectively. For further information, contact: fonts at gnome dot org.

emoji-icon-font license (MIT):

    MIT License
    
    Copyright (c) 2014 John Slegers
    
    Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
    
    The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
    
    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
