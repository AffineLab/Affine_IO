#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <windows.h>

#include "serial.h"

struct affine_serial_device {
    uint16_t vid;
    uint16_t pid;
    serial_port_t port;
    bool connected;
    DWORD last_scan_log;
};

void affine_serial_device_init(
        struct affine_serial_device *dev,
        uint16_t vid,
        uint16_t pid);

void affine_serial_device_set_pid(
        struct affine_serial_device *dev,
        uint16_t pid);

bool affine_serial_device_try_open(
        struct affine_serial_device *dev,
        DWORD baud);

void affine_serial_device_close(struct affine_serial_device *dev);
