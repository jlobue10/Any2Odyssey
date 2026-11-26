#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Install Any2Odyssey for Linux from the release archive, with what it needs around it.
#
#   ./install.sh                 install for this user and ask about each further step
#   ./install.sh --yes           the same, answering yes to every question
#   ./install.sh --uninstall     remove what this script installed
#
# What it does, in order (every step that needs root says so and uses sudo for that step only):
#   1. copies the program, its desktop entry, icon, documents and the Wine setup to ~/.local;
#   2. installs the packages the program needs, and the ones for the features you choose
#      (Arch, Debian/Ubuntu, Fedora and openSUSE families; elsewhere it lists what is missing);
#   3. installs the udev rules for the Odyssey 3D (lens board for the user at the seat, USB power saving off
#      for the monitor's webcam);
#   4. downloads the open depth model for 2D -> 3D (about 95 MB);
#   5. prepares the Odyssey 3D's lens, eye tracker and weaver under Wine from your own Windows installation
#      of the Odyssey 3D Hub, if it finds one mounted or is given one. This part expects a dual-boot PC:
#      Windows with the Odyssey 3D Hub on a drive of this machine, which has shown 3D on this monitor at
#      least once. Mount its system partition (read-only is enough) before running this script;
#   6. only with --scaler (it is experimental; no question is asked about it, and --yes does not agree to
#      it): has the Odyssey 3D output switch the monitor's scaler to its 3D mode, with the udev rule that
#      lets you send it the command.
#
# Options:
#   --yes               answer yes to every question
#   --no-deps           do not install packages (still says what is missing)
#   --no-udev           do not install the udev rules
#   --no-model          do not download the depth model
#   --no-odyssey        do not prepare the Wine side
#   --scaler            do step 6: the Odyssey 3D output switches the scaler's 3D mode (docs/LENS-SWITCH.md)
#   --no-scaler         switch that off again (the udev rule stays until --uninstall)
#   --windows-root DIR  the mounted Windows drive that has the Odyssey 3D Hub (the folder with "Program Files");
#                       needed when it is on a USB drive, a memory card or a network share, or when several
#                       drives have one
#   --prefix DIR        install under DIR instead of ~/.local (the installed copy of this script, which
#                       --uninstall is run from, knows where it was installed without it)
#   --packaged          the program is installed already (a distribution package): do steps 2 to 6 only
#   --dry-run           say what would be done and do none of it
#   --uninstall         remove the installed files (and, when confirmed, the udev rules and the Wine side)
set -eu
# Nothing this script makes is writable by others, whatever the shell's umask is: the program's entry in the
# menu and the Wine setup are run as the user later. A stricter umask is kept.
umask go-w

HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
PREFIX=$HOME/.local PREFIX_GIVEN=
YES= DEPS=1 UDEV=1 MODEL=1 ODYSSEY=1 SCALER= PACKAGED= DRY= UNINSTALL= WINROOT=
# For packaging/linux/tests only: a folder that stands in for / where this script looks for the lens board,
# for mounted drives (with the mount table and the /sys entries that describe them) and for the udev rules.
# It comes from the environment, so nothing it names reaches a command run as root: while it is set no step
# uses sudo, and udevadm, which acts on the real system whatever stands in for /, is named and not run.
TEST_ROOT=${A2O_INSTALL_TEST_ROOT:-}
RULES_NAME=70-any2odyssey-lens.rules
# The rule for the scaler's vendor device, which only step 6 installs.
SCALER_RULES_NAME=71-any2odyssey-scaler.rules
# The rules folder follows the stand-in. It was the real /etc/udev/rules.d in a test run too, with the udev
# step switched off there: no test could see what that step or its removal does, and the one that looked
# whether a rules file under the stand-in was left alone passed whatever this script did.
RULES_DIR=$TEST_ROOT/etc/udev/rules.d
PACKAGED_RULES_DIR=$TEST_ROOT/usr/lib/udev/rules.d
# The program in the archive is built for the GNU C library on x86_64: the kernel starts it through this
# loader, and it needs at least this version of the library (the workflows compare the number with the
# program they have built).
LOADER=/lib64/ld-linux-x86-64.so.2
GLIBC_MIN=2.35
# The oldest Wine the Odyssey 3D set-up takes. The number is wine-lens/setup.sh's (wine_too_old there says
# why, and refuses an older one); packaging/linux/tests compares the two.
WINE_MIN=11.6
# The steps that need root go through sudo; a root shell runs them itself (not every system has sudo).
SUDO=sudo
if [ "$(id -u)" = 0 ] || [ -n "$TEST_ROOT" ]; then SUDO=; fi

while [ $# -gt 0 ]; do
    case $1 in
        --yes | -y) YES=1 ;;
        --no-deps) DEPS= ;;
        --no-udev) UDEV= ;;
        --no-model) MODEL= ;;
        --no-odyssey) ODYSSEY= ;;
        --scaler) SCALER=yes ;;
        --no-scaler) SCALER=no ;;
        --packaged) PACKAGED=1 ;;
        --dry-run) DRY=1 ;;
        --uninstall) UNINSTALL=1 ;;
        --windows-root) [ $# -ge 2 ] && [ -n "${2%/}" ] || { echo "install: --windows-root needs a folder" >&2; exit 2; }; WINROOT=${2%/}; shift ;;
        --prefix) [ $# -ge 2 ] || { echo "install: --prefix needs a folder" >&2; exit 2; }; PREFIX=$2 PREFIX_GIVEN=1; shift ;;
        -h | --help) awk 'NR >= 3 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "$0"; exit 0 ;;
        *) echo "install: unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

# The copy of this script that an installation keeps for --uninstall (<prefix>/share/any2odyssey/install.sh,
# with no program beside it, unlike the archive) acts on the installation it is part of: after --prefix it
# would otherwise look in ~/.local, remove nothing of its own and whatever another installation has there.
if [ -z "$PREFIX_GIVEN" ] && [ -z "$PACKAGED" ] && [ ! -e "$HERE/any2odyssey" ]; then
    # Where share is a link to another place (~/.local/share kept on another drive), the installation is
    # where this script was called, with its program in bin there; where the link leads there is no bin,
    # and the program would be left behind.
    AT=$HERE
    CALLED=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || CALLED=
    case $CALLED in
        */share/any2odyssey) [ ! -e "${CALLED%/share/any2odyssey}/bin/any2odyssey" ] || AT=$CALLED ;;
    esac
    # Called from the folder itself in a shell that follows links (cd -P, set -P, zsh's CHASE_LINKS), the
    # name it was called by is where the link leads as well. The usual place is then asked whether it
    # leads here: without this the program in ~/.local/bin stayed, and the run reported success.
    USUAL=$HOME/.local/share/any2odyssey
    if [ "$AT" = "$HERE" ] && [ ! -e "${HERE%/share/any2odyssey}/bin/any2odyssey" ] && [ -e "$HOME/.local/bin/any2odyssey" ] &&
        [ "$(cd "$USUAL" 2>/dev/null && pwd -P)" = "$HERE" ]; then
        AT=$USUAL
    fi
    case $AT in
        */share/any2odyssey)
            # A distribution's package keeps this script in the same place and runs it through
            # any2odyssey-setup, which says --packaged. Run by its own name it is still the package's copy:
            # the files beside it are the package manager's to remove, not this script's.
            if [ -e "${AT%/share/any2odyssey}/bin/any2odyssey-setup" ]; then
                PACKAGED=1
            else
                PREFIX=${AT%/share/any2odyssey}
                PREFIX=${PREFIX:-/}
            fi
            ;;
    esac
fi
# Desktop entries are line based, and an empty prefix must not select /bin.
if [ -z "$PREFIX" ] || [ "$PREFIX" != "$(printf '%s' "$PREFIX" | tr -d '\r\n')" ]; then
    echo "install: --prefix must be a nonempty folder without line breaks" >&2
    exit 2
