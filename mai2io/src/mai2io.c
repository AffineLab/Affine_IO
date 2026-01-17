#include "mai2io.h"

#include <process.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>

#include "affine_io.h"
#include "dprintf.h"

#ifndef MAI2IO_VERSION
#define MAI2IO_VERSION "v1.1.0"
#endif

static uint8_t mai2_opbtn;
static uint16_t mai2_player1_btn;
static uint16_t mai2_player2_btn;
static bool mai2_affine_coin;
static bool mai2_io_initialized;
static HANDLE mai2_led_thread;
static bool mai2_led_thread_stop;
static bool mai2_led_init_done;
static CRITICAL_SECTION mai2_led_cs;
static bool mai2_cfg_loaded;
static bool mai2_p1_enabled = true;
static bool mai2_p2_enabled = true;
static DWORD mai2_last_nostate_log;
static uint8_t mai2_last_opbtn;
static uint16_t mai2_last_player1_btn;
static uint16_t mai2_last_player2_btn;

static const wchar_t *mai2_get_config_path(void)
{
    static wchar_t path[MAX_PATH];
    DWORD len;

    len = GetEnvironmentVariableW(L"SEGATOOLS_CONFIG_PATH", path, MAX_PATH);
    if (len == 0 || len >= MAX_PATH) {
        return L".\\segatools.ini";
    }

    return path;
}

typedef struct {
    uint8_t r;
    uint8_t g;
    uint8_t b;
} mai2_led_color_t;

typedef struct {
    mai2_led_color_t start;
    mai2_led_color_t target;
    mai2_led_color_t current;
    uint64_t start_time;
    uint64_t duration;
} mai2_led_fade_t;

static mai2_led_fade_t mai2_led_fades[2][8];
static bool mai2_led_force_update[2];

static unsigned int __stdcall mai2_io_led_thread_proc(void *ctx);
static void mai2_load_config(void);

static HANDLE mai2_gs_capture_file;
static CRITICAL_SECTION mai2_gs_capture_lock;
static bool mai2_gs_capture_ready;
static bool mai2_gs_capture_enabled;

static void mai2_load_config(void)
{
    const wchar_t *ini = mai2_get_config_path();

    if (mai2_cfg_loaded) {
        return;
    }

    mai2_cfg_loaded = true;
    mai2_p1_enabled = GetPrivateProfileIntW(L"touch", L"p1Enable", 1, ini) != 0;
    mai2_p2_enabled = GetPrivateProfileIntW(L"touch", L"p2Enable", 1, ini) != 0;

    affine_io_set_enabled(1, mai2_p1_enabled);
    affine_io_set_enabled(2, mai2_p2_enabled);

    dprintf("[Affine IO] Config: p1Enable=%d p2Enable=%d\n",
            mai2_p1_enabled ? 1 : 0,
            mai2_p2_enabled ? 1 : 0);
}

static void mai2_gs_capture_init(void)
{
    char env[8];
    DWORD len;

    if (mai2_gs_capture_ready) {
        return;
    }

    InitializeCriticalSection(&mai2_gs_capture_lock);
    mai2_gs_capture_ready = true;

    len = GetEnvironmentVariableA("AFFINE_IO_CAPTURE_GS", env, sizeof(env));
    if (len == 0 || len >= sizeof(env)) {
        return;
    }

    if (env[0] != '1' && env[0] != 'y' && env[0] != 'Y') {
        return;
    }

    mai2_gs_capture_file = CreateFileA(
            "affine_io_gs.log",
            FILE_APPEND_DATA,
            FILE_SHARE_READ,
            NULL,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            NULL);

    if (mai2_gs_capture_file == INVALID_HANDLE_VALUE) {
        mai2_gs_capture_file = NULL;
        return;
    }

    mai2_gs_capture_enabled = true;
    dprintf("[Affine IO] GS capture enabled -> affine_io_gs.log\n");
}

