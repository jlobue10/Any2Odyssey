/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-weaver.exe: shows a stereo picture on the Odyssey 3D through Leia's own weaver, under Wine.
 *
 * Leia's weaver (SimulatedRealityOpenGL.dll, part of the SR runtime the user copied from Windows) takes a
 * side-by-side texture and interleaves the two views for the lens, using the panel's factory calibration and
 * the eye positions Leia's eye tracker reports. This program is the small Windows side of that: it opens a
 * full-screen window on the Odyssey, asks for the lens, and weaves whatever picture the native program has
 * put into a shared file. Nothing of Leia's is linked: the libraries are found at run time.
 *
 * usage: any2odyssey-weaver.exe <frame file> <x> <y> <width> <height> [overlay]
 *   <frame file>   the shared picture (layout below), as a Windows path; the native program creates it
 *   <x> ... <height>  where the Odyssey 3D is on Wine's desktop
 *   overlay        the window lets the pointer through and never takes the keyboard (for a live desktop)
 *
 * Contract with the program that starts it, through the standard streams (as any2odyssey-lens-helper):
 *   prints "on" once the first picture has been woven with the lens asked for, or "error: <reason>" and exits;
 *   prints "eyes <0|1>" when eye tracking is lost or found;
 *   leaves (lens off) when its standard input closes, or when Esc is pressed in its window.
 *
 * The frame file: a 4096-byte header, then three places for a picture, each `capacity` bytes. A picture is
 * RGBA (8 bits each), rows from the bottom up as OpenGL takes them, the left view in the left half and the
 * right view in the right half. The writer goes round the three places and then points the header at the one
 * it has finished, so the picture being read here is not the one being written unless reading it takes
 * longer than the writer needs for two more (it is then read again).
 *   offset 0   char[8]  "A2OWEAV2"
 *          8   uint32   capacity: bytes of each place
 *         12   uint32   sequence: odd while the writer is changing the header, raised by 2 per picture
 *         16   uint32   width of the whole side-by-side picture, in pixels
 *         20   uint32   height
 *         24   uint32   the place (0 to 2) the newest picture is in
 */
#include <windows.h>
#include <GL/gl.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void *a2o_weaver_create(void *create, void *context, void *window, int *error);
void a2o_weaver_input(void *w, unsigned texture, int width, int height, unsigned format);
void a2o_weaver_weave(void *w);
void a2o_weaver_eyes(void *w, float *left, float *right);
void a2o_weaver_destroy(void *w);

#define GL_RGBA8 0x8058
#define GL_CLAMP_TO_EDGE 0x812F
#define HEADER_BYTES 4096
#define CREATE_GL_WEAVER "?CreateGLWeaver@SR@@YA?AW4WeaverErrorCode@@AEAVSRContext@1@PEAUHWND__@@PEAPEAVIGLWeaver1@1@@Z"

/* The header as the file holds it. Another program changes it while this one reads, so every field is
 * read once, through a volatile pointer, and checked before it is used. */
struct frame_header {
    char magic[8];
    UINT32 capacity;
    LONG sequence;
    UINT32 width, height, place;
};
#define PLACES 3

static volatile LONG quit;

static void fail(const char *reason)
{
    printf("error: %s\n", reason);
    fflush(stdout);
    ExitProcess(1);
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if ((message == WM_KEYDOWN && wparam == VK_ESCAPE) || message == WM_CLOSE) {
        InterlockedExchange(&quit, 1);
        return 0;
    }
    return DefWindowProcA(window, message, wparam, lparam);
}

/* The starting program closes our input when the picture is no longer wanted (or when it dies). */
static DWORD WINAPI watch_input(void *unused)
{
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE);
    char buffer[64];
    DWORD got;
    (void)unused;
    while (ReadFile(in, buffer, sizeof buffer, &got, NULL) && got) {}
    InterlockedExchange(&quit, 1);
    return 0;
}

/* Wine lists a monitor it has lost track of with a null handle, and Leia's display code gives up on the
 * whole list when it meets one. Leave such entries out of what Leia's libraries see: their imports of
 * EnumDisplayMonitors, in this process only, go through here. */
