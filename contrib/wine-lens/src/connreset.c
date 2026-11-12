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
 * a2ocam.dll is loaded into the tracker anyway (it is the tracker's camera). When it is loaded into a
 * program, the two Winsock calls that program imports are routed through here. Per socket, the destination
 * of its datagrams and the address buffer of its waiting receive are remembered, and the destination is put
 * into that buffer: when a receive starts, and when the destination changes while one waits (the tracker
 * starts its receive long before it has anybody to send to). A receive that brings a datagram overwrites
 * it; a receive that fails leaves the address of the last destination, which is the port that closed. Only
 * this process is touched, and only its own import table.
 */
#include <winsock2.h>
#include <windows.h>
#include <string.h>

#define SOCKETS 16

static struct {
    SOCKET socket;
    struct sockaddr_in to;    /* where its datagrams last went; family 0 before the first */
    struct sockaddr *waiting; /* the address buffer of its receive, while one waits; NULL otherwise */
    struct sockaddr_in held;  /* what that buffer held when the receive started waiting */
} last_sent[SOCKETS];
static CRITICAL_SECTION sent_lock;

static int (WINAPI *real_sendto)(SOCKET, WSABUF *, DWORD, DWORD *, DWORD, const struct sockaddr *, int,
                                 WSAOVERLAPPED *, LPWSAOVERLAPPED_COMPLETION_ROUTINE);
static int (WINAPI *real_recvfrom)(SOCKET, WSABUF *, DWORD, DWORD *, DWORD *, struct sockaddr *, int *,
                                   WSAOVERLAPPED *, LPWSAOVERLAPPED_COMPLETION_ROUTINE);

/* The table entry of a socket, taking a free one for a socket not seen before; -1 when the table is full.
 * Call with the lock held. */
static int slot_of(SOCKET s)
{
    int free_slot = -1;
    for (int i = 0; i < SOCKETS; i++) {
        if (last_sent[i].socket == s) return i;
        if (free_slot < 0 && !last_sent[i].socket) free_slot = i;
    }
    if (free_slot >= 0) {
        memset(&last_sent[free_slot], 0, sizeof last_sent[free_slot]);
        last_sent[free_slot].socket = s;
    }
    return free_slot;
}

static BOOL writable(void *address, SIZE_T bytes)
{
    MEMORY_BASIC_INFORMATION info;
    if (!address || !VirtualQuery(address, &info, sizeof info)) return FALSE;
    return info.State == MEM_COMMIT && (info.Protect & (PAGE_READWRITE | PAGE_EXECUTE_READWRITE)) &&
           !(info.Protect & PAGE_GUARD) &&
           (BYTE *)address + bytes <= (BYTE *)info.BaseAddress + info.RegionSize;
}

static int WINAPI sendto_noting(SOCKET s, WSABUF *buffers, DWORD count, DWORD *sent, DWORD flags,
                                const struct sockaddr *to, int to_len, WSAOVERLAPPED *overlapped,
                                LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    if (to && to_len >= (int)sizeof(struct sockaddr_in) && to->sa_family == AF_INET) {
        int slot;
        EnterCriticalSection(&sent_lock);
        slot = slot_of(s);
        if (slot >= 0 && memcmp(&last_sent[slot].to, to, sizeof last_sent[slot].to)) {
            memcpy(&last_sent[slot].to, to, sizeof last_sent[slot].to);
            /* A new destination while a receive waits: that receive is the one that will fail. The buffer
             * is the program's, and nothing here is told when the receive ends, so it is written only if
             * its memory is still there and still holds what it held when the receive began to wait: a
             * finished receive has put an address into it, and memory given to something else has
             * changed. */
            struct sockaddr *waiting = last_sent[slot].waiting;
            if (writable(waiting, sizeof last_sent[slot].held) &&
                !memcmp(waiting, &last_sent[slot].held, sizeof last_sent[slot].held)) {
                memcpy(waiting, to, sizeof last_sent[slot].to);
                memcpy(&last_sent[slot].held, to, sizeof last_sent[slot].held);
            } else {
                last_sent[slot].waiting = NULL;
            }
        }
        LeaveCriticalSection(&sent_lock);
    }
    return real_sendto(s, buffers, count, sent, flags, to, to_len, overlapped, routine);
}

static int WINAPI recvfrom_prefilled(SOCKET s, WSABUF *buffers, DWORD count, DWORD *received, DWORD *flags,
                                     struct sockaddr *from, int *from_len, WSAOVERLAPPED *overlapped,
                                     LPWSAOVERLAPPED_COMPLETION_ROUTINE routine)
{
    BOOL noted = from && from_len && *from_len >= (int)sizeof(struct sockaddr_in);
    int slot = -1, result, error;

    if (noted) {
        EnterCriticalSection(&sent_lock);
        slot = slot_of(s);
        if (slot >= 0) {
            last_sent[slot].waiting = NULL; /* until this receive is known to wait */
            if (last_sent[slot].to.sin_family) memcpy(from, &last_sent[slot].to, sizeof last_sent[slot].to);
        }
        LeaveCriticalSection(&sent_lock);
    }
    result = real_recvfrom(s, buffers, count, received, flags, from, from_len, overlapped, routine);
    error = WSAGetLastError();
    /* Only a receive left waiting has a buffer worth remembering: one that finished at once has been
     * handed back to the program already. */
    if (noted && slot >= 0 && result == SOCKET_ERROR && error == WSA_IO_PENDING) {
        EnterCriticalSection(&sent_lock);
        if (last_sent[slot].socket == s) {
            last_sent[slot].waiting = from;
            memcpy(&last_sent[slot].held, from, sizeof last_sent[slot].held);
        }
        LeaveCriticalSection(&sent_lock);
    }
    WSASetLastError(error);
    return result;
}

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
            if (real_sendto && real_recvfrom) {
                route_import(GetModuleHandleW(NULL), real_sendto, sendto_noting);
                route_import(GetModuleHandleW(NULL), real_recvfrom, recvfrom_prefilled);
            }
        }
    }
    return TRUE;
}
