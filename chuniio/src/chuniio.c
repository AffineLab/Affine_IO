#include "chuniio.h"

#include <stdbool.h>

#include "affine_serial.h"

#define AFFINE_VID 0xAFF1
#define AFFINE_PID_CHUNI 0x0000
#define AFFINE_BAUD_CHUNI 115200

static bool chuni_io_initialized;
static struct affine_serial_device chuni_serial;

static void chuni_io_ensure_serial(void)
{
    if (!chuni_io_initialized) {
        affine_serial_device_init(&chuni_serial, AFFINE_VID, AFFINE_PID_CHUNI);
        chuni_io_initialized = true;
    }

    if (!chuni_serial.connected) {
        affine_serial_device_try_open(&chuni_serial, AFFINE_BAUD_CHUNI);
    }
}

uint16_t chuni_io_get_api_version(void)
{
    return 0x0102;
}

HRESULT chuni_io_jvs_init(void)
{
    chuni_io_ensure_serial();
    return E_NOTIMPL;
}

void chuni_io_jvs_poll(uint8_t *opbtn, uint8_t *beams)
{
    chuni_io_ensure_serial();

    if (opbtn != NULL) {
        *opbtn = 0;
    }

    if (beams != NULL) {
        *beams = 0;
    }
}

void chuni_io_jvs_read_coin_counter(uint16_t *total)
{
    chuni_io_ensure_serial();

    if (total != NULL) {
        *total = 0;
    }
}

HRESULT chuni_io_slider_init(void)
{
    chuni_io_ensure_serial();
    return E_NOTIMPL;
}

void chuni_io_slider_start(chuni_io_slider_callback_t callback)
{
    chuni_io_ensure_serial();
    (void) callback;
}

void chuni_io_slider_stop(void)
{
    chuni_io_ensure_serial();
}

void chuni_io_slider_set_leds(const uint8_t *rgb)
{
    chuni_io_ensure_serial();
    (void) rgb;
}

HRESULT chuni_io_led_init(void)
{
    chuni_io_ensure_serial();
    return E_NOTIMPL;
}

void chuni_io_led_set_colors(uint8_t board, uint8_t *rgb)
{
    chuni_io_ensure_serial();
    (void) board;
    (void) rgb;
}
