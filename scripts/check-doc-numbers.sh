#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Check that the documents and the text of the release page name the oldest C library and the oldest
# ONNX Runtime that the sources name:
#
#   scripts/check-doc-numbers.sh [TREE]
#
# TREE is the checkout (the one this script is in by default).
#
# The two numbers are decided in one place each: GLIBC_MIN in packaging/linux/install.sh (the release
# workflow compares it with the program it has built) and the `api-<minor>` feature of the ONNX Runtime
# binding in crates/app/Cargo.toml (a test of the program compares it with the binding). README.md,
# INSTALL.txt, docs/PLATFORMS.md and the release text in release.yml repeat them for people. 0.9.5 lowered
# both, from 2.39 to 2.35 and from 1.22 to 1.17, and the release text was not changed with them: the pages
# of 0.9.5 and 0.9.6 told users of Debian 12, Ubuntu 22.04 and Debian 13 that the archive, or its 2D to 3D,
# was not for them. Wherever one of these files says "glibc 2.N or newer" or "ONNX Runtime 1.N or newer",
# N has to be the sources' number, and the files that must say it are looked at for it.
set -eu

tree=${1:-"$(dirname "$0")/.."}
[ -f "$tree/packaging/linux/install.sh" ] || { echo "usage: check-doc-numbers.sh [TREE]" >&2; exit 2; }

glibc=$(sed -n 's/^GLIBC_MIN=//p' "$tree/packaging/linux/install.sh")
ort=$(sed -n 's/^ort = .*"api-\([0-9][0-9]*\)".*$/\1/p' "$tree/crates/app/Cargo.toml")
case $glibc in 2.[0-9]*) ;; *) echo "check-doc-numbers: no GLIBC_MIN=2.N in packaging/linux/install.sh" >&2; exit 1 ;; esac
case $ort in [0-9]*) ;; *) echo "check-doc-numbers: no api-N feature of ort in crates/app/Cargo.toml" >&2; exit 1 ;; esac

failed=0
# <file> <what it must say at least once: glibc, ort, both or nothing>
look() {
    file=$1 must=$2
    [ -f "$tree/$file" ] || { echo "check-doc-numbers: $file is missing" >&2; failed=1; return; }
    said=$(grep -o -e 'glibc 2\.[0-9][0-9]* or newer' "$tree/$file" | sort -u)
    for phrase in $(printf '%s\n' "$said" | tr ' ' '_'); do
        [ "$phrase" = "glibc_${glibc}_or_newer" ] ||
            { echo "$file says \"$(printf %s "$phrase" | tr '_' ' ')\"; install.sh says GLIBC_MIN=$glibc"; failed=1; }
    done
    case $must in glibc | both)
        [ -n "$said" ] || { echo "$file does not say \"glibc $glibc or newer\""; failed=1; } ;;
    esac
    # "ONNX Runtime 1.17 or newer" and INSTALL.txt's "ONNX Runtime, 1.17 or newer"
    asked=$(grep -o -e 'ONNX Runtime,\{0,1\} 1\.[0-9][0-9]* or newer' "$tree/$file" | sed 's/,//' | sort -u)
    for phrase in $(printf '%s\n' "$asked" | tr ' ' '_'); do
        [ "$phrase" = "ONNX_Runtime_1.${ort}_or_newer" ] ||
            { echo "$file says \"$(printf %s "$phrase" | tr '_' ' ')\"; the binding in crates/app/Cargo.toml is built for 1.$ort (api-$ort)"; failed=1; }
    done
    case $must in ort | both)
        [ -n "$asked" ] || { echo "$file does not say \"ONNX Runtime 1.$ort or newer\""; failed=1; } ;;
    esac
}
look README.md both
look packaging/linux/INSTALL.txt both
look docs/PLATFORMS.md glibc
look .github/workflows/release.yml both
if [ "$failed" -ne 0 ]; then
    echo "check-doc-numbers: the documents above do not name what the sources name" >&2
    exit 1
fi
echo "check-doc-numbers: glibc $glibc and ONNX Runtime 1.$ort, in README.md, INSTALL.txt, docs/PLATFORMS.md and the release text"
