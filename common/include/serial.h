#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <windows.h>

typedef struct serial_port {
    HANDLE handle;
} serial_port_t;

bool serial_find_com_port(
        uint16_t vid,
        uint16_t pid,
        wchar_t *out,
        size_t out_len);

bool serial_open(serial_port_t *port, const wchar_t *path, DWORD baud);
void serial_close(serial_port_t *port);

bool serial_read(serial_port_t *port, uint8_t *buf, DWORD len, DWORD *out_read);
bool serial_write(serial_port_t *port, const uint8_t *buf, DWORD len);
