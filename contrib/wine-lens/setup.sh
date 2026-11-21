#!/bin/sh
# SPDX-License-Identifier: MIT
# Prepare the Wine route for switching the Odyssey 3D's lens from Linux: a Wine prefix holding your own copy
# of Leia's Windows SR runtime, plus the small stand-in libraries Wine needs before that runtime works.
# Nothing of Leia's is distributed with Any2Odyssey; the runtime is copied from a Windows installation of
# yours that has the Odyssey 3D Hub (LeiaSR) installed.
#
# usage: setup.sh <windows-root>
#   <windows-root>  a mounted Windows system drive, the folder that contains "Program Files" and "ProgramData"
#
# Everything goes under $ANY2ODYSSEY_DATA_DIR (default ~/.local/share/any2odyssey)/wine-lens.
#
# The picture through Leia's own weaver (any2odyssey-weaver) is prepared when clang++ is installed: one small
# file has to be compiled for the Microsoft C++ layout, which the MinGW compiler does not produce.
#
# Eye tracking is prepared as well when that Windows also has Leia's eye tracker ("LeiaSR/Tracker") and this
# machine has a C compiler: the tracker is copied, and it gets the monitor's webcam through a small native
# bridge (tools/tracking.sh starts and stops the two). ANY2ODYSSEY_CAMERA_PORT picks the bridge's loopback
# port (default 47816).
# Run it again after a Wine upgrade or if the monitor (lens board) is replaced.
set -eu

HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
# An XDG_DATA_HOME that is not an absolute path counts as unset, as the program reads it (the XDG Base
# Directory specification): taken as written, the Wine side was built under the folder this was run from.
case ${XDG_DATA_HOME:-} in /*) DATA_HOME=$XDG_DATA_HOME ;; *) DATA_HOME=$HOME/.local/share ;; esac
DATA=${ANY2ODYSSEY_DATA_DIR:-$DATA_HOME/any2odyssey}
T=$DATA/wine-lens
BOARD=/dev/odyssey-lens
CC=x86_64-w64-mingw32-gcc

die() { echo "setup: $*" >&2; exit 1; }

# --help prints the lines at the top of this file. It was taken for the Windows drive: "no Leia SR runtime
# in --help/Program Files/LeiaSR/Platform".
case ${1:-} in
    -h | --help) awk 'NR >= 3 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "$(readlink -f "$0")"; exit 0 ;;
esac
[ $# -eq 1 ] || die "usage: setup.sh <windows-root>"
WIN=${1%/}
RUNTIME="$WIN/Program Files/LeiaSR/Platform"
PROGDATA="$WIN/ProgramData/Simulated Reality"
[ -f "$RUNTIME/bin/SRService.exe" ] || die "no Leia SR runtime in $RUNTIME (is the Odyssey 3D Hub installed on that Windows?)"
[ -d "$PROGDATA" ] || die "missing $PROGDATA"
TRACKER="$WIN/Program Files/LeiaSR/Tracker"
PORT=${ANY2ODYSSEY_CAMERA_PORT:-47816}
TRACKING=
if [ ! -f "$TRACKER/bin/SREyeTracker.exe" ]; then
    echo "setup: no eye tracker in $TRACKER; preparing the lens switch only"
elif ! command -v cc >/dev/null; then
    echo "setup: no C compiler (cc) for the camera bridge; preparing the lens switch only"
else
    TRACKING=1
fi

# Ubuntu 26.04's wine package has wineserver beside Wine's libraries only, not on the PATH: the set-up ended
# there with "'wineserver' is needed". (tools/start-stack.sh looks in the same places.)
if ! command -v wineserver >/dev/null; then
    for d in /usr/lib/x86_64-linux-gnu/wine /usr/lib/wine /usr/lib64/wine; do
        if [ -x "$d/wineserver" ]; then PATH=$PATH:$d; break; fi
    done
fi
for tool in wine wineserver $CC x86_64-w64-mingw32-objdump python3 xrandr rsync stty flock setsid; do
    command -v "$tool" >/dev/null || die "'$tool' is needed (Arch: wine mingw-w64-gcc python xorg-xrandr rsync; Debian and Ubuntu: wine gcc-mingw-w64-x86-64 python3 x11-xserver-utils rsync; Fedora: wine mingw64-gcc python3 xrandr rsync; openSUSE: wine mingw64-cross-gcc python3 xrandr rsync)"
done
# The cfgmgr32 stand-in (src/cfgshim.c) leaves it to Wine's own cfgmgr32 to find a device by its name and to
# name the device it is given, and links parent and child itself. Wine does both since 11.6; before that
# its functions answer without doing the work (or are not there at all), Leia's service cannot walk from the
# lens board to its serial port, and the set-up was made without a complaint: every request then ended after
# 80 seconds with "Leia's service did not get the lens board ready". Debian 12 and 13 and Ubuntu up to 26.04
# have Wine 8.0 to 10.0. A version that cannot be read (a build with a name of its own) is let through.
wine_too_old() { # <what "wine --version" says>
    case $1 in wine-[0-9]*.[0-9]*) ;; *) return 1 ;; esac
    number=${1#wine-}
    major=${number%%.*}
    minor=${number#*.}
    minor=${minor%%[!0-9]*}
    case $major in *[!0-9]*) return 1 ;; esac
    [ "$major" -lt 11 ] || { [ "$major" -eq 11 ] && [ "${minor:-0}" -lt 6 ]; }
}
WINE_IS=$(wine --version 2>/dev/null | head -n 1) || WINE_IS=
# (WineHQ's packages by the names that put a wine on the PATH: winehq-devel and winehq-staging. The message
# named wine-devel and wine-staging, which install under /opt alone, and this check then found the old Wine
# again.)
if wine_too_old "$WINE_IS"; then
    die "this Wine is too old for the Odyssey 3D set-up: $WINE_IS, and Wine 11.6 or newer is needed (older ones cannot tell Leia's service where the lens board is). Where the distribution's own Wine is older, WineHQ's packages winehq-devel or winehq-staging are new enough."
fi
# The tools tell whether Leia's service runs by looking among this user's own processes. BusyBox's pgrep
# cannot (it has no -u) and fails instead: every start of the lens helper would restart the service.
pgrep -u "$(id -u)" -f . >/dev/null 2>&1 ||
    die "a pgrep that takes -u is needed: the one of procps (the package procps or procps-ng), not BusyBox's"
[ -e "$BOARD" ] || die "$BOARD is missing. Install the udev rule and wake the monitor:
    sudo cp '$HERE/70-any2odyssey-lens.rules' /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger"

# Wine's own 64-bit Windows libraries: the stand-ins forward to copies of them. They used to be taken from a
# list of usual folders, whichever wine runs. With two Wines on one machine (WineHQ's under /opt beside a
# distribution's libraries, or a build of one's own) the version asked for above was one Wine's, and the
# libraries listed, copied and linked against below the other's: the set-up ended with "done" and nothing in
# the prefix started. They are now looked for beside the wine that runs first, which is the PATH's wine with
# every link followed. A wine that is not kept beside its libraries (a script of the user's own that starts a
# Wine elsewhere, which is not read) is asked further down, once it has a prefix to run in, and its answer is
# noted in wine.libs; the usual folders come after both. tools/start-stack.sh has the same function, word
# for word.
wine_libraries() { # [beside]: with that word, only the folder beside the wine that runs
    wine_is=$(command -v wine 2>/dev/null) || wine_is=
    case $wine_is in /*) wine_is=$(readlink -f "$wine_is" 2>/dev/null) || wine_is= ;; *) wine_is= ;; esac
    case $wine_is in
        */bin/*)
            # <root>/bin/wine: the libraries where a distribution or WineHQ's packages keep them under <root>
            wine_root=${wine_is%/bin/*}
            for d in "$wine_root/lib/wine" "$wine_root/lib64/wine" "$wine_root/lib/x86_64-linux-gnu/wine" \
                     "$wine_root/lib64/wine-wow64/wine" "$wine_root/wine"; do
                if [ -f "$d/x86_64-windows/cfgmgr32.dll" ]; then echo "$d/x86_64-windows"; return 0; fi
            done ;;
        /*)
            # (a wine that is kept beside its libraries)
            d=${wine_is%/*}
            if [ -f "$d/x86_64-windows/cfgmgr32.dll" ]; then echo "$d/x86_64-windows"; return 0; fi ;;
    esac
    [ "${1:-}" != beside ] || return 1
    # (not beside it: what the Wine that runs said when setup.sh asked it, while that folder is there)
    d=$(cat "$T/wine.libs" 2>/dev/null) || d=
    if [ -n "$d" ] && [ -f "$d/cfgmgr32.dll" ]; then echo "$d"; return 0; fi
    for d in /usr/lib/wine/x86_64-windows /usr/lib64/wine/x86_64-windows /usr/lib/x86_64-linux-gnu/wine/x86_64-windows \
             /usr/lib64/wine-wow64/wine/x86_64-windows \
             /opt/wine-stable/lib/wine/x86_64-windows /opt/wine-devel/lib/wine/x86_64-windows /opt/wine-staging/lib/wine/x86_64-windows; do
        if [ -f "$d/cfgmgr32.dll" ]; then echo "$d"; return 0; fi
    done
    return 1
}
# (Found nowhere, they are not beside the wine that runs: the set-up goes on until that Wine can be asked.)
WINELIB=$(wine_libraries) || WINELIB=

export WINEPREFIX=$T/prefix WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
SYS=$WINEPREFIX/drive_c/windows/system32
mkdir -p "$T/build" "$T/tools"
chmod 700 "$T" # the prefix's registry will hold the camera secret
wineserver -k 2>/dev/null || true
# A camera bridge still running from an earlier set-up is the program as it was built then. The camera
# stand-in built below need not speak its handshake, and tools/tracking.sh neither takes it for the bridge
# once its file has been replaced nor can start another while it holds the port. It is ended here: the
# process of the noted number, and any other of this user's processes (the note can be lost while the bridge
# lives, see tools/tracking.sh), if its program is that one.
BRIDGE_IS=$(readlink -f "$T/build")/any2odyssey-camera-bridge
for pid in $(cat "${XDG_RUNTIME_DIR:-$T}/any2odyssey-camera-bridge.pid" 2>/dev/null) \
           $(pgrep -u "$(id -u)" -f 'any2odyssey-camera-[b]ridge' 2>/dev/null); do
    case $pid in *[!0-9]*) continue ;; esac
    case $(readlink "/proc/$pid/exe" 2>/dev/null) in
        "$BRIDGE_IS" | "$BRIDGE_IS (deleted)") kill "$pid" 2>/dev/null || true ;;
    esac
done

echo "setup: Wine prefix in $WINEPREFIX"
# A prefix with no finished set-up behind it (this script puts its tools into place last) is what a first run
# left that did not get through: Ctrl+C while Wine made the prefix, or a disk that filled up. Wine does not
# complete such a prefix, and with it every later run ended as the first had, some of them without a word
# (after a full disk: exit 53 in a fifth of a second, at every run). It is made anew.
if [ -d "$WINEPREFIX" ] && [ ! -f "$T/tools/start-stack.sh" ]; then rm -rf "$WINEPREFIX"; fi
if [ ! -f "$WINEPREFIX/system.reg" ]; then
    setsid wineboot -i >/dev/null 2>&1 </dev/null ||
        die "Wine could not make the prefix $WINEPREFIX (wineboot ended with $?). It needs about 2 GB there: is the disk full?"
    wineserver -w
else
    # A prefix from an earlier set-up, and maybe from an earlier Wine: Wine updates it at the first program
    # run in it, and has to get past the stand-ins that are in it. With a prefix set up under Wine 11.17 and
    # Wine then 11.22 that update never ended while the cfgmgr32 stand-in was in its way: this script stopped
    # two minutes later at its first program below, without a word, and left a prefix in which nothing
    # started. So Wine updates the prefix here, by a program that does nothing, with its own five libraries
    # in place of the stand-ins for that one run (tools/start-stack.sh does the same after an upgrade).
    WINEDLLOVERRIDES="cfgmgr32,dxva2,wtsapi32,mf,mfreadwrite=b;$WINEDLLOVERRIDES" \
        setsid timeout 300 wine cmd /c exit >/dev/null 2>&1 </dev/null ||
        die "Wine did not get the prefix $WINEPREFIX ready (it updates a prefix that an earlier Wine made). Moved out of the way, a new one is made by this script: mv '$WINEPREFIX' '$WINEPREFIX.old'"
    wineserver -k 2>/dev/null || true
fi

# The wine on the PATH is not kept beside its libraries: a script of the user's own that starts a Wine
# elsewhere. The folder taken above is then the first of the usual ones, and where another Wine is installed
# there it was that Wine's: with a script that starts Wine 11.22 and WineHQ's wine-stable under /opt, the
# set-up said "done" with all five copies and the stand-ins' lists taken from the other Wine, and with no
# other Wine it stopped at "could not find Wine's x86_64-windows library folder". Now that there is a prefix
# to run in, the Wine that runs is asked: it tells its programs where its libraries are (WINEDLLDIR0, as
# "\??\Z:\opt\wine-devel\lib\wine"; Z: is "/" in a prefix that Wine made), with its own five libraries in
# place of the stand-ins that a prefix from an earlier set-up has. The answer is noted for
# tools/start-stack.sh, which has to know the folder before it runs any program of Wine's.
if wine_libraries beside >/dev/null; then
    rm -f "$T/wine.libs"
else
    rm -f "$T/wine.libs"
    # (every "\" of the answer becomes "/" before it is looked at)
    asked=$(WINEDLLOVERRIDES="cfgmgr32,dxva2,wtsapi32,mf,mfreadwrite=b;$WINEDLLOVERRIDES" \
        setsid timeout 120 wine cmd /c 'echo %WINEDLLDIR0%' 2>/dev/null </dev/null |
        tr -d '\r' | tr "\\\\" / | head -n 1) || asked=
    wineserver -k 2>/dev/null || true
    case $asked in
        '/??/Z:/'* | '/??/z:/'*)
            asked=/${asked#/??/?:/}/x86_64-windows
            if [ -f "$asked/cfgmgr32.dll" ]; then
                WINELIB=$asked
                echo "$WINELIB" >"$T/wine.libs"
            fi ;;
    esac
    [ -n "$WINELIB" ] || die "could not find Wine's x86_64-windows library folder"
    echo "setup: the wine on the PATH is not kept beside Wine's libraries; they are taken from $WINELIB"
fi

echo "setup: copying Leia's runtime from $WIN"
mkdir -p "$WINEPREFIX/drive_c/Program Files/LeiaSR" "$WINEPREFIX/drive_c/ProgramData"
rsync -a --delete "$RUNTIME" "$WINEPREFIX/drive_c/Program Files/LeiaSR/"
rsync -a --exclude 'Log/*' --exclude 'Applications/*' "$PROGDATA" "$WINEPREFIX/drive_c/ProgramData/"

echo "setup: building the stand-ins"
python3 "$HERE/tools/mkboard.py" "$BOARD" "$T/build/board.h" "$T/build/device.reg"

# A .def that forwards every export of a Wine library to a renamed copy of it, except the named functions,
# which the stand-in provides as shim_<name> (or, written <name>=<library>.<function>, another library does).
forwarding_def() { # <library> <copy name> <own functions...>
    lib=$1 copy=$2; shift 2
    echo "LIBRARY $lib.dll"; echo "EXPORTS"
    # (in English: an objdump built with translations writes the heading looked for in the user's language)
    LC_ALL=C x86_64-w64-mingw32-objdump -p "$WINELIB/$lib.dll" |
        awk '/Ordinal\/Name Pointer/{p=1;next} /^$/{if(p)exit} p&&$NF!="Name"{print $NF}' |
        while read -r name; do
            target=$copy.$name
            for own in "$@"; do
                case $own in
                    "$name") target=shim_$name ;;
                    "$name="*) target=${own#*=} ;; # <name>=<library>.<function>: provided by another library
                esac
            done
            echo "    $name = $target"
        done
}
forwarding_def cfgmgr32 cfgmgw CM_Get_Child CM_Get_Child_Ex CM_Get_Parent CM_Get_Parent_Ex \
    CM_Get_DevNode_Status CM_Get_DevNode_Status_Ex >"$T/build/cfgmgr32.def"
forwarding_def dxva2 dxva2w GetNumberOfPhysicalMonitorsFromHMONITOR GetPhysicalMonitorsFromHMONITOR \
    DestroyPhysicalMonitor DestroyPhysicalMonitors >"$T/build/dxva2.def"
forwarding_def wtsapi32 wtsapiw WTSQuerySessionInformationA WTSQuerySessionInformationW >"$T/build/wtsapi32.def"

S=$HERE/src
B=$T/build
$CC -O2 -shared -o "$B/a2otoast.dll" "$S/a2otoast.c" "$S/a2otoast.def" -lole32 -luuid -lruntimeobject
$CC -O2 -shared -o "$B/cfgmgr32.dll" "$S/cfgshim.c" "$S/powernotify.c" "$B/cfgmgr32.def"
$CC -O2 -shared -o "$B/dxva2.dll" "$S/dxvashim.c" "$B/dxva2.def"
$CC -O2 -shared -o "$B/wtsapi32.dll" "$S/wtsshim.c" "$B/wtsapi32.def"
$CC -O2 -shared -nostdlib -nostartfiles -Wl,--subsystem,native -Wl,--entry,DriverEntry -I"$B" \
    -o "$B/a2ousb.sys" "$S/a2ousb.c" -L"$WINELIB" -lntoskrnl
$CC -O2 -o "$B/monprobe.exe" "$S/monprobe.c"
$CC -O2 -o "$B/any2odyssey-lens-helper.exe" "$S/lens-helper-win.c"
# The serial port's keeper as one small native program; without a C compiler tools/noxon.sh does the same
# work with a shell loop, at the cost of two programs started for every look at the port.
rm -f "$B/any2odyssey-noxon"
if command -v cc >/dev/null; then
    cc -O2 -o "$B/any2odyssey-noxon" "$S/noxon.c" || rm -f "$B/any2odyssey-noxon"
fi
WEAVER=
rm -f "$B/any2odyssey-weaver.exe"
if ! command -v clang++ >/dev/null; then
    echo "setup: no clang++; the output through Leia's weaver is not prepared (the package is clang)"
elif clang++ --target=x86_64-pc-windows-msvc -O2 -ffreestanding -fno-exceptions -fno-rtti -nostdinc \
        -c -o "$B/srweaver.obj" "$S/srweaver.cpp" &&
     $CC -O2 -o "$B/any2odyssey-weaver.exe" "$S/weaver-host.c" "$B/srweaver.obj" -lopengl32 -lgdi32 -luser32; then
    WEAVER=1
else
    rm -f "$B/any2odyssey-weaver.exe"
    echo "setup: the weaver host did not build (does this clang++ have the x86_64-pc-windows-msvc target?); the output through Leia's weaver is not prepared"
fi
if [ -n "$TRACKING" ]; then
    forwarding_def mf mfw MFEnumDeviceSources=a2ocam.A2OEnumDeviceSources >"$B/mf.def"
    forwarding_def mfreadwrite mfreadww \
        MFCreateSourceReaderFromMediaSource=a2ocam.A2OCreateSourceReaderFromMediaSource >"$B/mfreadwrite.def"
    $CC -O2 -shared -o "$B/a2ocam.dll" "$S/a2ocam.c" "$S/connreset.c" "$S/a2ocam.def" \
        -lmfplat -lmfuuid -lws2_32 -lole32 -luuid -lstrmiids -ladvapi32 -static-libgcc
    $CC -shared -o "$B/mf.dll" "$B/mf.def"
    $CC -shared -o "$B/mfreadwrite.dll" "$B/mfreadwrite.def"
    cc -O2 -o "$B/any2odyssey-camera-bridge" "$S/camera-bridge.c"
    cc -O2 -shared -fPIC -o "$B/any2odyssey-udp.so" "$S/udp-errors.c" -ldl
fi

echo "setup: installing them into the prefix"
cp "$WINELIB/cfgmgr32.dll" "$SYS/cfgmgw.dll"
cp "$WINELIB/dxva2.dll" "$SYS/dxva2w.dll"
cp "$WINELIB/wtsapi32.dll" "$SYS/wtsapiw.dll"
cp "$B/a2otoast.dll" "$B/cfgmgr32.dll" "$B/dxva2.dll" "$B/wtsapi32.dll" "$SYS/"
cp "$B/a2ousb.sys" "$SYS/drivers/"
# (Each of the three requests to Wine below that can fail now ends the set-up with its reason. Under "set -e"
# this first one ended the script without a word, after two minutes when it was Wine that did not come back.)
(cd "$B" && setsid timeout 120 wine regedit /S device.reg >/dev/null 2>&1 </dev/null) ||
    die "Wine did not import the lens board's entries into the prefix's registry (wine regedit ended with $?; 124 means it had not ended after two minutes). The set-up is not finished; run it again."
(cd "$WINEPREFIX/drive_c/Program Files/LeiaSR/Platform/bin" && setsid timeout 120 wine SRService.exe -install >/dev/null 2>&1 </dev/null) || true
timeout 60 wine sc query "SR Service" 2>/dev/null | grep -q "SERVICE_NAME" || die "Leia's service could not be registered in the prefix"
if [ -n "$TRACKING" ]; then
    echo "setup: copying Leia's eye tracker and installing the camera stand-in"
    rsync -a --delete "$TRACKER" "$WINEPREFIX/drive_c/Program Files/LeiaSR/"
    cp "$WINELIB/mf.dll" "$SYS/mfw.dll"
    cp "$WINELIB/mfreadwrite.dll" "$SYS/mfreadww.dll"
    cp "$B/a2ocam.dll" "$B/mf.dll" "$B/mfreadwrite.dll" "$SYS/"
    # The bridge hands the camera only to whoever knows this secret (other local users must not get it).
    trap 'rm -f "$B/camera.reg"' EXIT
    (umask 077
     [ -s "$T/camera.secret" ] || head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n' >"$T/camera.secret"
     echo "$PORT" >"$T/camera.port"
     { cat "$S/camera.reg"
       printf '\n[HKEY_LOCAL_MACHINE\\Software\\Any2Odyssey\\Camera]\n"Port"=dword:%08x\n"Secret"="%s"\n' \
           "$PORT" "$(cat "$T/camera.secret")"
       printf '"SymbolicLink"="%s"\n"FriendlyName"="3D Stereo WebCam"\n' \
           '\\\\?\\USB#VID_04E8&PID_20D7&MI_00#8&2939E249&0&0000#{E5323777-F976-4F5B-9B55-B94699C46E44}\\GLOBAL'
     } >"$B/camera.reg")
    # A failure of this import was let pass and never looked at: the set-up said "done", the camera stand-in
    # found neither the port nor the secret, and Leia's eye tracker never got a camera. It ends the set-up
    # now, and so does a secret that Wine does not have afterwards (asked for by its name only; what Wine
    # answers is not kept).
    (cd "$B" && setsid timeout 120 wine regedit /S camera.reg >/dev/null 2>&1 </dev/null) ||
        die "Wine did not import the camera's entries into the prefix's registry (wine regedit ended with $?; 124 means it had not ended after two minutes). The set-up is not finished; run it again."
    rm -f "$B/camera.reg" # it holds the secret
    timeout 60 wine reg query 'HKLM\Software\Any2Odyssey\Camera' /v Secret >/dev/null 2>&1 </dev/null ||
        die "the camera's entries are not there in the prefix's registry after Wine imported them (wine reg query ended with $?): Leia's eye tracker would get no camera. The set-up is not finished; run it again."
    # (The request to register the tracker was the last of an "||" list: under "set -e" its failure ended the
    # script before the line after it could say what had failed.)
    timeout 60 wine sc query "SR Eye Tracker" 2>/dev/null | grep -q "SERVICE_NAME" ||
        setsid timeout 60 wine sc create "SR Eye Tracker" \
            binPath= 'C:\Program Files\LeiaSR\Tracker\bin\SREyeTracker.exe' start= demand >/dev/null 2>&1 </dev/null ||
        true
    timeout 60 wine sc query "SR Eye Tracker" 2>/dev/null | grep -q "SERVICE_NAME" ||
        die "Leia's eye tracker could not be registered in the prefix (after \"wine sc create\", Wine has no service \"SR Eye Tracker\"). The set-up is not finished; run it again."
fi
wineserver -k 2>/dev/null || true

cp "$HERE"/tools/*.sh "$HERE"/tools/mkedid.py "$HERE"/tools/kwin-overlay.js "$T/tools/"
cp "$HERE/any2odyssey-lens-helper" "$HERE/any2odyssey-weaver" "$T/"
chmod +x "$T/any2odyssey-lens-helper" "$T/any2odyssey-weaver" "$T"/tools/*
ln -sf "$T/any2odyssey-lens-helper" "$DATA/any2odyssey-lens-helper"
if [ -n "$WEAVER" ]; then ln -sf "$T/any2odyssey-weaver" "$DATA/any2odyssey-weaver"; else rm -f "$DATA/any2odyssey-weaver"; fi
rm -f "$T/stack.started"

echo "setup: done. Any2Odyssey finds the helper at $DATA/any2odyssey-lens-helper."
echo "       Try it by hand (the lens goes off a minute after the command was given, and the first start takes"
echo "       about half of that minute):"
echo "           sleep 60 | '$DATA/any2odyssey-lens-helper'"
if [ -n "$TRACKING" ]; then
    echo "       Eye tracking is prepared too: '$T/tools/tracking.sh' start | stop | status"
    echo "       (start it while the lens helper is running; it needs Leia's service up)."
fi

# What this Wine side was made from: a digest of the set-up's own files (this script, the sources of the
# stand-ins, the tools). The installer (packaging/linux/install.sh, older_wine_side) makes the same digest of
# the set-up it has installed and says so when the two differ. Up to 1.0.0 it compared the copied scripts
# only: a release that changed nothing but a stand-in's source left the old libraries in the prefix, and
# nothing said so. Noted last: a set-up that stopped on the way leaves no note, or the one before it.
(cd "$HERE" && sha256sum setup.sh any2odyssey-lens-helper any2odyssey-weaver src/*.* tools/*.* | LC_ALL=C sort |
    sha256sum | cut -d' ' -f1) >"$T/made-from" 2>/dev/null || true
[ -s "$T/made-from" ] || rm -f "$T/made-from"
