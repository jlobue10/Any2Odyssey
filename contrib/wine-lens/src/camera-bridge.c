/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-camera-bridge: hands a V4L2 camera to a program running under Wine.
 *
 * Wine has no Media Foundation video capture, and Windows code cannot issue V4L2 ioctls. The capture stand-in
 * (a2ocam.c) therefore connects to this native program on a loopback TCP port:
 *
 *   input, one command per line:
 *     H nonce                 must come first: a random number of the client's (32 hex digits); the reply
 *                             is a fresh random challenge of the same form
 *     A answer                must come second: HMAC-SHA-256 of "client:<challenge>:<nonce>" under the shared
 *                             secret (hex); the reply is "ok " and the HMAC of "bridge:<nonce>:<challenge>"
 *     L                       list formats; reply has one "FOURCC width height fps" per line
 *     F FOURCC width height fps   start streaming in that format
 *     S                       stop streaming
 *     Q id                    describe control (id in hex): "min max step default value" or an error
 *     C id value              set a control
 *   output, packets: a 16-byte header { char type; char pad[3]; uint32 length; uint64 time_us } and `length`
 *     bytes. Type 'R' is the reply to a command (text; starts with "!" on failure), type 'F' is one frame.
 *
 * One client at a time. The camera is open only while a client is connected. The port listens on 127.0.0.1
 * only, and the secret (a file only its owner can read) keeps other local users from reading the camera: a
 * client has to answer the challenge within a second or is dropped. The secret itself never crosses the
 * connection, so a program that got hold of the port first learns nothing it could use here; and because the
 * bridge proves in its "ok" that it knows the secret too, such a program cannot pass for the bridge and feed
 * the client pictures of its own. The two proofs are made under different labels, so one cannot be handed
 * back as the other, and the bridge gives its proof only to a client that has given its own.
 *
 * usage: any2odyssey-camera-bridge <vendor:product | /dev/videoN> <port> <secret file>
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <arpa/inet.h>
#include <linux/videodev2.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

#include "sha256.h"

#define NBUF 4
/* How long a client may take over the handshake. The real one answers within milliseconds (it reads its
 * secret before it connects), and only one client is served at a time: with seconds each, a few connections
 * that say nothing kept the real client waiting beyond the time it gives a command. */
#define AUTH_TIMEOUT_MS 1000
/* How long a send may make no headway before the client is given up: one that has stopped reading must not
 * hold the bridge, and with it the camera, for good. */
#define SEND_TIMEOUT_S 5

static int64_t monotonic_ms(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) < 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static int cam = -1, conn = -1, gone;
static struct { void *start; size_t length; } bufs[NBUF];
static int nbufs, streaming;

static int xioctl(int fd, unsigned long req, void *arg)
{
    int r;
    do r = ioctl(fd, req, arg); while (r < 0 && errno == EINTR);
    return r;
}

static void put(char type, uint64_t time_us, const void *data, uint32_t length)
{
    unsigned char head[16] = { type };
    struct iovec parts[2] = { { head, 16 }, { (void *)data, length } };
    size_t skip = 0, total = 16 + (size_t)length;

    memcpy(head + 4, &length, 4);
    memcpy(head + 8, &time_us, 8);
    while (!gone && skip < total) {
        struct iovec v[2] = { parts[0], parts[1] };
        size_t s = skip;
        for (int i = 0; i < 2; i++) {
            size_t cut = s < v[i].iov_len ? s : v[i].iov_len;
            v[i].iov_base = (char *)v[i].iov_base + cut;
            v[i].iov_len -= cut;
            s -= cut;
        }
        ssize_t n = writev(conn, v, 2);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) gone = 1; /* the client went away, or took nothing for SEND_TIMEOUT_S */
        else skip += n;
    }
}

static void reply(const char *fmt, ...)
{
    char text[8192];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(text, sizeof text, fmt, ap);
    va_end(ap);
    put('R', 0, text, n < (int)sizeof text ? n : (int)sizeof text - 1);
}

