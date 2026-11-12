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
static struct event events[32];
static struct thread threads[32];
static struct handle handles[96];
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
    assert(handle_count < 96);
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
    assert(!manual && event_count < 32);
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
    atomic_int released, calls;
} IMFSourceReaderCallback;
struct source { IMFMediaSource IMFMediaSource_iface; atomic_int released; };
struct reader {
    IMFSourceReader IMFSourceReader_iface;
    LONG refs;
    struct source *source;
    IMFSourceReaderCallback *callback;
    CRITICAL_SECTION lock;
    HANDLE wake, thread;
    BOOL streaming, ticked, quit, flush;
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
    callback(c);
}
static void IMFSourceReaderCallback_OnReadSample(IMFSourceReaderCallback *c, HRESULT hr, DWORD stream,
                                                DWORD flags, LONGLONG time, IMFSample *sample)
{
    (void)hr; (void)stream; (void)flags; (void)time; (void)sample;
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
