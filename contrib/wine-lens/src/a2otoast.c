/* SPDX-License-Identifier: MIT
 *
 * a2otoast.dll: a do-nothing stand-in for the Windows toast-notification
 * classes that Wine does not implement yet. Programs that insist on a toast
 * notifier at start-up get objects that accept every call and show nothing.
 *
 * Classes: Windows.UI.Notifications.ToastNotificationManager,
 *          Windows.UI.Notifications.ToastNotification,
 *          Windows.Data.Xml.Dom.XmlDocument (accepts LoadXml, keeps nothing).
 *
 * 64-bit only: the generic "accept anything" methods rely on the x64 calling
 * convention, where a callee may ignore arguments it does not know about.
 * Every call is logged to the file named by A2OTOAST_LOG, when that is set.
 */
#define COBJMACROS
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>
#include <wchar.h>
#include <winstring.h>

#ifndef _WIN64
#error "a2otoast is 64-bit only"
#endif

/* ---- logging ---------------------------------------------------------- */

static void logf_(const char *fmt, ...)
{
    char path[MAX_PATH];
    if (!GetEnvironmentVariableA("A2OTOAST_LOG", path, sizeof path)) return;
    FILE *f = fopen(path, "a");
    if (!f) return;
    va_list ap;
    va_start(ap, fmt);
    fprintf(f, "[%lu] ", GetCurrentThreadId());
    vfprintf(f, fmt, ap);
    fputc('\n', f);
    va_end(ap);
    fclose(f);
}

static const char *guid_str(const GUID *g, char *buf)
{
    sprintf(buf, "{%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x}", g->Data1, g->Data2, g->Data3,
            g->Data4[0], g->Data4[1], g->Data4[2], g->Data4[3], g->Data4[4], g->Data4[5], g->Data4[6],
            g->Data4[7]);
    return buf;
}

/* ---- a tiny object model ---------------------------------------------- */

#define MAX_IFACES 4
#define SLOTS 40 /* more than any interface here has */

struct object;
struct iface {
    void **vtbl; /* must be first: this is what callers hold */
    struct object *obj;
    const GUID *iid;
    const char *name;
};
struct object {
    LONG ref;
    BOOL is_static;
    const WCHAR *class_name;
    struct object *held; /* released with this object (a toast keeps its XML) */
    struct iface ifaces[MAX_IFACES];
};

static const GUID IID_IUnknown_ = {0x00000000, 0, 0, {0xc0, 0, 0, 0, 0, 0, 0, 0x46}};
static const GUID IID_IInspectable_ = {0xaf86e2e0, 0xb12d, 0x4c6a, {0x9c, 0x5a, 0xd7, 0xaa, 0x65, 0x10, 0x1e, 0x90}};
static const GUID IID_IAgileObject_ = {0x94ea2b94, 0xe9cc, 0x49e0, {0xc0, 0xff, 0xee, 0x64, 0xca, 0x8f, 0x5b, 0x90}};
static const GUID IID_IActivationFactory_ = {0x00000035, 0, 0, {0xc0, 0, 0, 0, 0, 0, 0, 0x46}};

