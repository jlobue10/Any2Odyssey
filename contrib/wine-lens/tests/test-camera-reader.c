/* SPDX-License-Identifier: MIT
 * Production reader thread/release paths with pthread-backed Windows handles.
 * A callback barrier makes a shutdown timeout deterministic without sleeping
 * five seconds. The real camera, bridge and Media Foundation are not loaded.
 */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef int BOOL;
typedef int32_t LONG, HRESULT;
typedef uint32_t DWORD, ULONG;
typedef int64_t LONGLONG;
#define WINAPI
#define TRUE 1
#define FALSE 0
#define S_OK 0
#define E_OUTOFMEMORY ((HRESULT)-1)
#define HRESULT_FROM_WIN32(code) (-(HRESULT)(code))
#define INFINITE UINT32_MAX
#define WAIT_OBJECT_0 0
#define WAIT_TIMEOUT 258
#define WAIT_FAILED UINT32_MAX
#define DUPLICATE_SAME_ACCESS 2
#define MF_SOURCE_READERF_STREAMTICK 0x100
#define MF_SOURCE_READERF_ERROR 0x1
#define MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED ((HRESULT)-2)
#define say(...) ((void)0)
typedef pthread_mutex_t CRITICAL_SECTION;
static void InitializeCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_init(m, NULL)); }
static void EnterCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_lock(m)); }
static void LeaveCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_unlock(m)); }
static void DeleteCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_destroy(m)); }
static LONG InterlockedDecrement(LONG *n) { return __atomic_sub_fetch(n, 1, __ATOMIC_SEQ_CST); }

struct event { pthread_mutex_t lock; pthread_cond_t changed; BOOL signaled; };
struct thread {
    pthread_t native;
    pthread_mutex_t lock;
    pthread_cond_t changed;
    BOOL done;
    DWORD id;
    DWORD (*run)(void *);
    void *arg;
};
struct handle { BOOL is_thread; atomic_bool open; void *object; };
typedef struct handle *HANDLE;
static struct event events[64];
static struct thread threads[32];
static struct handle handles[160];
static unsigned event_count, thread_count, handle_count;
static atomic_int live_handles;
static _Thread_local DWORD current_thread = 1;
static BOOL fail_duplicate;
static BOOL fail_event, fail_thread;
static DWORD forced_wait_result;
static unsigned waits;
static DWORD GetLastError(void) { return 8; }

static HANDLE handle(BOOL is_thread, void *object)
{
    assert(handle_count < 160);
    HANDLE h = &handles[handle_count++];
    h->is_thread = is_thread;
    h->object = object;
    atomic_store(&h->open, TRUE);
    atomic_fetch_add(&live_handles, 1);
    return h;
}
static BOOL CloseHandle(HANDLE h)
{
    if (!h) return FALSE;
    assert(atomic_exchange(&h->open, FALSE));
    atomic_fetch_sub(&live_handles, 1);
    return TRUE;
}
static HANDLE CreateEventW(void *security, BOOL manual, BOOL initial, const void *name)
{
    (void)security; (void)name;
    if (fail_event) return NULL;
    assert(!manual && event_count < 64);
    struct event *e = &events[event_count++];
    assert(!pthread_mutex_init(&e->lock, NULL));
    assert(!pthread_cond_init(&e->changed, NULL));
    e->signaled = initial;
    return handle(FALSE, e);
}
static BOOL SetEvent(HANDLE h)
{
    assert(h && atomic_load(&h->open) && !h->is_thread);
    struct event *e = h->object;
    assert(!pthread_mutex_lock(&e->lock));
    e->signaled = TRUE;
    assert(!pthread_cond_signal(&e->changed));
    assert(!pthread_mutex_unlock(&e->lock));
    return TRUE;
}
static void *run_thread(void *arg)
{
    struct thread *t = arg;
    current_thread = t->id;
    t->run(t->arg);
    assert(!pthread_mutex_lock(&t->lock));
    t->done = TRUE;
    assert(!pthread_cond_broadcast(&t->changed));
    assert(!pthread_mutex_unlock(&t->lock));
    return NULL;
}
static HANDLE CreateThread(void *security, size_t stack, DWORD (*run)(void *), void *arg, DWORD flags, void *id)
{
    (void)security; (void)stack; (void)flags; (void)id;
    if (fail_thread) return NULL;
    assert(thread_count < 32);
    struct thread *t = &threads[thread_count++];
    t->id = thread_count + 1;
    t->run = run;
    t->arg = arg;
    assert(!pthread_mutex_init(&t->lock, NULL));
    assert(!pthread_cond_init(&t->changed, NULL));
    HANDLE h = handle(TRUE, t);
    assert(!pthread_create(&t->native, NULL, run_thread, t));
    return h;
}
static DWORD WaitForSingleObject(HANDLE h, DWORD timeout)
{
    assert(h && atomic_load(&h->open));
    if (!h->is_thread) {
        struct event *e = h->object;
        assert(timeout == INFINITE);
        assert(!pthread_mutex_lock(&e->lock));
        while (!e->signaled) assert(!pthread_cond_wait(&e->changed, &e->lock));
        e->signaled = FALSE;
        assert(!pthread_mutex_unlock(&e->lock));
        return WAIT_OBJECT_0;
    }
    assert(timeout == 5000); /* Shutdown stays bounded for the releasing caller. */
    waits++;
    if (forced_wait_result) return forced_wait_result;
    struct thread *t = h->object;
    assert(current_thread != t->id); /* Never wait for the callback's own thread. */
    assert(!pthread_mutex_lock(&t->lock));
    while (!t->done) assert(!pthread_cond_wait(&t->changed, &t->lock));
    assert(!pthread_mutex_unlock(&t->lock));
    return WAIT_OBJECT_0;
}
static DWORD GetCurrentThreadId(void) { return current_thread; }
static DWORD GetThreadId(HANDLE h)
{
    assert(h && atomic_load(&h->open) && h->is_thread);
    return ((struct thread *)h->object)->id;
}
static HANDLE GetCurrentProcess(void) { return NULL; }
static BOOL DuplicateHandle(HANDLE process, HANDLE h, HANDLE target, HANDLE *copy,
                            DWORD access, BOOL inherit, DWORD options)
{
    (void)process; (void)target; (void)access; (void)inherit;
    assert(h && atomic_load(&h->open) && options == DUPLICATE_SAME_ACCESS);
    if (fail_duplicate) return FALSE;
    *copy = handle(h->is_thread, h->object);
    return TRUE;
}