fi
# A launcher must work independently of the installer's working directory, so the prefix is made absolute,
# with its symbolic links followed as far as the path exists. GNU readlink does that; without it (BusyBox)
# the nearest folder that exists is resolved and the rest of the path is kept as written.
canonical() {
    readlink -m -- "$1" 2>/dev/null && return 0
    case $1 in /*) path=$1 ;; *) path=$PWD/$1 ;; esac
    rest=
    while [ ! -d "$path" ]; do
        while [ "${path%/}" != "$path" ]; do path=${path%/}; done
        rest=/${path##*/}$rest
        path=${path%/*}
        [ -n "$path" ] || path=/
    done
    path=$(cd "$path" && pwd -P) || return 1
    path=${path%/}$rest
    printf '%s\n' "${path:-/}"
}
PREFIX=$(canonical "$PREFIX") && [ -n "$PREFIX" ] || { echo "install: could not resolve the --prefix folder" >&2; exit 2; }
WANT_DEPS=$DEPS # as asked for: a test run still looks which of the packages exist
if [ -n "$TEST_ROOT" ]; then
    echo "note: A2O_INSTALL_TEST_ROOT is set (a test run): drives, the monitor and the udev rules are looked for under $TEST_ROOT, no packages are installed, and nothing is run with sudo or udevadm"
    DEPS=
fi

say() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
warn() { printf 'note: %s\n' "$*"; }
# Run a command, or only show it with --dry-run.
run() {
    if [ -n "$DRY" ]; then printf '   would run: %s\n' "$*"; else "$@"; fi
}
# Ask a yes/no question; yes with --yes, no when there is nobody to ask.
# With "no" as a second word the question is about removing something that is most likely meant to stay:
# Enter means no, and so does --yes, which agrees to the steps of this script and not to that.
ask() {
    if [ "${2:-}" = no ]; then
        [ -z "$YES" ] || { say "   ($1 -- no: --yes does not agree to this)"; return 1; }
        [ -t 0 ] || { say "   ($1 -- no: not asked without a terminal)"; return 1; }
        printf '%s [y/N] ' "$1"
        read -r answer || return 1
        case $answer in y | Y | yes | Yes) return 0 ;; *) return 1 ;; esac
    fi
    [ -n "$YES" ] && return 0
    [ -t 0 ] || { say "   ($1 -- skipped: not asked without a terminal; use --yes)"; return 1; }
    printf '%s [Y/n] ' "$1"
    read -r answer || return 1
    case $answer in '' | y | Y | yes | Yes) return 0 ;; *) return 1 ;; esac
}
have() { command -v "$1" >/dev/null 2>&1; }
# udevadm as root, or only shown with --dry-run and in a test run (see TEST_ROOT).
udev() {
    if [ -n "$TEST_ROOT" ]; then printf '   would run: udevadm %s\n' "$*"; else run $SUDO udevadm "$@"; fi
}
# Not root and no sudo to become it (Debian installed with a root password has none): the steps that need
# root are named for the user to run as root instead of being tried.
no_sudo() { [ -n "$SUDO" ] && ! have sudo; }
# A word as it can be pasted into a shell.
quoted() {
    case $1 in
        '' | *[!A-Za-z0-9_./+:@%,-]*) printf "'%s'\n" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

# The first folder, from the nearest one that exists at or above $1 up to /, whose entries somebody other
# than root can change: one that is not root's, or one that others may write to. A sticky folder such as
# /tmp counts only as that nearest one, where this script would make something itself; further up nobody
# but root can rename or replace what is root's in it. Prints nothing when every folder is root's alone.
# A group's write permission is not counted: Debian's /usr/local is the staff group's to write.
foreign_folder() {
    at=$1
    while [ ! -d "$at" ]; do
        at=${at%/*}
        [ -n "$at" ] || at=/
    done
    at=$(cd "$at" && pwd -P) || { printf '%s\n' "$1"; return 0; }
    nearest=1
    while :; do
        facts=$(stat -c '%u %a' -- "$at" 2>/dev/null) || { printf '%s\n' "$at"; return 0; }
        mode=${facts#* }
        case ${mode#"${mode%?}"} in 2 | 3 | 6 | 7) open=1 ;; *) open= ;; esac
        sticky=
        [ ${#mode} -lt 4 ] || case ${mode%???} in 1 | 3 | 5 | 7) sticky=1 ;; esac
        if [ "${facts% *}" != 0 ] || { [ -n "$open" ] && { [ -z "$sticky" ] || [ -n "$nearest" ]; }; }; then
            printf '%s\n' "$at"
            return 0
        fi
        [ "$at" != / ] || return 0
        at=${at%/*}
        [ -n "$at" ] || at=/
        nearest=
    done
}

BIN=$PREFIX/bin
SHARE=$PREFIX/share
APPDIR=$SHARE/any2odyssey
DOCDIR=$SHARE/doc/any2odyssey
# An XDG_DATA_HOME that is not an absolute path counts as unset (the XDG Base Directory specification), and
# that is how the program reads it: taken as written, this script looked for the model and built the Wine
# side in a folder under wherever it was run from, where the program never looks.
case ${XDG_DATA_HOME:-} in /*) DATA_HOME=$XDG_DATA_HOME ;; *) DATA_HOME=$HOME/.local/share ;; esac
DATA=${ANY2ODYSSEY_DATA_DIR:-$DATA_HOME/any2odyssey}
if [ -n "$PACKAGED" ]; then
    WINE_LENS=$HERE
    [ -f "$WINE_LENS/setup.sh" ] || WINE_LENS=/usr/share/any2odyssey/wine-lens
    PROGRAM=$(command -v any2odyssey || echo /usr/bin/any2odyssey)
else
    WINE_LENS=$APPDIR/wine-lens
    # Installed to ~/.local with the data folder in its usual place, share/any2odyssey is the program's data
    # folder as well, and wine-lens in it is what setup.sh builds there: the Wine prefix with the copy of
    # Leia's runtime, the stand-ins and the camera secret. The setup's own files go beside that, under
    # another name: as one folder, installing again removed the Wine side, and setup.sh could not copy its
    # tools onto themselves.
    [ "$(canonical "$APPDIR")" != "$(canonical "$DATA")" ] || WINE_LENS=$APPDIR/wine-lens-setup
    # The same when a Wine side has been built in share/any2odyssey/wine-lens although this run is told of
    # another data folder (XDG_DATA_HOME or ANY2ODYSSEY_DATA_DIR differs from the run that built it, as in
    # a sandboxed terminal): replacing the setup's files there would remove it.
    if [ -e "$APPDIR/wine-lens/prefix" ] || [ -e "$APPDIR/wine-lens/build" ]; then WINE_LENS=$APPDIR/wine-lens-setup; fi
    PROGRAM=$BIN/any2odyssey
fi
# share/any2odyssey/wine-lens is the setup's files and nothing else (as every installation outside the data
# folder has it, and 0.9.1 left it in the data folder too) when no Wine side was built in it.
only_the_setup() { [ -f "$APPDIR/wine-lens/setup.sh" ] && [ ! -e "$APPDIR/wine-lens/prefix" ] && [ ! -e "$APPDIR/wine-lens/build" ]; }
# Where 0.9.0 or 0.9.1 built a Wine side in it, their setup's own files are still among it: they go, by
# name. That setup.sh, run again, puts the old stand-ins into the prefix and stops as it did then. What
# setup.sh keeps there under the same names (tools/*.sh, the two helpers) is the Wine side's and stays.
drop_the_old_setup() {
    [ "$WINE_LENS" != "$APPDIR/wine-lens" ] && [ -f "$APPDIR/wine-lens/setup.sh" ] || return 0
    for old in setup.sh src tests README.md LICENSE-MIT "$RULES_NAME" tools/mkboard.py; do
        run rm -rf "$APPDIR/wine-lens/$old"
    done
}

# Tell GTK that the icons under the prefix have changed. An icon cache that is there is made again. Where
# there is none (the usual state of ~/.local/share/icons/hicolor) none is made: up to 1.0.0 this script made
# one, --uninstall left it, and GTK then believed that cache and did not see the icons other programs put
# there later without refreshing it. The folder's time is set instead, which is what makes GTK look again
# (and pass over a cache that could not be refreshed).
refresh_icons() {
    [ -d "$SHARE/icons/hicolor" ] || return 0
    if [ -f "$SHARE/icons/hicolor/icon-theme.cache" ] && have gtk-update-icon-cache &&
        run gtk-update-icon-cache -q -t "$SHARE/icons/hicolor" >/dev/null 2>&1; then
        return 0
    fi
    run touch "$SHARE/icons/hicolor" >/dev/null 2>&1 || true
}

# One run at a time puts files into a prefix or takes them out, the others wait their turn. The program and
# the desktop entry are each moved into place whole, but the rest is copied file by file and the Wine setup
# is removed and copied again: of two runs started together (0.9.4) one stopped at "File exists" in most
# pairs, and over an installation that was there some pairs ended with setup.sh and others of the Wine
# setup's files gone, or with a second copy of it inside the first, while a run said Done. A run racing
# --uninstall left half an installation. The lock is on the prefix folder itself (no file of its own to
# leave behind) and goes with this process however it ends. Without flock (util-linux), or in a folder
# that has no locks, the run goes on unlocked, as before. A run that has waited five minutes for another
# one, or whose wait fails, stops and changes nothing: up to 0.9.9 it went on beside the other run, which
# is what the lock is there to prevent.
lock_prefix() {
    [ -z "$DRY" ] && have flock && [ -d "$PREFIX" ] || return 0
    # (asked first: a redirection that fails on exec ends the whole script)
    ( : <"$PREFIX" ) 2>/dev/null || return 0
    exec 9<"$PREFIX"
    held=0
    flock -n 9 2>/dev/null || held=$?
    [ "$held" = 1 ] || return 0
    say "   another installation or removal is at work in $PREFIX; waiting for it to finish"
    if have timeout; then
        # --foreground: by itself GNU's timeout moves into a process group of its own, where Ctrl+C at the
        # terminal reaches neither it nor the flock it started. bash then waited on, for up to the five
        # minutes, and went on to install after the Ctrl+C; dash ended, but only when the wait was over.
        # BusyBox's timeout does not know the option and stays in this group anyway.
        waiting="timeout 300"
        if timeout --foreground 300 true 2>/dev/null; then waiting="timeout --foreground 300"; fi
        if $waiting flock 9 2>/dev/null; then return 0; fi
    else
        if flock 9 2>/dev/null; then return 0; fi
    fi
    echo "install: could not acquire the busy lock on $PREFIX; retry when the other run finishes" >&2
    exit 1
}
# Before anything is started that stays (the program, the Wine setup) or asks a question.
unlock_prefix() { exec 9<&-; }

# ---------------------------------------------------------------------------------------------------------
if [ -n "$UNINSTALL" ]; then
    step "Removing Any2Odyssey from $PREFIX"
    if [ -z "$PACKAGED" ]; then
        lock_prefix
        run rm -f "$BIN/any2odyssey" "$BIN/.any2odyssey.new" "$SHARE/applications/any2odyssey.desktop" \
            "$SHARE/icons/hicolor/256x256/apps/any2odyssey.png"
        # The desktop entry's staging folder, where a run that was killed left one (a link is only a name).
        for stale in "$SHARE/applications/any2odyssey.desktop".??????????; do
            if [ -d "$stale" ] && [ ! -L "$stale" ]; then
                run rm -f -- "$stale/entry"
                run rmdir -- "$stale" 2>/dev/null || true
            fi
        done
        # The same for the copy of the program that such a run was writing.
        for stale in "$BIN"/.any2odyssey.new.??????????; do
            if [ -f "$stale" ] && [ ! -L "$stale" ]; then run rm -f -- "$stale"; fi
        done
        # Only what this script put there, named in full. share/any2odyssey can be the program's data
        # folder (see WINE_LENS above): the settings, the depth model and a Wine side that was built stay,
        # as the last lines say; the Wine side is asked about below.
        run rm -f "$APPDIR/install.sh"
        [ -d "$APPDIR/wine-lens-setup" ] && run rm -rf "$APPDIR/wine-lens-setup"
        if only_the_setup; then run rm -rf "$APPDIR/wine-lens"; else drop_the_old_setup; fi
        if [ -z "$DRY" ] && [ -d "$APPDIR" ]; then rmdir "$APPDIR" 2>/dev/null || true; fi
        [ -d "$PREFIX/share/doc/any2odyssey" ] && run rm -rf "$PREFIX/share/doc/any2odyssey"
        unlock_prefix
        # The caches the installation refreshed still name the entry and the icon that are gone now.
        [ -d "$SHARE/applications" ] && have update-desktop-database && run update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
        refresh_icons
    else
        say "   the program itself belongs to your distribution's package; remove it with the package manager"
    fi
    if [ -f "$RULES_DIR/$RULES_NAME" ]; then
        if no_sudo; then
            say "   the udev rules stay (this system has no sudo); as root: rm -f $RULES_DIR/$RULES_NAME && udevadm control --reload"
        elif ask "Remove the udev rules ($RULES_DIR/$RULES_NAME${SUDO:+, needs sudo})?"; then
            # (a refusal by sudo must not end the removal half-way: the Wine side and the last lines follow)
            { run $SUDO rm -f "$RULES_DIR/$RULES_NAME" && udev control --reload; } ||
                warn "the udev rules could not be removed or reloaded (see the message above)"
        fi
    fi
    # The scaler's rule, where step 6 installed it. (scaler.on goes with the Wine side below.)
    if [ -f "$RULES_DIR/$SCALER_RULES_NAME" ]; then
        if no_sudo; then
            say "   the scaler's udev rule stays (this system has no sudo); as root: rm -f $RULES_DIR/$SCALER_RULES_NAME && udevadm control --reload"
        elif ask "Remove the scaler's udev rule ($RULES_DIR/$SCALER_RULES_NAME${SUDO:+, needs sudo})?"; then
            { run $SUDO rm -f "$RULES_DIR/$SCALER_RULES_NAME" && udev control --reload; } ||
                warn "the scaler's udev rule could not be removed or reloaded (see the message above)"
            # Without the rule the command cannot be sent. A Wine side that stays (below: beside a package's
            # program, or kept by choice) would go on trying at every picture and note a refusal each time.
            if [ -e "$DATA/wine-lens/scaler.on" ] && { [ -n "$DRY" ] || [ ! -f "$RULES_DIR/$SCALER_RULES_NAME" ]; }; then
                run rm -f "$DATA/wine-lens/scaler.on"
            fi
        fi
    fi
    # The Wine side is in the data folder, which a distribution's package of the program uses as well. The
    # package's set-up sends whoever still has an installation from the archive here to remove it, and up
    # to 1.0.0 this question then had yes for Enter and for --yes: the set-up of the Odyssey 3D that was to
    # be kept went with the old program. Beside a package it stays unless somebody types y; the package's
    # own removal (any2odyssey-setup --uninstall) asks as before.
    KEEP=
    if [ -z "$PACKAGED" ] && { [ -e "$TEST_ROOT/usr/bin/any2odyssey-setup" ] || { [ -z "$TEST_ROOT" ] && have any2odyssey-setup; }; }; then
        KEEP=no
        [ ! -d "$DATA/wine-lens" ] || say "   Any2Odyssey is also installed from your distribution's package, which uses the same Wine side."
    fi
    if [ -d "$DATA/wine-lens" ] && ask "Remove the Wine side ($DATA/wine-lens: your copy of Leia's runtime, about 1 GB)?" $KEEP; then
        if [ -x "$DATA/wine-lens/tools/tracking.sh" ]; then run "$DATA/wine-lens/tools/tracking.sh" stop >/dev/null 2>&1 || true; fi
        # The serial port's keeper (tools/noxon.sh, or the program it becomes when setup.sh could build one)
        # that tools/ensure-stack.sh left running, found the way that script finds it: the noted number
        # counts only while its process still is the keeper.
        for note in "${XDG_RUNTIME_DIR:-$DATA/wine-lens}/any2odyssey-noxon.pid" "$DATA/wine-lens/any2odyssey-noxon.pid"; do
            [ -f "$note" ] || continue
            keeper=$(cat "$note" 2>/dev/null) || keeper=
            case $keeper in
                '' | *[!0-9]*) ;;
                *) if grep -q -a -e 'noxon\.sh' -e 'any2odyssey-noxon' "/proc/$keeper/cmdline" 2>/dev/null; then run kill "$keeper" 2>/dev/null || true; fi ;;
            esac
            run rm -f "$note"
        done
        # Through env: an assignment in front of a shell function does not reach the command in every shell,
        # and without it wineserver -k ends the programs of the user's default prefix instead.
        # (wineserver where Ubuntu 26.04 keeps it, off the PATH: see setup.sh)
        if ! have wineserver; then
            for d in /usr/lib/x86_64-linux-gnu/wine /usr/lib/wine /usr/lib64/wine; do
                if [ -x "$d/wineserver" ]; then PATH=$PATH:$d; break; fi
            done
        fi
        run env WINEPREFIX="$DATA/wine-lens/prefix" WINEDEBUG=-all wineserver -k 2>/dev/null || true
        run rm -f "$DATA/any2odyssey-lens-helper" "$DATA/any2odyssey-weaver"
        run rm -rf "$DATA/wine-lens"
    elif [ -n "$KEEP" ] && [ -d "$DATA/wine-lens" ]; then
        say "   The Wine side stays, for the package's program. To remove it as well: any2odyssey-setup --uninstall"
    fi
    say ""
    say "Your settings and the depth model stay in $DATA; delete that folder yourself if you want them gone."
    # (what depth-runtime/setup.sh put there, and the forms of the model that TensorRT made with it)
    [ ! -d "$DATA/depth-runtime" ] ||
        say "So does the runtime for TensorRT, 2.8 GB. To remove that alone: rm -r $(quoted "$DATA/depth-runtime") $(quoted "$DATA/depth-engines")"
    exit 0
fi

# ---------------------------------------------------------------------------------------------------------
[ "$(uname -s)" = Linux ] || { echo "install: this is the Linux edition" >&2; exit 1; }
[ "$(uname -m)" = x86_64 ] || warn "this archive is built for x86_64; this machine is $(uname -m)"

if [ -z "$PACKAGED" ]; then
    step "1. Installing the program to $PREFIX"
    for f in any2odyssey any2odyssey.desktop any2odyssey.png wine-lens/setup.sh; do
        [ -e "$HERE/$f" ] || { echo "install: $HERE/$f is missing; run this script from the extracted archive" >&2; exit 1; }
    done
    # Can this system start the program at all? Where it cannot, everything was installed, the shell said
    # only "any2odyssey: not found" (no loader: Alpine and other systems whose C library is not glibc) or
    # the loader named a symbol version (an older glibc), and the last line still said Done.
    if [ "$(head -c 4 "$HERE/any2odyssey" 2>/dev/null | tr '\177' '^')" = '^ELF' ]; then
        if [ ! -e "$TEST_ROOT$LOADER" ]; then
            echo "install: this system cannot run the program: it is built for the GNU C library (glibc $GLIBC_MIN or newer), whose loader $LOADER is not here. Systems with another C library (Alpine Linux and others with musl) are not supported; nothing was installed." >&2
            exit 1
        fi
        libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || libc=
        # A loader of that name is not glibc yet: Alpine's gcompat package puts one in front of musl. The
        # program does not start with it ("Error relocating ...: mallopt: symbol not found"), and with
        # PipeWire's library and libxkbcommon installed the last line said Done all the same.
        if [ -z "$libc" ] && [ -e "$TEST_ROOT/lib/ld-musl-x86_64.so.1" ]; then
            echo "install: this system cannot run the program: it is built for the GNU C library (glibc $GLIBC_MIN or newer), and the C library here is musl (Alpine Linux and others). The loader $LOADER that a compatibility package provides (Alpine's gcompat) does not start it; nothing was installed." >&2
            exit 1
        fi
        minor=${libc#glibc 2.}
        minor=${minor%%[!0-9]*}
        if [ "$libc" != "$minor" ] && [ -n "$minor" ] && [ "$minor" -lt "${GLIBC_MIN#2.}" ]; then
            echo "install: this system cannot run the program: its C library is $libc, and the program needs glibc $GLIBC_MIN or newer (Ubuntu 22.04, Debian 12 and anything later have it; RHEL 9 and its relatives do not). Nothing was installed." >&2
            exit 1
        fi
    fi
    # As root, only into folders that are root's alone. Every step below writes by path (install, cp -R,
    # rm -rf, the desktop entry), and whoever can change a folder on the way can swap what a path names
    # between two system calls: a root installer in another user's folder writes where that user points it.
    # No care taken over one file closes that (the desktop entry's private staging below narrows it for that
    # file only), so such an installation is refused as a whole. Uninstalling is not: its files may have
    # been put there by a root run of an earlier version, and it removes names, it does not write.
    if [ "$(id -u)" = 0 ]; then
        for folder in "$BIN" "$SHARE/applications" "$SHARE/icons/hicolor/256x256/apps" "$APPDIR" "$DOCDIR"; do
            foreign=$(foreign_folder "$folder")
            [ -z "$foreign" ] && continue
            echo "install: not installing to $PREFIX as root: $foreign is not root's alone (it belongs to another user, or others may write to it), and whoever can change it could make a root installer overwrite other files. Run this script as the user it is for (it uses sudo by itself for the steps that need it), or give --prefix a folder that is root's, such as /usr/local." >&2
            exit 1
        done
    fi
    [ -n "$DRY" ] || mkdir -p "$PREFIX" 2>/dev/null || true
    lock_prefix
    # Written beside the program and moved into its place: when the copy stops half-way (a full disk, a
    # kill) the program of the installation that is there stays whole. Written over it, an update that
    # ran out of space left 14 of its 15 MB there, not executable, where a working program had been.
    # Under a name of this run's own. With one name for every run (0.9.3), two installations at the same
    # time removed each other's copy: both stopped with nothing installed, or one moved the other's
    # half-written copy into place, which stayed mode 600. One that a killed run left behind goes when it
    # is a day old, as the desktop entry's staging folder below does, and with --uninstall.
    if [ -n "$DRY" ]; then
        say "   would run: install -Dm755 $HERE/any2odyssey $BIN/any2odyssey"
    elif ! (
        install -d "$BIN" || exit 1
        for stale in "$BIN/.any2odyssey.new" "$BIN"/.any2odyssey.new.??????????; do
            if [ -f "$stale" ] && [ ! -L "$stale" ]; then
                find "$stale" -prune -type f -mtime +0 -exec rm -f -- {} + 2>/dev/null || true
            fi
        done
        NEW=$(mktemp "$BIN/.any2odyssey.new.XXXXXXXXXX") || exit 1
        trap 'rm -f -- "$NEW"' 0
        trap 'exit 1' HUP INT TERM
        install -m755 "$HERE/any2odyssey" "$NEW" && mv -fT "$NEW" "$BIN/any2odyssey"
    ); then
        echo "install: could not copy the program to $BIN (see the message above); an installation that was there is unchanged" >&2
        exit 1
    fi
    # The desktop entry names the program by its full path: ~/.local/bin is not on every session's PATH.
    if [ -n "$DRY" ]; then
        say "   would write: $SHARE/applications/any2odyssey.desktop"
    else
        (
            mkdir -p "$SHARE/applications"
            ENTRY=$SHARE/applications/any2odyssey.desktop
            # Written beside the entry and moved into its place: an entry that is there is whole (not half of
            # one when this script is interrupted or the disk fills), and an entry that is a link to another
            # file is replaced like the program and the icon are, not written through.
            # A private, unique staging directory keeps installs from sharing a temporary path.
            # Noclobber also rejects a substituted link before awk can truncate its target.
            # Only remove this run's staging directory. A matching name may belong to an
            # install still running; its name alone cannot show that it was abandoned.
            # Its age can: a run has its own for a moment, so one untouched for more than a day was left
            # by a run that was killed, and nothing else would ever remove it. A link is not followed.
            for stale in "$ENTRY".??????????; do
                # Only what a run leaves in one goes, and the folder when that was all: a folder of the
                # user's own that merely has such a name (any2odyssey.desktop.2026-11-14) keeps its files.
                if [ -d "$stale" ] && [ ! -L "$stale" ] && [ -n "$(find "$stale" -prune -type d -mtime +0 2>/dev/null)" ]; then
                    rm -f -- "$stale/entry"
                    rmdir -- "$stale" 2>/dev/null || true
                fi
            done
            ENTRY_TMP=$(mktemp -d "$ENTRY.XXXXXXXXXX") || exit 1
            trap 'rm -f -- "$ENTRY_TMP/entry"; rmdir -- "$ENTRY_TMP"' 0
            trap 'exit 1' HUP INT TERM
            set -C
            # Pass the path as data, never as a sed/awk program or replacement.
            # Exec arguments undergo both desktop-string and command-line unescaping.
            if [ -d "$ENTRY" ] || ! A2O_DESKTOP_EXE="$BIN/any2odyssey" awk '
                function quote_arg(value,    slash, quoted, i, c) {
                    slash = sprintf("%c", 92)
                    quoted = "\""
                    for (i = 1; i <= length(value); i++) {
                        c = substr(value, i, 1)
                        if (c == slash) quoted = quoted slash slash slash slash
                        else if (c == "\"" || c == "$" || c == "`") quoted = quoted slash slash c
                        else if (c == "%") quoted = quoted "%%"
                        else quoted = quoted c
                    }
                    return quoted "\""
                }
                BEGIN {
                    path = ENVIRON["A2O_DESKTOP_EXE"]
                    command = quote_arg(path)
                    # Exec forbids = in its executable name; GLib also looks up that
                    # name before expanding %%. Use a fixed script for those paths,
                    # keeping the path and file as data in positional arguments.
                    if (index(path, "=") || index(path, "%"))
                        command = "/bin/sh -c " quote_arg("exec \"$@\"") " any2odyssey " command
                }
                /^Exec=any2odyssey([ \t]|$)/ {
                    sub(/^Exec=any2odyssey/, "")
                    print "Exec=" command $0
                    rewritten++
                    next
                }
                { print }
                # An entry left with the bare name starts nothing where ~/.local/bin is not on the PATH.
                END { if (rewritten != 1) exit 1 }
            ' "$HERE/any2odyssey.desktop" >"$ENTRY_TMP/entry" || ! mv -fT "$ENTRY_TMP/entry" "$ENTRY"; then
                echo "install: could not write the desktop entry $ENTRY: $HERE/any2odyssey.desktop has no 'Exec=any2odyssey' line to point at $BIN/any2odyssey (or awk failed, or the entry could not be put in place)" >&2
                exit 1
            fi
        )
    fi
    run install -Dm644 "$HERE/any2odyssey.png" "$SHARE/icons/hicolor/256x256/apps/any2odyssey.png"
    for f in README.md CHANGELOG.md USER-GUIDE.html LICENSE NOTICE THIRD-PARTY-LICENSES.md THIRD-PARTY-LICENSES-LINUX.md \
        INSTALL.txt installer/README.md; do
        [ -f "$HERE/$f" ] && run install -Dm644 "$HERE/$f" "$DOCDIR/$f"
    done
    # Everything README.md links to goes with it: the other documents and its screenshots.
    for f in "$HERE"/docs/*.md "$HERE"/docs/assets/*.png "$HERE"/assets/*.png; do
        [ -f "$f" ] && run install -Dm644 "$f" "$DOCDIR/${f#"$HERE"/}"
    done
    # The Wine setup and this script (for --uninstall later), replaced as a whole.
    if [ -n "$DRY" ]; then
        say "   would copy: wine-lens -> $WINE_LENS"
    else
        mkdir -p "$APPDIR"
        # (the setup an earlier version left in wine-lens goes; a Wine side built there does not)
        if only_the_setup; then rm -rf "$APPDIR/wine-lens"; else drop_the_old_setup; fi
        rm -rf "$WINE_LENS"
        cp -R "$HERE/wine-lens" "$WINE_LENS"
        install -m755 "$HERE/install.sh" "$APPDIR/install.sh"
    fi
    unlock_prefix
    have update-desktop-database && run update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
    refresh_icons
    case ":$PATH:" in *":$BIN:"*) ;; *) warn "$BIN is not on your PATH; start the program from the application menu, or as $BIN/any2odyssey" ;; esac
else
    step "1. The program is installed by your distribution's package"
    # An installation from the release archive that is still in ~/.local comes first in the menu and, where
    # ~/.local/bin is on the PATH, by name: the older program went on being started, and nothing said so.
    # Removed through this script, whatever version made it: the --uninstall of 0.9.0 and 0.9.1 removes the
    # settings and the depth model with it.
    if [ -e "$HOME/.local/share/applications/any2odyssey.desktop" ] || [ -e "$HOME/.local/bin/any2odyssey" ]; then
        warn "an installation from the release archive is still in $HOME/.local: its menu entry and its program are used instead of the package's. To remove it (your settings and the depth model stay, and so does the Odyssey 3D's Wine side: the removal asks about it, and Enter there keeps it): $(quoted "$HERE/install.sh") --uninstall --prefix $(quoted "$HOME/.local")"
    fi
fi

# ---------------------------------------------------------------------------------------------------------
# Which Windows drive? Step 5 copies Leia's runtime from it and then runs that copy under Wine as this user,
# so a drive nobody named is used only when it is plainly this machine's own Windows: it has a Windows system
# folder beside the runtime, its files belong to this user or to root (not a drive somebody else mounted or
# a folder somebody else filled), and it is not on a USB or other removable drive. When more than one
# qualifies the script does not choose; --windows-root names the one to use, and is taken as given.
SERVICE='Program Files/LeiaSR/Platform/bin/SRService.exe'
MY_UID=$(id -u)
system_folder() { # <drive>: its Windows system folder, as the drive spells it
    for spelling in Windows/System32 WINDOWS/system32 Windows/system32 WINDOWS/System32 windows/system32; do
        [ -d "$1/$spelling" ] && { echo "$1/$spelling"; return 0; }
    done
    return 1
}
own_or_root() { # <file>...: each belongs to this user or to root
    for owned in "$@"; do
        # shellcheck disable=SC2012  # one named file; stat has no format option that every system has
        owner=$(ls -ldn "$owned" 2>/dev/null | awk 'NR == 1 { print $3 }')
        [ "$owner" = 0 ] || [ "$owner" = "$MY_UID" ] || return 1
    done
}
# The source of the file system a folder is on (the mount table's first column, of the longest mount point
# that holds the folder). Read from the kernel's table: findmnt and lsblk are not installed everywhere.
mount_source() { # <folder, absolute and without symbolic links>
    A2O_FOLDER=$1 awk '
        function plain(text,    out, i) { # the table writes space, tab, newline and backslash as \ooo
            out = ""
            for (i = 1; i <= length(text); i++) {
                if (substr(text, i, 1) == "\\" && substr(text, i + 1, 3) ~ /^[0-7][0-7][0-7]$/) {
                    out = out sprintf("%c", substr(text, i + 1, 1) * 64 + substr(text, i + 2, 1) * 8 + substr(text, i + 3, 1))
                    i += 3
                } else out = out substr(text, i, 1)
            }
            return out
        }
        BEGIN { folder = ENVIRON["A2O_FOLDER"]; longest = -1 }
        {
            point = plain($2)
            if (point != "/" && folder != point && substr(folder, 1, length(point) + 1) != point "/") next
            # A later line for the same mount point lies on top of the earlier one.
            # A FUSE file system names its own source, and a program of any user can call itself /dev/sda2.
            # Only fuseblk is believed: root sets that up over a real device (ntfs-3g).
            if (length(point) >= longest) { longest = length(point); source = ($3 == "fuse" || $3 ~ /^fuse\./) ? "" : plain($1) }
        }
        END { if (longest >= 0) print source }
    ' "$TEST_ROOT/proc/self/mounts" 2>/dev/null
}
# fixed, removable or unknown, for a block device by its kernel name. A mapped device (LUKS, LVM, RAID)
# is what the devices under it are. "unknown" is everything this cannot see through: nobody is told a
# drive is inside the machine on a guess.
block_kind() { # <kernel name>
    pending=$1 looked=0
    while [ -n "$pending" ]; do
        # shellcheck disable=SC2086  # a list of kernel names
        set -- $pending
        name=$1; shift; pending=$*
        looked=$((looked + 1))
        [ "$looked" -le 16 ] || { echo unknown; return 0; }
        node=$(readlink -f "$TEST_ROOT/sys/class/block/$name" 2>/dev/null) || node=
        [ -n "$node" ] && [ -d "$node" ] || { echo unknown; return 0; }
        [ -f "$node/partition" ] && node=${node%/*}
        under=
        for part in "$node"/slaves/*; do
            [ -e "$part" ] && under="$under ${part##*/}"
        done
        if [ -n "$under" ]; then pending="$pending$under"; continue; fi
        case ${node#"$TEST_ROOT"} in */virtual/*) echo unknown; return 0 ;; esac # a loop device, a RAM disk
        [ -r "$node/removable" ] || { echo unknown; return 0; }
        [ "$(cat "$node/removable")" = 0 ] || { echo removable; return 0; }
        # A memory card in a built-in reader says 0 there; the card's type tells it from soldered eMMC.
        [ "$(cat "$node/device/type" 2>/dev/null)" != SD ] || { echo removable; return 0; }
        case ${node#"$TEST_ROOT"} in */usb[0-9]*) echo removable; return 0 ;; esac
    done
    echo fixed
}
drive_kind() { # <folder>: fixed, removable or unknown (a network share, a file system without a device)
    folder=$(cd "$1" 2>/dev/null && pwd -P) || { echo unknown; return 0; }
    source=$(mount_source "$folder") || source=
    case $source in /dev/*) ;; *) echo unknown; return 0 ;; esac
    # /dev/mapper/<name> and /dev/disk/by-*/<name> are links to the kernel's own name for the device.
    device=$(readlink -f "$source" 2>/dev/null) || device=
    device=${device:-$source}
    block_kind "${device##*/}"
}
no_windows_help() {
    say "   No Windows installation of the Odyssey 3D Hub was found on a mounted drive."
    say "   This part needs one: Leia's runtime is not distributed and has no public Linux version, so it is"
    say "   copied from your own Windows. The expected set-up is a dual-boot PC, with Windows and the Odyssey"
    say "   3D Hub on a drive of this machine, which has shown 3D on this monitor at least once."
    NTFS=$(lsblk -n -l -o NAME,FSTYPE,MOUNTPOINT 2>/dev/null | awk '$2 == "ntfs" && $3 == "" { print $1 }')
    BITLOCKER=$(lsblk -n -l -o NAME,FSTYPE 2>/dev/null | awk 'tolower($2) == "bitlocker" { print $1 }')
    if [ -n "$NTFS" ]; then
        say "   Windows partitions here that are not mounted (the system one holds \"Program Files\"):"
        for p in $NTFS; do say "       udisksctl mount -b /dev/$p -o ro"; done
    fi
    # shellcheck disable=SC2086,SC2116  # the names on one line
    [ -n "$BITLOCKER" ] && say "   Encrypted with BitLocker (unlock it, or suspend BitLocker in Windows, first): $(echo $BITLOCKER)"
    say "   Mount the Windows system partition (read-only is enough), then run this script again, or:"
    say "       $WINE_LENS/setup.sh /path/to/that/drive"
    say "   Without a Windows installation this part is not available; everything else works."
}
# Sets WIN and succeeds, or says why there is no drive to use and fails.
find_windows() {
    WIN=$WINROOT
    [ -n "$WIN" ] && return 0
    login=${USER:-$(id -un)}
    passed_over=
    set --
    for drive in "$TEST_ROOT"/run/media/"$login"/* "$TEST_ROOT"/media/"$login"/* "$TEST_ROOT"/media/* "$TEST_ROOT"/mnt/*; do
        [ -f "$drive/$SERVICE" ] || continue
        if ! system=$(system_folder "$drive"); then
            passed_over="no Windows system folder beside Leia's runtime"
        elif ! own_or_root "$drive" "$system" "$drive/Program Files" "$drive/Program Files/LeiaSR" \
            "$drive/Program Files/LeiaSR/Platform" "$drive/Program Files/LeiaSR/Platform/bin" "$drive/$SERVICE"; then
            passed_over="its files belong to another user"
        elif [ "$(drive_kind "$drive")" = removable ]; then
            passed_over="a removable drive"
        else
            set -- "$@" "$drive"
            continue
        fi
        say "   not used: $drive ($passed_over)"
    done
    if [ $# -eq 0 ]; then
        no_windows_help
        [ -n "$passed_over" ] && say "   If a drive named above as not used is the right one after all, name it: --windows-root DIR"
        return 1
    fi
    if [ $# -gt 1 ]; then
        say "   More than one mounted drive has a Windows installation with the Odyssey 3D Hub:"
        for drive in "$@"; do say "       $drive"; done
        say "   This script does not choose between them: run it again with --windows-root and the right one."
        return 1
    fi
    if [ "$(drive_kind "$1")" != fixed ]; then
        if [ -n "$YES" ]; then
            say "   Found $1, but could not tell that it is on a drive inside this machine (a network share?)."
            say "   With --yes such a drive is not used unasked: run this script again with --windows-root '$1'"
            return 1
        fi
        warn "could not tell that $1 is on a drive inside this machine; make sure it is your own Windows before agreeing"
    fi
    WIN=$1
}
lens_board() { grep -q -s -i '^354b$' "$TEST_ROOT"/sys/bus/usb/devices/*/idVendor 2>/dev/null; }
webcam_dir() {
    for d in "$TEST_ROOT"/sys/bus/usb/devices/*; do
        [ "$(cat "$d/idVendor" 2>/dev/null):$(cat "$d/idProduct" 2>/dev/null)" = 04e8:20d7 ] && { echo "$d"; return 0; }
    done
    return 1
}
WANT_ODYSSEY=
if [ -n "$ODYSSEY" ]; then
    if lens_board || [ -e "$TEST_ROOT/dev/odyssey-lens" ] || [ -n "$WINROOT" ]; then
        WANT_ODYSSEY=1
    else
        say ""
        say "No Odyssey 3D is connected (or it is asleep): its lens, eye tracker and weaver are not prepared now."
        say "Wake the monitor and run this script again to add them."
    fi
fi

# ---------------------------------------------------------------------------------------------------------
step "2. Packages"
# What is needed, as: library or command to look for | Arch | Debian/Ubuntu | Fedora | openSUSE
# (libxkbcommon-x11: in an X11 session the program ends at once without it, and no package named here
# brings it; a desktop without GNOME's or KDE's window manager need not have it.)
NEEDED='
lib:libpipewire-0.3.so.0|libpipewire|libpipewire-0.3-0|pipewire-libs|libpipewire-0_3-0
lib:libxkbcommon.so.0|libxkbcommon|libxkbcommon0|libxkbcommon|libxkbcommon0
lib:libxkbcommon-x11.so.0|libxkbcommon-x11|libxkbcommon-x11-0|libxkbcommon-x11|libxkbcommon-x11-0
lib:libGL.so.1|libglvnd|libgl1|libglvnd-glx|libglvnd
cmd:curl|curl|curl|curl|curl
'
DIALOG='cmd:zenity|zenity|zenity|zenity|zenity'
# so: a library the program opens by this very name. Debian, Fedora and openSUSE keep libonnxruntime.so in
# the development package; the library package's libonnxruntime.so.1 alone is not found by the program.
ONNX_CPU='so:libonnxruntime.so|onnxruntime-cpu|libonnxruntime-dev|onnxruntime-devel|onnxruntime-devel'
ONNX_GPU='lib:libonnxruntime_providers_cuda.so|onnxruntime-cuda|libonnxruntime-dev|onnxruntime
lib:libcudnn.so|cudnn|nvidia-cudnn|'
# (pgrep: setup.sh stops without it, and a minimal installation or a container need not have it)
WINE_SIDE='
cmd:wine|wine|wine|wine|wine
cmd:x86_64-w64-mingw32-gcc|mingw-w64-gcc|gcc-mingw-w64-x86-64|mingw64-gcc|mingw64-cross-gcc
cmd:clang++|clang|clang|clang|clang
cmd:cc|gcc|gcc|gcc|gcc
cmd:python3|python|python3|python3|python3
cmd:xrandr|xorg-xrandr|x11-xserver-utils|xrandr|xrandr
cmd:rsync|rsync|rsync|rsync|rsync
cmd:pgrep|procps-ng|procps|procps-ng|procps
'
# One name that a release's package lists do not have makes dnf and zypper install nothing at all, as
# apt-get does (below): RHEL 10 and its relatives have no ONNX Runtime package, and with onnxruntime-devel
# among the names PipeWire's library was not installed either and the program did not start. Both can be
# told to leave such a name out; dnf 5 and dnf 4 call that differently.
# pacman asks a question of its own ("Proceed with installation? [Y/n]") after this script's: with --yes
# it is answered like every other one. It was not, and where nobody could answer (no terminal: a script,
# another installer) pacman installed nothing after --yes. Without --yes its question stays, with the
# list of what it would install; the other three are told not to ask in either case, as before.
if have pacman; then FAMILY=2 INSTALL="pacman -S --needed${YES:+ --noconfirm}"
elif have apt-get; then FAMILY=3 INSTALL="apt-get install -y"
elif have dnf; then
    FAMILY=4
    case $(dnf --version 2>/dev/null | head -n 1) in
        *dnf5*) INSTALL="dnf install -y --skip-unavailable" ;;
        *) INSTALL="dnf install -y --skip-broken" ;;
    esac
elif have zypper; then FAMILY=5 INSTALL="zypper --non-interactive --ignore-unknown install"
else FAMILY= INSTALL=; fi

present() {
    case $1 in
        cmd:*) have "${1#cmd:}" ;;
        lib:*)
            # The linker's list where there is one to ask (ldconfig is not on an ordinary user's PATH
            # on Debian), then the usual folders, each pattern by itself: ls fails for all of them when
            # one matches nothing, which reported every installed library as missing.
            if [ -z "$TEST_ROOT" ] && ldconfig -p 2>/dev/null | grep -q -F "${1#lib:}"; then return 0; fi
            for found in "$TEST_ROOT"/usr/lib*/"${1#lib:}"* "$TEST_ROOT"/usr/lib/*/"${1#lib:}"*; do
                [ -e "$found" ] && return 0
            done
            return 1
            ;;
        so:*)
            if [ -z "$TEST_ROOT" ] && ldconfig -p 2>/dev/null | awk -v name="${1#so:}" '$1 == name { found = 1 } END { exit !found }'; then return 0; fi
            for found in "$TEST_ROOT"/usr/lib*/"${1#so:}" "$TEST_ROOT"/usr/lib/*/"${1#so:}"; do
                [ -e "$found" ] && return 0
            done
            return 1
            ;;
    esac
}
# Whether the wine that apt would install is older than the set-up takes; WINE_CANDIDATE is its version.
# Asked of apt's lists, which needs no root. A version that cannot be read (no candidate, no apt-cache) is
# let through, as setup.sh lets through a Wine whose version it cannot read.
WINE_CANDIDATE=unasked
candidate_wine_too_old() {
    if [ "$WINE_CANDIDATE" = unasked ]; then
        WINE_CANDIDATE=$(LC_ALL=C apt-cache policy wine 2>/dev/null | sed -n 's/^ *Candidate: *//p' | head -n 1) || WINE_CANDIDATE=
    fi
    number=${WINE_CANDIDATE#*:} # (without an epoch)
    major=${number%%.*}
    case $major in '' | *[!0-9]*) return 1 ;; esac
    minor=${number#"$major"}
    minor=${minor#.}
    minor=${minor%%[!0-9]*}
    [ "$major" -lt "${WINE_MIN%%.*}" ] || { [ "$major" -eq "${WINE_MIN%%.*}" ] && [ "${minor:-0}" -lt "${WINE_MIN#*.}" ]; }
}
MISSING= UNNAMED= ABSENT=
check() { # <list> : adds the packages of what is missing to MISSING
    for line in $1; do
        what=${line%%|*}
        present "$what" && continue
        if [ -n "$FAMILY" ]; then
            package=$(printf %s "$line" | cut -d'|' -f"$FAMILY")
            if [ -n "$package" ]; then
                case " $MISSING " in *" $package "*) ;; *) MISSING="$MISSING $package" ;; esac
                continue
            fi
        fi
        UNNAMED="$UNNAMED ${what#*:}"
    done
}
look() { # sets MISSING and UNNAMED to what is not there now
    MISSING= UNNAMED= ONNX_ON_CPU=
    check "$NEEDED"
    have zenity || have kdialog || check "$DIALOG"
    if [ -n "$MODEL" ]; then
        # An NVIDIA GPU gets the CUDA build; depth then takes milliseconds instead of a tenth of a second.
        # Only on Arch, whose packages have one; the other families get the one runtime they have.
        # Not over a runtime that is installed without it: Arch's onnxruntime-cuda and onnxruntime-cpu
        # conflict (each provides onnxruntime), and pacman installs neither that nor anything else of its
        # list while one question is answered with no, which is what --noconfirm does with "Remove
        # onnxruntime-cpu? [y/N]". With --yes on such a machine 1.0.0 installed no package at all, at every
        # run. The runtime that is there is kept, and a note below says how to change over.
        if have nvidia-smi && [ "$FAMILY" = 2 ]; then
            if present "${ONNX_CPU%%|*}" && ! present "${ONNX_GPU%%|*}"; then ONNX_ON_CPU=1; else check "$ONNX_GPU"; fi
        else
            check "$ONNX_CPU"
        fi
    fi
    [ -z "$WANT_ODYSSEY" ] || check "$WINE_SIDE"
    MISSING=${MISSING# }
    # Debian 12 and 13 and Ubuntu up to 26.04 have Wine 8.0 to 10.0, and setup.sh refuses anything below
    # WINE_MIN. Up to 1.0.0 a missing wine was installed from the distribution all the same: a long
    # download, then "this Wine is too old" from the set-up, and a package that is in the way of WineHQ's.
    # Such a wine is left out of the list; a note below says where a newer one is, and step 5 is skipped.
    # (Not with --no-deps, which leaves the packages to the user: apt is not asked anything then.)
    WINE_OLD=
    if [ "$FAMILY" = 3 ] && [ -n "$WANT_DEPS" ]; then
        case " $MISSING " in
            *" wine "*)
                if candidate_wine_too_old; then
                    WINE_OLD=$WINE_CANDIDATE
                    KNOWN=
                    for package in $MISSING; do
                        [ "$package" = wine ] || KNOWN="$KNOWN $package"
                    done
                    MISSING=${KNOWN# }
                fi
                ;;
        esac
    fi
}
look
# One name that the release's package lists do not have makes apt-get refuse the whole list: Ubuntu 24.04
# has no ONNX Runtime package, and with libonnxruntime-dev among the names neither PipeWire's library nor
# curl was installed. A simulated installation (it needs no root) names them; they are set apart.
if [ "$FAMILY" = 3 ] && [ -n "$WANT_DEPS" ] && [ -n "$MISSING" ]; then
    # shellcheck disable=SC2086  # a list of package names
    ABSENT=$(LC_ALL=C apt-get install -s $MISSING 2>&1 |
        sed -n -e 's/^E: Unable to locate package //p' -e "s/^E: Package '\\(.*\\)' has no installation candidate\$/\\1/p" | tr '\n' ' ')
    KNOWN=
    for package in $MISSING; do
        case " $ABSENT " in *" $package "*) ;; *) KNOWN="$KNOWN $package" ;; esac
    done
    MISSING=${KNOWN# }
    ABSENT=${ABSENT% }
fi
if [ -z "$MISSING$UNNAMED$ABSENT" ]; then
    say "   everything needed is installed"
else
    [ -n "$MISSING" ] && say "   missing packages: $MISSING"
    [ -n "$ABSENT" ] && say "   missing, and not in this system's package lists (see INSTALL.txt for what needs them): $ABSENT"
    if [ -n "$UNNAMED" ]; then
        # (the names are files and commands: this script knows no package manager here to ask for packages)
        say "   ${MISSING:+also }missing, under a name this script does not know for your distribution:$UNNAMED"
        say "   install the packages that have them with your package manager, then run this script again"
        case " $UNNAMED " in *" libpipewire-0.3.so.0 "* | *" libxkbcommon.so.0 "*) say "   (without libpipewire-0.3 and libxkbcommon the program does not start)" ;; esac
    fi
    if [ -z "$DEPS" ] || [ -z "$INSTALL" ] || [ -z "$MISSING" ]; then
        [ -n "$MISSING" ] && say "   install them with your package manager, then run this script again"
    elif no_sudo; then
        say "   this system has no sudo. As root, run:   $INSTALL $MISSING"
        say "   then run this script again"
    elif ask "Install them now ($SUDO${SUDO:+ }$INSTALL ...${SUDO:+; needs sudo})?"; then
        # shellcheck disable=SC2086  # a list of package names
        run $SUDO $INSTALL $MISSING || warn "the package manager did not finish; the steps below may be incomplete"
        # What is there now is looked at, not what the package manager said: it leaves out a name its
        # lists do not have, and where it could do nothing (no rights, a system whose /usr cannot be
        # written, as on Fedora Silverblue or SteamOS) all of them are still missing.
        if [ -z "$DRY" ]; then
            look
            STILL=
            for package in $MISSING; do
                case " $ABSENT " in *" $package "*) ;; *) STILL="$STILL $package" ;; esac
            done
            [ -z "$STILL" ] || say "   not installed (not in this system's package lists, or the package manager could not install them):$STILL"
        fi
    fi
