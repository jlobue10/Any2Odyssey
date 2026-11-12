# Third-party licenses: the Linux build

`THIRD-PARTY-LICENSES.md` lists the works the Windows package contains and gives the texts of their licenses. A Linux build of Any2Odyssey contains most of those crates and, beyond them, the 93 crates below: the normal (non-build, non-test) dependencies of the `any2odyssey` binary for `x86_64-unknown-linux-gnu` that the Windows list does not have, as `Cargo.lock` resolves them. They are used by the same rule as there: under the MIT license when a crate offers it, under the Apache License 2.0 otherwise, and a crate with a single license under that one. The texts of the MIT, Apache 2.0, ISC and zlib licenses are in `THIRD-PARTY-LICENSES.md`. The notices are the copyright lines of the license and copyright files in each crate's source package as published on crates.io; where a package has none, the authors field of its manifest is given and marked as such.

Libraries the Linux build links at run time and does not contain (PipeWire, the C library, the desktop's graphics libraries, ONNX Runtime and cuDNN where installed) are the system's own, under their own terms. Samsung's and Leia's software is not included: the Wine setup in `contrib/wine-lens` copies the user's own installation.

Regenerate with `scripts/third-party-linux.py > THIRD-PARTY-LICENSES-LINUX.md` after changing dependencies.

| Crate and version | License offered | Used under | Copyright notices |
| --- | --- | --- | --- |
| `as-raw-xcb-connection` 1.0.1 | MIT OR Apache-2.0 | MIT | `Copyright 2019 as-raw-xcb-connection Contributers` |
| `ashpd` 0.13.13 | MIT | MIT | `Copyright (c) 2020 Bilal Elmoussaoui` |
| `async-broadcast` 0.7.2 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2020 Yoshua Wuyts` |
| `async-recursion` 1.2.0 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: Robert Usher |
| `async-trait` 0.1.92 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `bytes` 1.12.1 | MIT | MIT | `Copyright (c) 2018 Carl Lerche` |
| `calloop` 0.13.0 | MIT | MIT | `Copyright (c) 2018 Victor Berger` |
| `calloop` 0.14.5 | MIT | MIT | `Copyright (c) 2018 Victor Berger` |
| `calloop-wayland-source` 0.3.0 | MIT | MIT | `Copyright (c) 2023 Kirill Chibisov` |
| `calloop-wayland-source` 0.4.1 | MIT | MIT | `Copyright (c) 2023 Kirill Chibisov` |
| `cookie-factory` 0.3.3 | MIT | MIT | no notice in the package; authors per its manifest: Geoffroy Couprie, Pierre Chifflier |
| `dlib` 0.5.3 | MIT | MIT | `Copyright (c) 2015 Victor Berger` |
| `downcast-rs` 1.2.1 | MIT/Apache-2.0 | MIT | `Copyright (c) 2020 Ashish Myles and contributors` |
| `endi` 1.1.1 | MIT | MIT | no notice in the package; authors per its manifest: Zeeshan Ali Khan |
| `enumflags2` 0.7.12 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2017-2023 Maik Klein, Maja Kądziołka` |
| `enumflags2_derive` 0.7.12 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2017 Maik Klein` |
| `errno` 0.3.14 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2014 Chris Wong` |
| `event-listener` 5.4.2 | Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Stjepan Glavina, John Nunley |
| `event-listener-strategy` 0.5.4 | Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: John Nunley |
| `futures-core` 0.3.34 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `futures-io` 0.3.34 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `futures-lite` 2.6.1 | Apache-2.0 OR MIT | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `futures-macro` 0.3.34 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `futures-task` 0.3.34 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `futures-util` 0.3.34 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2016 Alex Crichton`<br>`Copyright (c) 2017 The Tokio Authors` |
| `gethostname` 1.1.0 | Apache-2.0 | Apache-2.0 | no notice in the package; authors per its manifest: Sebastian Wiesner |
| `getrandom` 0.3.4 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2018-2025 The rust-random Project Developers`<br>`Copyright (c) 2014 The Rust Project Developers` |
| `glutin_glx_sys` 0.6.1 | Apache-2.0 | Apache-2.0 | `Copyright 2022 Kirill Chibisov` |
| `hex` 0.4.3 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2013-2014 The Rust Project Developers.`<br>`Copyright (c) 2015-2020 The rust-hex Developers` |
| `is-docker` 0.2.0 | MIT | MIT | `Copyright (c) 2023 Sean Larkin` |
| `is-wsl` 0.4.0 | MIT | MIT | `Copyright (c) 2023 Sean Larkin` |
| `libc` 0.2.191 | MIT OR Apache-2.0 | MIT | `Copyright (c) The Rust Project Developers` |
| `libloading` 0.9.0 | ISC | ISC | `Copyright © 2015, Simonas Kazlauskas` |
| `libspa` 0.10.1 | MIT | MIT | no notice in the package; authors per its manifest: Tom Wagner, Guillaume Desmottes |
| `libspa-sys` 0.10.1 | MIT | MIT | no notice in the package; authors per its manifest: Tom Wagner, Guillaume Desmottes |
| `linux-raw-sys` 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Dan Gohman |
| `linux-raw-sys` 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Dan Gohman |
| `memmap2` 0.9.12 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2020 Yevhenii Reizner`<br>`Copyright (c) 2015 Dan Burkert` |
| `mio` 1.2.4 | MIT | MIT | `Copyright (c) 2014 Carl Lerche and other MIO contributors` |
| `nom` 8.0.0 | MIT | MIT | `Copyright (c) 2014-2019 Geoffroy Couprie` |
| `num_threads` 0.1.8 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2021 Jacob Pratt` |
| `ordered-stream` 0.2.0 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: Daniel De Graaf, Zeeshan Ali Khan |
| `ort` 2.0.0-rc.15 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2023-2026 pyke.io`<br>`Copyright (c) 2020 Nicolas Bigaouette` |
| `ort-sys` 2.0.0-rc.15 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2023-2026 pyke.io`<br>`Copyright (c) 2020 Nicolas Bigaouette` |
| `parking` 2.2.1 | Apache-2.0 OR MIT | MIT | `Copyright 2014-2020 The Rust Project Developers` |
| `pipewire` 0.10.1 | MIT | MIT | no notice in the package; authors per its manifest: Tom Wagner, Guillaume Desmottes |
| `pipewire-sys` 0.10.1 | MIT | MIT | no notice in the package; authors per its manifest: Tom Wagner, Guillaume Desmottes |
| `polling` 3.11.0 | Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Stjepan Glavina, John Nunley |
| `proc-macro-crate` 3.5.0 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: Bastian Köcher |
| `quick-xml` 0.41.0 | MIT | MIT | `Copyright (c) 2016 Johann Tuffe` |
| `rustix` 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Dan Gohman, Jakub Konka |
| `rustix` 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | MIT | no notice in the package; authors per its manifest: Dan Gohman, Jakub Konka |
| `scoped-tls` 1.0.1 | MIT/Apache-2.0 | MIT | `Copyright (c) 2014 Alex Crichton` |
| `serde_repr` 0.1.21 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `signal-hook-registry` 1.4.8 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2017 tokio-jsonrpc developers` |
| `slab` 0.4.12 | MIT | MIT | `Copyright (c) 2019 Carl Lerche` |
| `smithay-client-toolkit` 0.19.2 | MIT | MIT | `Copyright (c) 2018 Victor Berger` |
| `smithay-client-toolkit` 0.20.0 | MIT | MIT | `Copyright (c) 2018 Victor Berger` |
| `smithay-clipboard` 0.7.3 | MIT | MIT | `Copyright (c) 2018 Lucas Timmins & Victor Berger` |
| `socket2` 0.6.5 | MIT OR Apache-2.0 | MIT | `Copyright (c) 2014 Alex Crichton` |
| `thiserror` 1.0.69 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `thiserror-impl` 1.0.69 | MIT OR Apache-2.0 | MIT | no notice in the package; authors per its manifest: David Tolnay |
| `tokio` 1.54.1 | MIT | MIT | `Copyright (c) Tokio Contributors` |
| `toml_datetime` 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | MIT | `Copyright (c) Individual contributors` |
| `toml_edit` 0.25.19+spec-1.1.0 | MIT OR Apache-2.0 | MIT | `Copyright (c) Individual contributors` |
| `toml_parser` 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | MIT | `Copyright (c) Individual contributors` |
| `tracing-attributes` 0.1.31 | MIT | MIT | `Copyright (c) 2019 Tokio Contributors` |
| `uuid` 1.29.2 | Apache-2.0 OR MIT | MIT | `Copyright (c) 2014 The Rust Project Developers`<br>`Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon` |
| `wayland-backend` 0.3.17 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-client` 0.31.15 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-csd-frame` 0.3.0 | MIT | MIT | `Copyright (c) 2023 Kirill Chibisov` |
| `wayland-cursor` 0.31.14 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-protocols` 0.32.13 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-protocols-experimental` 20250721.0.1 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-protocols-misc` 0.3.12 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-protocols-plasma` 0.3.12 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-protocols-wlr` 0.3.12 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-scanner` 0.31.11 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `wayland-sys` 0.31.11 | MIT | MIT | `Copyright (c) 2015 Elinor Berger` |
| `winnow` 1.0.5 | MIT | MIT | no notice in the package, and its manifest names no authors |
| `x11-dl` 2.21.0 | MIT | MIT | no notice in the package; authors per its manifest: daggerbot, Erle Pereira, AltF02 |
| `x11rb` 0.13.2 | MIT OR Apache-2.0 | MIT | `Copyright 2019 x11rb Contributers` |
| `x11rb-protocol` 0.13.2 | MIT OR Apache-2.0 | MIT | `Copyright 2019 x11rb Contributers` |
| `xcursor` 0.3.11 | MIT | MIT | `Copyright (c) 2020 Samuele Esposito` |
| `xkbcommon-dl` 0.4.2 | MIT | MIT | `Copyright (c) 2023 Kirill Chibisov` |
| `xkeysym` 0.2.1 | MIT OR Apache-2.0 OR Zlib | MIT | `Copyright (c) 2022-2023 John Nunley` |
| `zbus` 5.19.0 | MIT | MIT | `Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors` |
| `zbus_macros` 5.19.0 | MIT | MIT | `Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors` |
| `zbus_names` 4.3.5 | MIT | MIT | `Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors` |
| `zcheapstr` 1.2.0 | MIT | MIT | `Copyright (c) 2026 Zeeshan Ali Khan & zcheapstr contributors` |
| `zvariant` 5.15.1 | MIT | MIT | `Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors` |
| `zvariant_derive` 5.15.1 | MIT | MIT | `Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors` |
| `zvariant_utils` 4.2.0 | MIT | MIT | no notice in the package; authors per its manifest: Zeeshan Ali Khan, turbocooler |
