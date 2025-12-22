#include "mercuryio.h"

#include <stdbool.h>

#include "affine_serial.h"

#define AFFINE_VID 0xAFF1
#define AFFINE_PID_MERCURY 0x0000
#define AFFINE_BAUD_MERCURY 115200

static bool mercury_io_initialized;
static struct affine_serial_device mercury_serial;
static uint8_t mercury_opbtn;
static uint8_t mercury_gamebtn;

static void mercury_io_ensure_serial(void)
{
    if (!mercury_io_initialized) {
        affine_serial_device_init(&mercury_serial, AFFINE_VID,
                AFFINE_PID_MERCURY);
        mercury_io_initialized = true;
    }

    if (!mercury_serial.connected) {
        affine_serial_device_try_open(&mercury_serial, AFFINE_BAUD_MERCURY);
    }
}

uint16_t mercury_io_get_api_version(void)
{
    return 0x0100;
}

HRESULT mercury_io_init(void)
{
    mercury_io_ensure_serial();
    return E_NOTIMPL;
}

HRESULT mercury_io_poll(void)
{
    mercury_io_ensure_serial();
    mercury_opbtn = 0;
    mercury_gamebtn = 0;
    return E_NOTIMPL;
}

void mercury_io_get_opbtns(uint8_t *opbtn)
{
    if (opbtn != NULL) {
        *opbtn = mercury_opbtn;
    }
}

void mercury_io_get_gamebtns(uint8_t *gamebtn)
{
    if (gamebtn != NULL) {
        *gamebtn = mercury_gamebtn;
    }
}

HRESULT mercury_io_touch_init(void)
{
    mercury_io_ensure_serial();
    return E_NOTIMPL;
}

void mercury_io_touch_start(mercury_io_touch_callback_t callback)
{
    mercury_io_ensure_serial();
    (void) callback;
}

void mercury_io_touch_set_leds(struct led_data data)
{
    mercury_io_ensure_serial();
    (void) data;
}
