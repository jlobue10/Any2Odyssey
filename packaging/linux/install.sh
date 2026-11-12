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
#      (Arch, Debian/Ubuntu and Fedora families; elsewhere it lists them);
#   3. installs the udev rules for the Odyssey 3D (lens board for the user at the seat, USB power saving off
#      for the monitor's webcam);
#   4. downloads the open depth model for 2D -> 3D (about 95 MB);
#   5. prepares the Odyssey 3D's lens, eye tracker and weaver under Wine from your own Windows installation
#      of the Odyssey 3D Hub, if it finds one mounted or is given one.
#
# Options:
#   --yes               answer yes to every question
#   --no-deps           do not install packages (still says what is missing)
#   --no-udev           do not install the udev rules
#   --no-model          do not download the depth model
#   --no-odyssey        do not prepare the Wine side
#   --windows-root DIR  the mounted Windows drive that has the Odyssey 3D Hub (the folder with "Program Files")
#   --prefix DIR        install under DIR instead of ~/.local
#   --packaged          the program is installed already (a distribution package): do steps 2 to 5 only
#   --dry-run           say what would be done and do none of it
#   --uninstall         remove the installed files (and, when confirmed, the udev rules and the Wine side)
set -eu

HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
PREFIX=$HOME/.local
YES= DEPS=1 UDEV=1 MODEL=1 ODYSSEY=1 PACKAGED= DRY= UNINSTALL= WINROOT=
RULES_NAME=70-any2odyssey-lens.rules
RULES_DIR=/etc/udev/rules.d

while [ $# -gt 0 ]; do
    case $1 in
        --yes | -y) YES=1 ;;
        --no-deps) DEPS= ;;
        --no-udev) UDEV= ;;
        --no-model) MODEL= ;;
        --no-odyssey) ODYSSEY= ;;
        --packaged) PACKAGED=1 ;;
        --dry-run) DRY=1 ;;
        --uninstall) UNINSTALL=1 ;;
        --windows-root) [ $# -ge 2 ] || { echo "install: --windows-root needs a folder" >&2; exit 2; }; WINROOT=${2%/}; shift ;;
        --prefix) [ $# -ge 2 ] || { echo "install: --prefix needs a folder" >&2; exit 2; }; PREFIX=${2%/}; shift ;;
        -h | --help) sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "install: unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

say() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
warn() { printf 'note: %s\n' "$*"; }
# Run a command, or only show it with --dry-run.
run() {
    if [ -n "$DRY" ]; then printf '   would run: %s\n' "$*"; else "$@"; fi
}
# Ask a yes/no question; yes with --yes, no when there is nobody to ask.
ask() {
    [ -n "$YES" ] && return 0
    [ -t 0 ] || { say "   ($1 -- skipped: not asked without a terminal; use --yes)"; return 1; }
    printf '%s [Y/n] ' "$1"
    read -r answer || return 1
    case $answer in '' | y | Y | yes | Yes) return 0 ;; *) return 1 ;; esac
}
have() { command -v "$1" >/dev/null 2>&1; }

BIN=$PREFIX/bin
SHARE=$PREFIX/share
APPDIR=$SHARE/any2odyssey
DOCDIR=$SHARE/doc/any2odyssey
DATA=${ANY2ODYSSEY_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/any2odyssey}
if [ -n "$PACKAGED" ]; then
    WINE_LENS=$HERE
    [ -f "$WINE_LENS/setup.sh" ] || WINE_LENS=/usr/share/any2odyssey/wine-lens
    PROGRAM=$(command -v any2odyssey || echo /usr/bin/any2odyssey)
else
    WINE_LENS=$APPDIR/wine-lens
    PROGRAM=$BIN/any2odyssey
fi