static void mai2_gs_capture_write(uint8_t board, const uint8_t *rgb)
{
    char line[512];
    char *p = line;
    size_t remaining = sizeof(line);
    DWORD written;
    uint64_t ts;

    if (!mai2_gs_capture_enabled || rgb == NULL) {
        return;
    }

    ts = GetTickCount64();
    p += snprintf(p, remaining, "TS=%llu B=%u ",
            (unsigned long long) ts, board);
    if (p < line) {
        return;
    }
    remaining = sizeof(line) - (size_t) (p - line);

    for (int i = 0; i < 8 && remaining > 9; i++) {
        int n = snprintf(p, remaining, "%02X%02X%02X%02X",
                rgb[i * 4],
                rgb[i * 4 + 1],
                rgb[i * 4 + 2],
                rgb[i * 4 + 3]);
        if (n <= 0) {
            break;
        }
        p += n;
        remaining = sizeof(line) - (size_t) (p - line);
        if (i + 1 < 8 && remaining > 1) {
            *p++ = ' ';
            *p = '\0';
            remaining = sizeof(line) - (size_t) (p - line);
        }
    }

    if (remaining > 1) {
        *p++ = '\n';
    }

    EnterCriticalSection(&mai2_gs_capture_lock);
    if (mai2_gs_capture_file != NULL) {
        WriteFile(mai2_gs_capture_file, line, (DWORD) (p - line),
                &written, NULL);
    }
    LeaveCriticalSection(&mai2_gs_capture_lock);
}

enum {
    MAI2_AFFINE_EXT_SELECT_BIT = 0,
    MAI2_AFFINE_EXT_TEST_BIT = 1,
    MAI2_AFFINE_EXT_SERVICE_BIT = 2,
    MAI2_AFFINE_EXT_COIN_BIT = 3,
};

static void mai2_apply_affine_buttons(
        uint8_t buttons0,
        uint8_t buttons1,
        uint16_t *out)
{
    if (buttons0 & 0x01) {
        *out |= MAI2_IO_GAMEBTN_1;
    }
    if (buttons0 & 0x02) {
        *out |= MAI2_IO_GAMEBTN_2;
    }
    if (buttons0 & 0x04) {
        *out |= MAI2_IO_GAMEBTN_3;
    }
    if (buttons0 & 0x08) {
        *out |= MAI2_IO_GAMEBTN_4;
    }
    if (buttons0 & 0x10) {
        *out |= MAI2_IO_GAMEBTN_5;
    }
    if (buttons0 & 0x20) {
        *out |= MAI2_IO_GAMEBTN_6;
    }
    if (buttons0 & 0x40) {
        *out |= MAI2_IO_GAMEBTN_7;
    }
    if (buttons0 & 0x80) {
        *out |= MAI2_IO_GAMEBTN_8;
    }
    if (buttons1 & (1 << MAI2_AFFINE_EXT_SELECT_BIT)) {
        *out |= MAI2_IO_GAMEBTN_SELECT;
    }
}

uint16_t mai2_io_get_api_version(void)
{
    return 0x0102;
}

HRESULT mai2_io_init(void)
{
    HRESULT hr;
    char module_path[MAX_PATH];
    const char *filename;

    if (mai2_io_initialized) {
        dprintf("[Affine IO] Already initialized\n");
        return S_OK;
    }

    dprintf("[Affine IO] Initializing...\n");
    dprintf("[Affine IO] Affine IO Version: %s\n", MAI2IO_VERSION);
    dprintf("[Affine IO] Mai2IO API Version: %d.%02d\n",
            mai2_io_get_api_version() >> 8,
            mai2_io_get_api_version() & 0xFF);

    mai2_load_config();

    module_path[0] = '\0';
    if (GetModuleFileNameA(NULL, module_path, MAX_PATH) > 0) {
        filename = strrchr(module_path, '\\');
        if (filename != NULL) {
            filename++;
        } else {
            filename = module_path;
        }
        dprintf("[Affine IO] Running in %s\n", filename);
    }

    if (_stricmp(filename, "Sinmai.exe") != 0) {
        dprintf("[Affine IO] Skipping device init for %s\n", filename);
        mai2_io_initialized = true;
        dprintf("[Affine IO] Initialization complete.\n");
        return S_OK;
    }

    hr = affine_io_init();
    if (FAILED(hr)) {
        dprintf("[Affine IO] Initialization failed: 0x%08lx\n", hr);
        return hr;
    }

    mai2_io_initialized = true;
    dprintf("[Affine IO] Initialization complete.\n");

    return S_OK;
}

