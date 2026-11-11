/* SPDX-License-Identifier: MIT
 *
 * wtsapi32.dll shim for Wine: answers WTSQuerySessionInformation(WTSSessionInfoEx), which Wine does not
 * implement. A service that asks "is the console session active and unlocked?" otherwise gets a failure and
 * treats the desktop as locked. The answer given is always: active, unlocked. Everything else goes to Wine's
 * own wtsapi32 (renamed wtsapiw.dll). Memory is taken from the process heap, as Wine's WTSFreeMemory expects.
 */
#include <windows.h>
#include <wtsapi32.h>

static HMODULE real;
static void *sym(const char *name)
{
    if (!real) real = LoadLibraryW(L"wtsapiw.dll");
    return real ? (void *)GetProcAddress(real, name) : NULL;
}

static void fill_times(LARGE_INTEGER *logon, LARGE_INTEGER *connect, LARGE_INTEGER *input, LARGE_INTEGER *now)
{
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    now->LowPart = ft.dwLowDateTime;
    now->HighPart = ft.dwHighDateTime;
    *input = *now;
    logon->QuadPart = connect->QuadPart = now->QuadPart - (LONGLONG)GetTickCount64() * 10000;
}

BOOL WINAPI shim_WTSQuerySessionInformationA(HANDLE server, DWORD session, WTS_INFO_CLASS class, char **buffer, DWORD *count)
{
    BOOL (WINAPI *f)(HANDLE, DWORD, WTS_INFO_CLASS, char **, DWORD *) = sym("WTSQuerySessionInformationA");
    WTSINFOEXA *info;
    DWORD size = 21;

    if (class != WTSSessionInfoEx) return f(server, session, class, buffer, count);
    if (!buffer || !count || !(info = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *info))) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    info->Level = 1;
    info->Data.WTSInfoExLevel1.SessionId = session == WTS_CURRENT_SESSION ? WTSGetActiveConsoleSessionId() : session;
    info->Data.WTSInfoExLevel1.SessionState = WTSActive;
    info->Data.WTSInfoExLevel1.SessionFlags = WTS_SESSIONSTATE_UNLOCK;
    lstrcpyA(info->Data.WTSInfoExLevel1.WinStationName, "Console");
    GetUserNameA(info->Data.WTSInfoExLevel1.UserName, &size);
    fill_times(&info->Data.WTSInfoExLevel1.LogonTime, &info->Data.WTSInfoExLevel1.ConnectTime,
               &info->Data.WTSInfoExLevel1.LastInputTime, &info->Data.WTSInfoExLevel1.CurrentTime);
    *buffer = (char *)info;
    *count = sizeof *info;
    return TRUE;
}

BOOL WINAPI shim_WTSQuerySessionInformationW(HANDLE server, DWORD session, WTS_INFO_CLASS class, WCHAR **buffer, DWORD *count)
{
    BOOL (WINAPI *f)(HANDLE, DWORD, WTS_INFO_CLASS, WCHAR **, DWORD *) = sym("WTSQuerySessionInformationW");
    WTSINFOEXW *info;
    DWORD size = 21;

    if (class != WTSSessionInfoEx) return f(server, session, class, buffer, count);
    if (!buffer || !count || !(info = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *info))) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    info->Level = 1;
    info->Data.WTSInfoExLevel1.SessionId = session == WTS_CURRENT_SESSION ? WTSGetActiveConsoleSessionId() : session;
    info->Data.WTSInfoExLevel1.SessionState = WTSActive;
    info->Data.WTSInfoExLevel1.SessionFlags = WTS_SESSIONSTATE_UNLOCK;
    lstrcpyW(info->Data.WTSInfoExLevel1.WinStationName, L"Console");
    GetUserNameW(info->Data.WTSInfoExLevel1.UserName, &size);
    fill_times(&info->Data.WTSInfoExLevel1.LogonTime, &info->Data.WTSInfoExLevel1.ConnectTime,
               &info->Data.WTSInfoExLevel1.LastInputTime, &info->Data.WTSInfoExLevel1.CurrentTime);
    *buffer = (WCHAR *)info;
    *count = sizeof *info;
    return TRUE;
}
