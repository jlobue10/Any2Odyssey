/* SPDX-License-Identifier: MIT
 *
 * a2ousb.sys: a minimal USB host-controller + root-hub "information" driver
 * for Wine. Wine has no USB hub devices, so programs that read a device's
 * descriptors the Windows way (open the host controller, ask for its root
 * hub, ask the hub about each port) find nothing.
 *
 * This driver answers exactly those read-only questions for one device, the
 * one whose descriptors are compiled in from board.h (generated from Linux
 * sysfs). It transfers no data to or from the device.
 *
 *   \\.\A2OHcd0  host controller: root hub name, driver key name
 *   \\.\A2OHub0  root hub with one port (index 1) holding the device
 */
#include <ddk/wdm.h>
#include "board.h"

#define IOCTL_GET_NODE_INFO_OR_ROOT_HUB_NAME 0x220408 /* same code, meaning depends on the device */
#define IOCTL_GET_NODE_CONNECTION_INFORMATION 0x22040c
#define IOCTL_GET_DESCRIPTOR_FROM_NODE_CONNECTION 0x220410
#define IOCTL_GET_NODE_CONNECTION_NAME 0x220414
#define IOCTL_GET_NODE_CONNECTION_DRIVERKEY_NAME 0x220420
#define IOCTL_GET_HCD_DRIVERKEY_NAME 0x220424
#define IOCTL_GET_NODE_CONNECTION_INFORMATION_EX 0x220448

static const WCHAR HUB_NAME[] = L"A2OHub0";
static const WCHAR HCD_DRIVER_KEY[] = L"{36fc9e60-c465-11cf-8056-444553540000}\\0000";
/* Must equal the "Driver" value of the device's registry entry. */
static const WCHAR DEVICE_DRIVER_KEY[] = L"{36fc9e60-c465-11cf-8056-444553540000}\\0116";

static const WCHAR HCD_INTERFACE_LINK[] =
    L"\\DosDevices\\root#a2ousb#0000#{3abf6f2d-71c4-462a-8a92-1e6861e6af27}";

static DEVICE_OBJECT *hcd_device, *hub_device;

static ULONG wlen_bytes(const WCHAR *s)
{
    ULONG n = 0;
    while (s[n]) n++;
    return (n + 1) * sizeof(WCHAR);
}

/* Write `prefix` ULONGs worth of header space, then ActualLength, then the name; all packed. */
static NTSTATUS put_name(UCHAR *buf, ULONG out_len, ULONG header, const WCHAR *name, ULONG_PTR *written)
{
    ULONG name_bytes = wlen_bytes(name);
    ULONG actual = header + sizeof(ULONG) + name_bytes;
    ULONG copy;

    if (out_len < header + sizeof(ULONG)) return STATUS_BUFFER_TOO_SMALL;
    RtlCopyMemory(buf + header, &actual, sizeof actual);
    copy = out_len - header - sizeof(ULONG);
    if (copy > name_bytes) copy = name_bytes;
    RtlCopyMemory(buf + header + sizeof(ULONG), name, copy);
    *written = header + sizeof(ULONG) + copy;
    return STATUS_SUCCESS;
}

static NTSTATUS hub_ioctl(ULONG code, UCHAR *buf, ULONG in_len, ULONG out_len, ULONG_PTR *written)
{
    ULONG index = 0;

    if (code == IOCTL_GET_NODE_INFO_OR_ROOT_HUB_NAME) {
        /* USB_NODE_INFORMATION: NodeType (4) + hub descriptor (71) + HubIsBusPowered (1) */
        if (out_len < 76) return STATUS_BUFFER_TOO_SMALL;
        RtlZeroMemory(buf, 76);
        buf[4] = 9;    /* bDescriptorLength */
        buf[5] = 0x29; /* hub descriptor */
        buf[6] = 1;    /* bNumberOfPorts */
        *written = 76;
        return STATUS_SUCCESS;
    }

    if (in_len < sizeof(ULONG)) return STATUS_INVALID_PARAMETER;
    RtlCopyMemory(&index, buf, sizeof index);

    switch (code) {
    case IOCTL_GET_NODE_CONNECTION_INFORMATION:
    case IOCTL_GET_NODE_CONNECTION_INFORMATION_EX: {
        /* index(4) descriptor(18) config(1) speed(1) isHub(1) address(2) pipes(4) status(4) */
        USHORT address = BOARD_ADDRESS;
        ULONG status = index == 1 ? 1 /* DeviceConnected */ : 0 /* NoDeviceConnected */;
        if (out_len < 35) return STATUS_BUFFER_TOO_SMALL;
        RtlZeroMemory(buf + 4, 31);
        if (index == 1) {
            RtlCopyMemory(buf + 4, board_device_desc, 18);
            buf[22] = board_config_desc[5];
            buf[23] = code == IOCTL_GET_NODE_CONNECTION_INFORMATION_EX ? 1 /* full speed */ : 0;
            RtlCopyMemory(buf + 25, &address, 2);
        }
        RtlCopyMemory(buf + 31, &status, 4);
        *written = 35;
        return STATUS_SUCCESS;
    }
    case IOCTL_GET_DESCRIPTOR_FROM_NODE_CONNECTION: {
        /* index(4) bmRequest(1) bRequest(1) wValue(2) wIndex(2) wLength(2) data... */
        UCHAR type, which, tmp[256];
        const UCHAR *src = NULL;
        ULONG len = 0, room;
        if (in_len < 12 || out_len < 12 || index != 1) return STATUS_INVALID_PARAMETER;
        which = buf[6];
        type = buf[7];
        room = out_len - 12;
        if (type == 1) {
            src = board_device_desc;
            len = sizeof board_device_desc;
        } else if (type == 2) {
            src = board_config_desc;
            len = sizeof board_config_desc;
        } else if (type == 3 && which == 0) {
            tmp[0] = 4; tmp[1] = 3; tmp[2] = 0x09; tmp[3] = 0x04; /* English (US) */
            src = tmp;
            len = 4;
        } else if (type == 3) {
            const WCHAR *s = NULL;
            ULONG i, n = 0;
            for (i = 0; i < 3; i++)
                if (board_string_index[i] == which) s = board_strings[i + 1];
            if (!s) return STATUS_INVALID_PARAMETER;
            while (s[n] && n < 126) n++;
            tmp[0] = (UCHAR)(2 + n * 2);
            tmp[1] = 3;
            RtlCopyMemory(tmp + 2, s, n * 2);
            src = tmp;
            len = tmp[0];
        } else
            return STATUS_INVALID_PARAMETER;
        if (len > room) len = room;
        RtlCopyMemory(buf + 12, src, len);
        *written = 12 + len;
        return STATUS_SUCCESS;
    }
    case IOCTL_GET_NODE_CONNECTION_DRIVERKEY_NAME:
        if (index != 1) return STATUS_INVALID_PARAMETER;
        return put_name(buf, out_len, sizeof(ULONG), DEVICE_DRIVER_KEY, written);
    case IOCTL_GET_NODE_CONNECTION_NAME: /* only hubs have a name; ours is not one */
        return put_name(buf, out_len, sizeof(ULONG), L"", written);
    }
    return STATUS_INVALID_DEVICE_REQUEST;
}

