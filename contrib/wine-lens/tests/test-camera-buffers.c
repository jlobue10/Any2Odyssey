/* SPDX-License-Identifier: MIT
 * Exercise the production bridge's capture start/stop paths with a V4L2 stand-in.
 * Driver buffer allocation and user mappings have separate lifetimes. No camera is needed.
 */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <sys/mman.h>

static int test_ioctl(int fd, unsigned long request, ...);
static void *test_mmap(void *at, size_t length, int prot, int flags, int fd, off_t offset);
static int test_munmap(void *at, size_t length);

#define ioctl test_ioctl
#define mmap test_mmap
#define munmap test_munmap
#define main camera_bridge_main
#include "../src/camera-bridge.c"
#undef main
#undef ioctl
#undef mmap
#undef munmap

enum failure { NONE, ALLOCATION, ONE_BUFFER, QUERY, MAP, QUEUE, STREAM_ON };
static enum failure fail_at;
static unsigned fail_index, allocated, mapped, releases;
static void *mappings[NBUF];

static int fail(int error) { errno = error; return -1; }

static int test_ioctl(int fd, unsigned long request, ...)
{
    va_list ap;
    void *arg;
    assert(fd == cam);
    va_start(ap, request);
    arg = va_arg(ap, void *);
    va_end(ap);
    if (request == VIDIOC_S_FMT) {
        /* A driver may refuse format changes until its existing buffers are freed. */
        return allocated ? fail(EBUSY) : 0;
    }
    if (request == VIDIOC_S_PARM || request == VIDIOC_STREAMOFF) return 0;
    if (request == VIDIOC_REQBUFS) {
        struct v4l2_requestbuffers *req = arg;
        if (!req->count) {
            assert(!mapped); /* unmap before freeing the driver buffers */
            assert(allocated);
            allocated = 0;
            releases++;
        } else {
            assert(!allocated);
            if (fail_at == ALLOCATION) return fail(ENOMEM);
            req->count = allocated = fail_at == ONE_BUFFER ? 1 : NBUF;
        }
        return 0;
    }
    if (request == VIDIOC_QUERYBUF || request == VIDIOC_QBUF) {
        struct v4l2_buffer *b = arg;
        assert(b->index < allocated);
        if (b->index == fail_index &&
            ((request == VIDIOC_QUERYBUF && fail_at == QUERY) ||
             (request == VIDIOC_QBUF && fail_at == QUEUE))) return fail(EIO);
        b->length = 4096;
        b->m.offset = b->index * 4096;
        return 0;
    }
    assert(request == VIDIOC_STREAMON);
    return fail_at == STREAM_ON ? fail(EIO) : 0;
}

static void *test_mmap(void *at, size_t length, int prot, int flags, int fd, off_t offset)
{
    unsigned index = (unsigned)(offset / 4096);
    (void)at; (void)prot; (void)flags;
    assert(fd == cam && index < allocated && length == 4096 && !mappings[index]);
    if (fail_at == MAP && index == fail_index) { errno = ENOMEM; return MAP_FAILED; }
    mappings[index] = malloc(length);
    assert(mappings[index]);
    mapped++;
    return mappings[index];
}

static int test_munmap(void *at, size_t length)
{
    assert(length == 4096);
    for (unsigned i = 0; i < NBUF; i++) {
        if (mappings[i] == at) {
            free(at);
            mappings[i] = NULL;
            mapped--;
            return 0;
        }
    }
    assert(!"unmapped a buffer that was not mapped");
    return -1;
}

static void start_then_retry(enum failure failure, unsigned index)
{
    char line[] = "F MJPG 640 480 60";
    unsigned before = releases;
    fail_at = failure;
    fail_index = index;
    gone = 1; /* no socket is needed to collect the command's reply */
    command(line);
    if (failure == NONE) {
        assert(streaming && allocated == NBUF && mapped == NBUF);
        stop();
    } else {
        assert(!streaming);
    }
    assert(!allocated && !mapped && !nbufs);
    assert(releases == before + (failure != ALLOCATION));
    stop(); /* cleanup remains safe when called again */
    assert(releases == before + (failure != ALLOCATION));

    fail_at = NONE;
    command(line); /* retry on the same camera, without closing and reopening it */
    assert(streaming && allocated == NBUF && mapped == NBUF);
    stop();
    assert(!allocated && !mapped && !nbufs && !streaming);
}

int main(void)
{
    start_then_retry(ONE_BUFFER, 0);
    start_then_retry(ALLOCATION, 0);
    for (unsigned i = 0; i < NBUF; i++) {
        start_then_retry(QUERY, i);
        start_then_retry(MAP, i);
        start_then_retry(QUEUE, i);
    }
    start_then_retry(STREAM_ON, 0);
    start_then_retry(NONE, 0);
    puts("camera buffer cleanup and retry tests passed (16 cases)");
    return 0;
}