typedef struct { int unused; } IMFSourceReader, IMFMediaSource, IMFSample;
struct reader;
typedef struct {
    struct reader *reader;
    BOOL release_self;
    BOOL free_running; /* count the calls and return at once, instead of waiting at the gate */
    atomic_int released, calls, errors, flushes;
} IMFSourceReaderCallback;
struct source { IMFMediaSource IMFMediaSource_iface; atomic_int released; };
struct reader {
    IMFSourceReader IMFSourceReader_iface;
    LONG refs;
    struct source *source;
    IMFSourceReaderCallback *callback;
    CRITICAL_SECTION lock;
    HANDLE wake, thread, arrived;
    BOOL streaming, ticked, quit, flush, lost;
    LONG pending;
    unsigned char *frame;
    BOOL have_frame;
};
static struct reader *reader_from_iface(IMFSourceReader *iface) { return (struct reader *)iface; }
static void reader_stop(struct reader *r) { r->streaming = FALSE; }
static void source_Release(IMFMediaSource *iface)
{
    struct source *s = (struct source *)iface;
    assert(atomic_fetch_add(&s->released, 1) == 0);
}
static void IMFSourceReaderCallback_Release(IMFSourceReaderCallback *c)
{
    assert(atomic_fetch_add(&c->released, 1) == 0);
}
static IMFSample *reader_take_sample(struct reader *r, LONGLONG *time)
{
    (void)r; (void)time;
    return NULL;
}
static void IMFSample_Release(IMFSample *sample) { (void)sample; }
static void IMFSourceReaderCallback_OnFlush(IMFSourceReaderCallback *c, DWORD stream);
static void IMFSourceReaderCallback_OnReadSample(IMFSourceReaderCallback *c, HRESULT hr, DWORD stream,
                                                DWORD flags, LONGLONG time, IMFSample *sample);

#define A2O_CAMERA_READER_TEST
#ifndef A2O_CAMERA_READER_SOURCE
#define A2O_CAMERA_READER_SOURCE "../src/a2ocam.c"
#endif
#include A2O_CAMERA_READER_SOURCE

static pthread_mutex_t gate_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t gate_changed = PTHREAD_COND_INITIALIZER;
static BOOL entered, allowed;