/* The first capture node of the USB device with these ids. */
static int open_by_id(unsigned vendor, unsigned product)
{
    DIR *d = opendir("/sys/class/video4linux");
    struct dirent *e;
    char path[512], best[64] = "";
    unsigned v, p, index, best_index = ~0u;
    FILE *f;

    if (!d) return -1;
    while ((e = readdir(d))) {
        if (strncmp(e->d_name, "video", 5)) continue;
        snprintf(path, sizeof path, "/sys/class/video4linux/%s/device/../idVendor", e->d_name);
        if (!(f = fopen(path, "r"))) continue;
        if (fscanf(f, "%x", &v) != 1) v = 0;
        fclose(f);
        snprintf(path, sizeof path, "/sys/class/video4linux/%s/device/../idProduct", e->d_name);
        if (!(f = fopen(path, "r"))) continue;
        if (fscanf(f, "%x", &p) != 1) p = 0;
        fclose(f);
        if (v != vendor || p != product) continue;
        snprintf(path, sizeof path, "/sys/class/video4linux/%s/index", e->d_name);
        index = 0;
        if ((f = fopen(path, "r"))) {
            if (fscanf(f, "%u", &index) != 1) index = 0;
            fclose(f);
        }
        if (index < best_index) {
            best_index = index;
            snprintf(best, sizeof best, "/dev/%.50s", e->d_name);
        }
    }
    closedir(d);
    return *best ? open(best, O_RDWR | O_NONBLOCK) : -1;
}

static void stop(void)
{
    enum v4l2_buf_type type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    struct v4l2_requestbuffers req = { .type = type, .memory = V4L2_MEMORY_MMAP };

    if (streaming) xioctl(cam, VIDIOC_STREAMOFF, &type);
    streaming = 0;
    for (int i = 0; i < nbufs; i++) munmap(bufs[i].start, bufs[i].length);
    if (nbufs) xioctl(cam, VIDIOC_REQBUFS, &req); /* count 0 frees them */
    nbufs = 0;
}

static const char *start(const char *fourcc, unsigned width, unsigned height, unsigned fps)
{
    enum v4l2_buf_type type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    struct v4l2_format fmt = { .type = type };
    struct v4l2_streamparm parm = { .type = type };
    struct v4l2_requestbuffers req = { .count = NBUF, .type = type, .memory = V4L2_MEMORY_MMAP };

    stop();
    fmt.fmt.pix.width = width;
    fmt.fmt.pix.height = height;
    fmt.fmt.pix.pixelformat = v4l2_fourcc(fourcc[0], fourcc[1], fourcc[2], fourcc[3]);
    fmt.fmt.pix.field = V4L2_FIELD_ANY;
    if (xioctl(cam, VIDIOC_S_FMT, &fmt) < 0) return "the camera refused the format";
    if (fmt.fmt.pix.width != width || fmt.fmt.pix.height != height) return "the camera changed the size";
    parm.parm.capture.timeperframe.numerator = 1;
    parm.parm.capture.timeperframe.denominator = fps ? fps : 30;
    xioctl(cam, VIDIOC_S_PARM, &parm); /* best effort */
    if (xioctl(cam, VIDIOC_REQBUFS, &req) < 0 || req.count < 2) return "no capture buffers";
    for (unsigned i = 0; i < req.count && i < NBUF; i++) {
        struct v4l2_buffer b = { .type = type, .memory = V4L2_MEMORY_MMAP, .index = i };
        if (xioctl(cam, VIDIOC_QUERYBUF, &b) < 0) return "cannot query a capture buffer";
        bufs[i].length = b.length;
        bufs[i].start = mmap(NULL, b.length, PROT_READ | PROT_WRITE, MAP_SHARED, cam, b.m.offset);
        if (bufs[i].start == MAP_FAILED) return "cannot map a capture buffer";
        nbufs = i + 1;
        if (xioctl(cam, VIDIOC_QBUF, &b) < 0) return "cannot queue a capture buffer";
    }
    if (xioctl(cam, VIDIOC_STREAMON, &type) < 0) return "the camera would not start";
    streaming = 1;
    return NULL;
}

