#include "affine_io.h"

#include <process.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#include "dprintf.h"
#include "serial.h"

#define AFFINE_VID 0xAFF1
#define AFFINE_PID_1P 0x52A5
#define AFFINE_PID_2P 0x52A6

#define AFFINE_CMD_HEARTBEAT 0x11
#define AFFINE_CMD_GET_BOARD_INFO 0xF0

#define AFFINE_HEARTBEAT_INTERVAL_MS 100
#define AFFINE_RESCAN_INTERVAL_MS 500
#define AFFINE_BOARD_INFO_DELAY_MS 500
#define AFFINE_BOARD_INFO_TIMEOUT_MS 1000
#define AFFINE_RX_BUF_SIZE 128
#define AFFINE_SHM_NAME_1 "mai_io_shm_1"
#define AFFINE_SHM_NAME_2 "mai_io_shm_2"
#define AFFINE_SHM_SIZE 2

struct affine_device {
    uint8_t player;
    uint16_t pid;
    serial_port_t port;
    HANDLE thread;
    CRITICAL_SECTION state_lock;
    CRITICAL_SECTION write_lock;
    volatile bool stop;
    volatile bool enabled;
    bool connected;
    bool touch_enabled;
    uint8_t buttons0;
    uint8_t buttons1;
    uint8_t touch[7];
    uint8_t rx_buf[AFFINE_RX_BUF_SIZE];
    size_t rx_len;
    DWORD last_heartbeat;
    DWORD last_scan_log;
    bool board_info_pending;
    bool board_info_logged;
    bool board_info_unknown;
    DWORD board_info_start_ms;
    DWORD board_info_request_ms;
};

static struct affine_device affine_devs[2];
static bool affine_started;
static bool affine_enabled[2] = { true, true };
static affine_io_touch_callback_t affine_touch_cb;
static HANDLE affine_capture_file;
static CRITICAL_SECTION affine_capture_lock;
static bool affine_capture_ready;
static bool affine_capture_enabled;
static bool affine_capture_raw_enabled;
typedef struct affine_rx_capture_state {
    bool valid;
    uint8_t data[AFFINE_RX_BUF_SIZE];
    size_t len;
    unsigned int repeat;
} affine_rx_capture_state_t;

static affine_rx_capture_state_t affine_rx_capture_state[2];
static HANDLE affine_shm_handle[2];
static uint8_t *affine_shm_ptr[2];

static void affine_init_device(struct affine_device *dev, uint8_t player,
        uint16_t pid, bool enabled);
static bool affine_try_open(struct affine_device *dev);
static void affine_close(struct affine_device *dev);
static void affine_send_cmd(struct affine_device *dev, uint8_t cmd,
        const uint8_t *payload, size_t len);
static void affine_rx_process(struct affine_device *dev,
        const uint8_t *data, size_t len);
static void affine_rx_try_parse(struct affine_device *dev);
static void affine_handle_board_info(struct affine_device *dev,
        const uint8_t *data, size_t len);
static void affine_update_state(struct affine_device *dev,
        const uint8_t *touch, const uint8_t *buttons);
static unsigned int __stdcall affine_thread_proc(void *ctx);
static void affine_capture_init(void);
static void affine_capture_write(uint8_t player, uint8_t cmd,
        const uint8_t *payload, size_t len);
static void affine_capture_write_rx(uint8_t player, const uint8_t *data,
        size_t len);
static void affine_capture_write_raw(uint8_t player, const uint8_t *data,
        size_t len);
static void affine_shm_write_buttons(uint8_t player, uint8_t buttons0,
        uint8_t buttons1);
static bool affine_shm_read_state(uint8_t player, struct affine_io_state *out);

HRESULT affine_io_init(void)
{
    if (affine_started) {
        return S_OK;
    }

    affine_capture_init();
    affine_init_device(&affine_devs[0], 1, AFFINE_PID_1P, affine_enabled[0]);
    affine_init_device(&affine_devs[1], 2, AFFINE_PID_2P, affine_enabled[1]);

    affine_started = true;

    return S_OK;
}