static void callback(IMFSourceReaderCallback *c)
{
    atomic_fetch_add(&c->calls, 1);
    if (c->free_running) return;
    if (c->release_self) {
        assert(reader_Release(&c->reader->IMFSourceReader_iface) == 0);
        return;
    }
    assert(!pthread_mutex_lock(&gate_lock));
    entered = TRUE;
    assert(!pthread_cond_signal(&gate_changed));
    while (!allowed) assert(!pthread_cond_wait(&gate_changed, &gate_lock));
    assert(!pthread_mutex_unlock(&gate_lock));
}
static void IMFSourceReaderCallback_OnFlush(IMFSourceReaderCallback *c, DWORD stream)
{
    (void)stream;
    atomic_fetch_add(&c->flushes, 1);
    callback(c);
}
static void IMFSourceReaderCallback_OnReadSample(IMFSourceReaderCallback *c, HRESULT hr, DWORD stream,
                                                DWORD flags, LONGLONG time, IMFSample *sample)
{
    (void)stream; (void)time;
    if (hr == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED) {
        /* The error comes the way a synchronous read reports it: flagged, and with no sample. */
        assert(flags == MF_SOURCE_READERF_ERROR && !sample);
        atomic_fetch_add(&c->errors, 1);
    } else {
        assert(!(flags & MF_SOURCE_READERF_ERROR));
    }
    callback(c);
}

static void release_case(BOOL flush, BOOL self, BOOL duplicate_fails, DWORD wait_result, BOOL idle)
{
    struct source source = {0};
    IMFSourceReaderCallback cb = {0};
    struct reader *r = calloc(1, sizeof *r);
    assert(r);
    r->refs = 2;
    r->source = &source;
    r->callback = &cb;
    cb.reader = r;
    cb.release_self = self;
    InitializeCriticalSection(&r->lock);
    assert(reader_launch(r) == S_OK);
    assert(reader_Release(&r->IMFSourceReader_iface) == 1);
    assert(!atomic_load(&source.released) && !atomic_load(&cb.released));
    struct thread *thread = r->thread->object;
    entered = allowed = FALSE;
    waits = 0;
    forced_wait_result = wait_result;
    fail_duplicate = duplicate_fails;
    if (!idle) {
        EnterCriticalSection(&r->lock);
        r->flush = flush;
        r->have_frame = TRUE;
        r->pending = 1;
        SetEvent(r->wake);
        LeaveCriticalSection(&r->lock);
        if (!self) {
            assert(!pthread_mutex_lock(&gate_lock));
            while (!entered) assert(!pthread_cond_wait(&gate_changed, &gate_lock));
            assert(!pthread_mutex_unlock(&gate_lock));
        }
    }
    if (!self) {
        assert(reader_Release(&r->IMFSourceReader_iface) == 0);
        assert(!pthread_mutex_lock(&gate_lock));
        allowed = TRUE;
        assert(!pthread_cond_signal(&gate_changed));
        assert(!pthread_mutex_unlock(&gate_lock));
    }
    assert(!pthread_join(thread->native, NULL));
    assert(atomic_load(&source.released) == 1);
    assert(atomic_load(&cb.released) == 1);
    assert(atomic_load(&cb.calls) == !idle);
    assert(atomic_load(&live_handles) == 0);
    if (self || duplicate_fails) assert(waits == 0);
    else assert(waits == 1);
    fail_duplicate = FALSE;
    forced_wait_result = 0;
}

static void wait_for_calls(IMFSourceReaderCallback *cb, int calls)
{
    struct timespec tick = { 0, 1000000 };
    for (int i = 0; atomic_load(&cb->calls) < calls && i < 10000; i++) nanosleep(&tick, NULL);
    assert(atomic_load(&cb->calls) == calls);
}

static struct reader *running_reader(struct source *source, IMFSourceReaderCallback *cb)
{
    struct reader *r = calloc(1, sizeof *r);
    assert(r);
    r->refs = 1;
    r->source = source;
    r->callback = cb;
    cb->reader = r;
    cb->free_running = TRUE;
    InitializeCriticalSection(&r->lock);
    assert(reader_launch(r) == S_OK);
    return r;
}

static void finish(struct reader *r, struct source *source, IMFSourceReaderCallback *cb, int calls)
{
    struct thread *thread = r->thread->object;
    assert(reader_Release(&r->IMFSourceReader_iface) == 0);
    assert(!pthread_join(thread->native, NULL));
    assert(atomic_load(&source->released) == 1 && atomic_load(&cb->released) == 1);
    assert(atomic_load(&cb->calls) == calls); /* nothing was delivered after the release */
    assert(atomic_load(&live_handles) == 0);
}