static void list_formats(void)
{
    char text[8192];
    size_t n = 0;

    for (unsigned i = 0;; i++) {
        struct v4l2_fmtdesc fd = { .index = i, .type = V4L2_BUF_TYPE_VIDEO_CAPTURE };
        if (xioctl(cam, VIDIOC_ENUM_FMT, &fd) < 0) break;
        for (unsigned j = 0;; j++) {
            struct v4l2_frmsizeenum fs = { .index = j, .pixel_format = fd.pixelformat };
            if (xioctl(cam, VIDIOC_ENUM_FRAMESIZES, &fs) < 0 || fs.type != V4L2_FRMSIZE_TYPE_DISCRETE) break;
            for (unsigned k = 0;; k++) {
                struct v4l2_frmivalenum fi = { .index = k, .pixel_format = fd.pixelformat,
                                               .width = fs.discrete.width, .height = fs.discrete.height };
                if (xioctl(cam, VIDIOC_ENUM_FRAMEINTERVALS, &fi) < 0 || fi.type != V4L2_FRMIVAL_TYPE_DISCRETE)
                    break;
                if (!fi.discrete.numerator || n > sizeof text - 64) continue;
                n += snprintf(text + n, sizeof text - n, "%c%c%c%c %u %u %u\n", fd.pixelformat & 0xff,
                              (fd.pixelformat >> 8) & 0xff, (fd.pixelformat >> 16) & 0xff,
                              (fd.pixelformat >> 24) & 0xff, fs.discrete.width, fs.discrete.height,
                              (fi.discrete.denominator + fi.discrete.numerator / 2) / fi.discrete.numerator);
            }
        }
    }
    put('R', 0, text, n);
}

static void command(char *line)
{
    char fourcc[8] = "";
    unsigned a, b, c;
    int value;

    if (line[0] == 'L') {
        list_formats();
    } else if (sscanf(line, "F %4s %u %u %u", fourcc, &a, &b, &c) == 4 && strlen(fourcc) == 4) {
        const char *error = start(fourcc, a, b, c);
        if (error) {
            stop();
            reply("!%s", error);
        } else {
            reply("ok");
        }
    } else if (line[0] == 'S') {
        stop();
        reply("ok");
    } else if (sscanf(line, "Q %x", &a) == 1) {
        struct v4l2_queryctrl q = { .id = a };
        struct v4l2_control ctl = { .id = a };
        if (xioctl(cam, VIDIOC_QUERYCTRL, &q) < 0 || (q.flags & V4L2_CTRL_FLAG_DISABLED)) {
            reply("!no such control");
        } else {
            xioctl(cam, VIDIOC_G_CTRL, &ctl);
            reply("%d %d %d %d %d", q.minimum, q.maximum, q.step, q.default_value, ctl.value);
        }
    } else if (sscanf(line, "C %x %d", &a, &value) == 2) {
        struct v4l2_control ctl = { .id = a, .value = value };
        if (xioctl(cam, VIDIOC_S_CTRL, &ctl) < 0) reply("!%s", strerror(errno));
        else reply("ok");
    } else {
        reply("!unknown command");
    }
}

static int open_camera(const char *name)
{
    unsigned vendor, product;
    if (name[0] == '/') return open(name, O_RDWR | O_NONBLOCK);
    if (sscanf(name, "%x:%x", &vendor, &product) == 2) return open_by_id(vendor, product);
    return -1;
}

