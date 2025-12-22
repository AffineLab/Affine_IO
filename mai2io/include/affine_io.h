#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <windows.h>

typedef void (*affine_io_touch_callback_t)(
        uint8_t player,
        const uint8_t state[7]);

struct affine_io_state {
    bool present;
    uint8_t buttons0;
    uint8_t buttons1;
    uint8_t touch[7];
};

HRESULT affine_io_init(void);
void affine_io_shutdown(void);

void affine_io_set_enabled(uint8_t player, bool enable);
void affine_io_set_touch_callback(affine_io_touch_callback_t callback);
void affine_io_set_touch_enabled(uint8_t player, bool enable);
bool affine_io_get_state(uint8_t player, struct affine_io_state *out);

void affine_io_send_led_buttons(uint8_t player, const uint8_t *rgb24);
void affine_io_send_led_billboard(uint8_t player, const uint8_t *rgb24);
void affine_io_send_led_pwm(uint8_t player, const uint8_t *pwm3);