fi
if [ -n "$WINE_OLD" ]; then
    warn "Wine is not installed, and the one in this system's package lists ($WINE_OLD) is older than Wine $WINE_MIN, which the Odyssey 3D set-up needs: it is not installed, because the set-up would refuse it and it would be in the way of a newer one. WineHQ's own packages for Debian and Ubuntu are new enough: add WineHQ's repository as the download page of https://www.winehq.org describes, install winehq-devel or winehq-staging from it, then run this script again. Everything else works without Wine."
fi
if [ -n "$ONNX_ON_CPU" ]; then
    warn "the ONNX Runtime installed here has no CUDA part (onnxruntime-cpu, or another package that provides onnxruntime): 2D -> 3D depth works and runs on the processor. To run it on the NVIDIA GPU, put onnxruntime-cuda in its place; the two cannot be installed together, so this script does not do that unasked: ${SUDO:+sudo }pacman -S onnxruntime-cuda cudnn (pacman asks whether to remove the installed one; answer yes)"
fi

# ---------------------------------------------------------------------------------------------------------
step "3. udev rules for the Odyssey 3D"
RULES=$WINE_LENS/$RULES_NAME
[ -f "$RULES" ] || RULES=$HERE/wine-lens/$RULES_NAME
if [ -z "$UDEV" ]; then
    say "   skipped (--no-udev)"