HRESULT mai2_io_poll(void)
{
    struct affine_io_state affine_state;
    bool affine_coin_pressed;
    bool have_p1;
    bool have_p2;
    DWORD now;

    mai2_opbtn = 0;
    mai2_player1_btn = 0;
    mai2_player2_btn = 0;

    now = GetTickCount();

    have_p1 = affine_io_get_state(1, &affine_state);
    if (have_p1) {
        mai2_apply_affine_buttons(
                affine_state.buttons0,
                affine_state.buttons1,
                &mai2_player1_btn);

        if (affine_state.buttons1 & (1 << MAI2_AFFINE_EXT_TEST_BIT)) {
            mai2_opbtn |= MAI2_IO_OPBTN_TEST;
        }

        if (affine_state.buttons1 & (1 << MAI2_AFFINE_EXT_SERVICE_BIT)) {
            mai2_opbtn |= MAI2_IO_OPBTN_SERVICE;
        }

        affine_coin_pressed = (affine_state.buttons1 &
                (1 << MAI2_AFFINE_EXT_COIN_BIT)) != 0;

        if (affine_coin_pressed) {
            if (!mai2_affine_coin) {
                mai2_affine_coin = true;
                mai2_opbtn |= MAI2_IO_OPBTN_COIN;
            }
        } else {
            mai2_affine_coin = false;
        }
    } else {
        mai2_affine_coin = false;
    }

    have_p2 = affine_io_get_state(2, &affine_state);
    if (have_p2) {
        mai2_apply_affine_buttons(
                affine_state.buttons0,
                affine_state.buttons1,
                &mai2_player2_btn);

        if (affine_state.buttons1 & (1 << MAI2_AFFINE_EXT_TEST_BIT)) {
            mai2_opbtn |= MAI2_IO_OPBTN_TEST;
        }

        if (affine_state.buttons1 & (1 << MAI2_AFFINE_EXT_SERVICE_BIT)) {
            mai2_opbtn |= MAI2_IO_OPBTN_SERVICE;
        }
    }

    if (!have_p1 && !have_p2) {
        if ((DWORD) (now - mai2_last_nostate_log) >= 1000) {
            dprintf("[Affine IO] poll state: no device\n");
            mai2_last_nostate_log = now;
        }
    } else if (mai2_opbtn != mai2_last_opbtn ||
            mai2_player1_btn != mai2_last_player1_btn ||
            mai2_player2_btn != mai2_last_player2_btn) {
        dprintf("[Affine IO] poll state: op=%02X p1=%04X p2=%04X\n",
                mai2_opbtn, mai2_player1_btn, mai2_player2_btn);
        mai2_last_opbtn = mai2_opbtn;
        mai2_last_player1_btn = mai2_player1_btn;
        mai2_last_player2_btn = mai2_player2_btn;
    }

    return S_OK;
}

void mai2_io_get_opbtns(uint8_t *opbtn)
{
    if (opbtn != NULL) {
        *opbtn = mai2_opbtn;
    }

}

void mai2_io_get_gamebtns(uint16_t *player1, uint16_t *player2)
{
    if (player1 != NULL) {
        *player1 = mai2_player1_btn;
    }

    if (player2 != NULL) {
        *player2 = mai2_player2_btn;
    }

}

HRESULT mai2_io_touch_init(mai2_io_touch_callback_t callback)
{
    affine_io_set_touch_callback(callback);
    return S_OK;
}

void mai2_io_touch_set_sens(uint8_t *bytes)
{
    (void) bytes;
}

void mai2_io_touch_update(bool player1, bool player2)
{
    affine_io_set_touch_enabled(1, player1);
    affine_io_set_touch_enabled(2, player2);
}

HRESULT mai2_io_led_init(void)
{
    if (!mai2_led_init_done) {
        InitializeCriticalSection(&mai2_led_cs);
        mai2_led_thread_stop = false;
        mai2_led_thread = (HANDLE) _beginthreadex(
                NULL, 0, mai2_io_led_thread_proc, NULL, 0, NULL);
        if (mai2_led_thread == NULL) {
            dprintf("[Affine IO] LED thread start failed\n");
            return E_FAIL;
        }
        mai2_led_init_done = true;
    }

    mai2_load_config();

    return affine_io_init();
}

