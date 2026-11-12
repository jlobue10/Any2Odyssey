/* SPDX-License-Identifier: MIT
 *
 * a2ocam.dll: a Media Foundation camera for Wine, enough of one for a program that reads a webcam through a
 * source reader (Leia's eye tracker does).
 *
 * Wine has no Media Foundation video capture. The mf.dll and mfreadwrite.dll in the prefix forward two calls
 * here and everything else to Wine's own:
 *   MFEnumDeviceSources                    reports one camera, described in the registry;
 *   MFCreateSourceReaderFromMediaSource    gives that camera a source reader.
 * The frames come from any2odyssey-camera-bridge (camera-bridge.c), a native program that owns the V4L2
 * device and listens on a loopback port. Compressed frames (MJPG) are handed on as they are.
 *
 * Registry, HKLM\Software\Any2Odyssey\Camera:
 *   Port (dword), Secret (string)          where the bridge listens, and its shared secret
 *   SymbolicLink, FriendlyName (strings)   what Windows calls the camera
 *
 * If C:\a2ocam.log exists, calls are logged to it.
 */
#if !defined(A2O_CAMERA_STREAM_TEST) && !defined(A2O_CAMERA_READER_TEST)
#define COBJMACROS
#include <winsock2.h>
#include <windows.h>
#include <math.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <stdarg.h>
#include <stdio.h>
#include <strmif.h>

#include "sha256.h"

#ifndef E_PROP_ID_UNSUPPORTED
#define E_PROP_ID_UNSUPPORTED ((HRESULT)0x80070490)
#endif

/* {6f3a1f0e-93c5-4f0b-9d7e-a2c0ca3e0001}: asks one of our sources for itself. */
static const GUID IID_A2OSource = { 0x6f3a1f0e, 0x93c5, 0x4f0b, { 0x9d, 0x7e, 0xa2, 0xc0, 0xca, 0x3e, 0x00, 0x01 } };

static void say(const char *fmt, ...)
{
    FILE *f = fopen("C:\\a2ocam.log", "r+");
    va_list ap;
    if (!f) return;
    fseek(f, 0, SEEK_END);
    fprintf(f, "%04lx ", GetCurrentThreadId());
    va_start(ap, fmt);
    vfprintf(f, fmt, ap);
    va_end(ap);
    fputc('\n', f);
    fclose(f);
}

static const char *guid_text(REFGUID g)
{
    static char text[4][40];
    static LONG next;
    char *t = text[InterlockedIncrement(&next) & 3];
    if (!g) return "(null)";
    snprintf(t, 40, "{%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x}", g->Data1, g->Data2, g->Data3,
             g->Data4[0], g->Data4[1], g->Data4[2], g->Data4[3], g->Data4[4], g->Data4[5], g->Data4[6], g->Data4[7]);
    return t;
}

static const WCHAR CONFIG_KEY[] = L"Software\\Any2Odyssey\\Camera";

static BOOL config_string(const WCHAR *name, WCHAR *value, DWORD bytes)
{
    return !RegGetValueW(HKEY_LOCAL_MACHINE, CONFIG_KEY, name, RRF_RT_REG_SZ, NULL, value, &bytes);
}

/* ---------------------------------------------------------------------------------------------------------
 * The connection to the bridge.
 */
typedef void (*frame_fn)(void *ctx, UINT64 time_us, const BYTE *data, DWORD length);

struct bridge {
    SOCKET sock;
    HANDLE thread, replied;
    CRITICAL_SECTION command_lock, frame_lock;
    char reply[8192];
    frame_fn on_frame; /* guarded by frame_lock */
    void *frame_ctx;
    BOOL dead;
};

static BOOL read_all(SOCKET s, void *buffer, DWORD length)
{
    char *p = buffer;
    while (length) {
        int n = recv(s, p, length, 0);
        if (n <= 0) return FALSE;
        p += n;
        length -= n;
    }
    return TRUE;
}

static DWORD WINAPI bridge_thread(void *arg)
{
    struct bridge *b = arg;
    BYTE head[16], *data = NULL;
    DWORD size = 0, length;
    UINT64 time_us;

    while (read_all(b->sock, head, 16)) {
        memcpy(&length, head + 4, 4);
        memcpy(&time_us, head + 8, 8);
        if (length > 64 * 1024 * 1024) break;
        if (length + 1 > size) {
            BYTE *bigger = realloc(data, length + 1);
            if (!bigger) break;
            data = bigger;
            size = length + 1;
        }
        if (!read_all(b->sock, data, length)) break;
        if (head[0] == 'R') {
            DWORD n = length < sizeof b->reply - 1 ? length : sizeof b->reply - 1;
            memcpy(b->reply, data, n);
            b->reply[n] = 0;
            SetEvent(b->replied);
        } else if (head[0] == 'F') {
            EnterCriticalSection(&b->frame_lock);
            if (b->on_frame) b->on_frame(b->frame_ctx, time_us, data, length);
            LeaveCriticalSection(&b->frame_lock);
        }
    }
    free(data);
    b->dead = TRUE;
    SetEvent(b->replied);
    say("bridge: connection closed");
    return 0;
}

/* Send one command and wait for its reply. FALSE if the bridge is gone or said no (reply then starts with '!'). */
static BOOL bridge_command(struct bridge *b, char *reply, DWORD size, const char *fmt, ...)
{
    char line[256];
    va_list ap;
    BOOL ok = FALSE;
    int n;

    va_start(ap, fmt);
    n = vsnprintf(line, sizeof line - 1, fmt, ap);
    va_end(ap);
    if (n < 0 || n >= (int)sizeof line - 1) return FALSE;
    line[n++] = '\n';

    EnterCriticalSection(&b->command_lock);
    ResetEvent(b->replied);
    if (!b->dead && send(b->sock, line, n, 0) == n && WaitForSingleObject(b->replied, 15000) == WAIT_OBJECT_0 &&
        !b->dead) {
        ok = b->reply[0] != '!';
        if (reply) snprintf(reply, size, "%s", b->reply);
        if (!ok) say("bridge: \"%.*s\" -> %s", n - 1, line[0] == 'A' ? "A ..." : line, b->reply);
    }
    LeaveCriticalSection(&b->command_lock);
    return ok;
}

static void bridge_set_frames(struct bridge *b, frame_fn fn, void *ctx)
{
    EnterCriticalSection(&b->frame_lock);
    b->on_frame = fn;
    b->frame_ctx = ctx;
    LeaveCriticalSection(&b->frame_lock);
}

static void bridge_close(struct bridge *b)
{
    if (!b) return;
    shutdown(b->sock, SD_BOTH);
    closesocket(b->sock);
    WaitForSingleObject(b->thread, 5000);
    CloseHandle(b->thread);
    CloseHandle(b->replied);
    DeleteCriticalSection(&b->command_lock);
    DeleteCriticalSection(&b->frame_lock);
    free(b);
}