static MONITORENUMPROC their_callback;
static BOOL CALLBACK skip_null_monitors(HMONITOR monitor, HDC dc, RECT *rect, LPARAM param)
{
    return monitor ? their_callback(monitor, dc, rect, param) : TRUE;
}
static BOOL WINAPI enum_real_monitors(HDC dc, const RECT *clip, MONITORENUMPROC callback, LPARAM param)
{
    their_callback = callback; /* Leia's code enumerates from one thread at a time */
    return EnumDisplayMonitors(dc, clip, skip_null_monitors, param);
}
static void patch_import(HMODULE module, const char *dll, void *from, void *to)
{
    BYTE *base = (BYTE *)module;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + ((IMAGE_DOS_HEADER *)base)->e_lfanew);
    IMAGE_DATA_DIRECTORY dir = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!dir.VirtualAddress) return;
    for (IMAGE_IMPORT_DESCRIPTOR *d = (void *)(base + dir.VirtualAddress); d->Name; d++) {
        if (_stricmp((char *)base + d->Name, dll)) continue;
        for (void **slot = (void **)(base + d->FirstThunk); *slot; slot++) {
            DWORD old;
            if (*slot != from) continue;
            VirtualProtect(slot, sizeof *slot, PAGE_READWRITE, &old);
            *slot = to;
            VirtualProtect(slot, sizeof *slot, old, &old);
        }
    }
}

