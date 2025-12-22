#include "affine_serial.h"

#include <string.h>
#include <wchar.h>

#include "dprintf.h"

#define AFFINE_SERIAL_LOG_INTERVAL_MS 5000

void affine_serial_device_init(
        struct affine_serial_device *dev,
        uint16_t vid,
        uint16_t pid)
{
    if (dev == NULL) {
        return;
    }

    memset(dev, 0, sizeof(*dev));
    dev->vid = vid;
    dev->pid = pid;
    dev->port.handle = INVALID_HANDLE_VALUE;
}

void affine_serial_device_set_pid(
        struct affine_serial_device *dev,
        uint16_t pid)
{
    if (dev == NULL) {
        return;
    }

    dev->pid = pid;
}

static bool affine_serial_log_gate(DWORD *last_log)
{
    DWORD now;

    if (last_log == NULL) {
        return true;
    }

    now = GetTickCount();
    if ((DWORD) (now - *last_log) >= AFFINE_SERIAL_LOG_INTERVAL_MS) {
        *last_log = now;
        return true;
    }

    return false;
}

bool affine_serial_device_try_open(struct affine_serial_device *dev, DWORD baud)
{
    wchar_t port_path[32];
    const wchar_t *port_label;

    if (dev == NULL) {
        return false;
    }

    if (dev->pid == 0) {
        if (affine_serial_log_gate(&dev->last_scan_log)) {
            dprintf("[Affine IO] Serial: PID not set for VID_%04X\n", dev->vid);
        }
        return false;
    }

    if (!serial_find_com_port(dev->vid, dev->pid,
                port_path, sizeof(port_path) / sizeof(port_path[0]))) {
        if (affine_serial_log_gate(&dev->last_scan_log)) {
            dprintf("[Affine IO] Serial: Device not found (VID_%04X PID_%04X)\n",
                    dev->vid, dev->pid);
        }
        return false;
    }

    if (!serial_open(&dev->port, port_path, baud)) {
        if (affine_serial_log_gate(&dev->last_scan_log)) {
            dprintf("[Affine IO] Serial: Failed to open %S\n", port_path);
        }
        return false;
    }

    port_label = port_path;
    if (port_path[0] == L'\\' && port_path[1] == L'\\' &&
            port_path[2] == L'.' && port_path[3] == L'\\') {
        port_label = port_path + 4;
    }

    dev->connected = true;
    dprintf("[Affine IO] Serial: Connected %S\n", port_label);

    return true;
}

void affine_serial_device_close(struct affine_serial_device *dev)
{
    if (dev == NULL) {
        return;
    }

    serial_close(&dev->port);
    dev->connected = false;
}
