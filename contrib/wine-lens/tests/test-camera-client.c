/* SPDX-License-Identifier: MIT
 * The production bridge client (handshake, commands and replies, the reading thread and its shutdown) against
 * a simulated bridge on the other end of a socket pair, with pthread-backed Windows handles. Timeouts are
 * shortened and a stuck thread is simulated, so nothing here waits for seconds. No Wine, camera or Media
 * Foundation is needed for these checks.
 */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

typedef int BOOL;
typedef int32_t LONG;
typedef uint32_t DWORD;
typedef uint64_t UINT64;
typedef unsigned char BYTE;
typedef int SOCKET;
#define WINAPI
#define TRUE 1
#define FALSE 0
#define INFINITE UINT32_MAX
#define WAIT_OBJECT_0 0
#define WAIT_TIMEOUT 258
#define INVALID_SOCKET (-1)
#define SD_BOTH SHUT_RDWR
#define say(...) ((void)0)
#define BRIDGE_REPLY_MS 1000 /* long enough for a busy machine to answer, short enough to test */

static atomic_int live_locks, live_handles, running_threads, socket_closes;

typedef pthread_mutex_t CRITICAL_SECTION;
static void InitializeCriticalSection(CRITICAL_SECTION *m)
{
    pthread_mutexattr_t attr;
    assert(!pthread_mutexattr_init(&attr));
    assert(!pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_ERRORCHECK));
    assert(!pthread_mutex_init(m, &attr));
    assert(!pthread_mutexattr_destroy(&attr));
    atomic_fetch_add(&live_locks, 1);
}
static void EnterCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_lock(m)); }
static void LeaveCriticalSection(CRITICAL_SECTION *m) { assert(!pthread_mutex_unlock(m)); }
static void DeleteCriticalSection(CRITICAL_SECTION *m)
{
    assert(!pthread_mutex_destroy(m));
    atomic_fetch_sub(&live_locks, 1);
}
static LONG InterlockedDecrement(LONG *n) { return __atomic_sub_fetch(n, 1, __ATOMIC_SEQ_CST); }

/* Events and threads. The handles come from a pool that is never reused, so that a thread which closes its
 * own handle can still be marked as finished. */
struct handle {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    BOOL is_thread, manual, set; /* for a thread, `set` says that it has finished */
    atomic_bool open;
    DWORD (*run)(void *);
    void *arg;
};
typedef struct handle *HANDLE;
static struct handle handles[64];
static unsigned handle_count;
static BOOL fail_event, fail_thread, thread_wait_times_out;

