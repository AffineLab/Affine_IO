#pragma once

#include <windows.h>

#include <stddef.h>
#include <stdint.h>

uint16_t aime_io_get_api_version(void);
HRESULT aime_io_init(void);
HRESULT aime_io_nfc_poll(uint8_t unit_no);
HRESULT aime_io_nfc_get_aime_id(
        uint8_t unit_no,
        uint8_t *luid,
        size_t luid_size);
HRESULT aime_io_nfc_get_felica_id(uint8_t unit_no, uint64_t *IDm);
void aime_io_led_set_color(uint8_t unit_no, uint8_t r, uint8_t g, uint8_t b);
void aime_io_nfc_set_vfd_text(const wchar_t *text);