void affine_io_shutdown(void)
{
    if (!affine_started) {
        return;
    }

    affine_close(&affine_devs[0]);
    affine_close(&affine_devs[1]);

    if (affine_capture_file != NULL &&
            affine_capture_file != INVALID_HANDLE_VALUE) {
        CloseHandle(affine_capture_file);
        affine_capture_file = NULL;
    }

    for (size_t i = 0; i < 2; i++) {
        if (affine_shm_ptr[i] != NULL) {
            UnmapViewOfFile(affine_shm_ptr[i]);
            affine_shm_ptr[i] = NULL;
        }
        if (affine_shm_handle[i] != NULL) {
            CloseHandle(affine_shm_handle[i]);
            affine_shm_handle[i] = NULL;
        }
    }

    affine_started = false;
}

void affine_io_set_enabled(uint8_t player, bool enable)
{
    if (player < 1 || player > 2) {
        return;
    }

    affine_enabled[player - 1] = enable;
    if (affine_started) {
        affine_devs[player - 1].enabled = enable;
    }

}

void affine_io_set_touch_callback(affine_io_touch_callback_t callback)
{
    affine_touch_cb = callback;
}

void affine_io_set_touch_enabled(uint8_t player, bool enable)
{
    if (!affine_started || player < 1 || player > 2) {
        return;
    }

    affine_devs[player - 1].touch_enabled = enable;
}

bool affine_io_get_state(uint8_t player, struct affine_io_state *out)
{
    struct affine_device *dev;

    if (out == NULL || player < 1 || player > 2) {
        return false;
    }

    if (affine_started) {
        dev = &affine_devs[player - 1];
        if (dev->connected) {
            EnterCriticalSection(&dev->state_lock);
            out->present = true;
            out->buttons0 = dev->buttons0;
            out->buttons1 = dev->buttons1;
            memcpy(out->touch, dev->touch, sizeof(out->touch));
            LeaveCriticalSection(&dev->state_lock);
            return true;
        }
    }

    return affine_shm_read_state(player, out);
}

void affine_io_send_led_buttons(uint8_t player, const uint8_t *rgb24)
{
    if (!affine_started || player < 1 || player > 2 || rgb24 == NULL) {
        return;
    }

    affine_send_cmd(&affine_devs[player - 1], 0x14, rgb24, 24);
}

void affine_io_send_led_billboard(uint8_t player, const uint8_t *rgb24)
{
    if (!affine_started || player < 1 || player > 2 || rgb24 == NULL) {
        return;
    }

    affine_send_cmd(&affine_devs[player - 1], 0x15, rgb24, 24);
}

void affine_io_send_led_pwm(uint8_t player, const uint8_t *pwm3)
{
    if (!affine_started || player < 1 || player > 2 || pwm3 == NULL) {
        return;
    }

    affine_send_cmd(&affine_devs[player - 1], 0x16, pwm3, 3);
}

static void affine_init_device(struct affine_device *dev, uint8_t player,
        uint16_t pid, bool enabled)
{
    memset(dev, 0, sizeof(*dev));
    dev->player = player;
    dev->pid = pid;
    dev->port.handle = INVALID_HANDLE_VALUE;
    dev->enabled = enabled;
    InitializeCriticalSection(&dev->state_lock);
    InitializeCriticalSection(&dev->write_lock);
    dev->thread = (HANDLE) _beginthreadex(
            NULL, 0, affine_thread_proc, dev, 0, NULL);
}

