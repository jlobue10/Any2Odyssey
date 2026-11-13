#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Check that every relative link in the Markdown documents under a folder leads to a file under it:
#
#   scripts/check-doc-links.sh FOLDER
#
# For the Linux packages, as package.ps1 does for the Windows ZIP: 0.9.1's Linux archive and Arch package
# shipped a README.md whose screenshots and links to docs/TESTING.md, docs/PORTING.md and
# installer/README.md led nowhere. Links with a scheme (https:, mailto:) and links within a page (#...)
# are not looked at.
set -eu

[ $# -eq 1 ] && [ -d "$1" ] || { echo "usage: check-doc-links.sh FOLDER" >&2; exit 2; }

broken=$(find "$1" -type f -name '*.md' | sort | while IFS= read -r doc; do
    # A document without a link makes grep fail; that is not an error here.
    { grep -o '\]([^)#[:space:]]*' "$doc" || true; } | sed 's/^](//' | sort -u | while IFS= read -r target; do
        case $target in '' | *:*) continue ;; esac
        [ -e "$(dirname "$doc")/$target" ] || printf '%s links to %s\n' "$doc" "$target"
    done
done)
if [ -n "$broken" ]; then
    printf '%s\n' "$broken" >&2
    echo "check-doc-links: the links above lead to files that are not under $1" >&2
    exit 1
fi
