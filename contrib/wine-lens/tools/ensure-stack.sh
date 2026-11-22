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
# Nor does a server count whose own program file has been replaced or removed since it started: that is what
# an upgrade of Wine leaves of a stack that was up at the time (a package manager puts the new file in place
# of the old one, and the kernel then names the running one "... (deleted)"). The stack went on running, but
# every program started after the upgrade is the new Wine's and is turned away by the old server ("version
# mismatch"): the lens helper ended without a word and eye tracking did not start, at every request, until
# the next login. Such a stack is started again, under the Wine that is installed now.
server_runs() {
    for pid in $(pgrep -u "$(id -u)" wineserver 2>/dev/null); do
        tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null | grep -q -x -F "WINEPREFIX=$WINEPREFIX" || continue
        case $(readlink "/proc/$pid/exe" 2>/dev/null) in *' (deleted)') continue ;; esac
        return 0
    done
    return 1
}

# The note of a start that ended well holds the time of the board's link as it was then, and the stack has
# the board for as long as the link still has that time. (The link itself, not the tty behind it: udev makes
# it anew when the board returns after a monitor sleep.) The note used to be an empty file whose own time was
# compared with the link's, and the stack counted while the link was not the newer of the two. That takes a
# clock that only goes forward. Where the clock is set back after udev made the link (a PC that also runs
# Windows, which keeps local time in the clock chip: Linux starts hours ahead east of Greenwich and is
# corrected from the network a little later), the link was newer than every note until the clock had caught
# up with it, and each request in those hours started the stack again, under whatever was using it.
board_time() { stat -c %Y "$BOARD" 2>/dev/null; }

stack_ok() {
    pgrep -u "$(id -u)" -f 'bin.SRService\.exe' >/dev/null && pgrep -u "$(id -u)" -f 'SRSession\.exe' >/dev/null &&
        [ -s "$STAMP" ] && [ "$(board_time)" = "$(cat "$STAMP" 2>/dev/null)" ] && server_runs
}

# Whether Leia's service has said, since this start began, that the lens board answered it. The line used to
# be looked for in the whole of the newest log file: where the service goes on writing into the file of the
# run before, the line of that run was found at once, and the start counted as ready before the board was.
# So every log file's checksum and size are noted before the start (in $LOGS, whose own time marks the
# start), and the line counts only in a file written since, and there only in what is new: all of a file
# that was not there or does not begin as it did (the service wrote it anew), and what was added to one
# that does.
LOGS=$T/stack.logs
note_logs() { cksum "$LOGDIR"/*.txt >"$LOGS" 2>/dev/null; }
board_ready() {
    find "$LOGDIR"/*.txt -newer "$LOGS" 2>/dev/null | (
        while IFS= read -r log; do
            had=$(LOG=$log awk '{ name = substr($0, length($1) + length($2) + 3) }
                                name == ENVIRON["LOG"] { print $1, $2; exit }' "$LOGS")
            from=0
            if [ -n "$had" ]; then
                size=${had#* }
                [ "$(head -c "$size" "$log" 2>/dev/null | cksum | awk '{ print $1, $2 }')" = "$had" ] && from=$size
            fi
            tail -c "+$((from + 1))" "$log" 2>/dev/null | grep -a -q 'responded to initial disable' && exit 0
        done
        exit 1
    )
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
    # The note of an earlier start goes before this one begins. It used to stay until this start had ended
    # well: a start that failed, or a caller ended while it waited for the board below, left Leia's programs
    # and Wine's server up with the old note, and the next request took that for a stack that had the board.
    rm -f "$STAMP"
    # Leia's programs begin a log file at every run and remove none. On Windows that is Leia's to look
    # after; here nothing did, and the eye tracker alone writes about 10 MB for every hour it follows a
    # face (880 kB in the first five minutes of a film, counted on the Odyssey). So the files that no
    # program has written to for two weeks go, now that Leia's programs are about to be started and none
    # of them has such a file open: the service's, the tracker's, and those of the programs that asked
    # the service for something. Nothing else in those folders is touched.
    # (find is started inside Leia's folder, so that the pattern is matched against the path below it.
    # Matched against the whole path, a folder named Log anywhere above the prefix made every .txt file
    # in Leia's folder a log. A folder that is a link to another place is left alone, as it was.)
    LEIA="$WINEPREFIX/drive_c/ProgramData/Simulated Reality"
    [ -L "$LEIA" ] || (cd "$LEIA" 2>/dev/null &&
        find . -path '*/Log/*.txt' -type f -mtime +14 -exec rm -f -- {} + 2>/dev/null)
    note_logs
    "$T/tools/start-stack.sh" >/dev/null 2>&1 9>&- || {
        # (start-stack.sh leaves its reason there when it has one of its own)
        if [ -s "$T/stack.error" ]; then
            echo "error: $(cat "$T/stack.error")"
        else
            echo "error: could not start Leia's service (see $T/edid.pairs)"
        fi
        exit 1
    }
    i=0
    until board_ready; do
        i=$((i + 1))
        [ $i -gt 60 ] && { echo "error: Leia's service did not get the lens board ready (log in $LOGDIR)"; exit 1; }
        sleep 1
    done
    # Leia's session is the last thing the start launches, and Wine 11.21 and 11.22 start it through a
    # program of their own: the process that was started as "wine SRSession.exe" becomes "start.exe /exec",
    # and the session comes a moment later as a process of its own, named by its Windows path. In between,
    # no process has the session's name. Where the board had answered by then, this request left in that
    # moment, and a second request that had been waiting at the lock above (the lens helper and the weaver
    # ask at the same time) looked for the session, did not find it and started the stack again, under the
    # first request's program: of eight such pairs under real Wine 11.22 six started the stack twice, and
    # in five the first helper never said "on". So the session itself is waited for here: a process whose
    # command line begins with the session's name ("SRSession.exe" up to Wine 11.17) or with a Windows path
    # to it, which "wine SRSession.exe" does not. Three seconds at the most: a session that does not come,
    # or has ended at once, is what the next request finds, as before.
    i=0
    until pgrep -u "$(id -u)" -f '^([A-Za-z]:.*)?SRSession\.exe' >/dev/null || [ $i -ge 30 ]; do
        i=$((i + 1))
        sleep 0.1
    done
    board_time >"$STAMP"
fi
exit 0