static HANDLE new_handle(BOOL is_thread)
{
    assert(handle_count < 64);
    HANDLE h = &handles[handle_count++];
    pthread_condattr_t attr;
    assert(!pthread_condattr_init(&attr));
    assert(!pthread_condattr_setclock(&attr, CLOCK_MONOTONIC)); /* timed waits must not follow clock changes */
    assert(!pthread_mutex_init(&h->lock, NULL));
    assert(!pthread_cond_init(&h->changed, &attr));
    assert(!pthread_condattr_destroy(&attr));
    h->is_thread = is_thread;
    atomic_store(&h->open, TRUE);
    atomic_fetch_add(&live_handles, 1);
    return h;
}
static BOOL CloseHandle(HANDLE h)
{
    assert(h && atomic_exchange(&h->open, FALSE)); /* never twice */
    atomic_fetch_sub(&live_handles, 1);
    return TRUE;
}
static HANDLE CreateEventW(void *security, BOOL manual, BOOL initial, const void *name)
{
    (void)security; (void)name;
    if (fail_event) return NULL;
    HANDLE h = new_handle(FALSE);
    h->manual = manual;
    h->set = initial;
    return h;
}
static void change(HANDLE h, BOOL set)
{
    assert(h && atomic_load(&h->open) && !h->is_thread);
    assert(!pthread_mutex_lock(&h->lock));
    h->set = set;
    assert(!pthread_cond_broadcast(&h->changed));
    assert(!pthread_mutex_unlock(&h->lock));
}
static BOOL SetEvent(HANDLE h) { change(h, TRUE); return TRUE; }
static BOOL ResetEvent(HANDLE h) { change(h, FALSE); return TRUE; }
static void *run_thread(void *arg)
{
    HANDLE h = arg;
    h->run(h->arg);
    assert(!pthread_mutex_lock(&h->lock));
    h->set = TRUE;
    assert(!pthread_cond_broadcast(&h->changed));
    assert(!pthread_mutex_unlock(&h->lock));
    atomic_fetch_sub(&running_threads, 1);
    return NULL;
}
static HANDLE CreateThread(void *security, size_t stack, DWORD (*run)(void *), void *arg, DWORD flags, void *id)
{
    pthread_t native;
    (void)security; (void)stack; (void)flags; (void)id;
    if (fail_thread) return NULL;
    HANDLE h = new_handle(TRUE);
    h->run = run;
    h->arg = arg;
    atomic_fetch_add(&running_threads, 1);
    assert(!pthread_create(&native, NULL, run_thread, h));
    assert(!pthread_detach(native));
    return h;
}
static DWORD WaitForSingleObject(HANDLE h, DWORD timeout)
{
    struct timespec until;
    DWORD result = WAIT_OBJECT_0;

    assert(h && atomic_load(&h->open));
    if (h->is_thread) {
        assert(timeout == 5000); /* Shutdown stays bounded for the closing caller. */
        if (thread_wait_times_out) return WAIT_TIMEOUT;
    }
    assert(!clock_gettime(CLOCK_MONOTONIC, &until));
    until.tv_sec += timeout / 1000;
    until.tv_nsec += (long)(timeout % 1000) * 1000000;
    if (until.tv_nsec >= 1000000000) until.tv_sec++, until.tv_nsec -= 1000000000;
    assert(!pthread_mutex_lock(&h->lock));
    while (!h->set && result == WAIT_OBJECT_0) {
        int error = timeout == INFINITE ? pthread_cond_wait(&h->changed, &h->lock)
                                        : pthread_cond_timedwait(&h->changed, &h->lock, &until);
        if (error == ETIMEDOUT) result = WAIT_TIMEOUT;
        else assert(!error);
    }
    if (result == WAIT_OBJECT_0 && !h->is_thread && !h->manual) h->set = FALSE;
    assert(!pthread_mutex_unlock(&h->lock));
    return result;
}
static int closesocket(SOCKET s)
{
    atomic_fetch_add(&socket_closes, 1);
    return close(s);
}

#include "../src/sha256.h"
#define A2O_CAMERA_CLIENT_TEST
#ifndef A2O_CAMERA_CLIENT_SOURCE
#define A2O_CAMERA_CLIENT_SOURCE "../src/a2ocam.c"
#endif
#include A2O_CAMERA_CLIENT_SOURCE

/* ---- the simulated bridge ---------------------------------------------------------------------------- */

static const char KEY[] = "client-test-secret-do-not-use";

struct fake {
    int fd;
    void (*script)(struct fake *);
    pthread_t thread;
    const char *key;
    atomic_bool saw_end;      /* the client closed the connection: the real bridge stops the camera then */
    atomic_int lines_after;   /* command lines that arrived when none should */
};

