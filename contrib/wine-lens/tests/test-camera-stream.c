/* SPDX-License-Identifier: MIT
 * Run the production reader start/stop paths with a simulated camera bridge.
 * A timed-out format command may still complete and deliver frames later.
 * No Wine, camera, or Media Foundation runtime is needed for these checks.
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int BOOL;
typedef int32_t HRESULT;
#define TRUE 1
#define FALSE 0
#define S_OK ((HRESULT)0)
#define MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED ((HRESULT)-1)
#define say(...) ((void)0)

typedef void (*frame_fn)(void *, uint64_t, const unsigned char *, uint32_t);
struct bridge {
    frame_fn on_frame;
    void *frame_ctx;
    BOOL accept_start;
    unsigned starts, stops;
};
struct format {
    char fourcc[5];
    uint32_t width, height, fps;
};
struct source {
    struct bridge *bridge;
    struct format formats[1];
};
struct reader {
    struct source *source;
    uint32_t current;
    BOOL streaming, have_frame;
    unsigned delivered;
};

static BOOL bridge_command(struct bridge *b, char *reply, uint32_t size, const char *fmt, ...)
{
    (void)reply;
    (void)size;
    if (fmt[0] == 'F') {
        b->starts++;
        return b->accept_start;
    }
    assert(!strcmp(fmt, "S"));
    b->stops++;
    return TRUE;
}

static void bridge_set_frames(struct bridge *b, frame_fn fn, void *ctx)
{
    b->on_frame = fn;
    b->frame_ctx = ctx;
}

static void reader_frame(void *ctx, uint64_t time, const unsigned char *data, uint32_t length)
{
    struct reader *r = ctx;
    (void)time;
    assert(length == 1);
    r->delivered += *data; /* ASan catches a callback into a freed reader here. */
}

#define A2O_CAMERA_STREAM_TEST
#ifndef A2O_CAMERA_STREAM_SOURCE
#define A2O_CAMERA_STREAM_SOURCE "../src/a2ocam.c"
#endif
#include A2O_CAMERA_STREAM_SOURCE

static void deliver_late_frame(struct bridge *b)
{
    const unsigned char data = 1;
    if (b->on_frame) b->on_frame(b->frame_ctx, 0, &data, 1);
}

static struct reader *new_reader(struct source *source)
{
    struct reader *r = calloc(1, sizeof *r);
    assert(r);
    r->source = source;
    return r;
}

int main(void)
{
    struct bridge bridge = {0};
    /* The source can outlive its reader: IMFActivate keeps a source reference. */
    struct source source = {&bridge, {{"MJPG", 640, 480, 60}}};
    struct reader *r = new_reader(&source);

    assert(reader_start(r) == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED);
    assert(!r->streaming);
    reader_stop(r); /* Release uses this, but a failed start never became streaming. */
    free(r);
    deliver_late_frame(&bridge);
    assert(!bridge.on_frame && !bridge.frame_ctx);
    assert(bridge.starts == 1 && bridge.stops == 0);

    /* A failed start may be retried on the same reader. */
    r = new_reader(&source);
    assert(reader_start(r) == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED);
    deliver_late_frame(&bridge);
    assert(r->delivered == 0);
    bridge.accept_start = TRUE;
    assert(reader_start(r) == S_OK && r->streaming);
    assert(reader_start(r) == S_OK && bridge.starts == 3);
    deliver_late_frame(&bridge);
    assert(r->delivered == 1);
    reader_stop(r);
    assert(!r->streaming && bridge.stops == 1);
    reader_stop(r);
    assert(bridge.stops == 1);
    free(r);
    deliver_late_frame(&bridge);
    assert(!bridge.on_frame && !bridge.frame_ctx);

    /* Restart a stopped reader and check that its frame callback is reinstalled. */
    r = new_reader(&source);
    assert(reader_start(r) == S_OK);
    reader_stop(r);
    assert(reader_start(r) == S_OK);
    deliver_late_frame(&bridge);
    assert(r->delivered == 1);
    reader_stop(r);
    free(r);
    assert(bridge.starts == 5 && bridge.stops == 3);
    puts("camera reader lifecycle tests passed");
    return 0;
}