elif [ -f "$PACKAGED_RULES_DIR/$RULES_NAME" ]; then
    say "   installed by your distribution's package"
elif [ ! -f "$RULES" ]; then
    warn "$RULES_NAME was not found; skipped"
elif [ -f "$RULES_DIR/$RULES_NAME" ] && cmp -s "$RULES" "$RULES_DIR/$RULES_NAME"; then
    say "   already installed"
elif [ ! -d "$RULES_DIR" ] && ! have udevadm; then
    # A container, or a system with another device manager: the installer stopped here with "install: cannot
    # create regular file", before the depth model and without its last lines.
    say "   skipped: this system has no udev (no $RULES_DIR and no udevadm; a container?). The rules belong on the system the monitor is connected to."
elif no_sudo; then
    say "   this system has no sudo. As root, run:   install -m644 $(quoted "$RULES") $RULES_DIR/$RULES_NAME && udevadm control --reload && udevadm trigger --subsystem-match=tty"
elif ask "Install $RULES_DIR/$RULES_NAME (lens board for you at the seat, no USB power saving for the webcam${SUDO:+; needs sudo})?"; then
    # A refusal here (no rights to use sudo, a wrong password, no udev daemon to tell) does not end the
    # installation: the depth model and the last lines follow.
    if ! run $SUDO install -m644 "$RULES" "$RULES_DIR/$RULES_NAME"; then
        warn "the udev rules were not installed (see the message above); the Odyssey 3D's lens board cannot be used until they are. As root: install -m644 $(quoted "$RULES") $RULES_DIR/$RULES_NAME && udevadm control --reload && udevadm trigger --subsystem-match=tty"
    else
        { udev control --reload && udev trigger --subsystem-match=tty; } ||
            warn "the udev rules are installed but could not be put into effect now (see the message above); they are read at the next start of the system"
        # The trigger only asks for the events: udev makes /dev/odyssey-lens a moment later. With nothing
        # slow between here and step 5 (the depth model is there already, or --no-model) that step looked
        # for the link before it was made and said the lens board is not available, over rules that had
        # just been installed. The events are waited for, a few seconds at most.
        udev settle --timeout=5 || true
        # The webcam rule acts when the camera appears; for one that is here already, set it now.
        if CAM=$(webcam_dir); then
            run $SUDO sh -c "echo on > '$CAM/power/control'" || true
        fi
    fi