void mai2_io_led_set_fet_output(uint8_t board, const uint8_t *rgb)
{
    if (rgb == NULL) {
        return;
    }

    affine_io_send_led_pwm(board + 1, rgb);
}

void mai2_io_led_dc_update(uint8_t board, const uint8_t *rgb)
{
    (void) board;
    (void) rgb;
}

void mai2_io_led_gs_update(uint8_t board, const uint8_t *rgb)
{
    uint64_t now;
    bool send_now = false;
    uint8_t payload[24];

    if (rgb == NULL) {
        return;
    }

    mai2_gs_capture_init();
    mai2_gs_capture_write(board, rgb);

    now = GetTickCount64();

    EnterCriticalSection(&mai2_led_cs);
    for (int i = 0; i < 8; i++) {
        uint8_t r = rgb[i * 4];
        uint8_t g = rgb[i * 4 + 1];
        uint8_t b = rgb[i * 4 + 2];
        uint8_t speed = rgb[i * 4 + 3];
        mai2_led_fade_t *fade = &mai2_led_fades[board][i];

        fade->start = fade->current;
        fade->target.r = r;
        fade->target.g = g;
        fade->target.b = b;
        fade->start_time = now;

        if (speed == 0) {
            fade->duration = 0;
            fade->current.r = r;
            fade->current.g = g;
            fade->current.b = b;
            send_now = true;
        } else {
            fade->duration = (4095 / speed) * 8;
        }
    }
    if (send_now) {
        mai2_led_force_update[board] = true;
        for (int i = 0; i < 8; i++) {
            const mai2_led_fade_t *fade = &mai2_led_fades[board][i];
            payload[i * 3] = fade->current.r;
            payload[i * 3 + 1] = fade->current.g;
            payload[i * 3 + 2] = fade->current.b;
        }
        affine_io_send_led_buttons(board + 1, payload);
    }
    LeaveCriticalSection(&mai2_led_cs);
}

void mai2_io_led_billboard_set(uint8_t board, const uint8_t *rgb)
{
    uint8_t payload[24];

    if (rgb == NULL) {
        return;
    }

    for (int i = 0; i < 8; i++) {
        payload[i * 3] = rgb[0];
        payload[i * 3 + 1] = rgb[1];
        payload[i * 3 + 2] = rgb[2];
    }

    affine_io_send_led_billboard(board + 1, payload);
}

void mai2_io_led_cam_set(uint8_t state)
{
    return;
}

static unsigned int __stdcall mai2_io_led_thread_proc(void *ctx)
{
    (void) ctx;

    while (!mai2_led_thread_stop) {
        uint64_t now = GetTickCount64();

        EnterCriticalSection(&mai2_led_cs);
        for (int board = 0; board < 2; board++) {
            uint8_t payload[24];
            bool need_update = false;

            for (int i = 0; i < 8; i++) {
                mai2_led_fade_t *fade = &mai2_led_fades[board][i];
                uint8_t r;
                uint8_t g;
                uint8_t b;

                if (fade->duration == 0 ||
                        now >= fade->start_time + fade->duration) {
                    r = fade->target.r;
                    g = fade->target.g;
                    b = fade->target.b;
                } else {
                    float progress = (float) (now - fade->start_time) /
                            (float) fade->duration;
                    r = (uint8_t) (fade->start.r +
                            (fade->target.r - fade->start.r) * progress);
                    g = (uint8_t) (fade->start.g +
                            (fade->target.g - fade->start.g) * progress);
                    b = (uint8_t) (fade->start.b +
                            (fade->target.b - fade->start.b) * progress);
                }

                if (r != fade->current.r ||
                        g != fade->current.g ||
                        b != fade->current.b) {
                    need_update = true;
                }

                fade->current.r = r;
                fade->current.g = g;
                fade->current.b = b;

                payload[i * 3] = r;
                payload[i * 3 + 1] = g;
                payload[i * 3 + 2] = b;
            }

        if (need_update) {
                affine_io_send_led_buttons(board + 1, payload);
                mai2_led_force_update[board] = false;
            } else if (mai2_led_force_update[board]) {
                affine_io_send_led_buttons(board + 1, payload);
                mai2_led_force_update[board] = false;
            }
        }
        LeaveCriticalSection(&mai2_led_cs);

        Sleep(8);
    }

    return 0;
}