static const GUID IID_ManagerStatics = {0x50ac103f, 0xd235, 0x4598, {0xbb, 0xef, 0x98, 0xfe, 0x4d, 0x1a, 0x3a, 0xd4}};
static const GUID IID_ManagerStatics2 = {0x7ab93c52, 0x0e48, 0x4750, {0xba, 0x9d, 0x1a, 0x41, 0x13, 0x98, 0x18, 0x47}};
static const GUID IID_Notifier = {0x75927b93, 0x03f3, 0x41ec, {0x91, 0xd3, 0x6e, 0x5b, 0xac, 0x1b, 0x38, 0xe7}};
static const GUID IID_History = {0x5caddc63, 0x01d3, 0x4c97, {0x98, 0x6f, 0x05, 0x33, 0x48, 0x3f, 0xee, 0x14}};
static const GUID IID_ToastFactory = {0x04124b20, 0x82c6, 0x4229, {0xb1, 0x09, 0xfd, 0x9e, 0xd4, 0x66, 0x2b, 0x53}};
static const GUID IID_Toast = {0x997e2675, 0x059e, 0x4e60, {0x8b, 0x06, 0x17, 0x60, 0x91, 0x7c, 0x8b, 0x80}};
static const GUID IID_Toast2 = {0x9dfb9fd1, 0x143a, 0x490e, {0x90, 0xbf, 0xb9, 0xfb, 0xa7, 0x13, 0x2d, 0xe7}};
static const GUID IID_XmlDocument = {0xf7f3a506, 0x1e87, 0x42d6, {0xbc, 0xfb, 0xb8, 0xc8, 0x09, 0xfa, 0x54, 0x94}};
static const GUID IID_XmlDocumentIO = {0x6cd0e74e, 0xee65, 0x4489, {0x9e, 0xbf, 0xca, 0x43, 0xe8, 0x7b, 0xa6, 0x37}};

static HRESULT WINAPI obj_QueryInterface(struct iface *This, const GUID *iid, void **out)
{
    struct object *o = This->obj;
    struct iface *found = NULL;
    char buf[64];

    if (IsEqualGUID(iid, &IID_IUnknown_) || IsEqualGUID(iid, &IID_IInspectable_) ||
        IsEqualGUID(iid, &IID_IAgileObject_))
        found = &o->ifaces[0];
    else
        for (int i = 0; i < MAX_IFACES && o->ifaces[i].vtbl; i++)
            if (IsEqualGUID(iid, o->ifaces[i].iid)) found = &o->ifaces[i];

    if (!found) {
        logf_("%ls: QueryInterface %s -> E_NOINTERFACE", o->class_name, guid_str(iid, buf));
        *out = NULL;
        return E_NOINTERFACE;
    }
    InterlockedIncrement(&o->ref);
    *out = found;
    return S_OK;
}

static ULONG WINAPI obj_AddRef(struct iface *This)
{
    return InterlockedIncrement(&This->obj->ref);
}

static ULONG WINAPI obj_Release(struct iface *This)
{
    struct object *o = This->obj;
    LONG ref = InterlockedDecrement(&o->ref);
    if (!ref && !o->is_static) {
        if (o->held) obj_Release(&o->held->ifaces[0]);
        HeapFree(GetProcessHeap(), 0, o);
    }
    return ref;
}

static HRESULT WINAPI obj_GetIids(struct iface *This, ULONG *count, GUID **iids)
{
    *count = 0;
    *iids = NULL;
    return S_OK;
}

static HRESULT WINAPI obj_GetRuntimeClassName(struct iface *This, HSTRING *name)
{
    return WindowsCreateString(This->obj->class_name, (UINT32)wcslen(This->obj->class_name), name);
}

static HRESULT WINAPI obj_GetTrustLevel(struct iface *This, int *level)
{
    *level = 0; /* BaseTrust */
    return S_OK;
}

/* Methods nobody wrote: log the slot and succeed. */
#define GENERIC(n)                                                                    \
    static HRESULT WINAPI generic_##n(struct iface *This)                             \
    {                                                                                 \
        logf_("%ls: %s method #%d (accepted, ignored)", This->obj->class_name, This->name, n); \
        return S_OK;                                                                  \
    }
GENERIC(6) GENERIC(7) GENERIC(8) GENERIC(9) GENERIC(10) GENERIC(11) GENERIC(12) GENERIC(13) GENERIC(14)
GENERIC(15) GENERIC(16) GENERIC(17) GENERIC(18) GENERIC(19) GENERIC(20) GENERIC(21) GENERIC(22) GENERIC(23)
GENERIC(24) GENERIC(25) GENERIC(26) GENERIC(27) GENERIC(28) GENERIC(29) GENERIC(30) GENERIC(31) GENERIC(32)
GENERIC(33) GENERIC(34) GENERIC(35) GENERIC(36) GENERIC(37) GENERIC(38) GENERIC(39)