static struct bridge *bridge_open(void)
{
    struct sockaddr_in addr = { .sin_family = AF_INET };
    WCHAR secret[128];
    char key[384], challenge[64] = "", answer[65] = "";
    DWORD port = 0, bytes = sizeof port;
    struct bridge *b;
    WSADATA wsa;
    int one = 1;

    if (RegGetValueW(HKEY_LOCAL_MACHINE, CONFIG_KEY, L"Port", RRF_RT_REG_DWORD, NULL, &port, &bytes) ||
        !config_string(L"Secret", secret, sizeof secret)) {
        say("bridge: no Port/Secret in the registry");
        return NULL;
    }
    WSAStartup(MAKEWORD(2, 2), &wsa);
    if (!(b = calloc(1, sizeof *b))) return NULL;
    b->sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    addr.sin_port = htons((u_short)port);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (b->sock == INVALID_SOCKET || connect(b->sock, (struct sockaddr *)&addr, sizeof addr)) {
        say("bridge: nothing listens on 127.0.0.1:%lu (error %d)", port, WSAGetLastError());
        if (b->sock != INVALID_SOCKET) closesocket(b->sock);
        free(b);
        return NULL;
    }
    setsockopt(b->sock, IPPROTO_TCP, TCP_NODELAY, (char *)&one, sizeof one);
    InitializeCriticalSection(&b->command_lock);
    InitializeCriticalSection(&b->frame_lock);
    b->replied = CreateEventW(NULL, TRUE, FALSE, NULL);
    b->thread = CreateThread(NULL, 0, bridge_thread, b, 0, NULL);
    /* Prove that we know the secret without sending it: the bridge gives a random challenge and gets its
     * HMAC back. Whatever answers on the port, it does not learn the secret. */
    WideCharToMultiByte(CP_UTF8, 0, secret, -1, key, sizeof key, NULL, NULL);
    key[sizeof key - 1] = 0;
    if (bridge_command(b, challenge, sizeof challenge, "H")) a2o_hmac_hex(key, challenge, answer);
    SecureZeroMemory(key, sizeof key);
    if (!answer[0] || !bridge_command(b, NULL, 0, "A %s", answer)) {
        say("bridge: refused (camera busy or missing, or a wrong secret)");
        bridge_close(b);
        return NULL;
    }
    return b;
}

/* ---------------------------------------------------------------------------------------------------------
 * The media source: little more than the owner of the bridge and of the camera controls. Nothing but our
 * own source reader reads from it.
 */
struct format {
    char fourcc[5];
    UINT32 width, height, fps;
};

struct source {
    IMFMediaSource IMFMediaSource_iface;
    IAMCameraControl IAMCameraControl_iface;
    IAMVideoProcAmp IAMVideoProcAmp_iface;
    LONG refs;
    struct bridge *bridge;
    struct format formats[64];
    UINT32 format_count;
};

static struct source *source_from_IMFMediaSource(IMFMediaSource *iface)
{
    return CONTAINING_RECORD(iface, struct source, IMFMediaSource_iface);
}

static HRESULT WINAPI source_QueryInterface(IMFMediaSource *iface, REFIID riid, void **out)
{
    struct source *s = source_from_IMFMediaSource(iface);

    if (IsEqualGUID(riid, &IID_IUnknown) || IsEqualGUID(riid, &IID_IMFMediaEventGenerator) ||
        IsEqualGUID(riid, &IID_IMFMediaSource) || IsEqualGUID(riid, &IID_A2OSource))
        *out = &s->IMFMediaSource_iface;
    else if (IsEqualGUID(riid, &IID_IAMCameraControl))
        *out = &s->IAMCameraControl_iface;
    else if (IsEqualGUID(riid, &IID_IAMVideoProcAmp))
        *out = &s->IAMVideoProcAmp_iface;
    else {
        say("source: no interface %s", guid_text(riid));
        *out = NULL;
        return E_NOINTERFACE;
    }
    InterlockedIncrement(&s->refs);
    return S_OK;
}

static ULONG WINAPI source_AddRef(IMFMediaSource *iface)
{
    return InterlockedIncrement(&source_from_IMFMediaSource(iface)->refs);
}

static ULONG WINAPI source_Release(IMFMediaSource *iface)
{
    struct source *s = source_from_IMFMediaSource(iface);
    ULONG refs = InterlockedDecrement(&s->refs);
    if (!refs) {
        say("source: released");
        bridge_close(s->bridge);
        free(s);
    }
    return refs;
}

static HRESULT WINAPI source_GetEvent(IMFMediaSource *iface, DWORD flags, IMFMediaEvent **event)
{
    say("source: GetEvent (not provided)");
    return E_NOTIMPL;
}

static HRESULT WINAPI source_BeginGetEvent(IMFMediaSource *iface, IMFAsyncCallback *callback, IUnknown *state)
{
    say("source: BeginGetEvent (not provided)");
    return E_NOTIMPL;
}

static HRESULT WINAPI source_EndGetEvent(IMFMediaSource *iface, IMFAsyncResult *result, IMFMediaEvent **event)
{
    return E_NOTIMPL;
}

static HRESULT WINAPI source_QueueEvent(IMFMediaSource *iface, MediaEventType type, REFGUID ext, HRESULT hr,
                                        const PROPVARIANT *value)
{
    return E_NOTIMPL;
}

static HRESULT WINAPI source_GetCharacteristics(IMFMediaSource *iface, DWORD *characteristics)
{
    *characteristics = MFMEDIASOURCE_IS_LIVE;
    return S_OK;
}

static HRESULT WINAPI source_CreatePresentationDescriptor(IMFMediaSource *iface, IMFPresentationDescriptor **pd)
{
    say("source: CreatePresentationDescriptor (not provided)");
    return E_NOTIMPL;
}

static HRESULT WINAPI source_Start(IMFMediaSource *iface, IMFPresentationDescriptor *pd, const GUID *format,
                                   const PROPVARIANT *position)
{
    say("source: Start (not provided)");
    return E_NOTIMPL;
}

static HRESULT WINAPI source_Stop(IMFMediaSource *iface)
{
    return S_OK;
}

static HRESULT WINAPI source_Pause(IMFMediaSource *iface)
{
    return S_OK;
}

static HRESULT WINAPI source_Shutdown(IMFMediaSource *iface)
{
    say("source: Shutdown");
    return S_OK;
}

static const IMFMediaSourceVtbl source_vtbl = {
    source_QueryInterface, source_AddRef, source_Release, source_GetEvent, source_BeginGetEvent,
    source_EndGetEvent, source_QueueEvent, source_GetCharacteristics, source_CreatePresentationDescriptor,
    source_Start, source_Stop, source_Pause, source_Shutdown,
};

