/* SPDX-License-Identifier: MIT
 *
 * cfgmgr32.dll shim for Wine: forwards everything to a renamed copy of Wine's
 * own cfgmgr32 ("cfgmgw.dll") and fills in parent/child navigation for
 * devices described by hand in the registry, so a program can walk between a
 * composite USB device and its functions (for instance the serial-port half
 * of a CDC device).
 *
 * The link follows the Windows convention: the parent's registry key has a
 * "ParentIdPrefix" value, and each child's instance name is "<prefix>&NNNN".
 * A device may also name its parent outright in a "ParentDevice" value, which
 * is this shim's own convention (Windows keeps that link in memory only).
 */
#include <windows.h>
#include <wchar.h>

typedef DWORD CONFIGRET, DEVINST;
#define CR_SUCCESS 0
#define CR_NO_SUCH_DEVNODE 0x0d
#define CR_INVALID_POINTER 0x03

static HMODULE real;
static CONFIGRET (WINAPI *p_get_id)(DEVINST, WCHAR *, ULONG, ULONG, HANDLE);
static CONFIGRET (WINAPI *p_locate)(DEVINST *, WCHAR *, ULONG, HANDLE);
static CONFIGRET (WINAPI *p_get_parent)(DEVINST *, DEVINST, ULONG, HANDLE);

static BOOL load_real(void)
{
    if (real) return TRUE;
    real = LoadLibraryW(L"cfgmgw.dll");
    if (!real) return FALSE;
    p_get_id = (void *)GetProcAddress(real, "CM_Get_Device_ID_ExW");
    p_locate = (void *)GetProcAddress(real, "CM_Locate_DevNode_ExW");
    p_get_parent = (void *)GetProcAddress(real, "CM_Get_Parent_Ex");
    return p_get_id && p_locate && p_get_parent;
}

static const WCHAR ENUM_KEY[] = L"System\\CurrentControlSet\\Enum";

/* Walk every device instance; stop when the callback returns TRUE and leave its id in `found`. */
typedef BOOL (*match_fn)(HKEY root, const WCHAR *id, const WCHAR *inst, const WCHAR *arg);

static BOOL find_device(match_fn match, const WCHAR *arg, WCHAR *found, size_t len)
{
    WCHAR bus[200], dev[200], inst[200];
    HKEY root, hbus, hdev;
    BOOL ok = FALSE;

    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, ENUM_KEY, 0, KEY_READ, &root)) return FALSE;
    for (DWORD i = 0; !ok && !RegEnumKeyW(root, i, bus, ARRAYSIZE(bus)); i++) {
        if (RegOpenKeyExW(root, bus, 0, KEY_READ, &hbus)) continue;
        for (DWORD j = 0; !ok && !RegEnumKeyW(hbus, j, dev, ARRAYSIZE(dev)); j++) {
            if (RegOpenKeyExW(hbus, dev, 0, KEY_READ, &hdev)) continue;
            for (DWORD k = 0; !ok && !RegEnumKeyW(hdev, k, inst, ARRAYSIZE(inst)); k++) {
                swprintf(found, len, L"%ls\\%ls\\%ls", bus, dev, inst);
                ok = match(root, found, inst, arg);
            }
            RegCloseKey(hdev);
        }
        RegCloseKey(hbus);
    }
    RegCloseKey(root);
    return ok;
}

static BOOL has_prefix(const WCHAR *inst, const WCHAR *prefix)
{
    size_t n = wcslen(prefix);
    return n && !_wcsnicmp(inst, prefix, n) && inst[n] == L'&';
}

/* arg = the parent's prefix: is this instance one of its children? */
static BOOL is_child(HKEY root, const WCHAR *id, const WCHAR *inst, const WCHAR *prefix)
{
    return has_prefix(inst, prefix);
}

/* arg = a child's instance name: does this device's prefix claim it? */
static BOOL is_parent(HKEY root, const WCHAR *id, const WCHAR *inst, const WCHAR *child_inst)
{
    WCHAR prefix[200];
    DWORD size = sizeof prefix;
    if (RegGetValueW(root, id, L"ParentIdPrefix", RRF_RT_REG_SZ, NULL, prefix, &size)) return FALSE;
    return has_prefix(child_inst, prefix);
}