static uint64_t now_ms(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static void pause_ms(unsigned ms)
{
    struct timespec t = { ms / 1000, (long)(ms % 1000) * 1000000 };
    while (nanosleep(&t, &t) && errno == EINTR) {}
}
/* One command line without its newline; FALSE at the end of the connection. */
static BOOL fake_line(struct fake *f, char *line, size_t size)
{
    size_t n = 0;
    for (;;) {
        char c;
        ssize_t got = recv(f->fd, &c, 1, 0);
        if (got <= 0) return FALSE;
        if (c == '\n') break;
        assert(n + 1 < size);
        line[n++] = c;
    }
    line[n] = 0;
    return TRUE;
}
static void fake_packet(struct fake *f, char type, const void *data, uint32_t length)
{
    unsigned char head[16] = { (unsigned char)type };
    memcpy(head + 4, &length, 4);
    /* (the client may have gone: a failed send is then what the script expects) */
    if (send(f->fd, head, 16, MSG_NOSIGNAL) == 16 && length) (void)!send(f->fd, data, length, MSG_NOSIGNAL);
}
static void fake_reply(struct fake *f, const char *text) { fake_packet(f, 'R', text, (uint32_t)strlen(text)); }
/* Read until the client closes, noting commands that should not have come. */
static void fake_until_end(struct fake *f)
{
    char line[256];
    while (fake_line(f, line, sizeof line)) atomic_fetch_add(&f->lines_after, 1);
    atomic_store(&f->saw_end, TRUE);
}
/* The bridge's side of the handshake, as camera-bridge.c does it. FALSE if the client was turned away. */
static BOOL fake_handshake(struct fake *f)
{
    char line[256], expected[65];
    if (!fake_line(f, line, sizeof line) || strcmp(line, "H")) return FALSE;
    fake_reply(f, "000102030405060708090a0b0c0d0e0f");
    a2o_hmac_hex(f->key, "000102030405060708090a0b0c0d0e0f", expected);
    if (!fake_line(f, line, sizeof line) || strncmp(line, "A ", 2) || !a2o_same(line + 2, expected)) return FALSE;
    fake_reply(f, "ok");
    return TRUE;
}
static void *fake_main(void *arg)
{
    struct fake *f = arg;
    f->script(f);
    close(f->fd);
    return NULL;
}
/* Start a simulated bridge and connect the production client to it. */
static struct bridge *open_bridge(struct fake *f, void (*script)(struct fake *), const char *bridge_key)
{
    int pair[2];
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    memset(f, 0, sizeof *f);
    f->fd = pair[1];
    f->script = script;
    f->key = bridge_key;
    atomic_store(&socket_closes, 0);
    assert(!pthread_create(&f->thread, NULL, fake_main, f));
    return bridge_start(pair[0], KEY);
}
/* Everything the client made is gone again, and it closed its socket exactly once. */
static void expect_clean(struct fake *f)
{
    assert(!pthread_join(f->thread, NULL));
    for (int i = 0; atomic_load(&running_threads) && i < 5000; i++) pause_ms(1);
    assert(!atomic_load(&running_threads));
    assert(!atomic_load(&live_handles) && !atomic_load(&live_locks));
    assert(atomic_load(&socket_closes) == 1);
}

/* ---- the cases ---------------------------------------------------------------------------------------- */

static void script_commands(struct fake *f)
{
    char line[256], big[9000];
    if (!fake_handshake(f)) return;
    while (fake_line(f, line, sizeof line)) {
        if (!strcmp(line, "Q 980900")) fake_reply(f, "-64 64 1 0 7");
        else if (!strcmp(line, "L")) {
            memset(big, 'x', sizeof big);
            fake_packet(f, 'R', big, sizeof big);
        } else fake_reply(f, "!unknown command");
    }
    atomic_store(&f->saw_end, TRUE);
}
static void commands_and_close(void)
{
    struct fake f;
    char reply[8192] = "";
    struct bridge *b = open_bridge(&f, script_commands, KEY);

    assert(b);
    assert(bridge_command(b, reply, sizeof reply, "Q %x", 0x980900) && !strcmp(reply, "-64 64 1 0 7"));
    assert(!bridge_command(b, reply, sizeof reply, "C %x %d", 1, 2) && !strcmp(reply, "!unknown command"));
    assert(bridge_command(b, NULL, 0, "Q 980900"));
    /* An overlong reply is cut, not written past the buffer. */
    assert(bridge_command(b, reply, sizeof reply, "L") && strlen(reply) == sizeof reply - 1);
    /* A command too long for a line is not sent at all. */
    assert(!bridge_command(b, NULL, 0, "%0300d", 1));
    bridge_close(b);
    expect_clean(&f);
    assert(atomic_load(&f.saw_end));
}

static void script_turn_away(struct fake *f)
{
    assert(!fake_handshake(f));
}
static void wrong_secret(void)
{
    struct fake f;
    assert(!open_bridge(&f, script_turn_away, "another-secret-entirely"));
    expect_clean(&f);
}

/* The reply to a command comes after the command has stopped waiting. */
static void script_late_reply(struct fake *f)
{
    char line[256];
    if (!fake_handshake(f)) return;
    assert(fake_line(f, line, sizeof line) && !strncmp(line, "F ", 2));
    pause_ms(BRIDGE_REPLY_MS + 200);
    fake_reply(f, "ok"); /* the camera was started after all */
    fake_until_end(f);
}
static void late_reply(void)
{
    struct fake f;
    char reply[64] = "untouched";
    struct bridge *b = open_bridge(&f, script_late_reply, KEY);
    uint64_t started = now_ms();

    assert(b);
    assert(!bridge_command(b, NULL, 0, "F MJPG 640 480 60"));
    assert(now_ms() - started >= BRIDGE_REPLY_MS);
    pause_ms(400); /* the late "ok" is on its way or here by now */
    /* It must not answer the next command (a control query would read "ok" as its numbers). The connection
     * is closed instead: nothing more is sent, and the bridge stops the camera it started late. */
    started = now_ms();
    assert(!bridge_command(b, reply, sizeof reply, "Q 980900") && !strcmp(reply, "untouched"));
    assert(now_ms() - started < BRIDGE_REPLY_MS);
    bridge_close(b);
    expect_clean(&f);
    assert(atomic_load(&f.saw_end) && !atomic_load(&f.lines_after));
}

/* The bridge goes away in the middle of a command (the camera was unplugged, or it was ended). */
static void script_leave(struct fake *f)
{
    char line[256];
    if (!fake_handshake(f)) return;
    assert(fake_line(f, line, sizeof line));
}
static void bridge_gone(void)
{
    struct fake f;
    struct bridge *b = open_bridge(&f, script_leave, KEY);
    uint64_t started = now_ms();

    assert(b);
    assert(!bridge_command(b, NULL, 0, "Q 980900"));
    assert(!bridge_command(b, NULL, 0, "S"));
    assert(now_ms() - started < BRIDGE_REPLY_MS); /* told at once, not after a timeout */
    bridge_close(b);
    expect_clean(&f);
}

/* bridge_close gives up waiting for a thread that is still inside a frame callback. */
static pthread_mutex_t gate_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t gate_changed = PTHREAD_COND_INITIALIZER;
static BOOL entered, allowed;
static atomic_int frames;

static void slow_frame(void *ctx, UINT64 time_us, const BYTE *data, DWORD length)
{
    (void)time_us;
    if (!data) return; /* the end of the connection, announced after the close below */
    assert(ctx == &frames && length == 3 && !memcmp(data, "abc", 3));
    atomic_fetch_add(&frames, 1);
    assert(!pthread_mutex_lock(&gate_lock));
    entered = TRUE;
    assert(!pthread_cond_broadcast(&gate_changed));
    while (!allowed) assert(!pthread_cond_wait(&gate_changed, &gate_lock));
    assert(!pthread_mutex_unlock(&gate_lock));
}
static void script_one_frame(struct fake *f)
{
    char line[256];
    if (!fake_handshake(f)) return;
    assert(fake_line(f, line, sizeof line) && !strncmp(line, "F ", 2));
    fake_reply(f, "ok");
    fake_packet(f, 'F', "abc", 3);
    fake_until_end(f);
}
static void close_while_thread_is_busy(void)
{
    struct fake f;
    struct bridge *b = open_bridge(&f, script_one_frame, KEY);

    assert(b);
    entered = allowed = FALSE;
    atomic_store(&frames, 0);
    bridge_set_frames(b, slow_frame, &frames);
    assert(bridge_command(b, NULL, 0, "F MJPG 640 480 60"));
    assert(!pthread_mutex_lock(&gate_lock));
    while (!entered) assert(!pthread_cond_wait(&gate_changed, &gate_lock));
    assert(!pthread_mutex_unlock(&gate_lock));

    thread_wait_times_out = TRUE;
    bridge_close(b); /* returns although the thread has not left */
    thread_wait_times_out = FALSE;
    /* The thread still uses the bridge's memory (ASan reports it if that was freed) and frees it itself. */
    assert(atomic_load(&live_handles) && atomic_load(&live_locks));
    assert(!pthread_mutex_lock(&gate_lock));
    allowed = TRUE;
    assert(!pthread_cond_broadcast(&gate_changed));
    assert(!pthread_mutex_unlock(&gate_lock));
    expect_clean(&f);
    assert(atomic_load(&frames) == 1 && atomic_load(&f.saw_end));
}

/* Whoever takes the frames is told once when the connection has gone, and nobody is told who had stopped
 * taking them. */
static atomic_int ends, pictures;

static void counting_frame(void *ctx, UINT64 time_us, const BYTE *data, DWORD length)
{
    (void)time_us;
    assert(ctx == &ends);
    if (data) assert(length == 3 && !memcmp(data, "abc", 3));
    else assert(!length);
    atomic_fetch_add(data ? &pictures : &ends, 1);
}
static void script_frame_then_leave(struct fake *f)
{
    char line[256];
    if (!fake_handshake(f)) return;
    assert(fake_line(f, line, sizeof line) && !strncmp(line, "F ", 2));
    fake_reply(f, "ok");
    fake_packet(f, 'F', "abc", 3);
    /* The next command finds the camera unplugged: the bridge closes the connection without a reply. */
    assert(fake_line(f, line, sizeof line) && !strcmp(line, "S"));
}
static void end_is_announced(BOOL still_listening)
{
    struct fake f;
    struct bridge *b = open_bridge(&f, script_frame_then_leave, KEY);

    assert(b);
    atomic_store(&ends, 0);
    atomic_store(&pictures, 0);
    bridge_set_frames(b, counting_frame, &ends);
    assert(bridge_command(b, NULL, 0, "F MJPG 640 480 60"));
    for (int i = 0; atomic_load(&pictures) < 1 && i < 5000; i++) pause_ms(1);
    if (!still_listening) bridge_set_frames(b, NULL, NULL);
    assert(!bridge_command(b, NULL, 0, "S"));
    for (int i = 0; atomic_load(&running_threads) && i < 5000; i++) pause_ms(1);
    assert(!atomic_load(&running_threads));
    assert(atomic_load(&pictures) == 1 && atomic_load(&ends) == (still_listening ? 1 : 0));
    bridge_close(b);
    expect_clean(&f);
    assert(atomic_load(&ends) == (still_listening ? 1 : 0));
}

/* A command that gets no reply ends the connection, and whoever takes the frames hears of that too. */
static void script_silent(struct fake *f)
{
    char line[256];
    if (!fake_handshake(f)) return;
    assert(fake_line(f, line, sizeof line));
    fake_until_end(f);
}
static void timeout_is_announced(void)
{
    struct fake f;
    struct bridge *b = open_bridge(&f, script_silent, KEY);

    assert(b);
    atomic_store(&ends, 0);
    atomic_store(&pictures, 0);
    bridge_set_frames(b, counting_frame, &ends);
    assert(!bridge_command(b, NULL, 0, "Q 980900"));
    for (int i = 0; atomic_load(&running_threads) && i < 5000; i++) pause_ms(1);
    assert(atomic_load(&ends) == 1 && !atomic_load(&pictures));
    bridge_close(b);
    expect_clean(&f);
    assert(atomic_load(&f.saw_end) && !atomic_load(&f.lines_after));
}

/* The event or the thread cannot be made: nothing is left behind, and the socket is closed. */
static void script_nobody(struct fake *f)
{
    fake_until_end(f);
}
static void creation_failure(BOOL event_fails)
{
    struct fake f;
    fail_event = event_fails;
    fail_thread = !event_fails;
    assert(!open_bridge(&f, script_nobody, KEY));
    fail_event = fail_thread = FALSE;
    expect_clean(&f);
    assert(atomic_load(&f.saw_end) && !atomic_load(&f.lines_after));
}

int main(void)
{
    signal(SIGPIPE, SIG_IGN);
    commands_and_close();
    wrong_secret();
    late_reply();
    bridge_gone();
    close_while_thread_is_busy();
    end_is_announced(TRUE);
    end_is_announced(FALSE);
    timeout_is_announced();
    creation_failure(TRUE);
    creation_failure(FALSE);
    puts("camera bridge client tests passed");
    return 0;
}
