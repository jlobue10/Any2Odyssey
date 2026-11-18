#!/bin/sh
# SPDX-License-Identifier: MIT
# Make sure Leia's SR service and session are running in the Wine prefix and have the lens board ready.
# Prints nothing when they are; prints "error: <reason>" and fails otherwise. Used by any2odyssey-lens-helper
# and any2odyssey-weaver; the first start after a login or a monitor standby takes about half a minute.
T=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
export WINEPREFIX=$T/prefix WINEDLLOVERRIDES="mscoree,mshtml="
LOGDIR="$WINEPREFIX/drive_c/ProgramData/Simulated Reality/Server/Log"
BOARD=/dev/odyssey-lens
STAMP=$T/stack.started

[ -d "$WINEPREFIX" ] || { echo "error: the Wine prefix is missing; run contrib/wine-lens/setup.sh"; exit 1; }
[ -e "$BOARD" ] || { echo "error: the lens board is not present (is the Odyssey 3D awake?)"; exit 1; }

# Whether Wine's server of this prefix is among this user's processes. A server that was killed outright
# (or crashed) leaves the prefix's programs behind: they stay in the list of processes, Leia's service and
# session among them, and can do nothing any more. Found by name alone they passed for a running stack, at
# every request, until the next login. (Some distributions call the server wineserver64.)
server_runs() {
    for pid in $(pgrep -u "$(id -u)" wineserver 2>/dev/null); do
        tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null | grep -q -x -F "WINEPREFIX=$WINEPREFIX" && return 0
    done
    return 1
}

stack_ok() {
    pgrep -u "$(id -u)" -f 'bin.SRService\.exe' >/dev/null && pgrep -u "$(id -u)" -f 'SRSession\.exe' >/dev/null &&
        [ -e "$STAMP" ] && [ "$(stat -c %Y "$BOARD")" -le "$(stat -c %Y "$STAMP")" ] && server_runs
    # (the link itself, not the tty behind it: udev makes it anew when the board returns after a monitor sleep)
}

# One caller at a time may (re)start the service; the others wait for it.
exec 9>"$T/stack.lock"
flock 9
if ! stack_ok; then
    # The serial port's keeper from an earlier start is ended, if the process of the noted number still is
    # that keeper (a number left over from before a restart can belong to anything). It is the script or the
    # program the script becomes; left alone it ends by itself some seconds after Leia's service has gone.
    NOXON=${XDG_RUNTIME_DIR:-$T}/any2odyssey-noxon.pid
    old=$(cat "$NOXON" 2>/dev/null)
    case $old in '' | *[!0-9]*) ;; *) grep -q -a -e 'noxon\.sh' -e 'any2odyssey-noxon' "/proc/$old/cmdline" 2>/dev/null && kill "$old" 2>/dev/null ;; esac
    setsid "$T/tools/noxon.sh" </dev/null >/dev/null 2>&1 9>&- &
    echo $! >"$NOXON"
    "$T/tools/start-stack.sh" >/dev/null 2>&1 9>&- ||
        { echo "error: could not start Leia's service (see $T/edid.pairs)"; exit 1; }
    i=0
    until grep -a -q 'responded to initial disable' "$(ls -t "$LOGDIR"/*.txt 2>/dev/null | head -1)" 2>/dev/null; do
        i=$((i + 1))
        [ $i -gt 60 ] && { echo "error: Leia's service did not get the lens board ready (log in $LOGDIR)"; exit 1; }
        sleep 1
    done
    touch "$STAMP"
fi
exit 0
