/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-noxon: keeps software flow control off on the lens board's serial port while Leia's service
 * runs under Wine.
 *
 * Wine turns IXON on when a program asks for XON/XOFF, which eats bytes of the board's replies; Windows' USB
 * serial driver never acts on those bytes. Twenty times a second this looks at the port's line settings and,
 * when IXON is on, switches IXON and IXOFF off again. It changes line settings only; it reads and writes
 * nothing. Every look opens the port and closes it again, as `stty -F` does, so the port is held open no
 * longer than it was by the shell loop this replaces (tools/noxon.sh, which started two or three programs
 * for every look and is still what runs where this could not be built).
 *
 * It leaves by itself when the program it keeps the port for has gone, or never came: nothing else ends it
 * when Leia's service stops.
 *
 * usage: any2odyssey-noxon <tty> <name> [seconds]
 *   <tty>      the port, /dev/odyssey-lens
 *   <name>     text in the command line of the program to stay for (SRService.exe)
 *   [seconds]  how often to look for that program (default 5)
 */
#define _GNU_SOURCE
#include <ctype.h>
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

#define LOOKS_A_SECOND 20
#define MISSES_ONCE_SEEN 2 /* the program was there and is not, twice in a row: it has gone */
#define MISSES_AT_START 24 /* it never came (two minutes at the default; a start takes about half a minute) */

static const char *base_name(const char *path)
{
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

/* Whether another process of this user has `name` in its command line. Another copy of this program
 * (`self` is how this one was started) has, as one of its arguments, and is not what is looked for: two
 * keepers would each take the other for the program and neither would ever leave. */
static int running(const char *name, const char *self)
{
    DIR *proc = opendir("/proc");
    struct dirent *e;
    char path[64], line[4096];
    size_t length = strlen(name);
    uid_t me = getuid();
    int found = 0;

    if (!proc) return 1; /* cannot tell: stay */
    while (!found && (e = readdir(proc))) {
        struct stat st;
        ssize_t n;
        int fd;

        if (!isdigit((unsigned char)e->d_name[0]) || atol(e->d_name) == (long)getpid()) continue;
        snprintf(path, sizeof path, "/proc/%.20s/cmdline", e->d_name);
        if ((fd = open(path, O_RDONLY | O_CLOEXEC)) < 0) continue;
        if (!fstat(fd, &st) && st.st_uid == me && (n = read(fd, line, sizeof line - 1)) > 0) {
            line[n] = 0; /* (the program's own name ends at the first zero in any case) */
            found = strcmp(base_name(line), base_name(self)) && memmem(line, (size_t)n, name, length) != NULL;
        }
        close(fd);
    }
    closedir(proc);
    return found;
}

int main(int argc, char **argv)
{
    const struct timespec tick = { 0, 1000000000 / LOOKS_A_SECOND };
    long seconds = argc > 3 ? atol(argv[3]) : 5;
    int seen = 0, missed = 0;

    /* (an interval whose count of looks does not fit a long would divide by zero, or by anything, below) */
    if (argc < 3 || argc > 4 || seconds < 1 || seconds > LONG_MAX / LOOKS_A_SECOND || !argv[2][0]) {
        fprintf(stderr, "usage: %s <tty> <name> [seconds]\n", argv[0]);
        return 2;
    }
    for (long look = 1;; look++) {
        /* (not as a controlling terminal: this program is started as the leader of a session of its own) */
        int fd = open(argv[1], O_RDONLY | O_NONBLOCK | O_NOCTTY | O_CLOEXEC);
        struct termios settings;

        if (fd >= 0) { /* the port is not there while the monitor sleeps */
            if (!tcgetattr(fd, &settings) && (settings.c_iflag & IXON)) {
                settings.c_iflag &= ~(tcflag_t)(IXON | IXOFF);
                tcsetattr(fd, TCSADRAIN, &settings); /* as stty sets them */
            }
            close(fd);
        }
        nanosleep(&tick, NULL);
        if (look % (seconds * LOOKS_A_SECOND)) continue;
        if (running(argv[2], argv[0])) {
            seen = 1;
            missed = 0;
        } else if (++missed >= (seen ? MISSES_ONCE_SEEN : MISSES_AT_START)) {
            return 0;
        }
    }
}