fi
if CAM=$(webcam_dir) && [ "$(cat "$CAM/power/runtime_status" 2>/dev/null)" = suspended ]; then
    warn "the Odyssey's webcam has been in USB power saving. It sends no pictures after that until the monitor has been without power for a few seconds: pull its power cable and plug it back in before using eye tracking."
fi

# ---------------------------------------------------------------------------------------------------------
step "4. The depth model for 2D -> 3D"
if [ -z "$MODEL" ]; then
    say "   skipped (--no-model)"
elif [ -f "$DATA/models/depth-anything-v2-small.onnx" ]; then
    say "   already installed"
elif [ -f "$PROGRAM" ] && [ ! -x "$PROGRAM" ] && [ -z "$DRY" ]; then
    warn "$PROGRAM cannot be run where it is (the last lines say more); skipped"
elif [ ! -x "$PROGRAM" ] && [ -z "$DRY" ]; then
    warn "$PROGRAM is not there; skipped"
elif ask "Download the open depth model (Depth Anything V2 Small, about 95 MB, checked against a fixed SHA-256)?"; then
    run "$PROGRAM" --install-depth-model || warn "the model was not installed; try '$PROGRAM --install-depth-model' later"
fi

# ---------------------------------------------------------------------------------------------------------
step "5. The Odyssey 3D's lens, eye tracker and weaver (Leia's runtime under Wine)"
if [ -z "$WANT_ODYSSEY" ]; then
    say "   skipped"