static bool affine_try_open(struct affine_device *dev)
{
    wchar_t port_path[32];
    const wchar_t *port_label;
    DWORD now;

    if (!serial_find_com_port(AFFINE_VID, dev->pid,
                port_path, sizeof(port_path) / sizeof(port_path[0]))) {
        now = GetTickCount();
        if ((DWORD)(now - dev->last_scan_log) > 5000) {
            dprintf("[Affine IO] P%u: Device not found (VID_%04X PID_%04X)\n", 
                dev->player, AFFINE_VID, dev->pid);
            dev->last_scan_log = now;
        }
        return false;
    }

    if (!serial_open(&dev->port, port_path, 115200)) {
        now = GetTickCount();
        if ((DWORD)(now - dev->last_scan_log) > 5000) {
            dprintf("[Affine IO] P%u: Failed to open port %S\n", 
                dev->player, port_path);
            dev->last_scan_log = now;
        }
        return false;
    }

    now = GetTickCount();
    dev->connected = true;
    dev->rx_len = 0;
    dev->last_heartbeat = now;
    dev->board_info_pending = true;
    dev->board_info_logged = false;
    dev->board_info_unknown = false;
    dev->board_info_start_ms = now;
    dev->board_info_request_ms = 0;

    port_label = port_path;
    if (port_path[0] == L'\\' && port_path[1] == L'\\' &&
            port_path[2] == L'.' && port_path[3] == L'\\') {
        port_label = port_path + 4;
    }
    dprintf("[Affine IO] Connected P%u: %S\n", dev->player, port_label);

    affine_shm_write_buttons(dev->player, dev->buttons0, dev->buttons1);

    return true;
}

static void affine_close(struct affine_device *dev)
{
    dev->stop = true;
    if (dev->thread != NULL) {
        WaitForSingleObject(dev->thread, INFINITE);
        CloseHandle(dev->thread);
        dev->thread = NULL;
    }

    serial_close(&dev->port);
    dev->connected = false;
    affine_shm_write_buttons(dev->player, 0, 0);
}

static void affine_send_cmd(struct affine_device *dev, uint8_t cmd,
        const uint8_t *payload, size_t len)
{
    uint8_t frame[64];
    size_t i;
    size_t total;
    uint8_t sum = 0;

    if (dev == NULL || !dev->connected) {
        return;
    }

    if (len > sizeof(frame) - 4) {
        return;
    }

    frame[0] = 0xFF;
    frame[1] = cmd;
    frame[2] = (uint8_t) len;
    if (len > 0 && payload != NULL) {
        memcpy(&frame[3], payload, len);
    }

    total = 3 + len;
    for (i = 0; i < total; i++) {
        sum += frame[i];
    }
    frame[total] = sum;
    total += 1;

    affine_capture_write(dev->player, cmd, payload, len);

    EnterCriticalSection(&dev->write_lock);
    if (!serial_write(&dev->port, frame, (DWORD) total)) {
        serial_close(&dev->port);
        dev->connected = false;
        dev->rx_len = 0;
        affine_shm_write_buttons(dev->player, 0, 0);
    }
    LeaveCriticalSection(&dev->write_lock);
}

static void affine_rx_process(struct affine_device *dev,
        const uint8_t *data, size_t len)
{
    size_t i;

    for (i = 0; i < len; i++) {
        if (dev->rx_len >= sizeof(dev->rx_buf)) {
            dev->rx_len = 0;
        }
        dev->rx_buf[dev->rx_len++] = data[i];
        affine_rx_try_parse(dev);
    }
}

static void affine_rx_consume(struct affine_device *dev, size_t count)
{
    if (count >= dev->rx_len) {
        dev->rx_len = 0;
        return;
    }

    memmove(dev->rx_buf, dev->rx_buf + count, dev->rx_len - count);
    dev->rx_len -= count;
}

