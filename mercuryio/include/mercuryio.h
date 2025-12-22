#pragma once

#include <windows.h>

#include <stdbool.h>
#include <stdint.h>

enum {
    MERCURY_IO_OPBTN_TEST = 0x01,
    MERCURY_IO_OPBTN_SERVICE = 0x02,
    MERCURY_IO_OPBTN_COIN = 0x04,
};

enum {
    MERCURY_IO_GAMEBTN_VOL_UP = 0x01,
    MERCURY_IO_GAMEBTN_VOL_DOWN = 0x02,
};

struct led_data {
    DWORD unitCount;
    uint8_t rgba[480 * 4];
};

typedef void (*mercury_io_touch_callback_t)(const bool *state);

uint16_t mercury_io_get_api_version(void);

HRESULT mercury_io_init(void);
HRESULT mercury_io_poll(void);
void mercury_io_get_opbtns(uint8_t *opbtn);
void mercury_io_get_gamebtns(uint8_t *gamebtn);

HRESULT mercury_io_touch_init(void);
void mercury_io_touch_start(mercury_io_touch_callback_t callback);
void mercury_io_touch_set_leds(struct led_data data);
