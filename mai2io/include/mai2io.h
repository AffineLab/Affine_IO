#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <windows.h>

enum {
    MAI2_IO_OPBTN_TEST = 0x01,
    MAI2_IO_OPBTN_SERVICE = 0x02,
    MAI2_IO_OPBTN_COIN = 0x04,
};

enum {
    MAI2_IO_GAMEBTN_1 = 0x01,
    MAI2_IO_GAMEBTN_2 = 0x02,
    MAI2_IO_GAMEBTN_3 = 0x04,
    MAI2_IO_GAMEBTN_4 = 0x08,
    MAI2_IO_GAMEBTN_5 = 0x10,
    MAI2_IO_GAMEBTN_6 = 0x20,
    MAI2_IO_GAMEBTN_7 = 0x40,
    MAI2_IO_GAMEBTN_8 = 0x80,
    MAI2_IO_GAMEBTN_SELECT = 0x100,
};

typedef void (*mai2_io_touch_callback_t)(
        const uint8_t player,
        const uint8_t state[7]);

uint16_t mai2_io_get_api_version(void);
HRESULT mai2_io_init(void);
HRESULT mai2_io_poll(void);
void mai2_io_get_opbtns(uint8_t *opbtn);
void mai2_io_get_gamebtns(uint16_t *player1, uint16_t *player2);

HRESULT mai2_io_touch_init(mai2_io_touch_callback_t callback);
void mai2_io_touch_set_sens(uint8_t *bytes);
void mai2_io_touch_update(bool player1, bool player2);

HRESULT mai2_io_led_init(void);
void mai2_io_led_set_fet_output(uint8_t board, const uint8_t *rgb);
void mai2_io_led_dc_update(uint8_t board, const uint8_t *rgb);
void mai2_io_led_gs_update(uint8_t board, const uint8_t *rgb);
void mai2_io_led_billboard_set(uint8_t board, uint8_t *rgb);
