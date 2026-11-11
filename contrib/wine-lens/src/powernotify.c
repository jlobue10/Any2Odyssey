/* SPDX-License-Identifier: MIT
 *
 * Monitor-power notification for Windows services under Wine.
 *
 * A service may call RegisterPowerSettingNotification(status handle, GUID_MONITOR_POWER_ON,
 * DEVICE_NOTIFY_SERVICE_HANDLE). Windows answers at once with a SERVICE_CONTROL_POWEREVENT /
 * PBT_POWERSETTINGCHANGE control carrying the current state (1 = monitor on). Wine's function is an empty
 * stub, so such a service waits for ever for a monitor it believes is off.
 *
 * This replaces the stub (the shim is already loaded in the process) and delivers that one "monitor on"
 * control to the service's handler, which it learns by watching RegisterServiceCtrlHandlerExW.
 */
#include <windows.h>

static const GUID MONITOR_POWER_ON = {0x02731015, 0x4510, 0x4526, {0x99, 0xe6, 0xe5, 0xa1, 0x7e, 0xbd, 0x1a, 0xea}};
static const GUID CONSOLE_DISPLAY = {0x6fe69556, 0x704a, 0x47a0, {0x8f, 0x24, 0xc2, 0x8d, 0x93, 0x6f, 0xda, 0x47}};

struct delivery {
    LPHANDLER_FUNCTION_EX handler;
    void *context;
    POWERBROADCAST_SETTING setting; /* Data[1] plus padding holds the DWORD value */
    DWORD pad;
};

static DWORD WINAPI deliver(void *arg)
{
    struct delivery *d = arg;
    Sleep(300); /* Windows sends it after the registering call has returned */
    d->handler(SERVICE_CONTROL_POWEREVENT, PBT_POWERSETTINGCHANGE, &d->setting, d->context);
    HeapFree(GetProcessHeap(), 0, d);
    return 0;
}

/* ---- patching -------------------------------------------------------- */

struct patch {
    BYTE *target;
    BYTE saved[12], jump[12];
};

static void write_bytes(BYTE *target, const BYTE *bytes)
{
    DWORD old;
    if (!VirtualProtect(target, 12, PAGE_EXECUTE_READWRITE, &old)) return;
    memcpy(target, bytes, 12);
    VirtualProtect(target, 12, old, &old);
    FlushInstructionCache(GetCurrentProcess(), target, 12);
}

/* Overwrite the start of a function with a jump to its replacement. */
static void install(struct patch *p, const WCHAR *dll, const char *name, void *dest)
{
    static const BYTE jump[12] = {0x48, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xe0}; /* mov rax, imm64; jmp rax */
    HMODULE mod = GetModuleHandleW(dll);

    if (!mod) mod = LoadLibraryW(dll);
    p->target = mod ? (BYTE *)GetProcAddress(mod, name) : NULL;
    if (!p->target) return;
    memcpy(p->saved, p->target, 12);
    memcpy(p->jump, jump, 12);
    memcpy(p->jump + 2, &dest, sizeof dest);
    write_bytes(p->target, p->jump);
}

/* ---- the service's control handler ------------------------------------ */

static struct patch handler_patch, power_patch;
static LPHANDLER_FUNCTION_EX service_handler;
static void *service_context;

/* Remember the handler, then let Wine register it. A service registers once, on its main thread, so the
 * brief moment with the original bytes restored is not a race worth guarding. */
static SERVICE_STATUS_HANDLE WINAPI shim_RegisterServiceCtrlHandlerExW(const WCHAR *name, LPHANDLER_FUNCTION_EX handler,
                                                                       void *context)
{
    SERVICE_STATUS_HANDLE (WINAPI *real)(const WCHAR *, LPHANDLER_FUNCTION_EX, void *) = (void *)handler_patch.target;
    SERVICE_STATUS_HANDLE ret;

    service_handler = handler;
    service_context = context;
    write_bytes(handler_patch.target, handler_patch.saved);
    ret = real(name, handler, context);
    write_bytes(handler_patch.target, handler_patch.jump);
    return ret;
}

static HPOWERNOTIFY WINAPI shim_RegisterPowerSettingNotification(HANDLE recipient, const GUID *setting, DWORD flags)
{
    struct delivery *d;
    DWORD on = 1;

    if ((flags & DEVICE_NOTIFY_SERVICE_HANDLE) && setting && service_handler &&
        (IsEqualGUID(setting, &MONITOR_POWER_ON) || IsEqualGUID(setting, &CONSOLE_DISPLAY)) &&
        (d = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *d))) {
        d->handler = service_handler;
        d->context = service_context;
        d->setting.PowerSetting = *setting;
        d->setting.DataLength = sizeof on;
        memcpy(d->setting.Data, &on, sizeof on);
        CloseHandle(CreateThread(NULL, 0, deliver, d, 0, NULL));
    }
    return (HPOWERNOTIFY)(ULONG_PTR)0xa20a20; /* never dereferenced; Wine's Unregister ignores it */
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, void *reserved)
{
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(inst);
        install(&handler_patch, L"sechost.dll", "RegisterServiceCtrlHandlerExW", shim_RegisterServiceCtrlHandlerExW);
        install(&power_patch, L"user32.dll", "RegisterPowerSettingNotification", shim_RegisterPowerSettingNotification);
    }
    return TRUE;
}
