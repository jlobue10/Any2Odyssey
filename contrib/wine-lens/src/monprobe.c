/* SPDX-License-Identifier: MIT
 * monprobe: print, for each active monitor, its position and size, the adapter it hangs off and the monitor
 * device id Wine reports for it. Used to learn which Enum\DISPLAY\...\000N&0000 slot is which physical monitor. */
#include <windows.h>
#include <stdio.h>
static BOOL CALLBACK each(HMONITOR m, HDC dc, RECT *r, LPARAM p)
{
    MONITORINFOEXA mi;
    DISPLAY_DEVICEA md;
    mi.cbSize = sizeof mi;
    md.cb = sizeof md;
    md.DeviceID[0] = 0;
    /* Wine can list a monitor it has lost track of, with a null handle: there is nothing to print for it, and
     * what was printed came from memory nobody had filled in. */
    if (!GetMonitorInfoA(m, (MONITORINFO *)&mi)) return TRUE;
    EnumDisplayDevicesA(mi.szDevice, 0, &md, 0);
    printf("%p %s %ldx%ld at %ld,%ld id=[%s]\n", (void *)m, mi.szDevice, mi.rcMonitor.right - mi.rcMonitor.left,
           mi.rcMonitor.bottom - mi.rcMonitor.top, mi.rcMonitor.left, mi.rcMonitor.top, md.DeviceID);
    return TRUE;
}
int main(void) { EnumDisplayMonitors(NULL, NULL, each, 0); return 0; }
