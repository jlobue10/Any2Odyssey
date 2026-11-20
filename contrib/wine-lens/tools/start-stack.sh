#!/bin/sh
# SPDX-License-Identifier: MIT
# (Re)start Leia's SR service and session in the Wine prefix. Used by any2odyssey-lens-helper.
# usage: start-stack.sh [WINEDEBUG for the service and session, default -all]
T=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
export WINEPREFIX=$T/prefix WINEDLLOVERRIDES="mscoree,mshtml="
BIN="$WINEPREFIX/drive_c/Program Files/LeiaSR/Platform/bin"
DEBUG=${1:--all}
# Every Wine process of the prefix hears about UDP datagrams that bounce (src/udp-errors.c): Leia's eye
# tracker pauses only when it learns that way that nobody listens any more.
[ -f "$T/build/any2odyssey-udp.so" ] && export LD_PRELOAD="$T/build/any2odyssey-udp.so${LD_PRELOAD:+:$LD_PRELOAD}"

# (wineserver where Ubuntu 26.04 keeps it, off the PATH: see setup.sh)
if ! command -v wineserver >/dev/null; then
    for d in /usr/lib/x86_64-linux-gnu/wine /usr/lib/wine /usr/lib64/wine; do
        if [ -x "$d/wineserver" ]; then PATH=$PATH:$d; break; fi
    done
fi
WINEDEBUG=-all wineserver -k 2>/dev/null
sleep 3
# A server asked to leave takes the prefix's programs with it. One that was killed outright, or crashed,
# leaves them behind, and they can do nothing without it; but Leia's service, session and tracker are looked
# for by name (tools/ensure-stack.sh, tools/tracking.sh), and the ones left behind were taken for running.
# What is still there of this prefix now is ended: Wine's own programs only (the camera bridge and the
# serial port's keeper have the prefix in their environment too, and are looked after elsewhere).
for pid in $(pgrep -u "$(id -u)" -f '\.exe' 2>/dev/null); do
    exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
    # (Wine's loader under the names it has had; "(deleted)" after an upgrade of Wine)
    case ${exe##*/} in
        wine | wine32 | wine64 | wine-preloader | wine32-preloader | wine64-preloader | wine*' (deleted)') ;;
        *) continue ;;
    esac
    tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null | grep -q -x -F "WINEPREFIX=$WINEPREFIX" && kill -9 "$pid" 2>/dev/null
done
# After an upgrade of Wine. setup.sh puts copies of five of Wine's own libraries into the prefix under other
# names (cfgmgw.dll for cfgmgr32.dll and so on), and each stand-in passes on to its copy every call it does not
# answer itself. Wine's update of a prefix, which comes with its first program after an upgrade, leaves the
# stand-ins and the copies alone, and has to get past them: Wine's own programs load the stand-ins too. From
# 11.21 to 11.22 it did. With a set-up made under Wine 11.10, 11.14 or 11.17 and Wine then 11.22, the update
# never ended (Wine's rundll32 stood still, with the cfgmgr32 stand-in in place; with Wine's own cfgmgr32 it
# took 14 seconds), and every program of the prefix waited for it: the lens helper gave no answer at all.
# So where the copies are not the installed Wine's (which is how an upgrade shows here), Wine first updates
# the prefix by itself, with its own five libraries in place of the stand-ins for that one run, and the copies
# are then brought up to the installed Wine, from the folder setup.sh takes them from.
# A stand-in also lists the names its library had when setup.sh built it. Where the installed Wine's library
# has other names now, a new copy is not enough (with a set-up made under Wine 11.0, Wine 11.22's setupapi
# asked the stand-in for a name it did not have, and nothing in the prefix started): that takes setup.sh,
# and this start says so instead of trying.
SYS=$WINEPREFIX/drive_c/windows/system32
WINELIB=
for d in /usr/lib/wine/x86_64-windows /usr/lib64/wine/x86_64-windows /usr/lib/x86_64-linux-gnu/wine/x86_64-windows \
         /usr/lib64/wine-wow64/wine/x86_64-windows \
         /opt/wine-stable/lib/wine/x86_64-windows /opt/wine-devel/lib/wine/x86_64-windows /opt/wine-staging/lib/wine/x86_64-windows; do
    if [ -f "$d/cfgmgr32.dll" ]; then WINELIB=$d; break; fi