int main(int argc, char **argv)
{
    static const char *const sr_modules[] = { "SimulatedRealityDisplays.dll", "SimulatedRealityOpenGL.dll",
                                              "SimulatedRealityCore.dll", "SimulatedReality.dll" };
    PIXELFORMATDESCRIPTOR pfd = { sizeof pfd, 1, PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                                  PFD_TYPE_RGBA, 32 };
    WNDCLASSA wc = { .style = CS_OWNDC, .lpfnWndProc = window_proc, .lpszClassName = "any2odyssey-weaver" };
    const volatile struct frame_header *frame;
    const BYTE *pixels;
    UINT32 capacity;
    HANDLE file, mapping;
    LARGE_INTEGER file_size;
    HMODULE core, displays, opengl;
    void *(*new_context)(void);
    void (*initialize_context)(void *);
    void *(*create_lens_hint)(void *);
    void (*lens_enable)(void *), (*lens_disable)(void *);
    void *create_weaver, *context, *lens, *weaver;
    int x, y, w, h, error = -1, eyes_found = -1;
    bool overlay, announced = false;
    UINT32 tex_w = 0, tex_h = 0;
    LONG shown = 0;
    DWORD last_eye_check = 0;
    GLuint texture;
    FILE *stats;
    LARGE_INTEGER qpf, last_turn, stats_since, before_upload, after_upload, after_swap;
    BOOL (WINAPI *swap_interval)(int);
    double worst_ms = 0, worst_upload_ms = 0;
    unsigned turns = 0, uploads = 0;
    bool uploaded;
    HWND window;
    HDC dc;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 6) fail("usage: any2odyssey-weaver.exe <frame file> <x> <y> <width> <height> [overlay]");
    x = atoi(argv[2]), y = atoi(argv[3]), w = atoi(argv[4]), h = atoi(argv[5]);
    overlay = argc > 6 && !strcmp(argv[6], "overlay");
    if (w <= 0 || h <= 0) fail("the Odyssey 3D's place on the desktop is not known");

    file = CreateFileA(argv[1], GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    if (file == INVALID_HANDLE_VALUE || !GetFileSizeEx(file, &file_size)) fail("cannot open the frame file");
    mapping = CreateFileMappingA(file, NULL, PAGE_READONLY, 0, 0, NULL);
    frame = mapping ? MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) : NULL;
    /* The capacity is taken once, here, where it is checked against the file's size. */
    capacity = frame ? frame->capacity : 0;
    if (!frame || file_size.QuadPart < HEADER_BYTES || memcmp((const void *)frame->magic, "A2OWEAV2", 8) ||
        (ULONGLONG)capacity * PLACES > (ULONGLONG)file_size.QuadPart - HEADER_BYTES)
        fail("the frame file is not usable");
    pixels = (const BYTE *)frame + HEADER_BYTES;

    core = LoadLibraryA("SimulatedRealityCore.dll");
    displays = LoadLibraryA("SimulatedRealityDisplays.dll");
    opengl = LoadLibraryA("SimulatedRealityOpenGL.dll");
    if (!core || !displays || !opengl) fail("Leia's SR libraries were not found (is the runtime on the path?)");
    new_context = (void *)GetProcAddress(core, "newSRContext");
    initialize_context = (void *)GetProcAddress(core, "initializeSRContext");
    create_lens_hint = (void *)GetProcAddress(displays, "createSwitchableLensHint");
    lens_enable = (void *)GetProcAddress(displays, "lensEnableHint");
    lens_disable = (void *)GetProcAddress(displays, "lensDisableHint");
    create_weaver = (void *)GetProcAddress(opengl, CREATE_GL_WEAVER);
    if (!new_context || !initialize_context || !create_lens_hint || !lens_enable || !lens_disable || !create_weaver)
        fail("this version of Leia's SR runtime does not have the expected functions");
    for (unsigned i = 0; i < ARRAYSIZE(sr_modules); i++) {
        HMODULE module = GetModuleHandleA(sr_modules[i]);
        if (module)
            patch_import(module, "USER32.dll", GetProcAddress(GetModuleHandleA("user32.dll"), "EnumDisplayMonitors"),
                         enum_real_monitors);
    }

    wc.hInstance = GetModuleHandleA(NULL);
    wc.hCursor = overlay ? NULL : LoadCursorA(NULL, (const char *)IDC_ARROW);
    RegisterClassA(&wc);
    window = CreateWindowExA(WS_EX_TOPMOST | (overlay ? WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE : 0),
                             wc.lpszClassName, "Any2Odyssey 3D output", WS_POPUP, x, y, w, h, NULL, NULL,
                             wc.hInstance, NULL);
    if (!window) fail("cannot create the output window");
    if (overlay) SetLayeredWindowAttributes(window, 0, 255, LWA_ALPHA);
    ShowWindow(window, overlay ? SW_SHOWNOACTIVATE : SW_SHOW);
    dc = GetDC(window);
    if (!SetPixelFormat(dc, ChoosePixelFormat(dc, &pfd), &pfd) || !wglMakeCurrent(dc, wglCreateContext(dc)))
        fail("cannot start OpenGL in the output window");
    /* The loop below is paced by SwapBuffers waiting for the display. Ask for that wait where it can be
     * asked for, rather than rely on it being the default. */
    swap_interval = (BOOL (WINAPI *)(int))(void *)wglGetProcAddress("wglSwapIntervalEXT");
    if (swap_interval) swap_interval(1);

    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_2D, texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glPixelStorei(GL_UNPACK_ALIGNMENT, 1);

    context = new_context();
    if (!context) fail("no connection to Leia's SR service");
    lens = create_lens_hint(context);
    weaver = a2o_weaver_create(create_weaver, context, window, &error);
    if (!weaver) {
        char reason[96];
        snprintf(reason, sizeof reason, "Leia's weaver could not be created (code %d)", error);
        fail(reason);
    }
    initialize_context(context);
    if (lens) lens_enable(lens);
    CloseHandle(CreateThread(NULL, 0, watch_input, NULL, 0, NULL));

    stats = getenv("A2O_WEAVER_STATS") ? fopen(getenv("A2O_WEAVER_STATS"), "w") : NULL;
    QueryPerformanceFrequency(&qpf);
    QueryPerformanceCounter(&last_turn);
    stats_since = last_turn;
    while (!quit) {
        MSG message;
        LONG sequence = frame->sequence;
        DWORD now;

        while (PeekMessageA(&message, NULL, 0, 0, PM_REMOVE)) DispatchMessageA(&message);

        /* Take a new picture when the writer has pointed the header at one. The header is read again
         * afterwards: if it changed meanwhile, what was read may be a mixture and is looked at afresh on the
         * next turn. */
        uploaded = false;
        QueryPerformanceCounter(&before_upload);
        after_upload = before_upload;
        if (sequence != shown && !(sequence & 1)) {
            UINT32 fw = frame->width, fh = frame->height, place = frame->place;
            if (frame->sequence == sequence && fw && fh && fw <= 16384 && fh <= 16384 && place < PLACES &&
                (ULONGLONG)fw * fh * 4 <= capacity) {
                const BYTE *picture = pixels + (size_t)place * capacity;
                glBindTexture(GL_TEXTURE_2D, texture);
                if (fw != tex_w || fh != tex_h) {
                    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, fw, fh, 0, GL_RGBA, GL_UNSIGNED_BYTE, picture);
                    a2o_weaver_input(weaver, texture, fw, fh, GL_RGBA8);
                    tex_w = fw, tex_h = fh;
                } else {
                    glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, fw, fh, GL_RGBA, GL_UNSIGNED_BYTE, picture);
                }
                /* The writer comes back to a place two pictures later. If it has moved on that far while
                 * this one was being read, what was read may be part of the next: read again next turn. */
                shown = (ULONG)(frame->sequence - sequence) < 4 ? sequence : shown;
                uploaded = true;
                QueryPerformanceCounter(&after_upload);
            }
        }

        glViewport(0, 0, w, h);
        glClearColor(0, 0, 0, 1);
        glClear(GL_COLOR_BUFFER_BIT);
        if (tex_w) a2o_weaver_weave(weaver);
        SwapBuffers(dc);
        /* Where the swap does not wait (no such extension, or a driver told to ignore it) nothing paces the
         * loop, and it would keep a processor core and the GPU busy weaving the same picture thousands of
         * times a second. A turn that took under two milliseconds was not paced by a display, none
         * refreshes that fast: it gives a millisecond back. A paced turn is left as it is. */
        QueryPerformanceCounter(&after_swap);
        if (tex_w && (after_swap.QuadPart - before_upload.QuadPart) * 500 < qpf.QuadPart) Sleep(1);
        if (stats) { /* a diagnostic: how often the picture is woven, and the longest wait between two */
            LARGE_INTEGER t;
            double ms, upload_ms = (double)(after_upload.QuadPart - before_upload.QuadPart) * 1000.0 / qpf.QuadPart;
            QueryPerformanceCounter(&t);
            ms = (double)(t.QuadPart - last_turn.QuadPart) * 1000.0 / qpf.QuadPart;
            last_turn = t;
            if (ms > worst_ms) worst_ms = ms;
            if (upload_ms > worst_upload_ms) worst_upload_ms = upload_ms;
            turns++;
            if (uploaded) uploads++;
            if (t.QuadPart - stats_since.QuadPart >= qpf.QuadPart) {
                fprintf(stats, "weaves %u pictures %u longest turn %.1f ms longest upload %.1f ms\n", turns, uploads, worst_ms, worst_upload_ms);
                fflush(stats);
                turns = uploads = 0; worst_ms = worst_upload_ms = 0; stats_since = t;
            }
        }
        if (tex_w && !announced) {
            announced = true;
            printf("on\n");
        }

        now = GetTickCount();
        if (now - last_eye_check >= 250) {
            /* Without a viewer the tracker reports both eyes at one fixed point. */
            float l[3] = { 0 }, r[3] = { 0 };
            int found;
            last_eye_check = now;
            a2o_weaver_eyes(weaver, l, r);
            found = l[0] != r[0] || l[1] != r[1] || l[2] != r[2];
            if (found != eyes_found) printf("eyes %d\n", eyes_found = found);
        }
        if (!tex_w) Sleep(5);
    }

    if (lens) {
        lens_disable(lens);
        Sleep(300); /* let the request reach the service before the connection goes */
    }
    a2o_weaver_destroy(weaver);
    return 0;
}
