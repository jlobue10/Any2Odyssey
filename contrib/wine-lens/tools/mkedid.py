#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write the .reg file that gives Wine's monitor entries their real EDIDs.

Wine (under XWayland) leaves every monitor as "Default_Monitor" with no EDID. This pairs each Wine monitor
slot with the physical monitor at the same place on the desktop (monprobe.exe output against xrandr), then
writes, per slot: the EDID on the Default_Monitor entry, a DISPLAY\\<MODEL>\\000N&0000 entry with a monitor
interface, and Linked=0 on the Default_Monitor interface.

usage: mkedid.py <monprobe output file> <out.reg>
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
    path = glob.glob(f"/sys/class/drm/card*-{connector}/edid")[0]
    return open(path, "rb").read()[:256]

def model_of(edid):
    v = (edid[8] << 8) | edid[9]
    pnp = "".join(chr(64 + ((v >> s) & 31)) for s in (10, 5, 0))
    return "%s%02X%02X" % (pnp, edid[11], edid[10])

def hexlist(b): return ",".join("%02x" % c for c in b)

def main(probe, out):
    real = xrandr_monitors()
    pairs = []
    for slot, w, h, x, y in wine_monitors(probe):
        match = [r for r in real if r[1:] == (w, h, x, y)]
        if len(match) != 1: sys.exit(f"slot {slot}: no single monitor at {w}x{h}+{x}+{y}")
        pairs.append((slot, match[0][0], edid_of(match[0][0])))
    # Wine sometimes reports the primary monitor without a usable name; one slot left over can only be it.
    left = [r[0] for r in real if r[0] not in [p[1] for p in pairs]]
    free = [s for s in range(len(real)) if s not in [p[0] for p in pairs]]
    if len(left) == 1 and len(free) == 1: pairs.append((free[0], left[0], edid_of(left[0])))
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

main(*sys.argv[1:3])
