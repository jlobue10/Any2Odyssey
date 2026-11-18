#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Write THIRD-PARTY-LICENSES-LINUX.md: the crates a Linux build contains beyond those of the Windows package.

THIRD-PARTY-LICENSES.md lists what the Windows package contains, with the license texts. A Linux build
compiles in further crates (the desktop portal and PipeWire bindings, ONNX Runtime's bindings, the Wayland and
X11 backends and what they depend on). This lists those, the license each is used under and the copyright
notices of its source package, by the rules the main file states. Run it from the repository root after
`cargo fetch`, whenever the dependencies change:

    scripts/third-party-linux.py > THIRD-PARTY-LICENSES-LINUX.md

It first checks THIRD-PARTY-LICENSES.md itself, which is written by hand: its two tables against the crates
of a Windows build and the license each one states, and the number its text gives. It stops with what
differs. The Linux list is "what the Windows list does not have", so it is only right while that list is;
and a crate only Windows builds (the windows bindings, the tray icon) was compared with nothing before.
"""
import glob, os, re, subprocess, sys

TARGET = "x86_64-unknown-linux-gnu"
WINDOWS_TARGET = "x86_64-pc-windows-msvc"

def tree(target):
    # Plain text whatever the environment says about colour (CI sets CARGO_TERM_COLOR=always).
    out = subprocess.run(["cargo", "tree", "--color", "never", "-p", "any2odyssey", "--target", target, "-e",
                          "normal", "--prefix", "none", "--format", "{p}|{l}"], capture_output=True, text=True,
                         check=True, env={**os.environ, "CARGO_TERM_COLOR": "never"}).stdout
    out = re.sub(r"\x1b\[[0-9;]*m", "", out)
    crates = {}
    for line in out.splitlines():
        m = re.match(r"(\S+) v(\S+)(?: \(.*?\))*\|(.*)$", line)
        if m and not m.group(1).startswith("any2odyssey"):
            crates[(m.group(1), m.group(2))] = m.group(3).replace(" (*)", "").strip()
    return crates

def listed_for_windows():
    listed = set()
    for line in open("THIRD-PARTY-LICENSES.md", encoding="utf-8"):
        m = re.match(r"\| `([^`]+)` \| (\S+) \|", line)
        if m: listed.add((m.group(1), m.group(2)))
    return listed

def check_windows_list():
    """Stop unless THIRD-PARTY-LICENSES.md lists exactly what a Windows build contains."""
    built = tree(WINDOWS_TARGET)
    listed, noticed, said = {}, set(), None
    for line in open("THIRD-PARTY-LICENSES.md", encoding="utf-8"):
        m = re.match(r"\| `([^`]+)` \| (\S+) \| (.*?) \|$", line)
        if m: listed[(m.group(1), m.group(2))] = m.group(3).strip()
        m = re.match(r"\| `([^`]+)` (\S+) \|", line)
        if m: noticed.add((m.group(1), m.group(2)))
        m = re.match(r"The (\d+) crates below", line)
        if m: said = int(m.group(1))
    wrong = ["%s %s is in a Windows build and not in the table of crates" % k for k in sorted(set(built) - set(listed))]
    wrong += ["%s %s is in the table of crates and not in a Windows build" % k for k in sorted(set(listed) - set(built))]
    wrong += ["%s %s is listed as %s; its manifest says %s" % (*k, listed[k], built[k])
              for k in sorted(set(built) & set(listed)) if listed[k] != built[k]]
    wrong += ["%s %s is in the table of crates and not in the table of notices" % k for k in sorted(set(listed) - noticed)]
    wrong += ["%s %s is in the table of notices and not in the table of crates" % k for k in sorted(noticed - set(listed))]
    if said != len(listed):
        wrong.append(f"the text says {said} crates; the table has {len(listed)}")
    if wrong:
        sys.exit("THIRD-PARTY-LICENSES.md does not match the dependencies of a Windows build "
                 f"(cargo tree -p any2odyssey --target {WINDOWS_TARGET} -e normal):\n  " + "\n  ".join(wrong))

def check_standard_library():
    """Stop unless THIRD-PARTY-LICENSES.md names the standard library of the toolchain that builds the programs.

    The standard library is in every program and in no `cargo tree`, so neither table has it and neither
    list named it up to 0.9.6. Its section is written by hand, with the crates that toolchain's library is
    built from: it has to be looked at again when rust-toolchain.toml changes."""
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', open("rust-toolchain.toml", encoding="utf-8").read(), re.M)
    text = open("THIRD-PARTY-LICENSES.md", encoding="utf-8").read()
    said = re.search(r"^## The Rust standard library\n\nBoth editions are built with Rust (\S+) ", text, re.M)
    if not channel:
        sys.exit("rust-toolchain.toml names no channel")
    if not said:
        sys.exit('THIRD-PARTY-LICENSES.md has no section "## The Rust standard library" that begins '
                 '"Both editions are built with Rust <version> "')
    if said.group(1) != channel.group(1):
        sys.exit(f"THIRD-PARTY-LICENSES.md describes the standard library of Rust {said.group(1)}; "
                 f"rust-toolchain.toml builds with {channel.group(1)}. Bring the section up to that toolchain "
                 "(its library/Cargo.lock names the crates and versions) and name it there.")

def used_under(expression):
    """MIT when it is one of the choices, Apache-2.0 otherwise; a single license as it is."""
    choices = [c.strip() for c in re.split(r"\s+OR\s+|/", expression)]
    if "MIT" in choices: return "MIT"
    for c in choices:
        if c == "Apache-2.0": return "Apache-2.0"
    return choices[0]

def package_dir(name, version):
    found = glob.glob(os.path.expanduser(f"~/.cargo/registry/src/*/{name}-{version}"))
    if not found: sys.exit(f"{name} {version} is not in the cargo registry; run cargo fetch")
    return found[0]

def notices(name, version, license_used):
    directory = package_dir(name, version)
    files = sorted(f for f in os.listdir(directory)
                   if re.match(r"(?i)(licen[cs]e|copying|copyright|notice|unlicense)", f)
                   and os.path.isfile(os.path.join(directory, f)))
    # The file of the license in use first; a file of another license of the choice is left out.
    other = "APACHE" if license_used == "MIT" else "MIT"
    files = [f for f in files if other not in f.upper() or license_used.split("-")[0].upper() in f.upper()]
    lines = []
    for f in files:
        for line in open(os.path.join(directory, f), encoding="utf-8", errors="replace"):
            line = line.strip().strip("*/# ").strip()
            if re.match(r"(?i)copyright\b", line) and re.search(r"\d{4}|\(c\)|©", line, re.I) \
                    and "[yyyy]" not in line and "{yyyy}" not in line and line not in lines:
                lines.append(line)
    if lines:
        return "<br>".join("`%s`" % l.replace("`", "'").replace("|", "/") for l in lines)
    manifest = open(os.path.join(directory, "Cargo.toml"), encoding="utf-8").read()
    m = re.search(r"^authors\s*=\s*\[(.*?)\]", manifest, re.S | re.M)
    authors = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in re.findall(r'"([^"]+)"', m.group(1))] if m else []
    if authors:
        return "no notice in the package; authors per its manifest: " + ", ".join(authors)
    return "no notice in the package, and its manifest names no authors"

def main():
    # The file is UTF-8 whatever the locale says: the notices have letters that Latin-1 cannot write.
    sys.stdout.reconfigure(encoding="utf-8")
    check_windows_list()
    check_standard_library()
    windows = listed_for_windows()
    extra = sorted((k, v) for k, v in tree(TARGET).items() if k not in windows)
    print("# Third-party licenses: the Linux build\n")
    print("`THIRD-PARTY-LICENSES.md` lists the works the Windows package contains and gives the texts of their "
          "licenses. A Linux build of Any2Odyssey contains most of those crates and, beyond them, the "
          f"{len(extra)} crates below: the normal (non-build, non-test) dependencies of the `any2odyssey` "
          f"binary for `{TARGET}` that the Windows list does not have, as `Cargo.lock` resolves them. They are "
          "used by the same rule as there: under the MIT license when a crate offers it, under the Apache "
          "License 2.0 otherwise, and a crate with a single license under that one. The texts of the MIT, "
          "Apache 2.0, ISC and zlib licenses are in `THIRD-PARTY-LICENSES.md`. The notices are the copyright "
          "lines of the license and copyright files in each crate's source package as published on crates.io; "
          "where a package has none, the authors field of its manifest is given and marked as such.\n")
    print("Like the Windows program, the Linux one also contains the Rust standard library and the crates it "
          "is built from; they are under \"The Rust standard library\" in `THIRD-PARTY-LICENSES.md`.\n")
    print("Libraries the Linux build links at run time and does not contain (PipeWire's library, the C library, "
          "the desktop's graphics libraries, ONNX Runtime and cuDNN where installed) are the system's own, under "
          "their own terms. Samsung's and Leia's software is not included: the Wine setup in "
          "`contrib/wine-lens` copies the user's own installation.\n")
    # Not a crate, so not in the table: `nm` on the published 0.9.6 program shows spa_pod_parser_get and six
    # more functions of these headers, compiled in by libspa-sys (its bindings wrap the headers' inline
    # functions in a C file of their own), and the lists said that nothing of PipeWire is contained.
    print("One part of PipeWire is contained: its SPA headers define small functions in the headers "
          "themselves (`spa/pod/parser.h`, `spa/pod/iter.h` and the format headers under `spa/param`), and "
          "the `libspa-sys` crate compiles those the program uses into it, from the PipeWire development "
          "files of the system the program is built on. They are under PipeWire's license, with this "
          "notice:\n")
    print("```text\n"
          "Copyright © 2018 Wim Taymans\n"
          "\n"
          "Permission is hereby granted, free of charge, to any person obtaining a\n"
          "copy of this software and associated documentation files (the \"Software\"),\n"
          "to deal in the Software without restriction, including without limitation\n"
          "the rights to use, copy, modify, merge, publish, distribute, sublicense,\n"
          "and/or sell copies of the Software, and to permit persons to whom the\n"
          "Software is furnished to do so, subject to the following conditions:\n"
          "\n"
          "The above copyright notice and this permission notice (including the next\n"
          "paragraph) shall be included in all copies or substantial portions of the\n"
          "Software.\n"
          "\n"
          "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\n"
          "IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\n"
          "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL\n"
          "THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\n"
          "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING\n"
          "FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER\n"
          "DEALINGS IN THE SOFTWARE.\n"
          "```\n")
    print("Regenerate with `scripts/third-party-linux.py > THIRD-PARTY-LICENSES-LINUX.md` after changing "
          "dependencies.\n")
    print("| Crate and version | License offered | Used under | Copyright notices |")
    print("| --- | --- | --- | --- |")
    for (name, version), expression in extra:
        used = used_under(expression)
        print(f"| `{name}` {version} | {expression} | {used} | {notices(name, version, used)} |")

main()
