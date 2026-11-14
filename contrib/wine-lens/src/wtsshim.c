/* SPDX-License-Identifier: MIT
 *
 * wtsapi32.dll shim for Wine: answers WTSQuerySessionInformation(WTSSessionInfoEx), which Wine does not
 * implement. A service that asks "is the console session active and unlocked?" otherwise gets a failure and
 * treats the desktop as locked. The answer given is always: active, unlocked. Everything else goes to Wine's
 * own wtsapi32 (renamed wtsapiw.dll). Memory is taken from the process heap, as Wine's WTSFreeMemory expects.
 */
#include <windows.h>
#include <wtsapi32.h>

/* The extended session information under names of this file's own: the wtsapi32.h of mingw-w64 11 (Ubuntu
 * 24.04) declares none of WTSSessionInfoEx, WTSINFOEXA/W and WTS_SESSIONSTATE_UNLOCK, and the stand-in did
 * not compile there, which stopped the whole set-up. The values and the layout are the Windows SDK's; the
 * two structures differ in their character type only. */
#define A2O_SESSION_INFO_EX 25      /* WTSSessionInfoEx */
#define A2O_SESSIONSTATE_UNLOCK 1   /* WTS_SESSIONSTATE_UNLOCK */

typedef struct {
    DWORD Level;
    union {
        struct {
            ULONG SessionId;
            WTS_CONNECTSTATE_CLASS SessionState;
            LONG SessionFlags;
            char WinStationName[WINSTATIONNAME_LENGTH + 1];
            char UserName[USERNAME_LENGTH + 1];
            char DomainName[DOMAIN_LENGTH + 1];
            LARGE_INTEGER LogonTime, ConnectTime, DisconnectTime, LastInputTime, CurrentTime;
            DWORD IncomingBytes, OutgoingBytes, IncomingFrames, OutgoingFrames;
            DWORD IncomingCompressedBytes, OutgoingCompressedBytes;
        } WTSInfoExLevel1;
    } Data;
} A2O_WTSINFOEXA;

typedef struct {
    DWORD Level;
    union {
        struct {
            ULONG SessionId;
            WTS_CONNECTSTATE_CLASS SessionState;
            LONG SessionFlags;
            WCHAR WinStationName[WINSTATIONNAME_LENGTH + 1];
            WCHAR UserName[USERNAME_LENGTH + 1];
            WCHAR DomainName[DOMAIN_LENGTH + 1];
            LARGE_INTEGER LogonTime, ConnectTime, DisconnectTime, LastInputTime, CurrentTime;
            DWORD IncomingBytes, OutgoingBytes, IncomingFrames, OutgoingFrames;
            DWORD IncomingCompressedBytes, OutgoingCompressedBytes;
        } WTSInfoExLevel1;
    } Data;
} A2O_WTSINFOEXW;

/* With headers that do declare the SDK's structures (mingw-w64 14 does), -DA2O_CHECK_SDK_LAYOUT checks
 * that these are the same; the set-up does not pass it, so no header version can fail the build over it. */
#ifdef A2O_CHECK_SDK_LAYOUT
_Static_assert(sizeof(A2O_WTSINFOEXA) == sizeof(WTSINFOEXA), "WTSINFOEXA size");
_Static_assert(sizeof(A2O_WTSINFOEXW) == sizeof(WTSINFOEXW), "WTSINFOEXW size");
_Static_assert(__builtin_offsetof(A2O_WTSINFOEXA, Data.WTSInfoExLevel1.UserName) == __builtin_offsetof(WTSINFOEXA, Data.WTSInfoExLevel1.UserName), "WTSINFOEXA UserName");
_Static_assert(__builtin_offsetof(A2O_WTSINFOEXA, Data.WTSInfoExLevel1.LogonTime) == __builtin_offsetof(WTSINFOEXA, Data.WTSInfoExLevel1.LogonTime), "WTSINFOEXA LogonTime");
_Static_assert(__builtin_offsetof(A2O_WTSINFOEXW, Data.WTSInfoExLevel1.UserName) == __builtin_offsetof(WTSINFOEXW, Data.WTSInfoExLevel1.UserName), "WTSINFOEXW UserName");
_Static_assert(__builtin_offsetof(A2O_WTSINFOEXW, Data.WTSInfoExLevel1.LogonTime) == __builtin_offsetof(WTSINFOEXW, Data.WTSInfoExLevel1.LogonTime), "WTSINFOEXW LogonTime");
_Static_assert(__builtin_offsetof(A2O_WTSINFOEXW, Data.WTSInfoExLevel1.CurrentTime) == __builtin_offsetof(WTSINFOEXW, Data.WTSInfoExLevel1.CurrentTime), "WTSINFOEXW CurrentTime");
_Static_assert(A2O_SESSION_INFO_EX == WTSSessionInfoEx, "WTSSessionInfoEx value");
_Static_assert(A2O_SESSIONSTATE_UNLOCK == WTS_SESSIONSTATE_UNLOCK, "WTS_SESSIONSTATE_UNLOCK value");
#endif

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
    A2O_WTSINFOEXA *info;
    DWORD size = 21;

    if ((int)class != A2O_SESSION_INFO_EX) return f(server, session, class, buffer, count);
    if (!buffer || !count || !(info = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *info))) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    info->Level = 1;
    info->Data.WTSInfoExLevel1.SessionId = session == WTS_CURRENT_SESSION ? WTSGetActiveConsoleSessionId() : session;
    info->Data.WTSInfoExLevel1.SessionState = WTSActive;
    info->Data.WTSInfoExLevel1.SessionFlags = A2O_SESSIONSTATE_UNLOCK;
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
    A2O_WTSINFOEXW *info;
    DWORD size = 21;

    if ((int)class != A2O_SESSION_INFO_EX) return f(server, session, class, buffer, count);
    if (!buffer || !count || !(info = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *info))) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    info->Level = 1;
    info->Data.WTSInfoExLevel1.SessionId = session == WTS_CURRENT_SESSION ? WTSGetActiveConsoleSessionId() : session;
    info->Data.WTSInfoExLevel1.SessionState = WTSActive;
    info->Data.WTSInfoExLevel1.SessionFlags = A2O_SESSIONSTATE_UNLOCK;
    lstrcpyW(info->Data.WTSInfoExLevel1.WinStationName, L"Console");
    GetUserNameW(info->Data.WTSInfoExLevel1.UserName, &size);
    fill_times(&info->Data.WTSInfoExLevel1.LogonTime, &info->Data.WTSInfoExLevel1.ConnectTime,
               &info->Data.WTSInfoExLevel1.LastInputTime, &info->Data.WTSInfoExLevel1.CurrentTime);
    *buffer = (WCHAR *)info;
    *count = sizeof *info;
    return TRUE;
}