# ---------------------------------------------------------------------------------------------------------
if [ -n "$UNINSTALL" ]; then
    step "Removing Any2Odyssey from $PREFIX"
    if [ -z "$PACKAGED" ]; then
        run rm -f "$BIN/any2odyssey" "$SHARE/applications/any2odyssey.desktop" \
            "$SHARE/icons/hicolor/256x256/apps/any2odyssey.png"
        # Only folders this script made, named in full.
        [ -d "$PREFIX/share/any2odyssey" ] && run rm -rf "$PREFIX/share/any2odyssey"
        [ -d "$PREFIX/share/doc/any2odyssey" ] && run rm -rf "$PREFIX/share/doc/any2odyssey"
    else
        say "   the program itself belongs to your distribution's package; remove it with the package manager"
    fi
    if [ -f "$RULES_DIR/$RULES_NAME" ] && ask "Remove the udev rules ($RULES_DIR/$RULES_NAME, needs sudo)?"; then
        run sudo rm -f "$RULES_DIR/$RULES_NAME"
        run sudo udevadm control --reload
    fi
    if [ -d "$DATA/wine-lens" ] && ask "Remove the Wine side ($DATA/wine-lens: your copy of Leia's runtime, about 1 GB)?"; then
        if [ -x "$DATA/wine-lens/tools/tracking.sh" ]; then run "$DATA/wine-lens/tools/tracking.sh" stop >/dev/null 2>&1 || true; fi
        WINEPREFIX="$DATA/wine-lens/prefix" WINEDEBUG=-all run wineserver -k 2>/dev/null || true
        run rm -f "$DATA/any2odyssey-lens-helper" "$DATA/any2odyssey-weaver"
        run rm -rf "$DATA/wine-lens"
    fi
    say ""
    say "Your settings and the depth model stay in $DATA; delete that folder yourself if you want them gone."
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
    run install -Dm755 "$HERE/any2odyssey" "$BIN/any2odyssey"
    # The desktop entry names the program by its full path: ~/.local/bin is not on every session's PATH.
    if [ -n "$DRY" ]; then
        say "   would write: $SHARE/applications/any2odyssey.desktop"
    else
        mkdir -p "$SHARE/applications"
        sed "s|^Exec=any2odyssey|Exec=\"$BIN/any2odyssey\"|" "$HERE/any2odyssey.desktop" >"$SHARE/applications/any2odyssey.desktop"
    fi
    run install -Dm644 "$HERE/any2odyssey.png" "$SHARE/icons/hicolor/256x256/apps/any2odyssey.png"
    for f in README.md CHANGELOG.md USER-GUIDE.html LICENSE NOTICE THIRD-PARTY-LICENSES.md THIRD-PARTY-LICENSES-LINUX.md \
        INSTALL.txt docs/LENS-SWITCH.md docs/PLATFORMS.md; do
        [ -f "$HERE/$f" ] && run install -Dm644 "$HERE/$f" "$DOCDIR/$f"
    done
    # The Wine setup and this script (for --uninstall later), replaced as a whole.
    if [ -n "$DRY" ]; then
        say "   would copy: wine-lens -> $WINE_LENS"
    else
        mkdir -p "$APPDIR"
        rm -rf "$APPDIR/wine-lens"
        cp -R "$HERE/wine-lens" "$APPDIR/wine-lens"
        install -m755 "$HERE/install.sh" "$APPDIR/install.sh"
    fi
    have update-desktop-database && run update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
    have gtk-update-icon-cache && run gtk-update-icon-cache -q -t "$SHARE/icons/hicolor" >/dev/null 2>&1 || true
    case ":$PATH:" in *":$BIN:"*) ;; *) warn "$BIN is not on your PATH; start the program from the application menu, or as $BIN/any2odyssey" ;; esac
else
    step "1. The program is installed by your distribution's package"
fi

