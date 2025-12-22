#include "serial.h"

#include <setupapi.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

static const GUID affine_guid_comport = {
    0x86e0d1e0, 0x8089, 0x11d0,
    {0x9c, 0xe4, 0x08, 0x00, 0x3e, 0x30, 0x1f, 0x73}
};


static bool serial_match_hwid(const wchar_t *hwid, uint16_t vid, uint16_t pid)
{
    wchar_t vid_str[16];
    wchar_t pid_str[16];
    const wchar_t *cur;

    if (hwid == NULL) {
        return false;
    }

    swprintf_s(vid_str, sizeof(vid_str) / sizeof(vid_str[0]),
            L"VID_%04X", vid);
    swprintf_s(pid_str, sizeof(pid_str) / sizeof(pid_str[0]),
            L"PID_%04X", pid);

    for (cur = hwid; *cur != L'\0'; cur += wcslen(cur) + 1) {
        if (wcsstr(cur, vid_str) != NULL && wcsstr(cur, pid_str) != NULL) {
            return true;
        }
    }

    return false;
}

static bool serial_parse_com_name(const wchar_t *name,
        wchar_t *out, size_t out_len)
{
    const wchar_t *com;
    wchar_t tmp[32];
    size_t i;

    if (name == NULL) {
        return false;
    }

    com = wcsstr(name, L"COM");
    if (com == NULL) {
        return false;
    }

    for (i = 0; i < (sizeof(tmp) / sizeof(tmp[0])) - 1 && com[i] != L'\0'; i++) {
        if (com[i] == L')') {
            break;
        }
        tmp[i] = com[i];
    }
    tmp[i] = L'\0';

    if (tmp[0] == L'\0') {
        return false;
    }

    swprintf_s(out, out_len, L"\\\\.\\%s", tmp);

    return true;
}

bool serial_find_com_port(
        uint16_t vid,
        uint16_t pid,
        wchar_t *out,
        size_t out_len)
{
    HDEVINFO info;
    DWORD index = 0;
    bool found = false;

    if (out == NULL || out_len == 0) {
        return false;
    }

    info = SetupDiGetClassDevsW(&affine_guid_comport,
            NULL, NULL, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
    if (info == INVALID_HANDLE_VALUE) {
        return false;
    }

    for (;;) {
        SP_DEVICE_INTERFACE_DATA if_data;
        SP_DEVINFO_DATA dev_info;
        DWORD required = 0;
        BYTE hwid[512];
        BYTE prop[256];
        DWORD reg_type = 0;
        PSP_DEVICE_INTERFACE_DETAIL_DATA_W detail;

        memset(&if_data, 0, sizeof(if_data));
        if_data.cbSize = sizeof(if_data);

        if (!SetupDiEnumDeviceInterfaces(info, NULL,
                    &affine_guid_comport, index++, &if_data)) {
            break;
        }

        memset(&dev_info, 0, sizeof(dev_info));
        dev_info.cbSize = sizeof(dev_info);

        SetupDiGetDeviceInterfaceDetailW(info, &if_data, NULL, 0,
                &required, NULL);

        detail = malloc(required);
        if (detail == NULL) {
            continue;
        }

        detail->cbSize = sizeof(*detail);
        if (!SetupDiGetDeviceInterfaceDetailW(info, &if_data, detail,
                    required, NULL, &dev_info)) {
            free(detail);
            continue;
        }

        if (SetupDiGetDeviceRegistryPropertyW(info, &dev_info,
                    SPDRP_HARDWAREID, &reg_type, hwid,
                    sizeof(hwid), NULL)) {
            if (serial_match_hwid((const wchar_t *) hwid, vid, pid)) {
                if (SetupDiGetDeviceRegistryPropertyW(info, &dev_info,
                            SPDRP_FRIENDLYNAME, &reg_type, prop,
                            sizeof(prop), NULL)) {
                    found = serial_parse_com_name((const wchar_t *) prop,
                            out, out_len);
                }
            }
        }

        free(detail);
        if (found) {
            break;
        }
    }

    SetupDiDestroyDeviceInfoList(info);

    return found;
}

bool serial_open(serial_port_t *port, const wchar_t *path, DWORD baud)
{
    DCB dcb;
    COMMTIMEOUTS timeouts;
    BOOL ok;

    if (port == NULL || path == NULL) {
        return false;
    }

    port->handle = CreateFileW(path,
            GENERIC_READ | GENERIC_WRITE,
            0,
            NULL,
            OPEN_EXISTING,
            0,
            NULL);

    if (port->handle == INVALID_HANDLE_VALUE) {
        return false;
    }

    memset(&dcb, 0, sizeof(dcb));
    dcb.DCBlength = sizeof(dcb);
    ok = GetCommState(port->handle, &dcb);
    if (!ok) {
        serial_close(port);
        return false;
    }

    dcb.BaudRate = baud;
    dcb.ByteSize = 8;
    dcb.StopBits = ONESTOPBIT;
    dcb.Parity = NOPARITY;
    dcb.fDtrControl = DTR_CONTROL_ENABLE;
    dcb.fRtsControl = RTS_CONTROL_ENABLE;

    ok = SetCommState(port->handle, &dcb);
    if (!ok) {
        serial_close(port);
        return false;
    }

    memset(&timeouts, 0, sizeof(timeouts));
    timeouts.ReadIntervalTimeout = 20;
    timeouts.ReadTotalTimeoutConstant = 20;
    timeouts.ReadTotalTimeoutMultiplier = 5;
    timeouts.WriteTotalTimeoutConstant = 50;
    timeouts.WriteTotalTimeoutMultiplier = 5;
    SetCommTimeouts(port->handle, &timeouts);

    return true;
}

void serial_close(serial_port_t *port)
{
    if (port == NULL) {
        return;
    }

    if (port->handle != NULL && port->handle != INVALID_HANDLE_VALUE) {
        CloseHandle(port->handle);
    }

    port->handle = INVALID_HANDLE_VALUE;
}

bool serial_read(serial_port_t *port, uint8_t *buf, DWORD len, DWORD *out_read)
{
    BOOL ok;

    if (out_read != NULL) {
        *out_read = 0;
    }

    if (port == NULL || port->handle == INVALID_HANDLE_VALUE || buf == NULL) {
        return false;
    }

    ok = ReadFile(port->handle, buf, len, out_read, NULL);
    if (!ok) {
        DWORD err = GetLastError();
        if (err == ERROR_INVALID_HANDLE || err == ERROR_DEVICE_NOT_CONNECTED) {
            return false;
        }
    }

    return ok != FALSE;
}

bool serial_write(serial_port_t *port, const uint8_t *buf, DWORD len)
{
    DWORD written = 0;
    BOOL ok;

    if (port == NULL || port->handle == INVALID_HANDLE_VALUE || buf == NULL) {
        return false;
    }

    ok = WriteFile(port->handle, buf, len, &written, NULL);
    if (!ok) {
        return false;
    }

    return written == len;
}