elif [ -n "$WINE_OLD" ]; then
    say "   skipped: this part needs Wine $WINE_MIN or newer, and this system's package lists have an older one (see the note in step 2)"
elif ! find_windows; then
    : # it has said why
elif [ ! -f "$WIN/$SERVICE" ]; then
    warn "$WIN has no Leia SR runtime (Program Files/LeiaSR); is the Odyssey 3D Hub installed on that Windows?"
elif [ ! -e "$TEST_ROOT/dev/odyssey-lens" ] && [ -z "$DRY" ]; then
    warn "the lens board is not available as /dev/odyssey-lens (step 3 installs the rule; the monitor must be awake). Then run: $WINE_LENS/setup.sh '$WIN'"
elif ask "Prepare it now from $WIN (copies about 1 GB of your own installation into $DATA/wine-lens)?"; then
    run "$WINE_LENS/setup.sh" "$WIN" || warn "the setup did not finish; see its message above and docs/LENS-SWITCH.md"
fi
# A Wine side that is there runs its own copies of the setup's scripts (setup.sh puts them into the data
# folder), and an update of the program replaces the setup's files only. Where this run did not make the
# Wine side again (no Windows drive mounted, the monitor asleep, the question declined, --no-odyssey), it went
# on with the scripts of the version that had made it, and nothing said so: what 0.9.8 changed in the start
# of Leia's programs reached nobody who did not run the setup once more. It is said now, here and in what
# any2odyssey-setup prints.
#
# That looked at the scripts setup.sh copies, and at nothing else: the libraries it compiles from
# wine-lens/src, the board description tools/mkboard.py writes and what setup.sh itself does to the prefix
# were not compared, and a release that changed only those left the old ones in place without a word.
# setup.sh now notes in the Wine side what it was made from (made-from: a digest of the set-up's own files,
# by the command that made_from repeats here), and the note is compared with the set-up installed now.
# A Wine side without the note was made by 1.0.0 or earlier, or on a system without sha256sum. Its copied
# scripts are compared as before; where they are the same, it counts as older only when this version's
# sources of what is compiled (src, tools/mkboard.py) are not the ones 1.0.0 had. Not for the mere lack of
# the note: that would tell everybody who updates to make the Wine side again, with the Windows drive
# mounted, also after an update that changed nothing in it.
STAMP_FILES='setup.sh any2odyssey-lens-helper any2odyssey-weaver src/*.* tools/*.*'
BUILT_FILES='src/*.* tools/mkboard.py'
BUILT_1_0_0=e38a9c8eeb3186d9717bf6b1fbf59a33d6c218f012e3285eeacec42533fae0f2
made_from() { # <files of the set-up, as patterns>: their digest, or nothing where it cannot be made
    have sha256sum && [ -f "$WINE_LENS/setup.sh" ] || return 0
    # shellcheck disable=SC2086  # patterns, for the set-up's folder
    (cd "$WINE_LENS" 2>/dev/null && sha256sum $1 2>/dev/null | LC_ALL=C sort | sha256sum 2>/dev/null | cut -d' ' -f1) || true
}
older_wine_side() {
    [ -f "$DATA/wine-lens/tools/start-stack.sh" ] || return 1
    made=$(cat "$DATA/wine-lens/made-from" 2>/dev/null) || made=
    now=$(made_from "$STAMP_FILES")
    if [ -n "$made" ] && [ -n "$now" ]; then
        [ "$made" != "$now" ] || return 1
        return 0
    fi
    for script in any2odyssey-lens-helper any2odyssey-weaver tools/start-stack.sh tools/ensure-stack.sh \
        tools/tracking.sh tools/noxon.sh tools/mkedid.py tools/kwin-overlay.js; do
        [ -f "$WINE_LENS/$script" ] || continue
        cmp -s "$WINE_LENS/$script" "$DATA/wine-lens/$script" || return 0
    done
    [ -z "$made" ] && [ -n "$now" ] && [ "$(made_from "$BUILT_FILES")" != "$BUILT_1_0_0" ]
}
if [ -z "$DRY" ] && older_wine_side; then
    warn "the Odyssey 3D's Wine side in $DATA/wine-lens was made by another version of Any2Odyssey and still runs that version's scripts and stand-in libraries; what this version changed in them does not apply until it is made again. With the Windows drive mounted and the monitor awake, run this script again, or: $(quoted "$WINE_LENS/setup.sh") /path/to/that/drive"