/* Camera controls. Windows numbers them per interface; V4L2 has one id space. */
#define V4L2_BRIGHTNESS 0x00980900
#define V4L2_CONTRAST 0x00980901
#define V4L2_SATURATION 0x00980902
#define V4L2_HUE 0x00980903
#define V4L2_AUTO_WHITE_BALANCE 0x0098090c
#define V4L2_GAMMA 0x00980910
#define V4L2_GAIN 0x00980913
#define V4L2_POWER_LINE 0x00980918
#define V4L2_WHITE_BALANCE 0x0098091a
#define V4L2_SHARPNESS 0x0098091b
#define V4L2_BACKLIGHT 0x0098091c
#define V4L2_EXPOSURE_AUTO 0x009a0901 /* 1 = manual, 3 = aperture priority */
#define V4L2_EXPOSURE 0x009a0902      /* units of 100 microseconds */
#define V4L2_FOCUS 0x009a090a
#define V4L2_ZOOM 0x009a090d
#define V4L2_IRIS 0x009a0911

static const unsigned procamp_ids[] = {
    V4L2_BRIGHTNESS, V4L2_CONTRAST, V4L2_HUE, V4L2_SATURATION, V4L2_SHARPNESS, V4L2_GAMMA, 0 /* colour enable */,
    V4L2_WHITE_BALANCE, V4L2_BACKLIGHT, V4L2_GAIN, 0, 0, 0, V4L2_POWER_LINE,
};
static const unsigned camera_ids[] = { 0, 0, 0, V4L2_ZOOM, V4L2_EXPOSURE, V4L2_IRIS, V4L2_FOCUS };

struct control {
    int min, max, step, def, value;
};

static BOOL control_query(struct source *s, unsigned id, struct control *c)
{
    char reply[128];
    return id && bridge_command(s->bridge, reply, sizeof reply, "Q %x", id) &&
           sscanf(reply, "%d %d %d %d %d", &c->min, &c->max, &c->step, &c->def, &c->value) == 5;
}

static BOOL control_set(struct source *s, unsigned id, int value)
{
    return bridge_command(s->bridge, NULL, 0, "C %x %d", id, value);
}

/* Windows gives exposure as log2 of seconds. */
static long exposure_to_log2(int v4l2)
{
    return lround(log2((v4l2 > 0 ? v4l2 : 1) / 10000.0));
}

static HRESULT control_range(struct source *s, const char *what, long property, unsigned id, long *min, long *max,
                             long *step, long *def, long *caps)
{
    struct control c;

    if (!control_query(s, id, &c)) {
        say("%s: GetRange %ld: not supported", what, property);
        return E_PROP_ID_UNSUPPORTED;
    }
    *caps = 2; /* manual */
    if (id == V4L2_EXPOSURE) {
        *min = exposure_to_log2(c.min);
        *max = exposure_to_log2(c.max);
        *step = 1;
        *def = exposure_to_log2(c.def);
        *caps = 3; /* auto and manual */
    } else {
        *min = c.min;
        *max = c.max;
        *step = c.step ? c.step : 1;
        *def = c.def;
        if (id == V4L2_WHITE_BALANCE) *caps = 3;
    }
    say("%s: GetRange %ld -> %ld..%ld step %ld default %ld", what, property, *min, *max, *step, *def);
    return S_OK;
}

static HRESULT control_put(struct source *s, const char *what, long property, unsigned id, long value, long flags)
{
    struct control c;
    BOOL ok;

    if (!control_query(s, id, &c)) return E_PROP_ID_UNSUPPORTED;
    if (id == V4L2_EXPOSURE) {
        if (flags & 1) { /* auto */
            ok = control_set(s, V4L2_EXPOSURE_AUTO, 3) || control_set(s, V4L2_EXPOSURE_AUTO, 0);
        } else {
            long units = lround(10000.0 * pow(2.0, value));
            if (units < c.min) units = c.min;
            if (units > c.max) units = c.max;
            control_set(s, V4L2_EXPOSURE_AUTO, 1);
            ok = control_set(s, V4L2_EXPOSURE, units);
        }
    } else if (id == V4L2_WHITE_BALANCE) {
        ok = control_set(s, V4L2_AUTO_WHITE_BALANCE, (flags & 1) ? 1 : 0);
        if (!(flags & 1)) ok = control_set(s, id, value);
    } else {
        ok = control_set(s, id, value);
    }
    say("%s: Set %ld = %ld flags %ld -> %s", what, property, value, flags, ok ? "ok" : "failed");
    return ok ? S_OK : E_FAIL;
}

static HRESULT control_get(struct source *s, unsigned id, long *value, long *flags)
{
    struct control c, automatic;

    if (!control_query(s, id, &c)) return E_PROP_ID_UNSUPPORTED;
    *flags = 2;
    if (id == V4L2_EXPOSURE) {
        *value = exposure_to_log2(c.value);
        if (control_query(s, V4L2_EXPOSURE_AUTO, &automatic) && automatic.value != 1) *flags = 1;
    } else {
        *value = c.value;
        if (id == V4L2_WHITE_BALANCE && control_query(s, V4L2_AUTO_WHITE_BALANCE, &automatic) && automatic.value)
            *flags = 1;
    }
    return S_OK;
}

static struct source *source_from_IAMCameraControl(IAMCameraControl *iface)
{
    return CONTAINING_RECORD(iface, struct source, IAMCameraControl_iface);
}

static HRESULT WINAPI camctl_QueryInterface(IAMCameraControl *iface, REFIID riid, void **out)
{
    return source_QueryInterface(&source_from_IAMCameraControl(iface)->IMFMediaSource_iface, riid, out);
}

static ULONG WINAPI camctl_AddRef(IAMCameraControl *iface)
{
    return source_AddRef(&source_from_IAMCameraControl(iface)->IMFMediaSource_iface);
}

static ULONG WINAPI camctl_Release(IAMCameraControl *iface)
{
    return source_Release(&source_from_IAMCameraControl(iface)->IMFMediaSource_iface);
}

static unsigned camera_id(long property)
{
    return property >= 0 && property < (long)ARRAYSIZE(camera_ids) ? camera_ids[property] : 0;
}

static HRESULT WINAPI camctl_GetRange(IAMCameraControl *iface, LONG property, LONG *min, LONG *max, LONG *step,
                                      LONG *def, LONG *caps)
{
    return control_range(source_from_IAMCameraControl(iface), "camera control", property, camera_id(property),
                         min, max, step, def, caps);
}

static HRESULT WINAPI camctl_Set(IAMCameraControl *iface, LONG property, LONG value, LONG flags)
{
    return control_put(source_from_IAMCameraControl(iface), "camera control", property, camera_id(property),
                       value, flags);
}

static HRESULT WINAPI camctl_Get(IAMCameraControl *iface, LONG property, LONG *value, LONG *flags)
{
    return control_get(source_from_IAMCameraControl(iface), camera_id(property), value, flags);
}

static const IAMCameraControlVtbl camctl_vtbl = {
    camctl_QueryInterface, camctl_AddRef, camctl_Release, camctl_GetRange, camctl_Set, camctl_Get,
};

static struct source *source_from_IAMVideoProcAmp(IAMVideoProcAmp *iface)
{
    return CONTAINING_RECORD(iface, struct source, IAMVideoProcAmp_iface);
}