done
rm -f "$T/stack.error"
STALE=
if [ -n "$WINELIB" ]; then
    for pair in cfgmgr32:cfgmgw dxva2:dxva2w wtsapi32:wtsapiw mf:mfw mfreadwrite:mfreadww; do
        lib=${pair%:*} copy=${pair#*:}
        [ -f "$SYS/$copy.dll" ] && [ -f "$WINELIB/$lib.dll" ] || continue
        cmp -s "$WINELIB/$lib.dll" "$SYS/$copy.dll" && continue
        STALE="$STALE $pair"
        # (the names as setup.sh reads them; without objdump, or without the stand-in's list, they are not compared)
        if [ ! -f "$T/build/$lib.def" ] || ! command -v x86_64-w64-mingw32-objdump >/dev/null; then continue; fi
        has=$(LC_ALL=C x86_64-w64-mingw32-objdump -p "$WINELIB/$lib.dll" 2>/dev/null |
            awk '/Ordinal\/Name Pointer/{p=1;next} /^$/{if(p)exit} p&&$NF!="Name"{print $NF}' | sort)
        listed=$(awk 'p { print $1 } /^EXPORTS/ { p = 1 }' "$T/build/$lib.def" | sort)
        if [ -n "$has" ] && [ "$has" != "$listed" ]; then
            echo "Wine has been upgraded since the Odyssey 3D set-up was made, and the set-up's stand-in for Wine's $lib.dll no longer fits it. Run the set-up again (any2odyssey-setup, or setup.sh with the Windows drive)." >"$T/stack.error"
            exit 1
        fi
    done
fi
if [ -n "$STALE" ]; then
    # (a program that does nothing: Wine updates the prefix before it runs. The server it started is ended
    # again, with the services that came up under these overrides; the copies stay as they are when the
    # update did not end, and the next start tries again.)
    if WINEDEBUG=-all WINEDLLOVERRIDES="cfgmgr32,dxva2,wtsapi32,mf,mfreadwrite=b;$WINEDLLOVERRIDES" \
        setsid timeout 300 wine cmd /c exit >/dev/null 2>&1 </dev/null; then
        for pair in $STALE; do
            lib=${pair%:*} copy=${pair#*:}
            if ! { cp "$WINELIB/$lib.dll" "$SYS/$copy.dll.new" && mv -f "$SYS/$copy.dll.new" "$SYS/$copy.dll"; }; then
                rm -f "$SYS/$copy.dll.new"
            fi
        done
    fi
    WINEDEBUG=-all wineserver -k 2>/dev/null
    sleep 3
fi
# Wine ends its services three seconds after the last ordinary program of the prefix has left, and a service
# counts as none. Until SRSession.exe runs (the last line here), the only ordinary programs are the short
# ones below, with pauses of 6, 8 and 2 seconds between them: where "wine sc start" came back early enough,
# Wine ended Leia's service, and the USB driver with it, in the middle of this start. The server is started
# here with a minute of patience instead of three seconds.
WINEDEBUG=-all wineserver -p60 >/dev/null 2>&1 </dev/null
# Detached: a wine process keeps its terminal busy otherwise.
(WINEDEBUG=$DEBUG setsid wine sc start a2ousb >"$T/service.trace" 2>&1 </dev/null &)
sleep 6
(WINEDEBUG=$DEBUG setsid wine sc start "SR Service" >>"$T/service.trace" 2>&1 </dev/null &)
sleep 8
# Wine rewrites its monitor entries at every start, so the EDIDs go in now, paired by desktop position.
WINEDEBUG=-all timeout 30 wine "$T/build/monprobe.exe" >"$T/monprobe.txt" 2>/dev/null </dev/null
python3 "$T/tools/mkedid.py" "$T/monprobe.txt" "$T/edid.reg" "$T/odyssey.place" 2>"$T/edid.pairs" || exit 1
# The toast stand-in's three classes go in with them, at every start. An upgrade of Wine updates the prefix
# at the first program run in it, and that registers Wine's own toast library for two of the classes again.
# Wine's has no notifier yet: Leia's session then uses one it was never given and ends within a second of
# its start, at every start. With no session nothing counted as a running stack (tools/ensure-stack.sh), so
# every request for the lens or the picture was a cold start of half a minute, until setup.sh was run again.
# (The entries are the set-up's own, from build/device.reg; edid.reg has Windows line ends.)
[ -f "$T/build/device.reg" ] && [ -f "$T/edid.reg" ] &&
    awk 'BEGIN { printf "\r\n" } { sub(/\r$/, "") } /WindowsRuntime.ActivatableClassId/ { p = 1 } p { printf "%s\r\n", $0 } /^$/ { p = 0 }' \
        "$T/build/device.reg" >>"$T/edid.reg"
WINEDEBUG=-all setsid timeout 60 wine regedit /S "$T/edid.reg" >/dev/null 2>&1 </dev/null
sleep 2
# (exec: without it a copy of this script's shell stays behind for as long as the session runs, and reads
# in the list of processes as a start that never ended)
(cd "$BIN" && export WINEDEBUG="$DEBUG" && exec setsid wine SRSession.exe >"$T/session.out" 2>&1 </dev/null &)
