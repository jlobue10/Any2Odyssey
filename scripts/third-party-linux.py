#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Write THIRD-PARTY-LICENSES-LINUX.md: the crates a Linux build contains beyond those of the Windows package.

THIRD-PARTY-LICENSES.md lists what the Windows package contains, with the license texts. A Linux build
compiles in further crates (the desktop portal and PipeWire bindings, ONNX Runtime's bindings, the Wayland and
X11 backends and what they depend on). This lists those, the license each is used under and the copyright
notices of its source package, by the rules the main file states. Run it from the repository root after
`cargo fetch`, whenever the dependencies change:

    scripts/third-party-linux.py > THIRD-PARTY-LICENSES-LINUX.md
"""
import glob, os, re, subprocess, sys

TARGET = "x86_64-unknown-linux-gnu"

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
    print("Libraries the Linux build links at run time and does not contain (PipeWire, the C library, the "
          "desktop's graphics libraries, ONNX Runtime and cuDNN where installed) are the system's own, under "
          "their own terms. Samsung's and Leia's software is not included: the Wine setup in "
          "`contrib/wine-lens` copies the user's own installation.\n")
    print("Regenerate with `scripts/third-party-linux.py > THIRD-PARTY-LICENSES-LINUX.md` after changing "
          "dependencies.\n")
    print("| Crate and version | License offered | Used under | Copyright notices |")
    print("| --- | --- | --- | --- |")
    for (name, version), expression in extra:
        used = used_under(expression)
        print(f"| `{name}` {version} | {expression} | {used} | {notices(name, version, used)} |")

main()