/* What reader_frame does when the bridge announces that the connection has gone. */
static void lose_camera(struct reader *r)
{
    EnterCriticalSection(&r->lock);
    r->lost = TRUE;
    LeaveCriticalSection(&r->lock);
    SetEvent(r->wake);
    SetEvent(r->arrived);
}

/* The camera goes while samples are asked for: each request gets the error once, later ones too, and
 * nothing comes without a request or after the release. */
static void camera_lost(BOOL frame_waiting)
{
    struct source source = {0};
    IMFSourceReaderCallback cb = {0};
    struct reader *r = running_reader(&source, &cb);

    if (frame_waiting) {
        EnterCriticalSection(&r->lock);
        r->have_frame = r->ticked = TRUE; /* a stale picture must not be handed on as the answer */
        LeaveCriticalSection(&r->lock);
    }
    lose_camera(r);
    wait_for_calls(&cb, 0); /* no request, no callback */
    for (int i = 0; i < 3; i++) reader_request(r);
    wait_for_calls(&cb, 3);
    assert(atomic_load(&cb.errors) == 3);
    reader_request(r);
    wait_for_calls(&cb, 4);
    assert(atomic_load(&cb.errors) == 4 && !atomic_load(&cb.flushes));
    /* Requests that are still waiting when the reader is released are dropped with it. */
    EnterCriticalSection(&r->lock);
    r->lost = r->have_frame = FALSE;
    LeaveCriticalSection(&r->lock);
    reader_request(r);
    finish(r, &source, &cb, 4);
}

/* Requests are counted while the thread hands samples on: every request gets exactly one callback. */
static void request_counting(void)
{
    enum { REQUESTS = 3000 };
    struct source source = {0};
    IMFSourceReaderCallback cb = {0};
    struct reader *r = running_reader(&source, &cb);
    struct timespec tick = { 0, 200000 };

    for (int i = 0; i < REQUESTS; i++) {
        reader_request(r);
        EnterCriticalSection(&r->lock);
        r->have_frame = TRUE;
        LeaveCriticalSection(&r->lock);
        SetEvent(r->wake);
    }
    for (int i = 0; atomic_load(&cb.calls) < REQUESTS && i < 50000; i++) {
        EnterCriticalSection(&r->lock);
        r->have_frame = TRUE; /* the next frame */
        LeaveCriticalSection(&r->lock);
        SetEvent(r->wake);
        nanosleep(&tick, NULL);
    }
    wait_for_calls(&cb, REQUESTS);
    EnterCriticalSection(&r->lock);
    assert(r->pending == 0);
    LeaveCriticalSection(&r->lock);
    assert(!atomic_load(&cb.errors));
    finish(r, &source, &cb, REQUESTS);
}

static void creation_failure(BOOL event_fails)
{
    struct source source = {0};
    IMFSourceReaderCallback cb = {0};
    struct reader *r = calloc(1, sizeof *r);
    assert(r);
    r->source = &source;
    r->callback = &cb;
    InitializeCriticalSection(&r->lock);
    fail_event = event_fails;
    fail_thread = !event_fails;
    assert(reader_launch(r) == HRESULT_FROM_WIN32(8));
    assert(!r->thread);
    reader_free(r); /* Factory retains ownership when no thread starts. */
    assert(atomic_load(&source.released) == 1 && atomic_load(&cb.released) == 1);
    assert(atomic_load(&live_handles) == 0);
    fail_event = fail_thread = FALSE;
}

int main(void)
{
    release_case(FALSE, FALSE, FALSE, WAIT_TIMEOUT, FALSE);
    release_case(TRUE, FALSE, FALSE, WAIT_FAILED, FALSE);
    release_case(FALSE, FALSE, TRUE, WAIT_TIMEOUT, FALSE);
    release_case(FALSE, TRUE, FALSE, 0, FALSE);
    release_case(TRUE, TRUE, FALSE, 0, FALSE);
    release_case(FALSE, FALSE, FALSE, 0, TRUE);
    creation_failure(TRUE);
    creation_failure(FALSE);
    camera_lost(FALSE);
    camera_lost(TRUE);
    request_counting();
    for (unsigned i = 0; i < event_count; i++) {
        assert(!pthread_mutex_destroy(&events[i].lock));
        assert(!pthread_cond_destroy(&events[i].changed));
    }
    for (unsigned i = 0; i < thread_count; i++) {
        assert(!pthread_mutex_destroy(&threads[i].lock));
        assert(!pthread_cond_destroy(&threads[i].changed));
    }
    puts("camera reader threaded lifetime tests passed");
    return 0;
}
