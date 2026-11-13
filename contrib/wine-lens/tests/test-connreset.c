/* SPDX-License-Identifier: MIT
 * Run the production shim's send/receive/close wrappers with a simulated Winsock provider.
 * The import-table loader is excluded; Windows/Wine integration remains a hardware check.
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <winsock2.h>
#include <windows.h>
#else
#include <netinet/in.h>
#include <pthread.h>
typedef int BOOL;
typedef uintptr_t SOCKET;
typedef uint32_t DWORD;
typedef struct { DWORD len; char *buf; } WSABUF;
typedef struct { int unused; } WSAOVERLAPPED;
typedef void (*LPWSAOVERLAPPED_COMPLETION_ROUTINE)(void);
typedef pthread_mutex_t CRITICAL_SECTION;
#define TRUE 1
#define FALSE 0
#define WINAPI
#define SOCKET_ERROR (-1)
#define WSA_IO_PENDING 997
#define WSAECONNRESET 10054
static void InitializeCriticalSection(CRITICAL_SECTION *lock) { assert(!pthread_mutex_init(lock, NULL)); }
static void EnterCriticalSection(CRITICAL_SECTION *lock) { assert(!pthread_mutex_lock(lock)); }
static void LeaveCriticalSection(CRITICAL_SECTION *lock) { assert(!pthread_mutex_unlock(lock)); }
static int last_error;
static void WSASetLastError(int error) { last_error = error; }
static int WSAGetLastError(void) { return last_error; }
#ifdef A2O_ORIGINAL_REGRESSION
/* The original heuristic sees recycled heap storage as mapped/writable too. */
typedef unsigned char BYTE;
typedef size_t SIZE_T;
typedef struct { int State, Protect; void *BaseAddress; size_t RegionSize; } MEMORY_BASIC_INFORMATION;
#define MEM_COMMIT 1
#define PAGE_READWRITE 2
#define PAGE_EXECUTE_READWRITE 4
#define PAGE_GUARD 8
static size_t VirtualQuery(void *address, MEMORY_BASIC_INFORMATION *info, size_t size) {
    *info = (MEMORY_BASIC_INFORMATION){ MEM_COMMIT, PAGE_READWRITE, address, 4096 };
    return size;
}
#endif
#endif

#define A2O_CONNRESET_TEST
#ifndef A2O_CONNRESET_SOURCE
#define A2O_CONNRESET_SOURCE "../src/connreset.c"
#endif
#include A2O_CONNRESET_SOURCE

static int receive_result = SOCKET_ERROR, receive_error = WSA_IO_PENDING;
static BOOL complete_before_return;
static unsigned sends, receives, closes;
static struct sockaddr_in observed;

static int WINAPI mock_send(SOCKET s, WSABUF *buffers, DWORD count, DWORD *sent, DWORD flags,
                             const struct sockaddr *to, int len, WSAOVERLAPPED *overlapped,
                             LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    (void)s; (void)buffers; (void)count; (void)sent; (void)flags; (void)to; (void)len;
    (void)overlapped; (void)routine;
    sends++;
    return 0;
}

static int WINAPI mock_recv(SOCKET s, WSABUF *buffers, DWORD count, DWORD *received, DWORD *flags,
                             struct sockaddr *from, int *len, WSAOVERLAPPED *overlapped,
                             LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    (void)s; (void)buffers; (void)count; (void)received; (void)flags; (void)len;
    (void)overlapped; (void)routine;
    receives++;
    if (from) memcpy(&observed, from, sizeof observed);
    /* An IOCP consumer can complete and reclaim storage before the submitting
     * thread returns from WSARecvFrom, even when that call reports IO_PENDING. */
    if (complete_before_return) free(from);
    WSASetLastError(receive_error);
    return receive_result;
}

#ifndef A2O_ORIGINAL_REGRESSION
static int WINAPI mock_close(SOCKET s) { (void)s; closes++; return 0; }
#endif

static struct sockaddr_in destination(unsigned port)
{
    struct sockaddr_in to;
    memset(&to, 0, sizeof to);
    to.sin_family = AF_INET;
    to.sin_port = htons(port);
    to.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    return to;
}

static void send_to(SOCKET s, unsigned port)
{
    struct sockaddr_in to = destination(port);
    assert(sendto_noting(s, NULL, 0, NULL, 0, (struct sockaddr *)&to, sizeof to, NULL, NULL) == 0);
}

static void receive_into(SOCKET s, struct sockaddr_in *from)
{
    int len = sizeof *from;
    WSAOVERLAPPED overlapped = {0};
    assert(recvfrom_prefilled(s, NULL, 0, NULL, NULL, (struct sockaddr *)from, &len, &overlapped, NULL)
           == receive_result);
    assert(WSAGetLastError() == receive_error);
}

int main(void)
{
    InitializeCriticalSection(&sent_lock);
    real_sendto = mock_send;
    real_recvfrom = mock_recv;
#ifndef A2O_ORIGINAL_REGRESSION
    real_closesocket = mock_close;
#endif
    /* Receive starts before any destination is known. After completion the
     * caller may reuse the same still-mapped address for unrelated data. */
    struct sockaddr_in reused = {0}, unchanged = {0};
    receive_into(1, &reused);
    send_to(1, 41001);
    assert(!memcmp(&reused, &unchanged, sizeof reused));
    /* A subsequent receive gets the latest destination, including one whose
     * failure is reported synchronously. No saved pointer is required. */
    receive_result = SOCKET_ERROR;
    receive_error = WSAECONNRESET;
    receive_into(1, &reused);
    struct sockaddr_in first = destination(41001);
    assert(!memcmp(&observed, &first, sizeof first));
    assert(!memcmp(&reused, &first, sizeof first));

    /* Completion can happen before the underlying call returns. ASan catches
     * any attempt by the wrapper to snapshot the now-freed address. */
    complete_before_return = TRUE;
    receive_error = WSA_IO_PENDING;
    struct sockaddr_in *freed = calloc(1, sizeof *freed);
    assert(freed);
    receive_into(2, freed);
    complete_before_return = FALSE;
    send_to(2, 41002);

#ifndef A2O_ORIGINAL_REGRESSION
    /* Closed sockets do not exhaust the fixed cache or contaminate reused
     * handles, and handle zero is not an empty-slot sentinel. */
    for (SOCKET s = 0; s < SOCKETS * 3; s++) {
        send_to(s, 42000 + (unsigned)s);
        struct sockaddr_in from = {0}, expected = destination(42000 + (unsigned)s);
        receive_into(s, &from);
        assert(!memcmp(&from, &expected, sizeof from));
        assert(closesocket_forgetting(s) == 0);
        memset(&from, 0, sizeof from);
        receive_into(s, &from);
        assert(!memcmp(&from, &unchanged, sizeof from));
        assert(closesocket_forgetting(s) == 0);
    }
    assert(closes == SOCKETS * 6);
#endif
    assert(sends && receives);
    puts("UDP shim buffer lifetime and socket lifecycle tests passed");
    return 0;
}