static HRESULT WINAPI procamp_QueryInterface(IAMVideoProcAmp *iface, REFIID riid, void **out)
{
    return source_QueryInterface(&source_from_IAMVideoProcAmp(iface)->IMFMediaSource_iface, riid, out);
}

static ULONG WINAPI procamp_AddRef(IAMVideoProcAmp *iface)
{
    return source_AddRef(&source_from_IAMVideoProcAmp(iface)->IMFMediaSource_iface);
}

static ULONG WINAPI procamp_Release(IAMVideoProcAmp *iface)
{
    return source_Release(&source_from_IAMVideoProcAmp(iface)->IMFMediaSource_iface);
}

static unsigned procamp_id(long property)
{
    return property >= 0 && property < (long)ARRAYSIZE(procamp_ids) ? procamp_ids[property] : 0;
}

static HRESULT WINAPI procamp_GetRange(IAMVideoProcAmp *iface, LONG property, LONG *min, LONG *max, LONG *step,
                                       LONG *def, LONG *caps)
{
    return control_range(source_from_IAMVideoProcAmp(iface), "video control", property, procamp_id(property), min,
                         max, step, def, caps);
}

static HRESULT WINAPI procamp_Set(IAMVideoProcAmp *iface, LONG property, LONG value, LONG flags)
{
    return control_put(source_from_IAMVideoProcAmp(iface), "video control", property, procamp_id(property), value,
                       flags);
}

static HRESULT WINAPI procamp_Get(IAMVideoProcAmp *iface, LONG property, LONG *value, LONG *flags)
{
    return control_get(source_from_IAMVideoProcAmp(iface), procamp_id(property), value, flags);
}

static const IAMVideoProcAmpVtbl procamp_vtbl = {
    procamp_QueryInterface, procamp_AddRef, procamp_Release, procamp_GetRange, procamp_Set, procamp_Get,
};

static HRESULT source_create(struct source **out)
{
    struct source *s = calloc(1, sizeof *s);
    char list[8192], *line, *next;

    if (!s) return E_OUTOFMEMORY;
    s->IMFMediaSource_iface.lpVtbl = (IMFMediaSourceVtbl *)&source_vtbl;
    s->IAMCameraControl_iface.lpVtbl = (IAMCameraControlVtbl *)&camctl_vtbl;
    s->IAMVideoProcAmp_iface.lpVtbl = (IAMVideoProcAmpVtbl *)&procamp_vtbl;
    s->refs = 1;
    if (!(s->bridge = bridge_open())) {
        free(s);
        return HRESULT_FROM_WIN32(ERROR_DEVICE_NOT_CONNECTED);
    }
    if (bridge_command(s->bridge, list, sizeof list, "L")) {
        for (line = list; *line && s->format_count < ARRAYSIZE(s->formats); line = next) {
            struct format *f = &s->formats[s->format_count];
            next = strchr(line, '\n');
            next = next ? next + 1 : line + strlen(line);
            if (sscanf(line, "%4s %u %u %u", f->fourcc, &f->width, &f->height, &f->fps) != 4) continue;
            if (strcmp(f->fourcc, "MJPG") && strcmp(f->fourcc, "YUYV")) continue;
            s->format_count++;
        }
    }
    say("source: created, %u formats", s->format_count);
    if (!s->format_count) {
        source_Release(&s->IMFMediaSource_iface);
        return MF_E_INVALIDMEDIATYPE;
    }
    *out = s;
    return S_OK;
}

