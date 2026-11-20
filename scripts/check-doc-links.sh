#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Check that every relative link in the Markdown documents under a folder leads to a file under it:
#
#   scripts/check-doc-links.sh FOLDER [PACKAGE]
#
# For the Linux packages, as package.ps1 does for the Windows ZIP: 0.9.1's Linux archive and Arch package
# shipped a README.md whose screenshots and links to docs/TESTING.md, docs/PORTING.md and
# installer/README.md led nowhere. Links with a scheme (https:, mailto:) and links within a page (#...)
# are not looked at.
#
# A link has to lead to a file that is there and, with every symbolic link on the way followed, lies
# under FOLDER: a file beside the folder is on the machine that builds the package and not in it. Where
# the documents hold symbolic links into another folder of the same package (the Arch package's license
# files are links to usr/share/licenses), PACKAGE is the folder the whole package is laid out in, and
# the files have to lie under that.
set -eu

[ $# -ge 1 ] && [ $# -le 2 ] && [ -d "$1" ] && [ -d "${2:-$1}" ] ||
    { echo "usage: check-doc-links.sh FOLDER [PACKAGE]" >&2; exit 2; }

root=$(cd -- "$1" && pwd -P)
within=$(cd -- "${2:-$1}" && pwd -P)
broken=$(find "$root" -type f -name '*.md' | sort | while IFS= read -r doc; do
    # A document without a link makes grep fail; that is not an error here.
    # Markdown's ](target) and, for the pictures and links written as HTML (README.md's first picture is
    # one, and 0.9.2 shipped it leading nowhere), src="target" and href="target".
    { grep -o -e '\]([^)#[:space:]]*' -e 'src="[^"#]*' -e 'href="[^"#]*' "$doc" || true; } |
        sed -e 's/^](//' -e 's/^src="//' -e 's/^href="//' | sort -u | while IFS= read -r target; do
        case $target in '' | *:*) continue ;; esac
        # (asked first, and no option given: BusyBox's realpath knows none, and GNU's names a file that
        # is not there as long as its folder is. The path starts at the root, so it is no option itself.)
        resolved=
        if [ -e "$(dirname "$doc")/$target" ]; then
            resolved=$(realpath "$(dirname "$doc")/$target" 2>/dev/null) || resolved=
        fi
        case $resolved in "$within" | "${within%/}"/*) continue ;; esac
        printf '%s links to %s\n' "$doc" "$target"
    done
done)
if [ -n "$broken" ]; then
    printf '%s\n' "$broken" >&2
    echo "check-doc-links: the links above lead to files that are not under ${2:-$1}" >&2
    exit 1
fi
