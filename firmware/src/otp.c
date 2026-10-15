// One ROM OTP handling.

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"

// First row after the commissioning area.
#define AREA_END ((uint16_t)(OTP_COMMISSIONING_AREA_LAST_ROW + 1))

// The chip ID is in OTP rows 0 to 3, least significant row first.
#define OTP_CHIP_ID_ROWS 4u

// The first page boundary after `row`.
static uint16_t page_after(uint16_t row) {
    return (uint16_t)((row / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS);
}

// Retrieves `key`'s value from the last complete commissioning instance.
// `row_out` is set to the value's first row and `len_out` to its length in
// bytes.  Returns 0 if the instance doesn't contain `key`.
//
// For COMMISSIONING_VERSION_KEY it retrieves the instance's version.  If the
// commissioning area contains an instance with an unsupported version it
// retrieves that version for COMMISSIONING_VERSION_KEY and returns 0 for any
// other key.
//
// Returns 0 if the commissioning area isn't laid out as expected so the
// firmware boots.
uint8_t otp_commissioning_value(uint16_t key, uint16_t *row_out, uint16_t *len_out) {
    if (key == OTP_KEY_NONE) {
        return 0;
    }

    uint8_t found = 0;
    uint16_t value_row = 0;
    uint16_t value_len = 0;

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
            uint8_t has_value = 0;
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
                uint16_t entry_key = otp_read_ecc(row);
                uint16_t len = otp_read_ecc(row + 1);
                if ((entry_key == OTP_KEY_NONE) && (len == 0)) {
                    last_row = row + 1;
                    break;
                }
                uint32_t next = (uint32_t)row + 2 + ((uint32_t)len + 1) / 2;
                if (next > AREA_END) {
                    return 0;
                }
                if (entry_key == key) {
                    has_value = 1;
                    this_row = row + 2;
                    this_len = len;
                }
                if (entry_key == OTP_KEY_COMMISSIONING_SIG) {
                    complete = 1;
                    last_row = (uint16_t)(next - 1);
                    break;
                }
                row = (uint16_t)next;
            }
            if (complete) {
                if (key == COMMISSIONING_VERSION_KEY) {
                    has_value = 1;
                    this_row = boundary + 1;
                    this_len = sizeof(version);
                }
                found = has_value;
                value_row = this_row;
                value_len = this_len;
            }
        } else if (version != 0) {
            if (key != COMMISSIONING_VERSION_KEY) {
                return 0;
            }
            *row_out = boundary + 1;
            *len_out = sizeof(version);
            return 1;
        }

        boundary = page_after(last_row);
    }

    if (found) {
        *row_out = value_row;
        *len_out = value_len;
    }
    return found;
}

uint8_t otp_commissioned_board(uint16_t *row_out, uint16_t *len_out) {
    return otp_commissioning_value(OTP_KEY_COMMISSIONING_BOARD, row_out, len_out);
}

// Retrieves byte `index` of the chip ID.  Byte 0 is the most significant.
// Returns 0 if `index` is beyond the last byte.
uint8_t otp_chip_id_byte(uint32_t index, uint8_t *byte_out) {
    if (index >= 2 * OTP_CHIP_ID_ROWS) {
        return 0;
    }
    uint16_t value = otp_read_ecc((uint16_t)(OTP_CHIP_ID_ROWS - 1 - index / 2));
    *byte_out = (uint8_t)((index & 1) ? value : (value >> 8));
    return 1;
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

// The chip in FLASH_DEVINFO's size field at `shift`.  A code above
// FLASH_SIZE_16MB counts as no chip.
static onerom_flash_size_t devinfo_flash_size(uint16_t devinfo, uint16_t shift) {
    uint16_t code = (devinfo >> shift) & OTP_FLASH_DEVINFO_SIZE_BITS;
    if (code > FLASH_SIZE_16MB) {
        return FLASH_SIZE_NONE;
    }
    return (onerom_flash_size_t)code;
}

// The size of the flash chip on each QSPI chip select, from FLASH_DEVINFO:
// - Where most of BOOT_FLAGS0's three copies leave FLASH_DEVINFO_ENABLE clear,
//   chip select 0 has 2MB and chip select 1 has no chip.  The bootrom assumes
//   16MB on chip select 0 instead.
// - Chip select 1 counts as flash only where the board's gpio_ext_flash_cs is
//   set.
// - A size code above FLASH_SIZE_16MB counts as no chip.
//
// The metadata must be valid, as gpio_ext_flash_cs comes from it.
void otp_flash_sizes(onerom_flash_size_t *cs0, onerom_flash_size_t *cs1) {
    uint32_t a = otp_read_raw(OTP_BOOT_FLAGS0_ROW);
    uint32_t b = otp_read_raw(OTP_BOOT_FLAGS0_R1_ROW);
    uint32_t c = otp_read_raw(OTP_BOOT_FLAGS0_R2_ROW);
    uint32_t majority = (a & b) | (a & c) | (b & c);
    if (!(majority & OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE)) {
        *cs0 = FLASH_SIZE_2MB;
        *cs1 = FLASH_SIZE_NONE;
        return;
    }

    uint16_t devinfo = otp_read_ecc(OTP_FLASH_DEVINFO_ROW);
    *cs0 = devinfo_flash_size(devinfo, OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT);
    *cs1 = FLASH_SIZE_NONE;
    if (HW->gpio_ext_flash_cs != GPIO_NONE) {
        *cs1 = devinfo_flash_size(devinfo, OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT);
    }
}

// The bytes of flash in a chip of `size`.
uint32_t flash_size_bytes(onerom_flash_size_t size) {
    if (size == FLASH_SIZE_NONE) {
        return 0;
    }
    return OTP_FLASH_DEVINFO_SIZE_UNIT << size;
}

// The board size from the total size of the flash chips.
// onerom_metadata::otp::board_size() uses the same rule for the host tools,
// except that it includes chip select 1 whatever gpio_ext_flash_cs is set to.
onerom_board_size_t otp_board_size(void) {
    onerom_flash_size_t cs0;
    onerom_flash_size_t cs1;
    otp_flash_sizes(&cs0, &cs1);

    uint32_t total = flash_size_bytes(cs0) + flash_size_bytes(cs1);
    if (total == TOTAL_FLASH_SIZE_M) {
        return BOARD_SIZE_M;
    } else if (total == TOTAL_FLASH_SIZE_L) {
        return BOARD_SIZE_L;
    } else {
        return BOARD_SIZE_OTHER;
    }
}
