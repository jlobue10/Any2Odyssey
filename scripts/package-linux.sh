#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Stage the Linux release archive from a finished release build:
#
#   cargo build --locked --release -p any2odyssey && scripts/package-linux.sh
#
# Writes dist/Any2Odyssey-<version>-linux-<arch>.tar.gz and its .sha256. The archive holds the executable,
# the installer, the documents and licenses, the desktop entry and icon, and contrib/wine-lens (the setup for the Odyssey
# 3D's lens, eye tracker and weaver), so nothing else of the source tree is needed to use it.
set -eu

cd "$(dirname "$(readlink -f "$0")")/.."
VERSION=$(tr -d '[:space:]' <VERSION)
ARCH=$(uname -m)
EXE=${CARGO_TARGET_DIR:-target}/release/Any2Odyssey
NAME=Any2Odyssey-$VERSION-linux-$ARCH
STAGE=dist/$NAME

[ -x "$EXE" ] || { echo "package-linux: $EXE is missing; build it first" >&2; exit 1; }
rm -rf "$STAGE" "dist/$NAME.tar.gz" "dist/$NAME.tar.gz.sha256"
mkdir -p "$STAGE/docs/assets" "$STAGE/installer" "$STAGE/assets"

install -m755 "$EXE" "$STAGE/any2odyssey"
install -m644 README.md CHANGELOG.md USER-GUIDE.html LICENSE NOTICE VERSION \
    THIRD-PARTY-LICENSES.md THIRD-PARTY-LICENSES-LINUX.md "$STAGE/"
# README.md links to every document in docs, to its screenshots and to installer/README.md; they keep
# those paths in the archive, so the packaged copy has no link or picture that leads nowhere.
install -m644 docs/*.md "$STAGE/docs/"
install -m644 docs/assets/*.png "$STAGE/docs/assets/"
install -m644 installer/README.md "$STAGE/installer/README.md"
# (and to the picture at its top, which is written as HTML)
install -m644 assets/Any2Odyssey-wordmark.png "$STAGE/assets/Any2Odyssey-wordmark.png"
install -m644 packaging/linux/any2odyssey.desktop "$STAGE/any2odyssey.desktop"
install -m644 assets/Any2Odyssey.png "$STAGE/any2odyssey.png"
install -m644 packaging/linux/INSTALL.txt "$STAGE/INSTALL.txt"
install -m755 packaging/linux/install.sh "$STAGE/install.sh"
# The Wine setup, with its file modes (some of it is scripts).
cp -R contrib/wine-lens "$STAGE/wine-lens"
# Its tests are for a checkout: nothing in the archive runs them (0.9.1 shipped them).
rm -rf "$STAGE/wine-lens/tests"
# Python leaves its compiled copy of tools/mkedid.py beside it when tests/test-mkedid.py has run in this
# tree (CI runs it before this script); that is no part of the archive.
find "$STAGE/wine-lens" -name __pycache__ -type d -prune -exec rm -rf {} +
install -m644 contrib/LICENSE-MIT "$STAGE/wine-lens/LICENSE-MIT"
sh scripts/check-doc-links.sh "$STAGE"

# The same archive from the same files: fixed order, owner and times.
EPOCH=$(git log -1 --format=%ct 2>/dev/null || date +%s)
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$EPOCH" -C dist -czf "dist/$NAME.tar.gz" "$NAME"
(cd dist && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
rm -rf "$STAGE"
echo "package-linux: dist/$NAME.tar.gz"
