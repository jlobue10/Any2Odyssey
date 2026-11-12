#!/bin/sh
# SPDX-License-Identifier: MIT
# Keep software flow control off on the lens board's port while Leia's service runs under Wine: Wine turns
# IXON on when a program asks for XON/XOFF, which eats bytes of the board's replies; Windows' USB serial driver
# never acts on those bytes. Changes line settings only; reads and writes nothing. Runs until Leia's service
# has gone or never came, or until killed (tools/ensure-stack.sh ends it before it starts the service again).
T=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
BOARD=/dev/odyssey-lens
# One small program does this when setup.sh could build it (src/noxon.c): it looks at the port itself, where
# the loop below has to start two programs for every look, twenty times a second.
[ -x "$T/build/any2odyssey-noxon" ] && exec "$T/build/any2odyssey-noxon" "$BOARD" SRService.exe
uid=$(id -u)
looks=0 seen='' missed=0
while :; do
    # ("ixon" without a "-" in front of it: it is on)
    case $(stty -F "$BOARD" -a 2>/dev/null) in *[!-]ixon*) stty -F "$BOARD" -ixon -ixoff 2>/dev/null ;; esac
    sleep 0.05
    looks=$((looks + 1))
    [ $((looks % 100)) -eq 0 ] || continue
    # Every hundred looks (five seconds and more): is the service there? Twice not, after it was, and it has
    # gone; so has one that did not come in two dozen (a start takes about half a minute).
    if pgrep -u "$uid" -f 'bin.SRService\.exe' >/dev/null; then
        seen=1 missed=0
    else
        missed=$((missed + 1))
        limit=24
        [ -n "$seen" ] && limit=2
        [ "$missed" -ge "$limit" ] && exit 0
    fi
done