fi

# ---------------------------------------------------------------------------------------------------------
# Experimental, and the one step that has this program send the monitor a command of its own (to the scaler,
# not to the lens board): see docs/LENS-SWITCH.md, "The scaler's 3D mode". It is done with --scaler and with
# nothing else: no question is asked about it, and --yes does not agree to it. A run without the option
# says nothing of the step, unless an earlier run switched it on.
SIDE=$DATA/wine-lens
if [ -n "$SCALER" ] || [ -e "$SIDE/scaler.on" ]; then step "6. The scaler's 3D mode (experimental)"; fi
SCALER_RULES=$WINE_LENS/$SCALER_RULES_NAME
[ -f "$SCALER_RULES" ] || SCALER_RULES=$HERE/wine-lens/$SCALER_RULES_NAME
scaler_rule_installed() { [ -f "$RULES_DIR/$SCALER_RULES_NAME" ] && cmp -s "$SCALER_RULES" "$RULES_DIR/$SCALER_RULES_NAME"; }
if [ "$SCALER" = no ]; then
    if [ -e "$SIDE/scaler.on" ]; then
        run rm -f "$SIDE/scaler.on"
        say "   switched off: the Odyssey 3D output no longer sends the scaler its command"
        [ ! -f "$RULES_DIR/$SCALER_RULES_NAME" ] || say "   the udev rule stays ($RULES_DIR/$SCALER_RULES_NAME); --uninstall asks about removing it"
    else
        say "   it was not switched on"
    fi