static HRESULT format_to_type(const struct format *f, IMFMediaType **out)
{
    IMFMediaType *type;
    BOOL jpeg = !strcmp(f->fourcc, "MJPG");
    HRESULT hr = MFCreateMediaType(&type);

    if (FAILED(hr)) return hr;
    IMFMediaType_SetGUID(type, &MF_MT_MAJOR_TYPE, &MFMediaType_Video);
    IMFMediaType_SetGUID(type, &MF_MT_SUBTYPE, jpeg ? &MFVideoFormat_MJPG : &MFVideoFormat_YUY2);
    IMFMediaType_SetUINT64(type, &MF_MT_FRAME_SIZE, (UINT64)f->width << 32 | f->height);
    IMFMediaType_SetUINT64(type, &MF_MT_FRAME_RATE, (UINT64)f->fps << 32 | 1);
    IMFMediaType_SetUINT64(type, &MF_MT_FRAME_RATE_RANGE_MIN, (UINT64)f->fps << 32 | 1);
    IMFMediaType_SetUINT64(type, &MF_MT_FRAME_RATE_RANGE_MAX, (UINT64)f->fps << 32 | 1);
    IMFMediaType_SetUINT64(type, &MF_MT_PIXEL_ASPECT_RATIO, (UINT64)1 << 32 | 1);
    IMFMediaType_SetUINT32(type, &MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    IMFMediaType_SetUINT32(type, &MF_MT_ALL_SAMPLES_INDEPENDENT, 1);
    if (!jpeg) {
        IMFMediaType_SetUINT32(type, &MF_MT_DEFAULT_STRIDE, f->width * 2);
        IMFMediaType_SetUINT32(type, &MF_MT_SAMPLE_SIZE, f->width * f->height * 2);
        IMFMediaType_SetUINT32(type, &MF_MT_FIXED_SIZE_SAMPLES, 1);
    }
    *out = type;
    return S_OK;
}

/* ---------------------------------------------------------------------------------------------------------
 * The source reader.
 */
struct reader {
    IMFSourceReader IMFSourceReader_iface;
    LONG refs;
    struct source *source;
    IMFSourceReaderCallback *callback; /* asynchronous mode when set */
    CRITICAL_SECTION lock;
    HANDLE wake, thread;
    UINT32 current;
    BOOL streaming, ticked, quit, flush;
    LONG pending;
    BYTE *frame; /* the newest frame not yet handed on */
    DWORD frame_length, frame_size;
    LONGLONG frame_time;
    BOOL have_frame;
};

static struct reader *reader_from_iface(IMFSourceReader *iface)
{
    return CONTAINING_RECORD(iface, struct reader, IMFSourceReader_iface);
}

/* Windows stamps a camera's samples with the system's running time (what MFGetSystemTime returns) at the
 * moment a frame has arrived, not with time counted from the first frame, and Leia's tracker compares the
 * stamp with the clock to know how old a picture is: its setting for the camera's delay (about 21 ms from
 * the scene to the stamp) is tuned to that. The kernel's own stamp, which the bridge sends along, marks the
 * start of the frame and is a whole frame time (16 ms) earlier; stamped with it, every picture looks 16 ms
 * older than it does on Windows and the tracker predicts the eyes that much too far ahead, which shows as
 * flicker while the head moves. So the stamp is the time of arrival here. */
static LONGLONG capture_time(UINT64 time_us)
{
    LARGE_INTEGER count, frequency;

    (void)time_us;
    QueryPerformanceCounter(&count);
    QueryPerformanceFrequency(&frequency);
    return (LONGLONG)((double)count.QuadPart * 10000000.0 / (double)frequency.QuadPart);
}

/* Called on the bridge's thread: keep the newest frame and let the reader's own thread deliver it, so a
 * callback is free to send the bridge commands. */
static void reader_frame(void *ctx, UINT64 time_us, const BYTE *data, DWORD length)
{
    struct reader *r = ctx;

    EnterCriticalSection(&r->lock);
    if (length > r->frame_size) {
        BYTE *bigger = realloc(r->frame, length);
        if (bigger) {
            r->frame = bigger;
            r->frame_size = length;
        }
    }
    if (length <= r->frame_size) {
        memcpy(r->frame, data, length);
        r->frame_length = length;
        r->frame_time = capture_time(time_us);
        r->have_frame = TRUE;
    }
    LeaveCriticalSection(&r->lock);
    SetEvent(r->wake);
}

/* Take the newest frame as a sample. Call with the lock held. */
static IMFSample *reader_take_sample(struct reader *r, LONGLONG *time)
{
    IMFMediaBuffer *buffer;
    IMFSample *sample = NULL;
    BYTE *bytes;

    r->have_frame = FALSE;
    *time = r->frame_time;
    if (FAILED(MFCreateMemoryBuffer(r->frame_length, &buffer))) return NULL;
    if (SUCCEEDED(IMFMediaBuffer_Lock(buffer, &bytes, NULL, NULL))) {
        memcpy(bytes, r->frame, r->frame_length);
        IMFMediaBuffer_Unlock(buffer);
        IMFMediaBuffer_SetCurrentLength(buffer, r->frame_length);
        if (SUCCEEDED(MFCreateSample(&sample))) {
            IMFSample_AddBuffer(sample, buffer);
            IMFSample_SetSampleTime(sample, *time);
            IMFSample_SetSampleDuration(sample, 10000000 / (r->source->formats[r->current].fps ?: 30));
        }
    }
    IMFMediaBuffer_Release(buffer);
    return sample;
}

#endif /* production types and sample handling */

#ifndef A2O_CAMERA_STREAM_TEST
static void reader_free(struct reader *r)
{
    CloseHandle(r->thread);
    CloseHandle(r->wake);
    if (r->callback) IMFSourceReaderCallback_Release(r->callback);
    source_Release(&r->source->IMFMediaSource_iface);
    DeleteCriticalSection(&r->lock);
    free(r->frame);
    free(r);
    say("reader: released");
}

static DWORD WINAPI reader_thread(void *arg)
{
    struct reader *r = arg;

    for (;;) {
        IMFSample *sample = NULL;
        LONGLONG time = 0;
        BOOL tick = FALSE, deliver = FALSE, flushed;

        WaitForSingleObject(r->wake, INFINITE);
        EnterCriticalSection(&r->lock);
        if (r->quit) {
            LeaveCriticalSection(&r->lock);
            /* The thread owns final cleanup: a caller's wait may have timed
             * out while OnReadSample or OnFlush was still using this reader. */
            reader_free(r);
            break;
        }
        flushed = r->flush;
        r->flush = FALSE;
        if (!flushed && r->callback && r->have_frame && r->pending > 0) {
            r->pending--;
            deliver = TRUE;
            if (!r->ticked) { /* Windows starts a live stream with a tick and no sample */
                r->ticked = tick = TRUE;
            } else {
                sample = reader_take_sample(r, &time);
            }
        }
        LeaveCriticalSection(&r->lock);
        if (flushed) IMFSourceReaderCallback_OnFlush(r->callback, 0);
        if (deliver) {
            IMFSourceReaderCallback_OnReadSample(r->callback, sample || tick ? S_OK : E_OUTOFMEMORY, 0,
                                                 tick ? MF_SOURCE_READERF_STREAMTICK : 0, time, sample);
            if (sample) IMFSample_Release(sample);
        }
    }
    return 0;
}

static HRESULT reader_launch(struct reader *r)
{
    r->wake = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!r->wake) return HRESULT_FROM_WIN32(GetLastError());
    r->thread = CreateThread(NULL, 0, reader_thread, r, 0, NULL);
    if (!r->thread) return HRESULT_FROM_WIN32(GetLastError());
    return S_OK;
}

#endif /* !A2O_CAMERA_STREAM_TEST */

/* Also compiled by the native lifecycle tests with a simulated bridge. */
#ifndef A2O_CAMERA_READER_TEST
static HRESULT reader_start(struct reader *r)
{
    const struct format *f = &r->source->formats[r->current];

    if (r->streaming) return S_OK;
    r->have_frame = FALSE;
    bridge_set_frames(r->source->bridge, reader_frame, r);
    if (!bridge_command(r->source->bridge, NULL, 0, "F %s %u %u %u", f->fourcc, f->width, f->height, f->fps)) {
        /* A timed-out command can still start the camera later. This reader
         * never became streaming, so reader_stop() will not detach it on
         * release. Drain any current callback and remove its borrowed pointer. */
        bridge_set_frames(r->source->bridge, NULL, NULL);
        say("reader: the camera would not start %s %ux%u@%u", f->fourcc, f->width, f->height, f->fps);
        return MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED;
    }
    say("reader: streaming %s %ux%u@%u", f->fourcc, f->width, f->height, f->fps);
    r->streaming = TRUE;
    return S_OK;
}

static void reader_stop(struct reader *r)
{
    if (!r->streaming) return;
    bridge_command(r->source->bridge, NULL, 0, "S");
    bridge_set_frames(r->source->bridge, NULL, NULL);
    r->streaming = FALSE;
    say("reader: stopped");
}
#endif /* !A2O_CAMERA_READER_TEST */

