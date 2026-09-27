// One ROM OTP reads at boot - the board OTP commissions the chip as, and the
// board size.  docs/wip/OTP.md describes the rows.

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"

// The row after the commissioning area's last.
#define AREA_END ((uint16_t)(OTP_COMMISSIONING_AREA_LAST_ROW + 1))

// The first page boundary after `row`.
static uint16_t page_after(uint16_t row) {
    return (uint16_t)((row / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS);
}

// Finds COMMISSIONING_BOARD in the last complete commissioning instance.
// Returns 1, with the value's first row and its length in bytes, where there
// is one.
//
// Walks the commissioning area as docs/wip/OTP.md lays it out, and checks
// nothing beyond what finding the board needs - the host tools check
// commissioning data.  Anything the layout doesn't expect returns 0, so the
// firmware boots.
uint8_t otp_commissioned_board(uint16_t *row_out, uint16_t *len_out) {
    uint8_t found = 0;
    uint16_t board_row = 0;
    uint16_t board_len = 0;

    uint16_t boundary = OTP_COMMISSIONING_AREA_FIRST_ROW;
    while (boundary < AREA_END) {
        uint16_t magic = otp_read_ecc(boundary);
        if (magic == 0) {
            // Unwritten, so every instance has been read.
            break;
        }
        if (magic != OTP_STORE_MAGIC) {
            return 0;
        }

        // An instance whose version row is unwritten ends there.
        uint16_t version = otp_read_ecc(boundary + 1);
        uint16_t last_row = boundary + 1;
        if (version == OTP_STORE_VERSION) {
            uint8_t complete = 0;
            uint8_t has_board = 0;
            uint16_t this_row = 0;
            uint16_t this_len = 0;
            uint16_t row = boundary + 2;
            while (1) {
                // An instance interrupted in the area's last page can fill it.
                if (row >= AREA_END) {
                    last_row = AREA_END - 1;
                    break;
                }
                if (row + 1 >= AREA_END) {
                    return 0;
                }
                uint16_t key = otp_read_ecc(row);
                uint16_t len = otp_read_ecc(row + 1);
                if ((key == OTP_KEY_NONE) && (len == 0)) {
                    last_row = row + 1;
                    break;
                }
                uint32_t next = (uint32_t)row + 2 + ((uint32_t)len + 1) / 2;
                if (next > AREA_END) {
                    return 0;
                }
                if (key == OTP_KEY_COMMISSIONING_BOARD) {
                    has_board = 1;
                    this_row = row + 2;
                    this_len = len;
                } else if (key == OTP_KEY_COMMISSIONING_SIG) {
                    complete = 1;
                    last_row = (uint16_t)(next - 1);
                    break;
                }
                row = (uint16_t)next;
            }
            if (complete) {
                found = has_board;
                board_row = this_row;
                board_len = this_len;
            }
        } else if (version != 0) {
            return 0;
        }

        boundary = page_after(last_row);
    }

    if (found) {
        *row_out = board_row;
        *len_out = board_len;
    }
    return found;
}

// Whether OTP commissions the chip as a board other than `hw_rev`, which it
// logs.  0 where OTP doesn't commission it, and where the metadata doesn't hold
// a board name.
uint8_t otp_board_mismatch(const char *hw_rev) {
    uint16_t row;
    uint16_t len;
    if (!otp_commissioned_board(&row, &len)) {
        return 0;
    }
    if ((hw_rev == NULL) || (hw_rev == (void *)0xFFFFFFFF)) {
        return 0;
    }

    // Two bytes to a row, low byte first.
    uint8_t mismatch = (strlen(hw_rev) != len);
    for (uint16_t ii = 0; !mismatch && (ii < len); ii += 2) {
        uint16_t value = otp_read_ecc(row + ii / 2);
        mismatch = ((uint8_t)hw_rev[ii] != (value & 0xFF))
                   || ((ii + 1 < len) && ((uint8_t)hw_rev[ii + 1] != (value >> 8)));
    }

    // The board name is printed straight from OTP, so it doesn't need a
    // buffer.
    if (mismatch) {
        ERR("Commissioning board mismatch - OTP: %.*s, metadata: %s",
            (int)len, otp_ecc_bytes(row), hw_rev);
    }
    return mismatch;
}

// The board size OTP configures, read as the bootrom reads it.
// onerom_metadata::otp::board_size() applies the same rule for the host tools.
onerom_board_size_t otp_board_size(void) {
    uint32_t a = otp_read_raw(OTP_BOOT_FLAGS0_ROW);
    uint32_t b = otp_read_raw(OTP_BOOT_FLAGS0_R1_ROW);
    uint32_t c = otp_read_raw(OTP_BOOT_FLAGS0_R2_ROW);
    uint32_t majority = (a & b) | (a & c) | (b & c);
    if (!(majority & OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE)) {
        return BOARD_SIZE_M;
    }

    uint16_t devinfo = otp_read_ecc(OTP_FLASH_DEVINFO_ROW);
    uint16_t cs1_size = (devinfo >> OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT)
                        & OTP_FLASH_DEVINFO_SIZE_BITS;
    if (cs1_size == 0) {
        return BOARD_SIZE_M;
    } else if (cs1_size == OTP_FLASH_DEVINFO_SIZE_2MB) {
        return BOARD_SIZE_L;
    } else {
        return BOARD_SIZE_OTHER;
    }
}
