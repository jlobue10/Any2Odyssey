# Third-party licenses

Any2Odyssey itself is GPL-3.0-only (see `LICENSE` and `NOTICE`). This file lists the other works the Windows package contains: the Rust crates compiled into `Any2Odyssey.exe` and the fonts embedded in it. Samsung's and Leia's software is not included (see below).

## Rust crates

The 157 crates below are the normal (non-build, non-test) dependencies of the `any2odyssey` binary for the `x86_64-pc-windows-msvc` target, as `Cargo.lock` resolves them. Their licenses are permissive (MIT, Apache-2.0, Zlib, BSD, ISC, BSL-1.0, Unicode-3.0, 0BSD, Unlicense) or file-level copyleft compatible with the GPL (MPL-2.0 for `option-ext`); the `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` entry is `epaint_default_fonts`, whose embedded fonts are listed under "Fonts embedded in the application". The license each crate is used under, its copyright notices and the full license texts are in [License texts and copyright notices for the Rust crates](#license-texts-and-copyright-notices-for-the-rust-crates) below.

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

## License texts and copyright notices for the Rust crates

Where a crate offers a choice of licenses, Any2Odyssey uses it under the MIT license when that is one of the choices, and under the Apache License 2.0 otherwise (`moxcms` and `pxfm`, offered as BSD-3-Clause OR Apache-2.0). A crate with a single license, or with licenses joined by AND, is used under exactly those. The table gives the result for each crate, and the text of every license that applies as a result follows once: MIT, Apache-2.0, Unicode-3.0, ISC, BSL-1.0 and MPL-2.0. No crate in the list ships an Apache `NOTICE` file.

The notices are the copyright lines of the license and copyright files in each crate's source package as published on crates.io, in the version listed. Where a package has no such file, or its license file names no holder, the authors field of its manifest is given instead and marked as such. `crossbeam-channel` also carries third-party notices (Go, a Creative Commons licensed example) for code in its `tests/` and `examples/` folders only; that code is not compiled into Any2Odyssey.

| Crate and version | Used under | Copyright notices |
| --- | --- | --- |
| `ab_glyph` 0.2.32 | Apache-2.0 | no notice in the package; authors per its manifest: Alex Butler |
| `ab_glyph_rasterizer` 0.1.10 | Apache-2.0 | no notice in the package; authors per its manifest: Alex Butler |
| `adler2` 2.0.1 | MIT | `Copyright (C) Jonas Schievink <jonasschievink@gmail.com>` |
| `ahash` 0.8.12 | MIT | `Copyright (c) 2018 Tom Kaitchuck` |
| `anyhow` 1.0.104 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `arboard` 3.7.0 | MIT | `Copyright (c) 2022 The Arboard contributors` |
| `bitflags` 2.13.2 | MIT | `Copyright (c) 2014 The Rust Project Developers` |
| `block-buffer` 0.10.4 | MIT | `Copyright (c) 2018-2019 The RustCrypto Project Developers` |
| `bytemuck` 1.25.2 | MIT | `Copyright (c) 2019 Daniel "Lokathor" Gee.` |
| `bytemuck_derive` 1.12.1 | MIT | `Copyright (c) 2019 Daniel "Lokathor" Gee.` |
| `byteorder-lite` 0.1.0 | MIT | `Copyright (c) 2015 Andrew Gallant` |
| `cfg-if` 1.0.5 | MIT | `Copyright (c) 2014 Alex Crichton` |
| `clipboard-win` 5.4.3 | BSL-1.0 | no notice in the package; authors per its manifest: Douman |
| `cpufeatures` 0.2.17 | MIT | `Copyright (c) 2020-2025 The RustCrypto Project Developers` |
| `crc32fast` 1.5.4 | MIT | `Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors` |
| `crossbeam-channel` 0.5.17 | MIT | `Copyright (c) 2019 The Crossbeam Project Developers` |
| `crossbeam-utils` 0.8.23 | MIT | `Copyright (c) 2019 The Crossbeam Project Developers` |
| `crypto-common` 0.1.7 | MIT | `Copyright (c) 2021 RustCrypto Developers` |
| `cursor-icon` 1.2.0 | MIT | `Copyright 2023 Kirill Chibisov` |
| `deranged` 0.5.9 | MIT | `Copyright 2024 Jacob Pratt et al.` |
| `digest` 0.10.7 | MIT | `Copyright (c) 2017 Artyom Pavlov` |
| `dirs` 6.0.0 | MIT | `Copyright (c) 2018-2019 dirs-rs contributors` |
| `dirs-sys` 0.5.0 | MIT | `Copyright (c) 2018-2019 dirs-rs contributors` |
| `displaydoc` 0.2.7 | MIT | no notice in the package; authors per its manifest: Jane Lusby |
| `document-features` 0.2.12 | MIT | `Copyright (c) 2020 Olivier Goffart <ogoffart@sixtyfps.io>` |
| `dpi` 0.1.2 | Apache-2.0 and MIT | `Copyright (c) 2018 Jorge Aparicio`; the libm notice reproduced below |
| `ecolor` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt, Andreas Reich |
| `eframe` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `egui` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `egui-winit` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `egui_glow` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `emath` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `epaint` 0.33.3 | MIT | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `epaint_default_fonts` 0.33.3 | MIT (code); fonts: see the fonts section | no notice in the package; authors per its manifest: Emil Ernerfeldt |
| `error-code` 3.4.0 | BSL-1.0 | no notice in the package; authors per its manifest: Douman |
| `fastrand` 2.5.0 | MIT | no notice in the package; authors per its manifest: Stjepan Glavina |
| `fdeflate` 0.3.7 | MIT | no notice in the package; authors per its manifest: The image-rs Developers |
| `flate2` 1.1.11 | MIT | `Copyright (c) 2014-2026 Alex Crichton` |
| `form_urlencoded` 1.2.2 | MIT | `Copyright (c) 2013-2016 The rust-url developers` |
| `generic-array` 0.14.7 | MIT | `Copyright (c) 2015 Bartłomiej Kamiński` |
| `getrandom` 0.4.3 | MIT | `Copyright (c) 2018-2026 The rust-random Project Developers`; `Copyright (c) 2014 The Rust Project Developers` |
| `global-hotkey` 0.7.0 | MIT | `Copyright (c) 2022-2022 Tauri Programme within The Commons Conservancy` |
| `glow` 0.16.0 | MIT | no notice in the package; authors per its manifest: Joshua Groves, Dzmitry Malyshau |
| `glutin` 0.32.3 | Apache-2.0 | `Copyright 2022 Kirill Chibisov` |
| `glutin-winit` 0.5.0 | MIT | `Copyright © 2022 Kirill Chibisov` |
| `glutin_egl_sys` 0.7.1 | Apache-2.0 | `Copyright 2022 Kirill Chibisov` |
| `glutin_wgl_sys` 0.6.1 | Apache-2.0 | `Copyright 2022 Kirill Chibisov` |
| `html-escape` 0.2.15 | MIT | `Copyright (c) 2020 magiclen.org (Ron Li)` |
| `icu_collections` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_locale_core` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_normalizer` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_normalizer_data` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_properties` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_properties_data` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `icu_provider` 2.4.0 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `idna` 1.1.0 | MIT | `Copyright (c) 2013-2025 The rust-url developers` |
| `idna_adapter` 1.2.2 | MIT | `Copyright (c) The rust-url developers` |
| `image` 0.25.10 | MIT | no notice in the package; authors per its manifest: The image-rs Developers |
| `itoa` 1.0.18 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `keyboard-types` 0.7.0 | MIT | `Copyright (c) 2017 Pyfisch` |
| `libloading` 0.8.9 | ISC | `Copyright © 2015, Simonas Kazlauskas` |
| `litemap` 0.8.3 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `litrs` 1.0.1 | MIT | `Copyright (c) 2020 Project Developers` |
| `lock_api` 0.4.14 | MIT | `Copyright (c) 2016 The Rust Project Developers` |
| `log` 0.4.35 | MIT | `Copyright (c) 2014 The Rust Project Developers` |
| `memchr` 2.8.3 | MIT | `Copyright (c) 2015 Andrew Gallant` |
| `memoffset` 0.9.1 | MIT | `Copyright (c) 2017 Gilad Naaman` |
| `miniz_oxide` 0.8.9 | MIT | `Copyright 2013-2014 RAD Game Tools and Valve Software`; `Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC`; `Copyright (c) 2017 Frommi`; `Copyright (c) 2017-2024 oyvindln`; `Copyright (c) 2020 Frommi` |
| `miniz_oxide` 0.9.1 | MIT | `Copyright 2013-2014 RAD Game Tools and Valve Software`; `Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC`; `Copyright (c) 2017 Frommi`; `Copyright (c) 2017-2024 oyvindln`; `Copyright (c) 2020 Frommi` |
| `moxcms` 0.8.1 | Apache-2.0 | `Copyright 2024 Radzivon Bartoshyk`; `Copyright (c) Radzivon Bartoshyk. All rights reserved.` |
| `muda` 0.17.2 | MIT | `Copyright (c) 2022-2022 Tauri Programme within The Commons Conservancy` |
| `nohash-hasher` 0.2.0 | MIT | `Copyright 2018 Parity Technologies (UK) Ltd.` |
| `num-conv` 0.2.2 | MIT | `Copyright (c) Jacob Pratt` |
| `num-traits` 0.2.19 | MIT | `Copyright (c) 2014 The Rust Project Developers` |
| `once_cell` 1.21.4 | MIT | no notice in the package; authors per its manifest: Aleksey Kladov |
| `open` 5.4.5 | MIT | `Copyright © 2015 Sebastian Thiel` |
| `option-ext` 0.2.0 | MPL-2.0 | no notice in the package; authors per its manifest: Simon Ochsenreither |
| `owned_ttf_parser` 0.25.1 | Apache-2.0 | no notice in the package; authors per its manifest: Alex Butler |
| `parking_lot` 0.12.5 | MIT | `Copyright (c) 2016 The Rust Project Developers` |
| `parking_lot_core` 0.9.12 | MIT | `Copyright (c) 2016 The Rust Project Developers` |
| `percent-encoding` 2.3.2 | MIT | `Copyright (c) 2013-2025 The rust-url developers` |
| `pin-project-lite` 0.2.17 | MIT | no notice and no authors field in the package |
| `png` 0.18.1 | MIT | `Copyright (c) 2015 nwin` |
| `potential_utf` 0.1.6 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `powerfmt` 0.2.1 | MIT | `Copyright 2023 Jacob Pratt et al.` |
| `proc-macro2` 1.0.107 | MIT | no notice in the package; authors per its manifest: David Tolnay, Alex Crichton |
| `profiling` 1.0.18 | MIT | no notice in the package; authors per its manifest: Philip Degarmo |
| `pxfm` 0.1.30 | Apache-2.0 | `Copyright 2024 Radzivon Bartoshyk`; `Copyright (c) Radzivon Bartoshyk. All rights reserved.` |
| `quote` 1.0.47 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `raw-window-handle` 0.6.2 | MIT | `Copyright (c) 2019 Osspial`; `Copyright (c) 2020 Osspial` |
| `same-file` 1.0.6 | MIT | `Copyright (c) 2017 Andrew Gallant` |
| `scopeguard` 1.2.0 | MIT | `Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers` |
| `serde` 1.0.229 | MIT | no notice in the package; authors per its manifest: Erick Tryzelaar, David Tolnay |
| `serde_core` 1.0.229 | MIT | no notice in the package; authors per its manifest: Erick Tryzelaar, David Tolnay |
| `serde_derive` 1.0.229 | MIT | no notice in the package; authors per its manifest: Erick Tryzelaar, David Tolnay |
| `serde_json` 1.0.154 | MIT | no notice and no authors field in the package |
| `sha2` 0.10.9 | MIT | `Copyright (c) 2006-2009 Graydon Hoare`; `Copyright (c) 2009-2013 Mozilla Foundation`; `Copyright (c) 2016 Artyom Pavlov` |
| `simd-adler32` 0.3.10 | MIT | `Copyright (c) [2021] [Marvin Countryman]` |
| `smallvec` 1.16.5 | MIT | `Copyright (c) 2018 The Servo Project Developers` |
| `smol_str` 0.2.2 | MIT | no notice in the package; authors per its manifest: Aleksey Kladov |
| `stable_deref_trait` 1.2.1 | MIT | `Copyright (c) 2017 Robert Grosse` |
| `static_assertions` 1.1.0 | MIT | `Copyright (c) 2017 Nikolai Vazquez` |
| `syn` 2.0.119 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `syn` 3.0.9 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `synstructure` 0.14.0 | MIT | `Copyright 2016 Nika Layzell` |
| `tempfile` 3.27.0 | MIT | `Copyright (c) 2015 Steven Allen` |
| `thiserror` 2.0.21 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `thiserror-impl` 2.0.21 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `time` 0.3.55 | MIT | `Copyright (c) Jacob Pratt et al.` |
| `time-core` 0.1.9 | MIT | `Copyright (c) Jacob Pratt et al.` |
| `time-macros` 0.2.32 | MIT | `Copyright (c) Jacob Pratt et al.` |
| `tinystr` 0.8.5 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `tracing` 0.1.44 | MIT | `Copyright (c) 2019 Tokio Contributors` |
| `tracing-core` 0.1.36 | MIT | `Copyright (c) 2019 Tokio Contributors` |
| `tray-icon` 0.21.3 | MIT | `Copyright (c) 2022-2022 Tauri Programme within The Commons Conservancy` |
| `ttf-parser` 0.25.1 | MIT | `Copyright (c) 2018 Yevhenii Reizner` |
| `typenum` 1.20.1 | MIT | `Copyright 2014 Paho Lurie-Gregg` |
| `unicode-ident` 1.0.27 | MIT and Unicode-3.0 | `Copyright © 1991-2023 Unicode, Inc.` |
| `unicode-segmentation` 1.13.3 | MIT | `Copyright (c) 2015 The Rust Project Developers` |
| `url` 2.5.8 | MIT | `Copyright (c) 2013-2025 The rust-url developers` |
| `utf8_iter` 1.1.0 | MIT | `Copyright Mozilla Foundation` |
| `utf_types` 1.0.0 | MIT | `Copyright Mozilla Foundation` |
| `web-time` 1.1.0 | MIT | `Copyright 2023 dAxpeDDa` |
| `webbrowser` 1.2.6 | MIT | `Copyright (c) 2015-2022 Amod Malviya` |
| `winapi-util` 0.1.11 | MIT | `Copyright (c) 2017 Andrew Gallant` |
| `windows` 0.62.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-collections` 0.3.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-core` 0.62.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-future` 0.3.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-implement` 0.60.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-interface` 0.59.3 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-link` 0.2.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-numerics` 0.3.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-result` 0.4.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-strings` 0.5.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-sys` 0.52.0 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-sys` 0.59.0 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-sys` 0.60.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-sys` 0.61.2 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-targets` 0.52.6 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-targets` 0.53.5 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows-threading` 0.2.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows_x86_64_msvc` 0.52.6 | MIT | `Copyright (c) Microsoft Corporation.` |
| `windows_x86_64_msvc` 0.53.1 | MIT | `Copyright (c) Microsoft Corporation.` |
| `winit` 0.30.13 | Apache-2.0 | no notice in the package; authors per its manifest: The winit contributors, Pierre Krieger |
| `writeable` 0.6.4 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `yoke` 0.8.4 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `yoke-derive` 0.8.4 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zerocopy` 0.8.63 | MIT | `Copyright 2023 The Fuchsia Authors`; `Copyright 2019 The Fuchsia Authors.` |
| `zerofrom` 0.1.8 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zerofrom-derive` 0.1.8 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zerotrie` 0.2.5 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zerovec` 0.11.9 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zerovec-derive` 0.11.7 | Unicode-3.0 | `Copyright © 2020-2024 Unicode, Inc.` |
| `zmij` 1.0.23 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `zune-core` 0.5.3 | MIT | `Copyright (c) zune-image developers` |
| `zune-jpeg` 0.5.15 | MIT | `Copyright (c) zune-image developers` |

### MIT License

Applies to every crate whose "Used under" entry above includes MIT, each with its own copyright notice from the table in place of the first line.

```text
Copyright (c) <the holders named for the crate in the table above>

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

### `dpi`: libm notice

`dpi` contains code derived from `rust-lang/libm`; its package carries this notice (`LICENSE-LIBM-MIT`).

```text
rust-lang/libm as a whole is available for use under the MIT license:

------------------------------------------------------------------------------
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
------------------------------------------------------------------------------

This Rust library contains the following copyrights:

    Copyright (c) 2018 Jorge Aparicio

Portions of this software are derived from third-party works licensed under
terms compatible with the above MIT license:

* musl libc https://www.musl-libc.org/. This library contains the following
  copyright:

      Copyright © 2005-2020 Rich Felker, et al.

* The CORE-MATH project https://core-math.gitlabpages.inria.fr/. CORE-MATH
  routines are available under the MIT license on a per-file basis.

The musl libc COPYRIGHT file also includes the following notice relevant to
math portions of the library:

------------------------------------------------------------------------------
Much of the math library code (src/math/* and src/complex/*) is
Copyright © 1993,2004 Sun Microsystems or
Copyright © 2003-2011 David Schultz or
Copyright © 2003-2009 Steven G. Kargl or
Copyright © 2003-2009 Bruce D. Evans or
Copyright © 2008 Stephen L. Moshier or
Copyright © 2017-2018 Arm Limited
and labelled as such in comments in the individual source files. All
have been licensed under extremely permissive terms.
------------------------------------------------------------------------------
```

### Apache License 2.0

Applies to: `ab_glyph`, `ab_glyph_rasterizer`, `glutin`, `glutin_egl_sys`, `glutin_wgl_sys`, `moxcms`, `owned_ttf_parser`, `pxfm`, `winit`, `dpi`.

```text
                              Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

   To apply the Apache License to your work, attach the following
   boilerplate notice, with the fields enclosed by brackets "[]"
   replaced with your own identifying information. (Don't include
   the brackets!)  The text should be enclosed in the appropriate
   comment syntax for the file format. We also recommend that a
   file or class name and description of purpose be included on the
   same "printed page" as the copyright notice for easier
   identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

	http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

### Unicode License v3

Applies to: `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `yoke-derive`, `zerofrom`, `zerofrom-derive`, `zerotrie`, `zerovec`, `zerovec-derive`, `unicode-ident`. The text below is the `LICENSE` file of the ICU4X crates; `unicode-ident` carries the same license with the notice `Copyright © 1991-2023 Unicode, Inc.`

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 2020-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.
```

### ISC License

Applies to: `libloading`.

```text
Copyright © 2015, Simonas Kazlauskas

Permission to use, copy, modify, and/or distribute this software for any purpose with or without
fee is hereby granted, provided that the above copyright notice and this permission notice appear
in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
```

### Boost Software License 1.0

Applies to: `clipboard-win`, `error-code`.

```text
Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization
obtaining a copy of the software and accompanying documentation covered by
this license (the "Software") to use, reproduce, display, distribute,
execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to
do so, all subject to the following:

The copyright notices in the Software and this entire statement, including
the above license grant, this restriction and the following disclaimer,
must be included in all copies of the Software, in whole or in part, and
all derivative works of the Software, unless such copies or derivative
works are solely in the form of machine-executable object code generated by
a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT
SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE
FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

### Mozilla Public License 2.0

Applies to: `option-ext`, which is compiled into the executable unmodified. Its source code is available from https://crates.io/crates/option-ext/0.2.0 and from its repository, https://github.com/soc/option-ext.git.

```text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at https://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
```

## Microsoft C runtime

The Windows executable links the startup and runtime-support code of Microsoft's Visual C++ toolchain statically (`-C target-feature=+crt-static`), so it does not need the Visual C++ Redistributable. That code is part of the compiler's runtime libraries, which Microsoft licenses for inclusion in programs built with the toolchain; the Universal C Runtime itself remains a component of Windows and is not included.

## Samsung / Leia

Samsung's Odyssey 3D Hub, its `ConversionWrapper.dll`, the LeiaSR runtime, AI models and calibration data are **not** included. They are obtained and licensed separately from Samsung and are loaded from their own installation folders at run time.

## Fonts embedded in the application

The application embeds six typefaces: Fira Sans, Libre Baskerville and the four fonts egui brings as its defaults (Ubuntu Light, Hack, Noto Emoji and emoji-icon-font). Their license texts follow.

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