static NTSTATUS hcd_ioctl(ULONG code, UCHAR *buf, ULONG out_len, ULONG_PTR *written)
{
    if (code == IOCTL_GET_NODE_INFO_OR_ROOT_HUB_NAME) return put_name(buf, out_len, 0, HUB_NAME, written);
    if (code == IOCTL_GET_HCD_DRIVERKEY_NAME) return put_name(buf, out_len, 0, HCD_DRIVER_KEY, written);
    return STATUS_INVALID_DEVICE_REQUEST;
}

static NTSTATUS WINAPI dispatch_ioctl(DEVICE_OBJECT *device, IRP *irp)
{
    IO_STACK_LOCATION *stack = IoGetCurrentIrpStackLocation(irp);
    ULONG code = stack->Parameters.DeviceIoControl.IoControlCode;
    ULONG in_len = stack->Parameters.DeviceIoControl.InputBufferLength;
    ULONG out_len = stack->Parameters.DeviceIoControl.OutputBufferLength;
    UCHAR *buf = irp->AssociatedIrp.SystemBuffer;
    ULONG_PTR written = 0;
    NTSTATUS status;

    if (!buf)
        status = STATUS_INVALID_PARAMETER;
    else if (device == hub_device)
        status = hub_ioctl(code, buf, in_len, out_len, &written);
    else
        status = hcd_ioctl(code, buf, out_len, &written);

    irp->IoStatus.Status = status;
    irp->IoStatus.Information = status ? 0 : written;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}

static NTSTATUS WINAPI dispatch_ok(DEVICE_OBJECT *device, IRP *irp)
{
    irp->IoStatus.Status = STATUS_SUCCESS;
    irp->IoStatus.Information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return STATUS_SUCCESS;
}

static NTSTATUS make_device(DRIVER_OBJECT *driver, const WCHAR *nt_name, const WCHAR *dos_name, DEVICE_OBJECT **out)
{
    UNICODE_STRING nt, dos;
    NTSTATUS status;

    RtlInitUnicodeString(&nt, nt_name);
    RtlInitUnicodeString(&dos, dos_name);
    status = IoCreateDevice(driver, 0, &nt, FILE_DEVICE_UNKNOWN, 0, FALSE, out);
    if (status) return status;
    (*out)->Flags |= DO_BUFFERED_IO;
    (*out)->Flags &= ~DO_DEVICE_INITIALIZING;
    return IoCreateSymbolicLink(&dos, &nt);
}

static void WINAPI unload(DRIVER_OBJECT *driver)
{
    UNICODE_STRING dos;
    RtlInitUnicodeString(&dos, L"\\DosDevices\\A2OHcd0");
    IoDeleteSymbolicLink(&dos);
    RtlInitUnicodeString(&dos, L"\\DosDevices\\A2OHub0");
    IoDeleteSymbolicLink(&dos);
    RtlInitUnicodeString(&dos, HCD_INTERFACE_LINK);
    IoDeleteSymbolicLink(&dos);
    if (hcd_device) IoDeleteDevice(hcd_device);
    if (hub_device) IoDeleteDevice(hub_device);
}

NTSTATUS WINAPI DriverEntry(DRIVER_OBJECT *driver, UNICODE_STRING *path)
{
    NTSTATUS status;

    driver->MajorFunction[IRP_MJ_CREATE] = dispatch_ok;
    driver->MajorFunction[IRP_MJ_CLOSE] = dispatch_ok;
    driver->MajorFunction[IRP_MJ_DEVICE_CONTROL] = dispatch_ioctl;
    driver->DriverUnload = unload;

    status = make_device(driver, L"\\Device\\A2OHcd0", L"\\DosDevices\\A2OHcd0", &hcd_device);
    if (!status) {
        /* The path SetupDi hands out for the host-controller interface of ROOT\A2OUSB\0000. */
        UNICODE_STRING nt, link;
        RtlInitUnicodeString(&nt, L"\\Device\\A2OHcd0");
        RtlInitUnicodeString(&link, HCD_INTERFACE_LINK);
        status = IoCreateSymbolicLink(&link, &nt);
    }
    if (!status) status = make_device(driver, L"\\Device\\A2OHub0", L"\\DosDevices\\A2OHub0", &hub_device);
    if (status) unload(driver);
    return status;
}