#define BASE_SLOTS                                                                             \
    obj_QueryInterface, obj_AddRef, obj_Release, obj_GetIids, obj_GetRuntimeClassName, obj_GetTrustLevel
#define GENERIC_FROM_6                                                                          \
    generic_6, generic_7, generic_8, generic_9, generic_10, generic_11, generic_12, generic_13,   \
        generic_14, generic_15, generic_16, generic_17, generic_18, generic_19, generic_20,      \
        generic_21, generic_22, generic_23, generic_24, generic_25, generic_26, generic_27,      \
        generic_28, generic_29, generic_30, generic_31, generic_32, generic_33, generic_34,      \
        generic_35, generic_36, generic_37, generic_38, generic_39

/* Each table starts fully generic; init_tables() then fills in the methods
 * that have out-parameters or deserve a readable log line. */
#define TABLE(name) static void *name[SLOTS] = {BASE_SLOTS, GENERIC_FROM_6}
TABLE(vt_manager_activation);
TABLE(vt_manager_statics);
TABLE(vt_manager_statics2);
TABLE(vt_notifier);
TABLE(vt_history);
TABLE(vt_toast_activation);
TABLE(vt_toast_factory);
TABLE(vt_toast);
TABLE(vt_toast2);
TABLE(vt_xml_activation);
TABLE(vt_xml_document);
TABLE(vt_xml_io);

static struct object *new_object(const WCHAR *class_name)
{
    struct object *o = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof *o);
    if (!o) return NULL;
    o->ref = 1;
    o->class_name = class_name;
    return o;
}

static void add_iface(struct object *o, void **vtbl, const GUID *iid, const char *name)
{
    for (int i = 0; i < MAX_IFACES; i++)
        if (!o->ifaces[i].vtbl) {
            o->ifaces[i] = (struct iface){vtbl, o, iid, name};
            return;
        }
}

static const char *hstr(HSTRING s, char *buf, size_t n)
{
    const WCHAR *w = WindowsGetStringRawBuffer(s, NULL);
    WideCharToMultiByte(CP_UTF8, 0, w ? w : L"", -1, buf, (int)n, NULL, NULL);
    buf[n - 1] = 0;
    return buf;
}

/* ---- XmlDocument ------------------------------------------------------ */

static const WCHAR XML_CLASS[] = L"Windows.Data.Xml.Dom.XmlDocument";

static struct object *new_xml(void)
{
    struct object *o = new_object(XML_CLASS);
    if (!o) return NULL;
    add_iface(o, vt_xml_document, &IID_XmlDocument, "IXmlDocument");
    add_iface(o, vt_xml_io, &IID_XmlDocumentIO, "IXmlDocumentIO");
    return o;
}

static HRESULT WINAPI xml_io_LoadXml(struct iface *This, HSTRING xml)
{
    char buf[2048];
    logf_("XmlDocument.LoadXml: %s", hstr(xml, buf, sizeof buf));
    return S_OK;
}

static HRESULT WINAPI xml_ActivateInstance(struct iface *This, void **instance)
{
    struct object *o = new_xml();
    logf_("XmlDocument: new instance");
    *instance = o ? &o->ifaces[0] : NULL;
    return o ? S_OK : E_OUTOFMEMORY;
}

/* ---- ToastNotification ------------------------------------------------- */

static const WCHAR TOAST_CLASS[] = L"Windows.UI.Notifications.ToastNotification";

static HRESULT WINAPI toast_get_Content(struct iface *This, void **value)
{
    struct object *xml = This->obj->held;
    if (xml) InterlockedIncrement(&xml->ref);
    *value = xml ? &xml->ifaces[0] : NULL;
    return S_OK;
}