# ---------------------------------------------------------------------------------------------------------
# Is the Odyssey 3D part wanted at all? Without its monitor there is nothing to prepare.
find_windows() {
    [ -n "$WINROOT" ] && { echo "$WINROOT"; return 0; }
    for d in /run/media/"$USER"/* /media/"$USER"/* /media/* /mnt/*; do
        [ -f "$d/Program Files/LeiaSR/Platform/bin/SRService.exe" ] && { echo "$d"; return 0; }
    done
    return 1
}
lens_board() { grep -q -s -i '^354b$' /sys/bus/usb/devices/*/idVendor 2>/dev/null; }
webcam_dir() {
    for d in /sys/bus/usb/devices/*; do
        [ "$(cat "$d/idVendor" 2>/dev/null):$(cat "$d/idProduct" 2>/dev/null)" = 04e8:20d7 ] && { echo "$d"; return 0; }
    done
    return 1
}
WANT_ODYSSEY=
if [ -n "$ODYSSEY" ]; then
    if lens_board || [ -e /dev/odyssey-lens ] || [ -n "$WINROOT" ]; then
        WANT_ODYSSEY=1
    else
        say ""
        say "No Odyssey 3D is connected (or it is asleep): its lens, eye tracker and weaver are not prepared now."
        say "Wake the monitor and run this script again to add them."
    fi
fi

# ---------------------------------------------------------------------------------------------------------
step "2. Packages"
# What is needed, as: library or command to look for | Arch | Debian/Ubuntu | Fedora
NEEDED='
lib:libpipewire-0.3.so.0|libpipewire|libpipewire-0.3-0|pipewire-libs
lib:libxkbcommon.so.0|libxkbcommon|libxkbcommon0|libxkbcommon
lib:libGL.so.1|libglvnd|libgl1|libglvnd-glx
cmd:curl|curl|curl|curl
'
DIALOG='cmd:zenity|zenity|zenity|zenity'
ONNX_CPU='lib:libonnxruntime.so|onnxruntime-cpu|libonnxruntime-dev|onnxruntime'
ONNX_GPU='lib:libonnxruntime_providers_cuda.so|onnxruntime-cuda|libonnxruntime-dev|onnxruntime
lib:libcudnn.so|cudnn|nvidia-cudnn|'
WINE_SIDE='
cmd:wine|wine|wine|wine
cmd:x86_64-w64-mingw32-gcc|mingw-w64-gcc|gcc-mingw-w64-x86-64|mingw64-gcc
cmd:clang++|clang|clang|clang
cmd:cc|gcc|gcc|gcc
cmd:python3|python|python3|python3
cmd:xrandr|xorg-xrandr|x11-xserver-utils|xrandr
cmd:rsync|rsync|rsync|rsync
'
if have pacman; then FAMILY=2 INSTALL="sudo pacman -S --needed"
elif have apt-get; then FAMILY=3 INSTALL="sudo apt-get install -y"
elif have dnf; then FAMILY=4 INSTALL="sudo dnf install -y"
else FAMILY= INSTALL=; fi

present() {
    case $1 in
        cmd:*) have "${1#cmd:}" ;;
        lib:*) ldconfig -p 2>/dev/null | grep -q -F "${1#lib:}" || ls /usr/lib*/"${1#lib:}"* /usr/lib/*/"${1#lib:}"* >/dev/null 2>&1 ;;
    esac
}
MISSING= UNNAMED=
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
check "$NEEDED"
have zenity || have kdialog || check "$DIALOG"
if [ -n "$MODEL" ]; then
    # An NVIDIA GPU gets the CUDA build; depth then takes milliseconds instead of a tenth of a second.
    if have nvidia-smi && [ "$FAMILY" = 2 ]; then check "$ONNX_GPU"; else check "$ONNX_CPU"; fi
fi
[ -n "$WANT_ODYSSEY" ] && check "$WINE_SIDE"
MISSING=${MISSING# }
if [ -z "$MISSING$UNNAMED" ]; then
    say "   everything needed is installed"
else
    [ -n "$MISSING" ] && say "   missing packages: $MISSING"
    [ -n "$UNNAMED" ] && say "   also missing, under a name this script does not know for your distribution:$UNNAMED"
    if [ -z "$DEPS" ] || [ -z "$INSTALL" ] || [ -z "$MISSING" ]; then
        [ -n "$MISSING" ] && say "   install them with your package manager, then run this script again"
    elif ask "Install them now ($INSTALL ...; needs sudo)?"; then
        # shellcheck disable=SC2086  # a list of package names
        run $INSTALL $MISSING || warn "the package manager did not finish; the steps below may be incomplete"
    fi
fi

# ---------------------------------------------------------------------------------------------------------
step "3. udev rules for the Odyssey 3D"
RULES=$WINE_LENS/$RULES_NAME
[ -f "$RULES" ] || RULES=$HERE/wine-lens/$RULES_NAME
if [ -z "$UDEV" ]; then
    say "   skipped (--no-udev)"
elif [ -f /usr/lib/udev/rules.d/$RULES_NAME ]; then
    say "   installed by your distribution's package"
elif [ ! -f "$RULES" ]; then
    warn "$RULES_NAME was not found; skipped"
elif [ -f "$RULES_DIR/$RULES_NAME" ] && cmp -s "$RULES" "$RULES_DIR/$RULES_NAME"; then
    say "   already installed"
elif ask "Install $RULES_DIR/$RULES_NAME (lens board for you at the seat, no USB power saving for the webcam; needs sudo)?"; then
    run sudo install -m644 "$RULES" "$RULES_DIR/$RULES_NAME"
    run sudo udevadm control --reload
    run sudo udevadm trigger --subsystem-match=tty
    # The webcam rule acts when the camera appears; for one that is here already, set it now.
    if CAM=$(webcam_dir); then
        run sudo sh -c "echo on > '$CAM/power/control'"
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
elif [ ! -x "$PROGRAM" ] && [ -z "$DRY" ]; then
    warn "$PROGRAM is not there; skipped"
elif ask "Download the open depth model (Depth Anything V2 Small, about 95 MB, checked against a fixed SHA-256)?"; then
    run "$PROGRAM" --install-depth-model || warn "the model was not installed; try '$PROGRAM --install-depth-model' later"
fi

# ---------------------------------------------------------------------------------------------------------
step "5. The Odyssey 3D's lens, eye tracker and weaver (Leia's runtime under Wine)"
if [ -z "$WANT_ODYSSEY" ]; then
    say "   skipped"
elif ! WIN=$(find_windows); then
    say "   No Windows installation of the Odyssey 3D Hub was found on a mounted drive."
    say "   Mount the Windows system drive (read-only is enough) and run:"
    say "       $WINE_LENS/setup.sh /path/to/that/drive"
    say "   or this script again with --windows-root /path/to/that/drive."
elif [ ! -f "$WIN/Program Files/LeiaSR/Platform/bin/SRService.exe" ]; then
    warn "$WIN has no Leia SR runtime (Program Files/LeiaSR); is the Odyssey 3D Hub installed on that Windows?"
elif [ ! -e /dev/odyssey-lens ] && [ -z "$DRY" ]; then
    warn "the lens board is not available as /dev/odyssey-lens (step 3 installs the rule; the monitor must be awake). Then run: $WINE_LENS/setup.sh '$WIN'"
elif ask "Prepare it now from $WIN (copies about 1 GB of your own installation into $DATA/wine-lens)?"; then
    run "$WINE_LENS/setup.sh" "$WIN" || warn "the setup did not finish; see its message above and docs/LENS-SWITCH.md"
fi

say ""
say "Done. Start Any2Odyssey from the application menu or as: $PROGRAM"
[ -z "$PACKAGED" ] && say "To remove it later: $APPDIR/install.sh --uninstall"
exit 0
