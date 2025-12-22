#pragma once

#include <windows.h>

#include <stdbool.h>
#include <stdint.h>

enum {
    CHUNI_IO_OPBTN_TEST = 0x01,
    CHUNI_IO_OPBTN_SERVICE = 0x02,
    CHUNI_IO_OPBTN_COIN = 0x04,
};

uint16_t chuni_io_get_api_version(void);

HRESULT chuni_io_jvs_init(void);
void chuni_io_jvs_poll(uint8_t *opbtn, uint8_t *beams);
void chuni_io_jvs_read_coin_counter(uint16_t *total);

HRESULT chuni_io_slider_init(void);

typedef void (*chuni_io_slider_callback_t)(const uint8_t *state);

void chuni_io_slider_start(chuni_io_slider_callback_t callback);
void chuni_io_slider_stop(void);
void chuni_io_slider_set_leds(const uint8_t *rgb);

HRESULT chuni_io_led_init(void);
void chuni_io_led_set_colors(uint8_t board, uint8_t *rgb);
