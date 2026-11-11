/* SPDX-License-Identifier: MIT
 *
 * dxva2.dll shim for Wine: answers the "physical monitor" questions that Wine leaves as failing stubs.
 * Each display handle is reported as exactly one physical monitor. No DDC/CI traffic is produced; the
 * monitor-control (VCP) functions still go to Wine's own dxva2 (renamed dxva2w.dll), which declines them.
 */
#include <windows.h>

typedef struct {
    HANDLE hPhysicalMonitor;
    WCHAR szPhysicalMonitorDescription[128];
} PHYSICAL_MONITOR_;

BOOL WINAPI shim_GetNumberOfPhysicalMonitorsFromHMONITOR(HMONITOR monitor, DWORD *count)
{
    MONITORINFO mi = {sizeof mi};
    if (!count || !GetMonitorInfoW(monitor, &mi)) {
        SetLastError(ERROR_INVALID_HANDLE);
        return FALSE;
    }
    *count = 1;
    return TRUE;
}

BOOL WINAPI shim_GetPhysicalMonitorsFromHMONITOR(HMONITOR monitor, DWORD size, PHYSICAL_MONITOR_ *out)
{
    if (!out || !size) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    out[0].hPhysicalMonitor = (HANDLE)((ULONG_PTR)monitor | 0xa2000000);
    lstrcpyW(out[0].szPhysicalMonitorDescription, L"Generic PnP Monitor");
    return TRUE;
}

BOOL WINAPI shim_DestroyPhysicalMonitor(HANDLE monitor)
{
    return TRUE;
}

BOOL WINAPI shim_DestroyPhysicalMonitors(DWORD count, PHYSICAL_MONITOR_ *monitors)
{
    return TRUE;
}
