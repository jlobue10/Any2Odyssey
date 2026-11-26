/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-scaler: tells the Odyssey 3D's scaler to enter or leave its 3D mode. Experimental, and not
 * part of the ordinary set-up: nothing runs it unless the user has asked for it (docs/LENS-SWITCH.md,
 * "The scaler's 3D mode").
 *
 * Besides the lens board and the webcam, the monitor's USB hub carries a mass-storage style device whose
 * SCSI vendor name is "GCREADER" (Linux gives it a /dev/sgN). A vendor command to it switches a "3D mode" of
 * the monitor's scaler. The Odyssey 3D output of Any2Odyssey has been seen working without this command
 * (Leia's service under Wine does not send it and does not know the device), so what the mode changes on the
 * panel is not known here; this program exists so that it can be tried with and without.
 *
 * The command is the one odysseyvk-linux sends (the-cave-dweller, MIT; src/scaler.cpp), which has it from
 * m-j-tilley's Linux_3D_SBS_Player: a 16-byte CDB E9 61 00 00 00 00 00 00 02 00..., with 512 bytes of data
 * to the device of which byte 0 is 1 for on and 0 for off and byte 4 is D2. Nothing else is sent, and
 * nothing is read from the device.
 *
 * usage: any2odyssey-scaler on|off [--dry]
 *   --dry   find the device and print the command, send nothing
 * Prints "on" or "off" when the device took the command, else "error: <reason>" and leaves with 1.
 *
 * For tests: ANY2ODYSSEY_SCALER_SYSFS is looked in instead of /sys/class/scsi_generic, and
 * ANY2ODYSSEY_SCALER_DEV instead of /dev.
 */
#define _GNU_SOURCE
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <scsi/sg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define VENDOR "GCREADER"
#define DATA_BYTES 512

static const unsigned char COMMAND[16] = {0xE9, 0x61, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 0, 0, 0, 0};

static int fail(const char *what, const char *detail)
{
    if (detail)
        printf("error: %s: %s\n", what, detail);
    else
        printf("error: %s\n", what);
    return 1;
}

/* Whether the SCSI device `name` under `sysfs` says its vendor is the scaler's. */
static int is_scaler(const char *sysfs, const char *name)
{
    char path[4096], vendor[64];
    size_t n;
    FILE *f;

    if (snprintf(path, sizeof path, "%s/%s/device/vendor", sysfs, name) >= (int)sizeof path) return 0;
    f = fopen(path, "r");
    if (!f) return 0;
    n = fread(vendor, 1, sizeof vendor - 1, f);
    fclose(f);
    vendor[n] = 0;
    for (char *c = vendor; *c; c++) *c = (char)toupper((unsigned char)*c);
    return strstr(vendor, VENDOR) != NULL;
}

/* The name (sgN) of the scaler's device into `found`: the first in the order of the names, as the same
 * machine then gets the same one every time. 0 when there is none. */
static int find_scaler(const char *sysfs, char *found, size_t size)
{
    DIR *dir = opendir(sysfs);
    struct dirent *e;

    found[0] = 0;
    if (!dir) return 0;
    while ((e = readdir(dir))) {
        if (e->d_name[0] == '.' || strlen(e->d_name) >= size) continue;
        if (!is_scaler(sysfs, e->d_name)) continue;
        if (!found[0] || strcmp(e->d_name, found) < 0) strcpy(found, e->d_name);
    }
    closedir(dir);
    return found[0] != 0;
}

int main(int argc, char **argv)
{
    const char *sysfs = getenv("ANY2ODYSSEY_SCALER_SYSFS");
    const char *dev = getenv("ANY2ODYSSEY_SCALER_DEV");
    unsigned char data[DATA_BYTES] = {0}, command[sizeof COMMAND], sense[32] = {0};
    char name[256], path[4096], reason[128];
    sg_io_hdr_t io;
    int on, dry = 0, fd;

    if (!sysfs || !*sysfs) sysfs = "/sys/class/scsi_generic";
    if (!dev || !*dev) dev = "/dev";
    if (argc == 3 && !strcmp(argv[2], "--dry")) dry = 1;
    if (argc != 2 + dry || (strcmp(argv[1], "on") && strcmp(argv[1], "off"))) {
        fprintf(stderr, "usage: any2odyssey-scaler on|off [--dry]\n");
        return 2;
    }
    on = !strcmp(argv[1], "on");

    if (!find_scaler(sysfs, name, sizeof name))
        return fail("the monitor's scaler was not found (no SCSI device of the vendor " VENDOR
                    "; is the monitor's USB cable connected?)", NULL);
    if (snprintf(path, sizeof path, "%s/%s", dev, name) >= (int)sizeof path) return fail("path too long", name);

    data[0] = on ? 1 : 0;
    data[4] = 0xD2;
    memcpy(command, COMMAND, sizeof command);
    if (dry) {
        printf("%s\ncommand:", path);
        for (size_t i = 0; i < sizeof command; i++) printf(" %02X", command[i]);
        printf("\ndata (%d bytes):", DATA_BYTES);
        for (size_t i = 0; i < 8; i++) printf(" %02X", data[i]);
        printf(" ...\n");
        return 0;
    }

    fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        int why = errno;
        if (why == EACCES || why == EPERM)
            return fail(path, "no permission (install 71-any2odyssey-scaler.rules, see docs/LENS-SWITCH.md)");
        return fail(path, strerror(why));
    }
    memset(&io, 0, sizeof io);
    io.interface_id = 'S';
    io.dxfer_direction = SG_DXFER_TO_DEV;
    io.cmd_len = sizeof command;
    io.cmdp = command;
    io.dxfer_len = sizeof data;
    io.dxferp = data;
    io.mx_sb_len = sizeof sense;
    io.sbp = sense;
    io.timeout = 5000; /* milliseconds */
    if (ioctl(fd, SG_IO, &io) < 0) {
        int why = errno;
        close(fd);
        return fail("the command could not be sent", strerror(why));
    }
    close(fd);
    if (io.status || io.host_status || io.driver_status) {
        snprintf(reason, sizeof reason, "status %d, host %d, driver %d", io.status, io.host_status,
                 io.driver_status);
        return fail("the scaler refused the command", reason);
    }
    puts(on ? "on" : "off");
    return 0;
}