/* Serve one client until it disconnects. */
static void serve(const char *camera, const char *secret)
{
    char line[256], nonce[33] = "", challenge[33] = "", message[128], expected[65];
    size_t have = 0;
    int authorised = 0;
    int64_t now = monotonic_ms();
    if (now < 0) return;
    const int64_t deadline = now + AUTH_TIMEOUT_MS;

    gone = 0;
    while (!gone) {
        struct pollfd fds[2] = { { conn, POLLIN, 0 }, { cam, POLLIN, 0 } };
        /* One deadline for the whole handshake: partial bytes, the challenge and interrupted polls
         * must not renew an unauthenticated client's hold on the only connection. */
        int timeout = -1;
        if (!authorised) {
            now = monotonic_ms();
            if (now < 0 || now >= deadline) return;
            timeout = (int)(deadline - now);
        }
        int ready = poll(fds, streaming ? 2 : 1, timeout);
        if (ready < 0) {
            if (errno == EINTR) continue;
            break;
        }
        if (!ready) return;
        if (fds[0].revents) {
            ssize_t n = read(conn, line + have, sizeof line - 1 - have);
            char *end;
            if (n <= 0) break;
            have += n;
            line[have] = 0;
            while ((end = strchr(line, '\n'))) {
                if (!authorised && ((now = monotonic_ms()) < 0 || now >= deadline)) return;
                *end = 0;
                if (end > line && end[-1] == '\r') end[-1] = 0;
                if (!authorised && !challenge[0]) {
                    unsigned char random[16];
                    int source = open("/dev/urandom", O_RDONLY | O_CLOEXEC);
                    if (strncmp(line, "H ", 2) || strlen(line + 2) != 32 || strspn(line + 2, "0123456789abcdef") != 32 ||
                        source < 0 || read(source, random, sizeof random) != sizeof random) {
                        if (source >= 0) close(source);
                        return;
                    }
                    close(source);
                    memcpy(nonce, line + 2, 33);
                    for (int i = 0; i < 16; i++) snprintf(challenge + i * 2, 3, "%02x", random[i]);
                    reply("%s", challenge);
                } else if (!authorised) {
                    snprintf(message, sizeof message, "client:%s:%s", challenge, nonce);
                    a2o_hmac_hex(secret, message, expected);
                    if (line[0] != 'A' || line[1] != ' ' || !a2o_same(line + 2, expected)) return;
                    if ((cam = open_camera(camera)) < 0) {
                        reply("!cannot open the camera");
                        return;
                    }
                    authorised = 1;
                    snprintf(message, sizeof message, "bridge:%s:%s", nonce, challenge);
                    a2o_hmac_hex(secret, message, expected);
                    reply("ok %s", expected);
                } else {
                    command(line);
                }
                have -= end + 1 - line;
                memmove(line, end + 1, have + 1);
            }
            if (have == sizeof line - 1) return; /* overlong junk */
        }
        if (streaming && (fds[1].revents & POLLIN)) {
            struct v4l2_buffer b = { 0 };
            int have = 0;
            /* Take every picture that is waiting and send the newest only. More than one waits when the
             * client was slow to take the last one and the send above it blocked meanwhile. The client
             * stamps a picture with the time it arrives, so old ones sent now would pass for new ones and
             * the tracker would place the eyes where they were. (The camera is open non-blocking: the
             * loop ends at the first buffer that is not ready.) */
            for (int i = 0; i < NBUF; i++) {
                struct v4l2_buffer next = { .type = V4L2_BUF_TYPE_VIDEO_CAPTURE, .memory = V4L2_MEMORY_MMAP };
                if (xioctl(cam, VIDIOC_DQBUF, &next) < 0) break;
                if (have) xioctl(cam, VIDIOC_QBUF, &b);
                b = next;
                have = 1;
            }
            if (have) {
                /* (an empty buffer is what this camera sends after a USB suspend: not a picture) */
                if (!(b.flags & V4L2_BUF_FLAG_ERROR) && b.index < (unsigned)nbufs && b.bytesused)
                    put('F', (uint64_t)b.timestamp.tv_sec * 1000000 + b.timestamp.tv_usec, bufs[b.index].start,
                        b.bytesused);
                xioctl(cam, VIDIOC_QBUF, &b);
            }
        }
        if (streaming && (fds[1].revents & (POLLERR | POLLHUP | POLLNVAL))) break; /* the camera was unplugged */
    }
}

int main(int argc, char **argv)
{
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    const struct timeval patience = { .tv_sec = SEND_TIMEOUT_S };
    char secret[128] = "";
    int listener, one = 1;
    FILE *f;

    if (argc != 4) {
        fprintf(stderr, "usage: %s <vendor:product | /dev/videoN> <port> <secret file>\n", argv[0]);
        return 2;
    }
    if (!(f = fopen(argv[3], "r")) || !fgets(secret, sizeof secret, f) || strlen(secret) < 16) {
        fprintf(stderr, "%s: no usable secret in %s\n", argv[0], argv[3]);
        return 1;
    }
    fclose(f);
    secret[strcspn(secret, "\r\n")] = 0;

    signal(SIGPIPE, SIG_IGN);
    addr.sin_port = htons(atoi(argv[2]));
    listener = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    if (listener < 0 || bind(listener, (struct sockaddr *)&addr, sizeof addr) < 0 || listen(listener, 1) < 0) {
        fprintf(stderr, "%s: cannot listen on 127.0.0.1:%s: %s\n", argv[0], argv[2], strerror(errno));
        return 1;
    }
    for (;;) {
        conn = accept4(listener, NULL, NULL, SOCK_CLOEXEC);
        if (conn < 0) {
            if (errno == EINTR) continue;
            return 1;
        }
        setsockopt(conn, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
        setsockopt(conn, SOL_SOCKET, SO_SNDTIMEO, &patience, sizeof patience);
        serve(argv[1], secret);
        stop();
        if (cam >= 0) close(cam);
        cam = -1;
        close(conn);
    }
}