CONFIGRET WINAPI shim_CM_Get_Child_Ex(DEVINST *child, DEVINST node, ULONG flags, HANDLE machine)
{
    WCHAR id[400], prefix[200], found[640];
    DWORD size = sizeof prefix;

    if (!child) return CR_INVALID_POINTER;
    *child = 0;
    if (!load_real()) return CR_NO_SUCH_DEVNODE;
    if (p_get_id(node, id, ARRAYSIZE(id), 0, machine)) return CR_NO_SUCH_DEVNODE;
    swprintf(found, ARRAYSIZE(found), L"%ls\\%ls", ENUM_KEY, id);
    if (RegGetValueW(HKEY_LOCAL_MACHINE, found, L"ParentIdPrefix", RRF_RT_REG_SZ, NULL, prefix, &size))
        return CR_NO_SUCH_DEVNODE;
    if (!find_device(is_child, prefix, found, ARRAYSIZE(found))) return CR_NO_SUCH_DEVNODE;
    return p_locate(child, found, 0, machine) ? CR_NO_SUCH_DEVNODE : CR_SUCCESS;
}

CONFIGRET WINAPI shim_CM_Get_Child(DEVINST *child, DEVINST node, ULONG flags)
{
    return shim_CM_Get_Child_Ex(child, node, flags, NULL);
}

CONFIGRET WINAPI shim_CM_Get_Parent_Ex(DEVINST *parent, DEVINST node, ULONG flags, HANDLE machine)
{
    WCHAR id[400], found[640], key[640], *inst;
    DWORD size = sizeof found;

    if (!parent) return CR_INVALID_POINTER;
    if (!load_real()) return CR_NO_SUCH_DEVNODE;
    if (p_get_id(node, id, ARRAYSIZE(id), 0, machine)) return p_get_parent(parent, node, flags, machine);

    /* An explicit "ParentDevice" value (this shim's own convention) wins. */
    swprintf(key, ARRAYSIZE(key), L"%ls\\%ls", ENUM_KEY, id);
    if (!RegGetValueW(HKEY_LOCAL_MACHINE, key, L"ParentDevice", RRF_RT_REG_SZ, NULL, found, &size) &&
        !p_locate(parent, found, 0, machine))
        return CR_SUCCESS;
    if ((inst = wcsrchr(id, L'\\')) && find_device(is_parent, inst + 1, found, ARRAYSIZE(found)) &&
        !p_locate(parent, found, 0, machine))
        return CR_SUCCESS;
    return p_get_parent(parent, node, flags, machine);
}

CONFIGRET WINAPI shim_CM_Get_Parent(DEVINST *parent, DEVINST node, ULONG flags)
{
    return shim_CM_Get_Parent_Ex(parent, node, flags, NULL);
}

/* Wine's CM_Get_DevNode_Status succeeds without writing anything, so callers read stack garbage. Report what
 * Windows shows for a present, working device (0x0180600a in Device Manager's "Status" for a USB function). */
#define DN_DRIVER_LOADED 0x00000002
#define DN_STARTED 0x00000008
#define DN_DISABLEABLE 0x00002000
#define DN_REMOVABLE 0x00004000
#define DN_NT_ENUMERATOR 0x00800000
#define DN_NT_DRIVER 0x01000000

CONFIGRET WINAPI shim_CM_Get_DevNode_Status_Ex(ULONG *status, ULONG *problem, DEVINST node, ULONG flags, HANDLE machine)
{
    WCHAR id[400];

    if (!status || !problem) return CR_INVALID_POINTER;
    if (!load_real() || p_get_id(node, id, ARRAYSIZE(id), 0, machine)) return CR_NO_SUCH_DEVNODE;
    *status = DN_DRIVER_LOADED | DN_STARTED | DN_DISABLEABLE | DN_REMOVABLE | DN_NT_ENUMERATOR | DN_NT_DRIVER;
    *problem = 0;
    return CR_SUCCESS;
}

CONFIGRET WINAPI shim_CM_Get_DevNode_Status(ULONG *status, ULONG *problem, DEVINST node, ULONG flags)
{
    return shim_CM_Get_DevNode_Status_Ex(status, problem, node, flags, NULL);
}
