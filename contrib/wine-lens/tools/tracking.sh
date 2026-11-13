#!/bin/sh
# SPDX-License-Identifier: MIT
# Start or stop eye tracking in the Wine prefix: the native camera bridge (the monitor's webcam, handed to
# Wine over a loopback port) and Leia's eye tracker, which then feeds Leia's service like it does on Windows.
#
# usage: tracking.sh start | stop | status
#
# "start" needs Leia's service and session running already (any2odyssey-lens-helper starts them), and prints
# "on" or "error: <reason>". Stop it when no program needs eye positions: under Wine the tracker does not
# pause by itself, so it keeps the camera streaming and one processor core busy for as long as it runs.
T=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
export WINEPREFIX=$T/prefix WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all
BRIDGE=$T/build/any2odyssey-camera-bridge
CAMERA=04e8:20d7
# The bridge's process id is noted where it does not outlive the session, and is believed only while the
# process of that number is the bridge: a number left over from before can belong to anything.
RUN=${XDG_RUNTIME_DIR:-$T}
PIDFILE=$RUN/any2odyssey-camera-bridge.pid
# Locked for as long as a request to start the tracker is on its way to Leia's service, whatever becomes of
# the "start" that made it: "stop" waits for that request before it decides what there is to stop.
STARTING=$RUN/any2odyssey-tracker.starting

bridge_pid() {
    pid=$(cat "$PIDFILE" 2>/dev/null) || return 1
    case $pid in '' | *[!0-9]*) return 1 ;; esac
    [ "$(readlink -f "/proc/$pid/exe" 2>/dev/null)" = "$(readlink -f "$BRIDGE")" ] && echo "$pid"
}
bridge_running() { bridge_pid >/dev/null; }
tracker_running() { pgrep -u "$(id -u)" -f '[S]REyeTracker\.exe' >/dev/null; }

camera_dir() {
    for d in /sys/bus/usb/devices/*; do
        [ -f "$d/idVendor" ] && [ "$(cat "$d/idVendor"):$(cat "$d/idProduct")" = "$CAMERA" ] && { echo "$d"; return 0; }
    done
    return 1
}

case ${1:-} in
start)
    [ -x "$BRIDGE" ] || { echo "error: eye tracking was not set up (run contrib/wine-lens/setup.sh)"; exit 1; }
    pgrep -u "$(id -u)" -f 'bin.SRService\.exe' >/dev/null && pgrep -u "$(id -u)" -f 'SRSession\.exe' >/dev/null ||
        { echo "error: Leia's service is not running (start any2odyssey-lens-helper first)"; exit 1; }
    CAM=$(camera_dir) || { echo "error: the Odyssey 3D's webcam is not present (is the monitor awake?)"; exit 1; }
    [ "$(cat "$CAM/power/control" 2>/dev/null)" = on ] ||
        { echo "error: USB power saving is on for the webcam; install 70-any2odyssey-lens.rules and power-cycle the monitor"; exit 1; }
    if ! bridge_running; then
        setsid "$BRIDGE" "$CAMERA" "$(cat "$T/camera.port")" "$T/camera.secret" </dev/null >/dev/null 2>&1 &
        echo $! >"$PIDFILE"
        # It leaves at once when it cannot have its port (something else has it): say so instead of
        # letting the tracker talk to whatever that is.
        sleep 0.3
        bridge_running || { rm -f "$PIDFILE"; echo "error: the camera bridge could not start (is port $(cat "$T/camera.port") taken?)"; exit 1; }
    fi
    if ! tracker_running; then
        # The request outlives this script when Any2Odyssey ends it (a Stop right after a Start), and the
        # tracker it starts would then stay with nobody to stop it. So it holds a lock (file descriptor 9,
        # kept by the shell around it and not handed to Wine, whose programs stay) that "stop" looks at.
        {
            flock 9
            (setsid sh -c 'timeout 25 wine sc start "SR Eye Tracker" 9>&-; :' >/dev/null 2>&1 </dev/null &)
        } 9>"$STARTING"
        i=0
        until tracker_running; do
            i=$((i + 1))
            [ $i -gt 20 ] && { echo "error: Leia's eye tracker did not start"; exit 1; }
            sleep 1
        done
    fi
    echo on
    ;;
stop)
    # A request to start the tracker may still be on its way (see "start"): it is waited for, for as long
    # as it can take, and the tracker it was for is then stopped whether or not its process shows yet.
    # Only looked at, never waited on: a lock that cannot be had for another reason holds nothing up.
    asked=
    i=0
    while [ -e "$STARTING" ] && [ $i -lt 150 ]; do
        flock -n "$STARTING" true 2>/dev/null
        [ $? -eq 1 ] || break
        asked=1
        i=$((i + 1))
        sleep 0.2
    done
    if [ -n "$asked" ] || tracker_running; then
        setsid timeout 30 wine sc stop "SR Eye Tracker" >/dev/null 2>&1 </dev/null
        # Leia's service may answer before the tracker has gone. A "start" that came now would find the
        # tracker, start none and be left without one, so this waits (about ten seconds at most) until it
        # has gone.
        i=0
        while [ $i -lt 50 ] && tracker_running; do
            i=$((i + 1))
            sleep 0.2
        done
    fi
    pid=$(bridge_pid) && kill "$pid" 2>/dev/null
    rm -f "$PIDFILE"
    ;;
status)
    tracker_running && echo "tracker: running" || echo "tracker: stopped"
    bridge_running && echo "camera bridge: running" || echo "camera bridge: stopped"
    CAM=$(camera_dir) && echo "webcam: present, USB power saving $(cat "$CAM/power/control" | sed 's/^on$/off/; s/^auto$/ON (must be off)/')" ||
        echo "webcam: not present"
    ;;
*)
    echo "usage: tracking.sh start | stop | status" >&2
    exit 2
    ;;
esac
