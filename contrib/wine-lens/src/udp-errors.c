/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-udp.so: lets a UDP socket under Wine learn that the port it sends to has closed.
 *
 * Leia's eye tracker sends eye positions to Leia's service by UDP, and finds out that the service has stopped
 * listening the Windows way: the next datagram bounces, and Windows fails the socket's pending receive with
 * "connection reset". Only then does the tracker pause, switch the camera off and stop using a processor core.
 * Linux reports a bounced datagram on an unconnected UDP socket only if the socket asked for it (IP_RECVERR),
 * and Wine does not ask, so under Wine the tracker never pauses.
 *
 * This library is preloaded into the Wine processes of the prefix (tools/start-stack.sh) and asks for those
 * reports on every IPv4 UDP socket they create. Nothing else changes. The other half, giving the failed
 * receive the address of the port that closed as Windows does, is connreset.c.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <netinet/in.h>
#include <sys/socket.h>

int socket(int domain, int type, int protocol)
{
    static int (*real_socket)(int, int, int);
    int fd, one = 1;

    if (!real_socket) real_socket = dlsym(RTLD_NEXT, "socket");
    fd = real_socket(domain, type, protocol);
    if (fd >= 0 && domain == AF_INET && (type & 0xf) == SOCK_DGRAM)
        setsockopt(fd, IPPROTO_IP, IP_RECVERR, &one, sizeof one);
    return fd;
}