static HRESULT WINAPI toast_get_ExpirationTime(struct iface *This, void **value)
{
    *value = NULL;
    return S_OK;
}

static HRESULT WINAPI toast_add_event(struct iface *This, void *handler, INT64 *token)
{
    static LONG next;
    *token = InterlockedIncrement(&next);
    return S_OK;
}

static HRESULT WINAPI toast_remove_event(struct iface *This, INT64 token)
{
    return S_OK;
}

static HRESULT WINAPI toast2_put_string(struct iface *This, HSTRING value)
{
    return S_OK;
}

static HRESULT WINAPI toast2_get_string(struct iface *This, HSTRING *value)
{
    *value = NULL;
    return S_OK;
}

static HRESULT WINAPI toast2_get_SuppressPopup(struct iface *This, BOOLEAN *value)
{
    *value = FALSE;
    return S_OK;
}

static HRESULT WINAPI toast_factory_Create(struct iface *This, struct iface *content, void **value)
{
    struct object *o = new_object(TOAST_CLASS);
    *value = NULL;
    if (!o) return E_OUTOFMEMORY;
    add_iface(o, vt_toast, &IID_Toast, "IToastNotification");
    add_iface(o, vt_toast2, &IID_Toast2, "IToastNotification2");
    if (content) {
        /* Only keep documents of our own making: anything else has a layout we don't know. */
        if (content->vtbl == vt_xml_document || content->vtbl == vt_xml_io) {
            o->held = content->obj;
            InterlockedIncrement(&o->held->ref);
        }
    }
    logf_("ToastNotification: created");
    *value = &o->ifaces[0];
    return S_OK;
}

static HRESULT WINAPI no_default_instance(struct iface *This, void **instance)
{
    *instance = NULL;
    return E_NOTIMPL;
}

/* ---- ToastNotifier, history, manager ----------------------------------- */

static HRESULT WINAPI notifier_Show(struct iface *This, void *toast)
{
    logf_("ToastNotifier.Show (nothing is displayed)");
    return S_OK;
}

static HRESULT WINAPI notifier_Hide(struct iface *This, void *toast)
{
    logf_("ToastNotifier.Hide");
    return S_OK;
}

static HRESULT WINAPI notifier_get_Setting(struct iface *This, int *value)
{
    *value = 0; /* NotificationSetting_Enabled */
    return S_OK;
}

static HRESULT WINAPI notifier_get_scheduled(struct iface *This, void **value)
{
    *value = NULL;
    return E_NOTIMPL;
}

static HRESULT new_notifier(void **out)
{
    struct object *o = new_object(L"Windows.UI.Notifications.ToastNotifier");
    *out = NULL;
    if (!o) return E_OUTOFMEMORY;
    add_iface(o, vt_notifier, &IID_Notifier, "IToastNotifier");
    *out = &o->ifaces[0];
    return S_OK;
}

static HRESULT WINAPI manager_CreateToastNotifier(struct iface *This, void **out)
{
    logf_("ToastNotificationManager.CreateToastNotifier()");
    return new_notifier(out);
}

static HRESULT WINAPI manager_CreateToastNotifierWithId(struct iface *This, HSTRING id, void **out)
{
    char buf[256];
    logf_("ToastNotificationManager.CreateToastNotifier(%s)", hstr(id, buf, sizeof buf));
    return new_notifier(out);
}

static HRESULT WINAPI manager_GetTemplateContent(struct iface *This, int type, void **out)
{
    struct object *o = new_xml();
    logf_("ToastNotificationManager.GetTemplateContent(%d)", type);
    *out = o ? &o->ifaces[0] : NULL;
    return o ? S_OK : E_OUTOFMEMORY;
}

