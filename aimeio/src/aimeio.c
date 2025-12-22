#include "aimeio.h"

#include <stdbool.h>

#include "affine_serial.h"

#define AFFINE_VID 0xAFF1
#define AFFINE_PID_AIME 0x0000
#define AFFINE_BAUD_AIME 115200

static bool aime_io_initialized;
static struct affine_serial_device aime_serial;

static void aime_io_ensure_serial(void)
{
    if (!aime_io_initialized) {
        affine_serial_device_init(&aime_serial, AFFINE_VID, AFFINE_PID_AIME);
        aime_io_initialized = true;
    }

    if (!aime_serial.connected) {
        affine_serial_device_try_open(&aime_serial, AFFINE_BAUD_AIME);
    }
}

uint16_t aime_io_get_api_version(void)
{
    return 0x0100;
}

HRESULT aime_io_init(void)
{
    aime_io_ensure_serial();
    return E_NOTIMPL;
}

HRESULT aime_io_nfc_poll(uint8_t unit_no)
{
    aime_io_ensure_serial();
    (void) unit_no;
    return E_NOTIMPL;
}

HRESULT aime_io_nfc_get_aime_id(
        uint8_t unit_no,
        uint8_t *luid,
        size_t luid_size)
{
    aime_io_ensure_serial();
    (void) unit_no;
    (void) luid;
    (void) luid_size;
    return E_NOTIMPL;
}

HRESULT aime_io_nfc_get_felica_id(uint8_t unit_no, uint64_t *IDm)
{
    aime_io_ensure_serial();
    (void) unit_no;
    (void) IDm;
    return E_NOTIMPL;
}

void aime_io_led_set_color(uint8_t unit_no, uint8_t r, uint8_t g, uint8_t b)
{
    aime_io_ensure_serial();
    (void) unit_no;
    (void) r;
    (void) g;
    (void) b;
}

void aime_io_nfc_set_vfd_text(const wchar_t *text)
{
    aime_io_ensure_serial();
    (void) text;
}
