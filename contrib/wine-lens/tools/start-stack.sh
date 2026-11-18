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
WINEDEBUG=-all setsid timeout 60 wine regedit /S "$T/edid.reg" >/dev/null 2>&1 </dev/null
sleep 2
# (exec: without it a copy of this script's shell stays behind for as long as the session runs, and reads
# in the list of processes as a start that never ended)
(cd "$BIN" && export WINEDEBUG="$DEBUG" && exec setsid wine SRSession.exe >"$T/session.out" 2>&1 </dev/null &)