#if !defined(A2O_CAMERA_STREAM_TEST) && !defined(A2O_CAMERA_READER_TEST)
static BOOL is_video_stream(DWORD index)
{
    return index == 0 || index == (DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM;
}

static HRESULT WINAPI reader_QueryInterface(IMFSourceReader *iface, REFIID riid, void **out)
{
    if (IsEqualGUID(riid, &IID_IUnknown) || IsEqualGUID(riid, &IID_IMFSourceReader)) {
        IMFSourceReader_AddRef(iface);
        *out = iface;
        return S_OK;
    }
    say("reader: no interface %s", guid_text(riid));
    *out = NULL;
    return E_NOINTERFACE;
}

static ULONG WINAPI reader_AddRef(IMFSourceReader *iface)
{
    return InterlockedIncrement(&reader_from_iface(iface)->refs);
}
#endif /* production COM methods */

#ifndef A2O_CAMERA_STREAM_TEST
static ULONG WINAPI reader_Release(IMFSourceReader *iface)
{
    struct reader *r = reader_from_iface(iface);
    ULONG refs = InterlockedDecrement(&r->refs);

    if (!refs) {
        HANDLE thread = NULL;

        reader_stop(r);
        /* reader_free closes the original handle on the reader thread. Hold
         * a separate handle for this wait, and never wait from a callback.
         * If duplication fails, cleanup can still finish asynchronously. */
        if (GetCurrentThreadId() != GetThreadId(r->thread) &&
            !DuplicateHandle(GetCurrentProcess(), r->thread, GetCurrentProcess(), &thread, 0, FALSE,
                             DUPLICATE_SAME_ACCESS))
            thread = NULL;
        EnterCriticalSection(&r->lock);
        r->quit = TRUE;
        /* Signal before releasing the lock: the thread may free r as soon
         * as it sees quit. Nothing below the unlock may access r again. */
        SetEvent(r->wake);
        LeaveCriticalSection(&r->lock);
        if (thread) {
            WaitForSingleObject(thread, 5000);
            CloseHandle(thread);
        }
    }
    return refs;
}
#endif /* !A2O_CAMERA_STREAM_TEST */

#if !defined(A2O_CAMERA_STREAM_TEST) && !defined(A2O_CAMERA_READER_TEST)
static HRESULT WINAPI reader_GetStreamSelection(IMFSourceReader *iface, DWORD index, BOOL *selected)
{
    say("reader: GetStreamSelection %#lx", index);
    if (!is_video_stream(index)) return MF_E_INVALIDSTREAMNUMBER;
    *selected = TRUE;
    return S_OK;
}

static HRESULT WINAPI reader_SetStreamSelection(IMFSourceReader *iface, DWORD index, BOOL selected)
{
    say("reader: SetStreamSelection %#lx %d", index, selected);
    return is_video_stream(index) || index == (DWORD)MF_SOURCE_READER_ALL_STREAMS ? S_OK : MF_E_INVALIDSTREAMNUMBER;
}

static HRESULT WINAPI reader_GetNativeMediaType(IMFSourceReader *iface, DWORD index, DWORD type_index,
                                                IMFMediaType **type)
{
    struct reader *r = reader_from_iface(iface);

    if (!is_video_stream(index)) return MF_E_INVALIDSTREAMNUMBER;
    if (type_index == (DWORD)MF_SOURCE_READER_CURRENT_TYPE_INDEX) type_index = r->current;
    say("reader: GetNativeMediaType %lu", type_index);
    if (type_index >= r->source->format_count) return MF_E_NO_MORE_TYPES;
    return format_to_type(&r->source->formats[type_index], type);
}

static HRESULT WINAPI reader_GetCurrentMediaType(IMFSourceReader *iface, DWORD index, IMFMediaType **type)
{
    struct reader *r = reader_from_iface(iface);
    say("reader: GetCurrentMediaType %#lx", index);
    if (!is_video_stream(index)) return MF_E_INVALIDSTREAMNUMBER;
    return format_to_type(&r->source->formats[r->current], type);
}

static HRESULT WINAPI reader_SetCurrentMediaType(IMFSourceReader *iface, DWORD index, DWORD *reserved,
                                                 IMFMediaType *type)
{
    struct reader *r = reader_from_iface(iface);
    GUID subtype = { 0 };
    UINT64 size = 0, rate = 0;
    UINT32 want_fps, best = ~0u;
    BOOL was_streaming;

    if (!is_video_stream(index)) return MF_E_INVALIDSTREAMNUMBER;
    IMFMediaType_GetGUID(type, &MF_MT_SUBTYPE, &subtype);
    IMFMediaType_GetUINT64(type, &MF_MT_FRAME_SIZE, &size);
    IMFMediaType_GetUINT64(type, &MF_MT_FRAME_RATE, &rate);
    want_fps = (UINT32)rate ? (UINT32)((rate >> 32) / (UINT32)rate) : 0;
    say("reader: SetCurrentMediaType subtype %s %ux%u@%u", guid_text(&subtype), (UINT32)(size >> 32), (UINT32)size,
        want_fps);
    for (UINT32 i = 0; i < r->source->format_count; i++) {
        const struct format *f = &r->source->formats[i];
        const GUID *g = !strcmp(f->fourcc, "MJPG") ? &MFVideoFormat_MJPG : &MFVideoFormat_YUY2;
        if (!IsEqualGUID(g, &subtype)) continue;
        if (size && (f->width != (UINT32)(size >> 32) || f->height != (UINT32)size)) continue;
        if (best == ~0u || (want_fps && f->fps == want_fps)) best = i;
    }
    if (best == ~0u) {
        say("reader: the camera has no such format (this stand-in does not convert)");
        return MF_E_INVALIDMEDIATYPE;
    }
    EnterCriticalSection(&r->lock);
    was_streaming = r->streaming;
    LeaveCriticalSection(&r->lock);
    if (was_streaming && best != r->current) reader_stop(r);
    r->current = best;
    return was_streaming ? reader_start(r) : S_OK;
}

static HRESULT WINAPI reader_SetCurrentPosition(IMFSourceReader *iface, REFGUID format, REFPROPVARIANT position)
{
    return S_OK;
}

static HRESULT WINAPI reader_ReadSample(IMFSourceReader *iface, DWORD index, DWORD control, DWORD *actual,
                                        DWORD *flags, LONGLONG *time, IMFSample **sample)
{
    struct reader *r = reader_from_iface(iface);
    HRESULT hr;

    if (!r->streaming) say("reader: first ReadSample %#lx", index);
    if (!is_video_stream(index) && index != (DWORD)MF_SOURCE_READER_ANY_STREAM) return MF_E_INVALIDSTREAMNUMBER;
    if (FAILED(hr = reader_start(r))) return hr;

    if (r->callback) {
        if (actual || flags || time || sample) return E_INVALIDARG;
        InterlockedIncrement(&r->pending);
        SetEvent(r->wake);
        return S_OK;
    }

    if (!sample) return E_POINTER;
    *sample = NULL;
    for (int waited = 0; waited < 500; waited++) { /* at most five seconds */
        EnterCriticalSection(&r->lock);
        if (r->have_frame) {
            LONGLONG t;
            *sample = reader_take_sample(r, &t);
            LeaveCriticalSection(&r->lock);
            if (actual) *actual = 0;
            if (flags) *flags = 0;
            if (time) *time = t;
            return *sample ? S_OK : E_OUTOFMEMORY;
        }
        LeaveCriticalSection(&r->lock);
        if (r->source->bridge->dead) break;
        Sleep(10);
    }
    if (actual) *actual = 0;
    if (flags) *flags = MF_SOURCE_READERF_ERROR;
    return MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED;
}

static HRESULT WINAPI reader_Flush(IMFSourceReader *iface, DWORD index)
{
    struct reader *r = reader_from_iface(iface);

    say("reader: Flush %#lx", index);
    EnterCriticalSection(&r->lock);
    r->pending = 0;
    r->have_frame = FALSE;
    if (r->callback) r->flush = TRUE;
    LeaveCriticalSection(&r->lock);
    SetEvent(r->wake);
    return S_OK;
}

static HRESULT WINAPI reader_GetServiceForStream(IMFSourceReader *iface, DWORD index, REFGUID service, REFIID riid,
                                                 void **out)
{
    struct reader *r = reader_from_iface(iface);

    if (index == (DWORD)MF_SOURCE_READER_MEDIASOURCE)
        return source_QueryInterface(&r->source->IMFMediaSource_iface, riid, out);
    say("reader: GetServiceForStream %#lx %s %s (not provided)", index, guid_text(service), guid_text(riid));
    *out = NULL;
    return MF_E_UNSUPPORTED_SERVICE;
}

static HRESULT WINAPI reader_GetPresentationAttribute(IMFSourceReader *iface, DWORD index, REFGUID attribute,
                                                      PROPVARIANT *value)
{
    if (index == (DWORD)MF_SOURCE_READER_MEDIASOURCE &&
        IsEqualGUID(attribute, &MF_SOURCE_READER_MEDIASOURCE_CHARACTERISTICS)) {
        value->vt = VT_UI4;
        value->ulVal = MFMEDIASOURCE_IS_LIVE;
        return S_OK;
    }
    say("reader: GetPresentationAttribute %#lx %s (not provided)", index, guid_text(attribute));
    return MF_E_ATTRIBUTENOTFOUND;
}

static const IMFSourceReaderVtbl reader_vtbl = {
    reader_QueryInterface, reader_AddRef, reader_Release, reader_GetStreamSelection, reader_SetStreamSelection,
    reader_GetNativeMediaType, reader_GetCurrentMediaType, reader_SetCurrentMediaType, reader_SetCurrentPosition,
    reader_ReadSample, reader_Flush, reader_GetServiceForStream, reader_GetPresentationAttribute,
};

typedef HRESULT (WINAPI *create_reader_fn)(IMFMediaSource *, IMFAttributes *, IMFSourceReader **);

HRESULT WINAPI A2OCreateSourceReaderFromMediaSource(IMFMediaSource *source, IMFAttributes *attributes,
                                                    IMFSourceReader **out)
{
    IMFMediaSource *ours;
    struct reader *r;
    HRESULT hr;

    if (!source || FAILED(IMFMediaSource_QueryInterface(source, &IID_A2OSource, (void **)&ours))) {
        HMODULE real = LoadLibraryW(L"mfreadww.dll"); /* Wine's own, under the name setup.sh gave it */
        create_reader_fn create = real ? (create_reader_fn)GetProcAddress(real, "MFCreateSourceReaderFromMediaSource") : NULL;
        return create ? create(source, attributes, out) : E_NOTIMPL;
    }
    if (!(r = calloc(1, sizeof *r))) {
        IMFMediaSource_Release(ours);
        return E_OUTOFMEMORY;
    }
    r->IMFSourceReader_iface.lpVtbl = (IMFSourceReaderVtbl *)&reader_vtbl;
    r->refs = 1;
    r->source = source_from_IMFMediaSource(ours); /* keeps the reference */
    if (attributes)
        IMFAttributes_GetUnknown(attributes, &MF_SOURCE_READER_ASYNC_CALLBACK, &IID_IMFSourceReaderCallback,
                                 (void **)&r->callback);
    InitializeCriticalSection(&r->lock);
    if (FAILED(hr = reader_launch(r))) {
        reader_free(r); /* No thread started, so this caller still owns cleanup. */
        return hr;
    }
    say("reader: created, %s", r->callback ? "asynchronous" : "synchronous");
    *out = &r->IMFSourceReader_iface;
    return S_OK;
}

/* ---------------------------------------------------------------------------------------------------------
 * The activation object MFEnumDeviceSources hands out: an attribute store (Wine's) plus ActivateObject.
 */
struct activate {
    IMFActivate IMFActivate_iface;
    LONG refs;
    IMFAttributes *attributes;
    struct source *source;
};

static struct activate *activate_from_iface(IMFActivate *iface)
{
    return CONTAINING_RECORD(iface, struct activate, IMFActivate_iface);
}

static HRESULT WINAPI activate_QueryInterface(IMFActivate *iface, REFIID riid, void **out)
{
    if (IsEqualGUID(riid, &IID_IUnknown) || IsEqualGUID(riid, &IID_IMFAttributes) ||
        IsEqualGUID(riid, &IID_IMFActivate)) {
        IMFActivate_AddRef(iface);
        *out = iface;
        return S_OK;
    }
    *out = NULL;
    return E_NOINTERFACE;
}

static ULONG WINAPI activate_AddRef(IMFActivate *iface)
{
    return InterlockedIncrement(&activate_from_iface(iface)->refs);
}

static ULONG WINAPI activate_Release(IMFActivate *iface)
{
    struct activate *a = activate_from_iface(iface);
    ULONG refs = InterlockedDecrement(&a->refs);
    if (!refs) {
        if (a->source) source_Release(&a->source->IMFMediaSource_iface);
        IMFAttributes_Release(a->attributes);
        free(a);
    }
    return refs;
}

#define A (activate_from_iface(iface)->attributes)
static HRESULT WINAPI activate_GetItem(IMFActivate *iface, REFGUID k, PROPVARIANT *v) { return IMFAttributes_GetItem(A, k, v); }
static HRESULT WINAPI activate_GetItemType(IMFActivate *iface, REFGUID k, MF_ATTRIBUTE_TYPE *t) { return IMFAttributes_GetItemType(A, k, t); }
static HRESULT WINAPI activate_CompareItem(IMFActivate *iface, REFGUID k, REFPROPVARIANT v, BOOL *r) { return IMFAttributes_CompareItem(A, k, v, r); }
static HRESULT WINAPI activate_Compare(IMFActivate *iface, IMFAttributes *o, MF_ATTRIBUTES_MATCH_TYPE t, BOOL *r) { return IMFAttributes_Compare(A, o, t, r); }
static HRESULT WINAPI activate_GetUINT32(IMFActivate *iface, REFGUID k, UINT32 *v) { return IMFAttributes_GetUINT32(A, k, v); }
static HRESULT WINAPI activate_GetUINT64(IMFActivate *iface, REFGUID k, UINT64 *v) { return IMFAttributes_GetUINT64(A, k, v); }
static HRESULT WINAPI activate_GetDouble(IMFActivate *iface, REFGUID k, double *v) { return IMFAttributes_GetDouble(A, k, v); }
static HRESULT WINAPI activate_GetGUID(IMFActivate *iface, REFGUID k, GUID *v) { return IMFAttributes_GetGUID(A, k, v); }
static HRESULT WINAPI activate_GetStringLength(IMFActivate *iface, REFGUID k, UINT32 *n) { return IMFAttributes_GetStringLength(A, k, n); }
static HRESULT WINAPI activate_GetString(IMFActivate *iface, REFGUID k, WCHAR *v, UINT32 size, UINT32 *n) { return IMFAttributes_GetString(A, k, v, size, n); }
static HRESULT WINAPI activate_GetAllocatedString(IMFActivate *iface, REFGUID k, WCHAR **v, UINT32 *n) { return IMFAttributes_GetAllocatedString(A, k, v, n); }
static HRESULT WINAPI activate_GetBlobSize(IMFActivate *iface, REFGUID k, UINT32 *n) { return IMFAttributes_GetBlobSize(A, k, n); }
static HRESULT WINAPI activate_GetBlob(IMFActivate *iface, REFGUID k, UINT8 *b, UINT32 size, UINT32 *n) { return IMFAttributes_GetBlob(A, k, b, size, n); }
static HRESULT WINAPI activate_GetAllocatedBlob(IMFActivate *iface, REFGUID k, UINT8 **b, UINT32 *n) { return IMFAttributes_GetAllocatedBlob(A, k, b, n); }
static HRESULT WINAPI activate_GetUnknown(IMFActivate *iface, REFGUID k, REFIID riid, void **out) { return IMFAttributes_GetUnknown(A, k, riid, out); }
static HRESULT WINAPI activate_SetItem(IMFActivate *iface, REFGUID k, REFPROPVARIANT v) { return IMFAttributes_SetItem(A, k, v); }
static HRESULT WINAPI activate_DeleteItem(IMFActivate *iface, REFGUID k) { return IMFAttributes_DeleteItem(A, k); }
static HRESULT WINAPI activate_DeleteAllItems(IMFActivate *iface) { return IMFAttributes_DeleteAllItems(A); }
static HRESULT WINAPI activate_SetUINT32(IMFActivate *iface, REFGUID k, UINT32 v) { return IMFAttributes_SetUINT32(A, k, v); }
static HRESULT WINAPI activate_SetUINT64(IMFActivate *iface, REFGUID k, UINT64 v) { return IMFAttributes_SetUINT64(A, k, v); }
static HRESULT WINAPI activate_SetDouble(IMFActivate *iface, REFGUID k, double v) { return IMFAttributes_SetDouble(A, k, v); }
static HRESULT WINAPI activate_SetGUID(IMFActivate *iface, REFGUID k, REFGUID v) { return IMFAttributes_SetGUID(A, k, v); }
static HRESULT WINAPI activate_SetString(IMFActivate *iface, REFGUID k, const WCHAR *v) { return IMFAttributes_SetString(A, k, v); }
static HRESULT WINAPI activate_SetBlob(IMFActivate *iface, REFGUID k, const UINT8 *b, UINT32 n) { return IMFAttributes_SetBlob(A, k, b, n); }
static HRESULT WINAPI activate_SetUnknown(IMFActivate *iface, REFGUID k, IUnknown *u) { return IMFAttributes_SetUnknown(A, k, u); }
static HRESULT WINAPI activate_LockStore(IMFActivate *iface) { return IMFAttributes_LockStore(A); }
static HRESULT WINAPI activate_UnlockStore(IMFActivate *iface) { return IMFAttributes_UnlockStore(A); }
static HRESULT WINAPI activate_GetCount(IMFActivate *iface, UINT32 *n) { return IMFAttributes_GetCount(A, n); }
static HRESULT WINAPI activate_GetItemByIndex(IMFActivate *iface, UINT32 i, GUID *k, PROPVARIANT *v) { return IMFAttributes_GetItemByIndex(A, i, k, v); }
static HRESULT WINAPI activate_CopyAllItems(IMFActivate *iface, IMFAttributes *dest) { return IMFAttributes_CopyAllItems(A, dest); }
#undef A

static HRESULT WINAPI activate_ActivateObject(IMFActivate *iface, REFIID riid, void **out)
{
    struct activate *a = activate_from_iface(iface);
    HRESULT hr;

    say("activate: ActivateObject %s", guid_text(riid));
    *out = NULL;
    if (!a->source && FAILED(hr = source_create(&a->source))) return hr;
    return source_QueryInterface(&a->source->IMFMediaSource_iface, riid, out);
}

static HRESULT WINAPI activate_ShutdownObject(IMFActivate *iface)
{
    struct activate *a = activate_from_iface(iface);

    say("activate: ShutdownObject");
    if (a->source) source_Release(&a->source->IMFMediaSource_iface);
    a->source = NULL;
    return S_OK;
}

static HRESULT WINAPI activate_DetachObject(IMFActivate *iface)
{
    struct activate *a = activate_from_iface(iface);

    if (a->source) source_Release(&a->source->IMFMediaSource_iface);
    a->source = NULL;
    return S_OK;
}

static const IMFActivateVtbl activate_vtbl = {
    activate_QueryInterface, activate_AddRef, activate_Release, activate_GetItem, activate_GetItemType,
    activate_CompareItem, activate_Compare, activate_GetUINT32, activate_GetUINT64, activate_GetDouble,
    activate_GetGUID, activate_GetStringLength, activate_GetString, activate_GetAllocatedString,
    activate_GetBlobSize, activate_GetBlob, activate_GetAllocatedBlob, activate_GetUnknown, activate_SetItem,
    activate_DeleteItem, activate_DeleteAllItems, activate_SetUINT32, activate_SetUINT64, activate_SetDouble,
    activate_SetGUID, activate_SetString, activate_SetBlob, activate_SetUnknown, activate_LockStore,
    activate_UnlockStore, activate_GetCount, activate_GetItemByIndex, activate_CopyAllItems,
    activate_ActivateObject, activate_DetachObject, activate_ShutdownObject,
};

typedef HRESULT (WINAPI *enum_sources_fn)(IMFAttributes *, IMFActivate ***, UINT32 *);

HRESULT WINAPI A2OEnumDeviceSources(IMFAttributes *attributes, IMFActivate ***sources, UINT32 *count)
{
    WCHAR link[512], name[256];
    struct activate *a;
    GUID kind = { 0 };

    if (!attributes || !sources || !count) return E_INVALIDARG;
    IMFAttributes_GetGUID(attributes, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &kind);
    if (!IsEqualGUID(&kind, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)) {
        HMODULE real = LoadLibraryW(L"mfw.dll"); /* Wine's own, under the name setup.sh gave it */
        enum_sources_fn list = real ? (enum_sources_fn)GetProcAddress(real, "MFEnumDeviceSources") : NULL;
        return list ? list(attributes, sources, count) : E_NOTIMPL;
    }
    *sources = NULL;
    *count = 0;
    if (!config_string(L"SymbolicLink", link, sizeof link)) {
        say("enum: no camera is described in the registry");
        return S_OK;
    }
    if (!config_string(L"FriendlyName", name, sizeof name)) wcscpy(name, L"Camera");
    if (!(a = calloc(1, sizeof *a))) return E_OUTOFMEMORY;
    a->IMFActivate_iface.lpVtbl = (IMFActivateVtbl *)&activate_vtbl;
    a->refs = 1;
    if (FAILED(MFCreateAttributes(&a->attributes, 3)) || !(*sources = CoTaskMemAlloc(sizeof **sources))) {
        if (a->attributes) IMFAttributes_Release(a->attributes);
        free(a);
        return E_OUTOFMEMORY;
    }
    IMFAttributes_SetGUID(a->attributes, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                          &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID);
    IMFAttributes_SetString(a->attributes, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, name);
    IMFAttributes_SetString(a->attributes, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, link);
    (*sources)[0] = &a->IMFActivate_iface;
    *count = 1;
    say("enum: one camera, %ls", link);
    return S_OK;
}
#endif /* production COM and activation methods */
