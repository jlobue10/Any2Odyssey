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

for tool in wine wineserver $CC x86_64-w64-mingw32-objdump python3 xrandr rsync stty flock setsid; do
    command -v "$tool" >/dev/null || die "'$tool' is needed (Arch: wine mingw-w64-gcc python xorg-xrandr rsync)"
done
# The tools tell whether Leia's service runs by looking among this user's own processes. BusyBox's pgrep
# cannot (it has no -u) and fails instead: every start of the lens helper would restart the service.
pgrep -u "$(id -u)" -f . >/dev/null 2>&1 ||
    die "a pgrep that takes -u is needed: the one of procps (procps-ng), not BusyBox's"
[ -e "$BOARD" ] || die "$BOARD is missing. Install the udev rule and wake the monitor:
    sudo cp '$HERE/70-any2odyssey-lens.rules' /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger"

# Wine's own 64-bit Windows libraries: the stand-ins forward to copies of them.
WINELIB=
for d in /usr/lib/wine/x86_64-windows /usr/lib64/wine/x86_64-windows /usr/lib/x86_64-linux-gnu/wine/x86_64-windows \
         /opt/wine-stable/lib/wine/x86_64-windows /opt/wine-devel/lib/wine/x86_64-windows /opt/wine-staging/lib/wine/x86_64-windows; do
    [ -f "$d/cfgmgr32.dll" ] && WINELIB=$d && break
done
[ -n "$WINELIB" ] || die "could not find Wine's x86_64-windows library folder"

export WINEPREFIX=$T/prefix WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
SYS=$WINEPREFIX/drive_c/windows/system32
mkdir -p "$T/build" "$T/tools"
chmod 700 "$T" # the prefix's registry will hold the camera secret
wineserver -k 2>/dev/null || true
# A camera bridge still running from an earlier set-up is the program as it was built then. The camera
# stand-in built below need not speak its handshake, and tools/tracking.sh neither takes it for the bridge
# once its file has been replaced nor can start another while it holds the port. It is ended here, if the
# process of the noted number is that program.
BRIDGE_PID=$(cat "${XDG_RUNTIME_DIR:-$T}/any2odyssey-camera-bridge.pid" 2>/dev/null) || BRIDGE_PID=
case $BRIDGE_PID in '' | *[!0-9]*) ;; *)
    case $(readlink "/proc/$BRIDGE_PID/exe" 2>/dev/null) in
        "$(readlink -f "$T/build")/any2odyssey-camera-bridge"*) kill "$BRIDGE_PID" 2>/dev/null || true ;;
    esac ;;
esac

echo "setup: Wine prefix in $WINEPREFIX"
if [ ! -f "$WINEPREFIX/system.reg" ]; then
    setsid wineboot -i >/dev/null 2>&1 </dev/null
    wineserver -w
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
    x86_64-w64-mingw32-objdump -p "$WINELIB/$lib.dll" |
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
    echo "setup: no clang++; the output through Leia's weaver is not prepared (Arch: clang)"
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
(cd "$B" && setsid timeout 120 wine regedit /S device.reg >/dev/null 2>&1 </dev/null)
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
    (cd "$B" && setsid timeout 120 wine regedit /S camera.reg >/dev/null 2>&1 </dev/null) || true
    rm -f "$B/camera.reg" # it holds the secret
    timeout 60 wine sc query "SR Eye Tracker" 2>/dev/null | grep -q "SERVICE_NAME" ||
        setsid timeout 60 wine sc create "SR Eye Tracker" \
            binPath= 'C:\Program Files\LeiaSR\Tracker\bin\SREyeTracker.exe' start= demand >/dev/null 2>&1 </dev/null
    timeout 60 wine sc query "SR Eye Tracker" 2>/dev/null | grep -q "SERVICE_NAME" ||
        die "Leia's eye tracker could not be registered in the prefix"
fi
wineserver -k 2>/dev/null || true

cp "$HERE"/tools/*.sh "$HERE"/tools/mkedid.py "$HERE"/tools/kwin-overlay.js "$T/tools/"
cp "$HERE/any2odyssey-lens-helper" "$HERE/any2odyssey-weaver" "$T/"
chmod +x "$T/any2odyssey-lens-helper" "$T/any2odyssey-weaver" "$T"/tools/*
ln -sf "$T/any2odyssey-lens-helper" "$DATA/any2odyssey-lens-helper"
if [ -n "$WEAVER" ]; then ln -sf "$T/any2odyssey-weaver" "$DATA/any2odyssey-weaver"; else rm -f "$DATA/any2odyssey-weaver"; fi
rm -f "$T/stack.started"

echo "setup: done. Any2Odyssey finds the helper at $DATA/any2odyssey-lens-helper."
echo "       Try it by hand (lens on for 20 s; the first start takes about half a minute):"
echo "           sleep 20 | '$DATA/any2odyssey-lens-helper'"
if [ -n "$TRACKING" ]; then
    echo "       Eye tracking is prepared too: '$T/tools/tracking.sh' start | stop | status"
    echo "       (start it while the lens helper is running; it needs Leia's service up)."
fi
