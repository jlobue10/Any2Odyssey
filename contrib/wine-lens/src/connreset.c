/* SPDX-License-Identifier: MIT
 *
 * Part of a2ocam.dll: tells Leia's eye tracker which listener has gone, the way Windows does.
 *
 * When a UDP datagram bounces because nothing listens on the port any more, Windows fails the sending
 * socket's pending WSARecvFrom and fills in the address of that port. The tracker takes the listener at that
 * address off its list and pauses when the list is empty. Under Wine the receive fails too (once
 * any2odyssey-udp.so has asked Linux to report the bounce) but the address is left as it was, so the tracker
 * removes nobody and keeps running.
 *
 * a2ocam.dll is loaded into the tracker anyway (it is the tracker's camera). Its Winsock imports are
 * routed through here. Each socket keeps only its last destination, never pointers to caller-owned receive
 * storage. The address is filled before submitting a receive, while that storage belongs to the caller.
 * A real datagram overwrites it. If the first destination arrives after a receive was submitted, that
 * receive cannot safely be amended: the next receive picks up the address. Closing a socket forgets it.
 *
 * Only the program's own closesocket calls are seen here. A socket closed by other code keeps its entry, so
 * an entry is taken only where a destination is learned (a send), never for a socket that just receives, and
 * when the table is full the entry not written for longest makes room: the table cannot fill up for good
 * and leave the tracker without its pause.
 */
#ifndef A2O_CONNRESET_TEST
#include <winsock2.h>
#include <windows.h>
#endif
#include <string.h>

#define SOCKETS 16

static struct {
    BOOL used;
    SOCKET socket;
    unsigned long long noted; /* when the destination was last noted, counted in sends */
    struct sockaddr_in to;    /* where its datagrams last went */
} last_sent[SOCKETS];
static unsigned long long sends_noted;
static CRITICAL_SECTION sent_lock;

static int (WINAPI *real_sendto)(SOCKET, WSABUF *, DWORD, DWORD *, DWORD, const struct sockaddr *, int,
                                 WSAOVERLAPPED *, LPWSAOVERLAPPED_COMPLETION_ROUTINE);
static int (WINAPI *real_closesocket)(SOCKET);
static int (WINAPI *real_recvfrom)(SOCKET, WSABUF *, DWORD, DWORD *, DWORD *, struct sockaddr *, int *,
                                   WSAOVERLAPPED *, LPWSAOVERLAPPED_COMPLETION_ROUTINE);

/* The table entry of a socket, or -1 if it has none. Call with the lock held. */
static int slot_of(SOCKET s)
{
    for (int i = 0; i < SOCKETS; i++)
        if (last_sent[i].used && last_sent[i].socket == s) return i;
    return -1;
}

/* The entry to note a socket's destination in: its own, else a free one, else the one whose destination was
 * noted longest ago (most likely a socket that was closed without this shim seeing it). Call with the lock
 * held. */
static int slot_for_sender(SOCKET s)
{
    int slot = slot_of(s), oldest = 0;
    if (slot >= 0) return slot;
    for (int i = 0; i < SOCKETS && slot < 0; i++) {
        if (!last_sent[i].used) slot = i;
        else if (last_sent[i].noted < last_sent[oldest].noted) oldest = i;
    }
    if (slot < 0) slot = oldest;
    memset(&last_sent[slot], 0, sizeof last_sent[slot]);
    last_sent[slot].used = TRUE;
    last_sent[slot].socket = s;
    return slot;
}

static int WINAPI sendto_noting(SOCKET s, WSABUF *buffers, DWORD count, DWORD *sent, DWORD flags,
                                const struct sockaddr *to, int to_len, WSAOVERLAPPED *overlapped,
                                LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    if (to && to_len >= (int)sizeof(struct sockaddr_in) && to->sa_family == AF_INET) {
        int slot;
        EnterCriticalSection(&sent_lock);
        slot = slot_for_sender(s);
        memcpy(&last_sent[slot].to, to, sizeof last_sent[slot].to);
        last_sent[slot].noted = ++sends_noted;
        LeaveCriticalSection(&sent_lock);
    }
    return real_sendto(s, buffers, count, sent, flags, to, to_len, overlapped, routine);
}

static int WINAPI recvfrom_prefilled(SOCKET s, WSABUF *buffers, DWORD count, DWORD *received, DWORD *flags,
                                     struct sockaddr *from, int *from_len, WSAOVERLAPPED *overlapped,
                                     LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    if (from && from_len && *from_len >= (int)sizeof(struct sockaddr_in)) {
        EnterCriticalSection(&sent_lock);
        int slot = slot_of(s); /* a socket that has not sent has no entry, and gets none for receiving */
        if (slot >= 0) memcpy(from, &last_sent[slot].to, sizeof last_sent[slot].to);
        LeaveCriticalSection(&sent_lock);
    }
    /* Once submitted, Winsock may complete on another thread at any time. The caller can then free or
     * reuse the buffers, even before this call returns. Neither VirtualQuery nor unchanged bytes prove
     * their lifetime, so do not retain, inspect or write them after entering the real receive. */
    return real_recvfrom(s, buffers, count, received, flags, from, from_len, overlapped, routine);
}

static int WINAPI closesocket_forgetting(SOCKET s)
{
    EnterCriticalSection(&sent_lock);
    for (int i = 0; i < SOCKETS; i++) {
        if (last_sent[i].used && last_sent[i].socket == s) {
            memset(&last_sent[i], 0, sizeof last_sent[i]);
            break;
        }
    }
    LeaveCriticalSection(&sent_lock);
    return real_closesocket(s);
}

#ifndef A2O_CONNRESET_TEST
/* Point the program's own imports of `name` from ws2_32.dll at `to`. */
static void route_import(HMODULE program, void *from, void *to)
{
    BYTE *base = (BYTE *)program;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + ((IMAGE_DOS_HEADER *)base)->e_lfanew);
    IMAGE_DATA_DIRECTORY dir = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];

    if (!from || !dir.VirtualAddress) return;
    for (IMAGE_IMPORT_DESCRIPTOR *d = (void *)(base + dir.VirtualAddress); d->Name; d++) {
        if (_stricmp((char *)base + d->Name, "WS2_32.dll")) continue;
        for (void **slot = (void **)(base + d->FirstThunk); *slot; slot++) {
            DWORD old;
            if (*slot != from) continue;
            VirtualProtect(slot, sizeof *slot, PAGE_READWRITE, &old);
            *slot = to;
            VirtualProtect(slot, sizeof *slot, old, &old);
        }
    }
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void *reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        HMODULE winsock = GetModuleHandleW(L"ws2_32.dll");
        DisableThreadLibraryCalls(instance);
        InitializeCriticalSection(&sent_lock);
        if (winsock) { /* a program that does not use Winsock has nothing to route */
            real_sendto = (void *)GetProcAddress(winsock, "WSASendTo");
            real_recvfrom = (void *)GetProcAddress(winsock, "WSARecvFrom");
            real_closesocket = (void *)GetProcAddress(winsock, "closesocket");
            if (real_sendto && real_recvfrom && real_closesocket) {
                route_import(GetModuleHandleW(NULL), real_sendto, sendto_noting);
                route_import(GetModuleHandleW(NULL), real_recvfrom, recvfrom_prefilled);
                route_import(GetModuleHandleW(NULL), real_closesocket, closesocket_forgetting);
            }
        }
    }
    return TRUE;
}

#endif