static void affine_rx_try_parse(struct affine_device *dev)
{
    for (;;) {
        if (dev->rx_len == 0) {
            break;
        }

        if (dev->rx_buf[0] == 0xFF) {
            if (dev->rx_len < 3) {
                break;
            }
            if (dev->rx_buf[1] == 0x01 && dev->rx_buf[2] == 0x0A) {
                if (dev->rx_len < 14) {
                    break;
                }
                if (dev->rx_buf[13] == 0x0A) {
                    affine_capture_write_rx(dev->player, dev->rx_buf, 14);
                    affine_update_state(dev, &dev->rx_buf[6], &dev->rx_buf[3]);
                    affine_rx_consume(dev, 14);
                    continue;
                }
            } else if (dev->rx_buf[1] == AFFINE_CMD_GET_BOARD_INFO) {
                size_t total = 3 + (size_t) dev->rx_buf[2] + 1;
                if (total > sizeof(dev->rx_buf)) {
                    affine_rx_consume(dev, 1);
                    continue;
                }
                if (dev->rx_len < total) {
                    break;
                }
                affine_capture_write_rx(dev->player, dev->rx_buf, total);
                affine_handle_board_info(dev, &dev->rx_buf[3], dev->rx_buf[2]);
                affine_rx_consume(dev, total);
                continue;
            } else if (dev->rx_buf[1] == 0x01) {
                // Handle short touch packet (FF 01 00 00 00 00 00 00 00 00 00 00 00 0A)
                if (dev->rx_len < 14) {
                    break;
                }
                if (dev->rx_buf[13] == 0x0A) {
                    affine_capture_write_rx(dev->player, dev->rx_buf, 14);
                    affine_update_state(dev, &dev->rx_buf[6], &dev->rx_buf[3]);
                    affine_rx_consume(dev, 14);
                    continue;
                }
            }
            
            affine_rx_consume(dev, 1);
            continue;
        }

        if (dev->rx_buf[0] == 0x28) {
            if (dev->rx_len < 9) {
                break;
            }
            if (dev->rx_buf[8] == 0x29) {
                affine_capture_write_rx(dev->player, dev->rx_buf, 9);
                affine_update_state(dev, &dev->rx_buf[1], NULL);
                affine_rx_consume(dev, 9);
                continue;
            }
            affine_rx_consume(dev, 1);
            continue;
        }

        affine_rx_consume(dev, 1);
    }
}

static void affine_update_state(struct affine_device *dev,
        const uint8_t *touch, const uint8_t *buttons)
{
    uint8_t touch_copy[7];
    bool fire_cb = false;

    EnterCriticalSection(&dev->state_lock);
    if (buttons != NULL) {
        uint8_t low = buttons[0] & 0x0F;
        uint8_t high = buttons[1] & 0xF0;
        dev->buttons0 = low | high;
        dev->buttons1 = buttons[2] & 0x3F;
    }
    if (touch != NULL) {
        memcpy(dev->touch, touch, sizeof(dev->touch));
    }
    if (touch != NULL && dev->touch_enabled && affine_touch_cb != NULL) {
        memcpy(touch_copy, dev->touch, sizeof(touch_copy));
        fire_cb = true;
    }
    LeaveCriticalSection(&dev->state_lock);

    if (fire_cb) {
        affine_touch_cb(dev->player, touch_copy);
    }

    affine_shm_write_buttons(dev->player, dev->buttons0, dev->buttons1);
}

static void affine_handle_board_info(struct affine_device *dev,
        const uint8_t *data, size_t len)
{
    char version[32];
    size_t copy_len;
    size_t pos = 0;
    uint8_t ver_len;

    if (dev == NULL || data == NULL || len < 1) {
        dprintf("[Affine IO] Board info parse failed: null or len=%u\n",
                (unsigned) len);
        return;
    }

    ver_len = data[pos++];
    if (ver_len == 0 || pos + ver_len > len) {
        dprintf("[Affine IO] Board info parse failed: len=%u ver_len=%u\n",
                (unsigned) len, (unsigned) ver_len);
        dev->board_info_pending = false;
        return;
    }

    copy_len = ver_len;
    if (copy_len >= sizeof(version)) {
        copy_len = sizeof(version) - 1;
    }
    memcpy(version, &data[pos], copy_len);
    version[copy_len] = '\0';

    if (!dev->board_info_logged || dev->board_info_unknown) {
        dprintf("[Affine IO] P%u Firmware: %s\n", dev->player, version);
        dev->board_info_logged = true;
        dev->board_info_unknown = false;
    }
    dev->board_info_pending = false;
}