static HRESULT WINAPI manager_get_History(struct iface *This, void **out)
{
    struct object *o = new_object(L"Windows.UI.Notifications.ToastNotificationHistory");
    logf_("ToastNotificationManager.History");
    *out = NULL;
    if (!o) return E_OUTOFMEMORY;
    add_iface(o, vt_history, &IID_History, "IToastNotificationHistory");
    *out = &o->ifaces[0];
    return S_OK;
}

/* ---- factories --------------------------------------------------------- */

static struct object manager_factory = {.ref = 1, .is_static = TRUE,
                                        .class_name = L"Windows.UI.Notifications.ToastNotificationManager"};
static struct object toast_factory = {.ref = 1, .is_static = TRUE, .class_name = TOAST_CLASS};
static struct object xml_factory = {.ref = 1, .is_static = TRUE, .class_name = XML_CLASS};

static INIT_ONCE once = INIT_ONCE_STATIC_INIT;

static BOOL CALLBACK init_tables(INIT_ONCE *o, void *param, void **ctx)
{
    vt_manager_activation[6] = no_default_instance;
    vt_manager_statics[6] = manager_CreateToastNotifier;
    vt_manager_statics[7] = manager_CreateToastNotifierWithId;
    vt_manager_statics[8] = manager_GetTemplateContent;
    vt_manager_statics2[6] = manager_get_History;

    vt_notifier[6] = notifier_Show;
    vt_notifier[7] = notifier_Hide;
    vt_notifier[8] = notifier_get_Setting;
    vt_notifier[11] = notifier_get_scheduled;

    vt_toast_activation[6] = no_default_instance;
    vt_toast_factory[6] = toast_factory_Create;
    vt_toast[6] = toast_get_Content;
    vt_toast[8] = toast_get_ExpirationTime;
    vt_toast[9] = toast_add_event;
    vt_toast[10] = toast_remove_event;
    vt_toast[11] = toast_add_event;
    vt_toast[12] = toast_remove_event;
    vt_toast[13] = toast_add_event;
    vt_toast[14] = toast_remove_event;
    vt_toast2[6] = toast2_put_string;
    vt_toast2[7] = toast2_get_string;
    vt_toast2[8] = toast2_put_string;
    vt_toast2[9] = toast2_get_string;
    vt_toast2[11] = toast2_get_SuppressPopup;

    vt_xml_activation[6] = xml_ActivateInstance;
    vt_xml_io[6] = xml_io_LoadXml;

    add_iface(&manager_factory, vt_manager_activation, &IID_IActivationFactory_, "IActivationFactory");
    add_iface(&manager_factory, vt_manager_statics, &IID_ManagerStatics, "IToastNotificationManagerStatics");
    add_iface(&manager_factory, vt_manager_statics2, &IID_ManagerStatics2, "IToastNotificationManagerStatics2");
    add_iface(&toast_factory, vt_toast_activation, &IID_IActivationFactory_, "IActivationFactory");
    add_iface(&toast_factory, vt_toast_factory, &IID_ToastFactory, "IToastNotificationFactory");
    add_iface(&xml_factory, vt_xml_activation, &IID_IActivationFactory_, "IActivationFactory");
    return TRUE;
}

HRESULT WINAPI DllGetActivationFactory(HSTRING class_id, void **factory)
{
    const WCHAR *name = WindowsGetStringRawBuffer(class_id, NULL);
    struct object *all[] = {&manager_factory, &toast_factory, &xml_factory};

    InitOnceExecuteOnce(&once, init_tables, NULL, NULL);
    for (int i = 0; i < 3; i++)
        if (name && !wcscmp(name, all[i]->class_name)) {
            logf_("activation factory for %ls", name);
            InterlockedIncrement(&all[i]->ref);
            *factory = &all[i]->ifaces[0];
            return S_OK;
        }
    logf_("activation factory for %ls -> not available", name ? name : L"(null)");
    *factory = NULL;
    return CLASS_E_CLASSNOTAVAILABLE;
}

HRESULT WINAPI DllCanUnloadNow(void)
{
    return S_FALSE;
}