elif [ "$SCALER" != yes ]; then
    if [ -e "$SIDE/scaler.on" ]; then
        say "   already switched on (--no-scaler switches it off)"
        # (the rule of another version, or one taken away by hand: said, and not put right unasked)
        if [ -n "$UDEV" ] && [ -f "$SCALER_RULES" ] && ! scaler_rule_installed; then
            warn "the scaler's udev rule in $RULES_DIR is missing or is not this version's, and the scaler may not take its command until it is; --scaler installs it"
        fi
    fi
elif [ ! -f "$SIDE/any2odyssey-weaver" ] && [ -z "$DRY" ]; then
    warn "the Odyssey 3D's Wine side is not prepared (step 5), and it is what switches the scaler; run this script again with --scaler once it is"
else
    if [ -z "$UDEV" ]; then
        say "   the udev rule is not installed (--no-udev)"
    elif [ ! -f "$SCALER_RULES" ]; then
        warn "$SCALER_RULES_NAME was not found; the rule is not installed"
    elif scaler_rule_installed; then
        say "   the udev rule is already installed"
    elif [ ! -d "$RULES_DIR" ] && ! have udevadm; then
        say "   the udev rule is not installed: this system has no udev (no $RULES_DIR and no udevadm; a container?)"
    elif no_sudo; then
        say "   this system has no sudo. As root, run:   install -m644 $(quoted "$SCALER_RULES") $RULES_DIR/$SCALER_RULES_NAME && udevadm control --reload"
    elif ! run $SUDO install -m644 "$SCALER_RULES" "$RULES_DIR/$SCALER_RULES_NAME"; then
        warn "the scaler's udev rule was not installed (see the message above); the scaler cannot be sent its command until it is. As root: install -m644 $(quoted "$SCALER_RULES") $RULES_DIR/$SCALER_RULES_NAME && udevadm control --reload"
    else
        udev control --reload || warn "the scaler's udev rule is installed but could not be reloaded (see the message above); it is read at the next start of the system"
        # No "udevadm trigger" here: on this device it is reported to re-initialise the monitor's hub and
        # take the lens board and the webcam off the bus (odysseyvk-linux's README).
        say "   The rule takes effect once the monitor's USB cable has been unplugged and plugged in again, or the monitor switched off and on."
    fi
    [ -x "$SIDE/build/any2odyssey-scaler" ] || [ -n "$DRY" ] ||
        warn "any2odyssey-scaler is not in $SIDE/build yet: run the Odyssey 3D set-up once more ($WINE_LENS/setup.sh, with a C compiler installed), which builds it"
    if [ -n "$DRY" ]; then
        say "   would make: $SIDE/scaler.on"
    else
        : >"$SIDE/scaler.on"
    fi
    say "   switched on: the Odyssey 3D output sends the scaler its command before a picture and after it. What the scaler answered is in $SIDE/scaler.log; --no-scaler switches it off."
fi

say ""
# The last line does not say Done over a program that cannot start: the two libraries it is linked with or
# opens first are looked for once more (the package step may have been declined, refused, or without names
# for this distribution). A distribution's package brings them itself.
LACKS=
if [ -z "$PACKAGED" ] && [ -z "$DRY" ]; then
    for needed in lib:libpipewire-0.3.so.0 lib:libxkbcommon.so.0; do
        present "$needed" || LACKS="$LACKS ${needed#lib:}"
    done
fi
if [ -z "$PACKAGED" ] && [ -z "$DRY" ] && [ -f "$PROGRAM" ] && [ ! -x "$PROGRAM" ]; then
    # A home folder mounted noexec: the program was "not there" for the depth model, and then Done.
    say "Installed, but the system does not let a program run from $BIN (a file system mounted noexec?)."
    say "Remove it with the command below and install to a folder that allows it: --prefix DIR"
elif [ -n "$LACKS" ]; then
    say "Installed, but these were not found, and the program does not start without them:$LACKS (see step 2 above)."
    say "With them, start Any2Odyssey from the application menu or as: $PROGRAM"
else
    say "Done. Start Any2Odyssey from the application menu or as: $PROGRAM"
fi
if [ -z "$PACKAGED" ]; then
    # The prefix is named when it is not the usual one, so the command also says where it removes from.
    REMOVE="$(quoted "$APPDIR/install.sh") --uninstall"
    [ "$PREFIX" = "$(canonical "$HOME/.local")" ] || REMOVE="$REMOVE --prefix $(quoted "$PREFIX")"
    say "To remove it later: $REMOVE"
fi
exit 0