static unsigned int __stdcall affine_thread_proc(void *ctx)
{
    struct affine_device *dev = ctx;
    uint8_t buf[64];

    while (!dev->stop) {
        DWORD now = GetTickCount();
        DWORD read = 0;

        if (!dev->enabled) {
            if (dev->connected) {
                serial_close(&dev->port);
                dev->connected = false;
                dev->rx_len = 0;
                affine_shm_write_buttons(dev->player, 0, 0);
            }
            Sleep(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if (!dev->connected) {
            if (!affine_try_open(dev)) {
                Sleep(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
        }

        if (dev->board_info_pending) {
            if (dev->board_info_request_ms == 0) {
                if ((DWORD) (now - dev->board_info_start_ms) >=
                        AFFINE_BOARD_INFO_DELAY_MS) {
                    affine_send_cmd(dev, AFFINE_CMD_GET_BOARD_INFO, NULL, 0);
                    if (dev->connected) {
                        dev->board_info_request_ms = now;
                    }
                }
            } else if ((DWORD) (now - dev->board_info_request_ms) >=
                    AFFINE_BOARD_INFO_TIMEOUT_MS) {
                // Retry once if timeout
                if (!dev->board_info_logged) {
                     affine_send_cmd(dev, AFFINE_CMD_GET_BOARD_INFO, NULL, 0);
                     dev->board_info_request_ms = now;
                     // Only mark as unknown if we've already retried or waited too long overall
                     if ((DWORD)(now - dev->board_info_start_ms) > 3000) {
                        dev->board_info_pending = false;
                        dprintf("[Affine IO] P%u Firmware: unknown\n", dev->player);
                        dev->board_info_logged = true;
                        dev->board_info_unknown = true;
                     }
                }
            }
            // Do not send heartbeat while waiting for board info to keep bus clean
        } else if ((DWORD) (now - dev->last_heartbeat) >=
                AFFINE_HEARTBEAT_INTERVAL_MS) {
            affine_send_cmd(dev, AFFINE_CMD_HEARTBEAT, NULL, 0);
            dev->last_heartbeat = now;
        }

        if (!serial_read(&dev->port, buf, sizeof(buf), &read)) {
            serial_close(&dev->port);
            dev->connected = false;
            affine_shm_write_buttons(dev->player, 0, 0);
            Sleep(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if (read > 0) {
            affine_capture_write_raw(dev->player, buf, read);
            affine_rx_process(dev, buf, read);
        }
    }

    serial_close(&dev->port);
    dev->connected = false;

    return 0;
}

static void affine_capture_init(void)
{
    char env[8];
    DWORD len;
    bool enable_capture = false;
    bool enable_raw = false;

    if (affine_capture_ready) {
        return;
    }

    InitializeCriticalSection(&affine_capture_lock);
    affine_capture_ready = true;

    len = GetEnvironmentVariableA("AFFINE_IO_CAPTURE", env, sizeof(env));
    if (len > 0 && len < sizeof(env)) {
        if (env[0] == '1' || env[0] == 'y' || env[0] == 'Y') {
            enable_capture = true;
        }
    }

    len = GetEnvironmentVariableA("AFFINE_IO_CAPTURE_RAW", env, sizeof(env));
    if (len > 0 && len < sizeof(env)) {
        if (env[0] == '1' || env[0] == 'y' || env[0] == 'Y') {
            enable_raw = true;
        }
    }

    if (!enable_capture && !enable_raw) {
        return;
    }

    affine_capture_file = CreateFileA(
            "affine_io_capture.log",
            FILE_APPEND_DATA,
            FILE_SHARE_READ,
            NULL,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            NULL);

    if (affine_capture_file == INVALID_HANDLE_VALUE) {
        affine_capture_file = NULL;
        return;
    }

    affine_capture_enabled = enable_capture;
    affine_capture_raw_enabled = enable_raw;
}

static void affine_capture_write(uint8_t player, uint8_t cmd,
        const uint8_t *payload, size_t len)
{
    char line[256];
    char *p = line;
    size_t remaining = sizeof(line);
    DWORD written;

    if (!affine_capture_enabled || cmd == 0x11) {
        return;
    }

    if (payload == NULL && len != 0) {
        return;
    }

    p += snprintf(p, remaining, "P%u TX CMD=%02X LEN=%u DATA=",
            player, cmd, (unsigned) len);
    if (p < line) {
        return;
    }
    remaining = sizeof(line) - (size_t) (p - line);

    for (size_t i = 0; i < len && remaining > 3; i++) {
        int n = snprintf(p, remaining, "%02X", payload[i]);
        if (n <= 0) {
            break;
        }
        p += n;
        remaining = sizeof(line) - (size_t) (p - line);
        if (i + 1 < len && remaining > 2) {
            *p++ = ' ';
            *p = '\0';
            remaining = sizeof(line) - (size_t) (p - line);
        }
    }

    if (remaining > 1) {
        *p++ = '\n';
    }

    EnterCriticalSection(&affine_capture_lock);
    if (affine_capture_file != NULL) {
        WriteFile(affine_capture_file, line, (DWORD) (p - line),
                &written, NULL);
    }
    LeaveCriticalSection(&affine_capture_lock);
}

static void affine_capture_write_rx(uint8_t player, const uint8_t *data,
        size_t len)
{
    char line[256];
    char *p = line;
    size_t remaining = sizeof(line);
    DWORD written;
    affine_rx_capture_state_t *state = NULL;
    size_t copy_len;

    if (!affine_capture_enabled || data == NULL || len == 0) {
        return;
    }

    if (player >= 1 && player <= 2) {
        state = &affine_rx_capture_state[player - 1];
    }

    EnterCriticalSection(&affine_capture_lock);

    if (state != NULL && state->valid && state->len == len &&
            memcmp(state->data, data, len) == 0) {
        state->repeat++;
        LeaveCriticalSection(&affine_capture_lock);
        return;
    }

    if (state != NULL && state->repeat > 0) {
        p = line;
        remaining = sizeof(line);
        p += snprintf(p, remaining, "P%u RX REPEAT=%u DATA=",
                player, state->repeat);
        if (p >= line) {
            remaining = sizeof(line) - (size_t) (p - line);
            for (size_t i = 0; i < state->len && remaining > 3; i++) {
                int n = snprintf(p, remaining, "%02X", state->data[i]);
                if (n <= 0) {
                    break;
                }
                p += n;
                remaining = sizeof(line) - (size_t) (p - line);
                if (i + 1 < state->len && remaining > 2) {
                    *p++ = ' ';
                    *p = '\0';
                    remaining = sizeof(line) - (size_t) (p - line);
                }
            }

            if (remaining > 1) {
                *p++ = '\n';
            }

            if (affine_capture_file != NULL) {
                WriteFile(affine_capture_file, line, (DWORD) (p - line),
                        &written, NULL);
            }
        }
        state->repeat = 0;
    }

    p = line;
    remaining = sizeof(line);
    p += snprintf(p, remaining, "P%u RX LEN=%u DATA=",
            player, (unsigned) len);
    if (p < line) {
        LeaveCriticalSection(&affine_capture_lock);
        return;
    }
    remaining = sizeof(line) - (size_t) (p - line);

    for (size_t i = 0; i < len && remaining > 3; i++) {
        int n = snprintf(p, remaining, "%02X", data[i]);
        if (n <= 0) {
            break;
        }
        p += n;
        remaining = sizeof(line) - (size_t) (p - line);
        if (i + 1 < len && remaining > 2) {
            *p++ = ' ';
            *p = '\0';
            remaining = sizeof(line) - (size_t) (p - line);
        }
    }

    if (remaining > 1) {
        *p++ = '\n';
    }

    if (affine_capture_file != NULL) {
        WriteFile(affine_capture_file, line, (DWORD) (p - line),
                &written, NULL);
    }

    if (state != NULL) {
        copy_len = len;
        if (copy_len > sizeof(affine_rx_capture_state[0].data)) {
            copy_len = sizeof(affine_rx_capture_state[0].data);
        }
        memcpy(state->data, data, copy_len);
        state->len = copy_len;
        state->valid = true;
    }
    LeaveCriticalSection(&affine_capture_lock);
}

static void affine_capture_write_raw(uint8_t player, const uint8_t *data,
        size_t len)
{
    char line[256];
    char *p = line;
    size_t remaining = sizeof(line);
    DWORD written;

    if (!affine_capture_raw_enabled || data == NULL || len == 0) {
        return;
    }

    p += snprintf(p, remaining, "P%u RAW LEN=%u DATA=",
            player, (unsigned) len);
    if (p < line) {
        return;
    }
    remaining = sizeof(line) - (size_t) (p - line);

    for (size_t i = 0; i < len && remaining > 3; i++) {
        int n = snprintf(p, remaining, "%02X", data[i]);
        if (n <= 0) {
            break;
        }
        p += n;
        remaining = sizeof(line) - (size_t) (p - line);
        if (i + 1 < len && remaining > 2) {
            *p++ = ' ';
            *p = '\0';
            remaining = sizeof(line) - (size_t) (p - line);
        }
    }

    if (remaining > 1) {
        *p++ = '\n';
    }

    EnterCriticalSection(&affine_capture_lock);
    if (affine_capture_file != NULL) {
        WriteFile(affine_capture_file, line, (DWORD) (p - line),
                &written, NULL);
    }
    LeaveCriticalSection(&affine_capture_lock);
}

static uint8_t *affine_shm_map(uint8_t player)
{
    size_t idx;
    const char *name;
    HANDLE handle;
    uint8_t *ptr;

    if (player < 1 || player > 2) {
        return NULL;
    }

    idx = player - 1;
    if (affine_shm_ptr[idx] != NULL) {
        return affine_shm_ptr[idx];
    }

    name = (player == 1) ? AFFINE_SHM_NAME_1 : AFFINE_SHM_NAME_2;
    handle = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL,
            PAGE_READWRITE, 0, AFFINE_SHM_SIZE, name);
    if (handle == NULL) {
        return NULL;
    }

    ptr = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, AFFINE_SHM_SIZE);
    if (ptr == NULL) {
        CloseHandle(handle);
        return NULL;
    }

    affine_shm_handle[idx] = handle;
    affine_shm_ptr[idx] = ptr;

    return ptr;
}

static uint8_t affine_encode_io_status(uint8_t player, uint8_t buttons1)
{
    uint8_t io_status = 0;

    if (buttons1 & (1 << 1)) {
        io_status |= 0x01;
    }
    if (buttons1 & (1 << 2)) {
        io_status |= 0x02;
    }
    if (buttons1 & (1 << 3)) {
        io_status |= 0x04;
    }
    if (buttons1 & (1 << 0)) {
        io_status |= (player == 1) ? 0x10 : 0x20;
    }

    return io_status;
}

static uint8_t affine_decode_io_status(uint8_t player, uint8_t io_status)
{
    uint8_t buttons1 = 0;

    if (io_status & 0x01) {
        buttons1 |= (1 << 1);
    }
    if (io_status & 0x02) {
        buttons1 |= (1 << 2);
    }
    if (io_status & 0x04) {
        buttons1 |= (1 << 3);
    }
    if (io_status & ((player == 1) ? 0x10 : 0x20)) {
        buttons1 |= (1 << 0);
    }

    return buttons1;
}

static void affine_shm_write_buttons(uint8_t player, uint8_t buttons0,
        uint8_t buttons1)
{
    uint8_t *ptr = affine_shm_map(player);

    if (ptr == NULL) {
        return;
    }

    ptr[0] = buttons0;
    ptr[1] = affine_encode_io_status(player, buttons1);
}

static bool affine_shm_read_state(uint8_t player, struct affine_io_state *out)
{
    uint8_t *ptr = affine_shm_map(player);

    if (ptr == NULL || out == NULL) {
        return false;
    }

    out->present = true;
    out->buttons0 = ptr[0];
    out->buttons1 = affine_decode_io_status(player, ptr[1]);
    memset(out->touch, 0, sizeof(out->touch));

    return true;
}
