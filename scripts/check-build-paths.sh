#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Fails when a built program holds the path of a folder of the machine that built it.
#
#   scripts/check-build-paths.sh PROGRAM FOLDER...
#
# A program names the source files of its dependencies in its panic messages, by the paths those files had
# when it was built: without more, the build machine's home folder and checkout (some 600 times
# "/home/runner/.cargo/registry/src/..." in the Linux program of 0.9.7, which is what makepkg reports as a
# "reference to $srcdir"). The release builds give those folders other names with --remap-path-prefix, and
# this is the check that it took: the workflows run it on each program they publish, with the home folder
# and the checkout of the runner. A FOLDER is looked for as the start of a path, with either separator
# in it and after it, and whatever the case of its letters.
set -eu

[ $# -ge 2 ] || { echo "usage: check-build-paths.sh PROGRAM FOLDER..." >&2; exit 2; }
program=$1
shift
[ -f "$program" ] || { echo "check-build-paths: $program is not a file" >&2; exit 2; }

found=0
# shellcheck disable=SC1003  # the backslashes below are backslashes, quoted as they have to be
for folder; do
    # (an empty name would be the start of every path)
    [ -n "$folder" ] || { echo "check-build-paths: a folder without a name" >&2; exit 2; }
    # The program's bytes as lines of text, one for each run of printable letters (space to tilde, named
    # so because BusyBox's tr does not know the class [:print:]); and both the text and the folder with
    # one separator, the forward one.
    start=$(printf '%s/' "$folder" | tr '\\' '/')
    named=$(LC_ALL=C tr -c ' -~' '\n' <"$program" | tr '\\' '/' | LC_ALL=C grep -i -F -e "$start" || true)
    [ -n "$named" ] || continue
    found=1
    count=$(printf '%s\n' "$named" | wc -l | tr -d ' ')
    printf '%s\n' "check-build-paths: $program names $folder $count times, for example:" >&2
    printf '%s\n' "$named" | head -n 3 | cut -c 1-200 >&2
done
[ "$found" -eq 0 ] || exit 1
printf '%s\n' "check-build-paths: $program names none of: $*"
