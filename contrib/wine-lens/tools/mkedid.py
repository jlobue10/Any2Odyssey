#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write the .reg file that gives Wine's monitor entries their real EDIDs.

Wine (under XWayland) leaves every monitor as "Default_Monitor" with no EDID. This pairs each Wine monitor
slot with the physical monitor at the same place on the desktop (monprobe.exe output against xrandr), then
writes, per slot: the EDID on the Default_Monitor entry, a DISPLAY\\<MODEL>\\000N&0000 entry with a monitor
interface, and Linked=0 on the Default_Monitor interface.

It also notes where the Odyssey 3D is on Wine's desktop ("x y width height" in <odyssey file>, empty when it
is not connected), for the program that shows pictures through Leia's weaver.

usage: mkedid.py <monprobe output file> <out.reg> [<odyssey file>]
       mkedid.py --place <odyssey file>      only note where the Odyssey 3D is now
"""
import glob, re, subprocess, sys

IFACE = "{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
ENUM = r"HKEY_LOCAL_MACHINE\System\CurrentControlSet\Enum\DISPLAY"
CLASSES = r"HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\DeviceClasses" + "\\" + IFACE

def xrandr_monitors():
    out = subprocess.run(["xrandr", "--listmonitors"], capture_output=True, text=True).stdout
    mons, origin = [], (0, 0)
    for m in re.finditer(r"\+(\*?)(\S+) (\d+)/\d+x(\d+)/\d+\+(\d+)\+(\d+)", out):
        primary, name, w, h, x, y = m.group(1), m.group(2), *map(int, m.groups()[2:])
        mons.append((name, w, h, x, y))
        if primary: origin = (x, y)
    return [(n, w, h, x - origin[0], y - origin[1]) for n, w, h, x, y in mons]

def wine_monitors(path):
    res = []
    for m in re.finditer(r"DISPLAY(\d+) (\d+)x(\d+) at (-?\d+),(-?\d+)", open(path).read()):
        n, w, h, x, y = map(int, m.groups())
        res.append((n - 1, w, h, x, y))
    return res

def edid_of(connector):
    # Two graphics cards can both have a connector of this name; the monitor is on the one with an EDID.
    for path in sorted(glob.glob(f"/sys/class/drm/card*-{connector}/edid")):
        edid = open(path, "rb").read()[:256]
        if len(edid) >= 128: return edid
    return bytes(128)

def model_of(edid):
    v = (edid[8] << 8) | edid[9]
    # Three letters of five bits each, 1 to 26 for A to Z. The model goes into registry key names and
    # strings: the values past Z, which a malformed EDID can hold, would put "[", "\" or "]" there.
    pnp = "".join(chr(64 + n) if n <= 26 else "_" for n in ((v >> s) & 31 for s in (10, 5, 0)))
    return "%s%02X%02X" % (pnp, edid[11], edid[10])

def name_of(edid):
    """The monitor's name from its EDID (the 0xFC descriptor)."""
    for i in range(54, 126, 18):
        if edid[i:i + 3] == b"\0\0\0" and edid[i + 3] == 0xFC:
            return edid[i + 5:i + 18].split(b"\n")[0].decode("ascii", "replace").strip()
    return ""

def is_odyssey_3d(edid): return "G90XF" in name_of(edid)

def hexlist(b): return ",".join("%02x" % c for c in b)

def note_place(real, odyssey):
    places = ["%d %d %d %d" % (x, y, w, h) for name, w, h, x, y in real if is_odyssey_3d(edid_of(name))]
    open(odyssey, "w").write(places[0] + "\n" if len(places) == 1 else "")

def preference(monitor):
    """Among mirrored monitors, which are at the same place: the Odyssey 3D, then one with an EDID, then by name."""
    edid = edid_of(monitor[0])
    return (not is_odyssey_3d(edid), not any(edid), monitor[0])

def main(probe, out, odyssey=None):
    real = xrandr_monitors()
    if odyssey: note_place(real, odyssey)
    pairs = []
    for slot, w, h, x, y in wine_monitors(probe):
        match = [r for r in real if r[1:] == (w, h, x, y) and r[0] not in [p[1] for p in pairs]]
        if not match: sys.exit(f"slot {slot}: no monitor at {w}x{h}+{x}+{y}")
        name = min(match, key=preference)[0]
        pairs.append((slot, name, edid_of(name)))
    # Wine sometimes reports the primary monitor without a usable name; one slot left over can only be it.
    # Mirrored monitors share a place and, when Wine shows them as one, a slot: the others need none.
    placed = {r[1:] for r in real if r[0] in [p[1] for p in pairs]}
    left = [r for r in real if r[1:] not in placed]
    free = [s for s in range(len({r[1:] for r in real})) if s not in [p[0] for p in pairs]]
    if len({r[1:] for r in left}) == 1 and len(free) == 1:
        name = min(left, key=preference)[0]
        pairs.append((free[0], name, edid_of(name)))
    pairs.sort()
    models = {model_of(e) for _, _, e in pairs}
    lines = ["REGEDIT4", ""]
    for slot in range(len(real)):                      # forget pairings from an earlier layout
        for model in sorted(models):
            inst = "%04d&0000" % slot
            lines += [f"[-{ENUM}\\{model}\\{inst}]", f"[-{CLASSES}\\##?#DISPLAY#{model}#{inst}#{IFACE}]", ""]
    for slot, name, edid in pairs:
        model, inst = model_of(edid), "%04d&0000" % slot
        link = f"##?#DISPLAY#{model}#{inst}#{IFACE}"
        hwid = hexlist(("MONITOR\\" + model).encode() + b"\0\0")
        lines += [
            f"[{ENUM}\\Default_Monitor\\{inst}\\Device Parameters]", '"BAD_EDID"=-', f'"EDID"=hex:{hexlist(edid)}', "",
            f"[{CLASSES}\\##?#DISPLAY#Default_Monitor#{inst}#{IFACE}\\#\\Control]", '"Linked"=dword:00000000', "",
            f"[{ENUM}\\{model}\\{inst}]", '"Class"="Monitor"',
            # lowercase on purpose: Wine's own monitor scan matches the uppercase GUID and must not pick these up
            '"ClassGUID"="{4d36e96e-e325-11ce-bfc1-08002be10318}"', '"DeviceDesc"="Generic PnP Monitor"',
            '"Driver"="{4D36E96E-E325-11CE-BFC1-08002BE10318}\\\\%04d"' % (10 + slot), f'"HardwareID"=hex(7):{hwid}', "",
            f"[{ENUM}\\{model}\\{inst}\\Device Parameters]", f'"EDID"=hex:{hexlist(edid)}', "",
            f"[{CLASSES}\\{link}]", f'"DeviceInstance"="DISPLAY\\\\{model}\\\\{inst}"', "",
            f"[{CLASSES}\\{link}\\#]", f'"SymbolicLink"="\\\\\\\\?\\\\DISPLAY#{model}#{inst}#{IFACE}"', "",
            f"[{CLASSES}\\{link}\\#\\Control]", '"Linked"=dword:00000001', "",
        ]
        print(f"slot {slot}: {name} {model}", file=sys.stderr)
    open(out, "w").write("\r\n".join(lines))

if sys.argv[1:2] == ["--place"]: note_place(xrandr_monitors(), sys.argv[2])
else: main(*sys.argv[1:4])
