// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include <stdio.h>

#include <include.h>
#include <apio.h>
#include <epio.h>
#include <test/stub.h>
#include <test/func.h>

// We use int32_t for some functions so we can represent an invalid address
// with -1.  Hence the max address we can support is 0x7FFFFFFF.
#define MAX_SUPPORTED_ADDR 0x7FFFFFFF

static uint8_t addr_pins[32];
static uint8_t data_pins[16];

// Return the GPIO drive state for a given active high/low CS configuration
// and logical CS active state (active = 1, inactive = 0).
static uint8_t get_cs_gpio_state(sdrr_cs_state_t state, uint8_t active) {
    uint8_t cs_state;
    switch (state) {
        case CS_ACTIVE_LOW:
            cs_state = active ? 0 : 1;
            break;
        case CS_ACTIVE_HIGH:
            cs_state = active ? 1 : 0;
            break;
        case CS_NOT_USED:
            cs_state = 2; // Not used, so we won't drive it
            break;
        default:
            assert(0 && "Invalid CS state");
    }
    return cs_state;
}

// Gets the appropriate GPIO drive state to simulate a particular address and
// CS state being driven on the ROM by the host.  Returns bit masks ready to
// be applied via epio_drive_gpios_ext.
//
// addr is a word address in the 16-bit case.
static void get_gpio_drive_from_addr_cs(
    sdrr_rom_type_t rom_type,
    int32_t addr,
    uint8_t num_addr_bits,
    uint8_t cs1,
    uint8_t cs2,
    uint8_t cs3,
    uint8_t x1,
    uint8_t x2,
    uint64_t *gpios_to_drive,
    uint64_t *gpio_levels,
    uint8_t bit_mode
) {
    assert(addr < (512*1024) && "Address out of range for One ROM");
    assert(addr < MAX_SUPPORTED_ADDR && "Address too large to represent as int32_t");
    uint64_t drive_mask = 0;
    uint64_t level_mask = 0;

    // Figure out the drive and level mask for the address lines
    if (addr >= 0) {
        uint8_t local_addr_pins[32];
        memcpy(local_addr_pins, addr_pins, sizeof(local_addr_pins));

        // Handle any ROM type uniqueness
        if (rom_type == CHIP_TYPE_2732 && sdrr_info.pins->chip_pins == 24) {
            // Swap pins A11 and A12
            uint8_t temp = local_addr_pins[11];
            local_addr_pins[11] = local_addr_pins[12];
            local_addr_pins[12] = temp;
        } else if (rom_type == CHIP_TYPE_27C301 || rom_type == CHIP_TYPE_SST39SF040) {
            // Pin A16 (27C301) or A18 (SST39SF040) should be 1 after the highest address pin
            uint8_t highest_addr_pin = 0;
            for (int ii = 0; ii < 19; ii++) {
                if (local_addr_pins[ii] > highest_addr_pin) {
                    highest_addr_pin = local_addr_pins[ii];
                }
            }
            if (rom_type == CHIP_TYPE_27C301) {
                if (local_addr_pins[15] == 19) {
                    // fire-28-b
                    local_addr_pins[16] = highest_addr_pin + 2;
                } else {
                    // fire-28-a
                    local_addr_pins[16] = highest_addr_pin + 1;
                }
            } else if (rom_type == CHIP_TYPE_SST39SF040) {
                // fire-32-b
                local_addr_pins[18] = highest_addr_pin + 1;
            }
        } else if (rom_type == CHIP_TYPE_28C256) {
            // Swap pins 14 and 15
            uint8_t temp = local_addr_pins[14];
            local_addr_pins[14] = local_addr_pins[15];
            local_addr_pins[15] = temp;
        } else if (rom_type == CHIP_TYPE_27C400) {
            assert(num_addr_bits == 19 && "27C400 should have 19 address bits");

            if (bit_mode == 8) {
                // Replace the first (least significant) address line with D15
                assert(addr < (512*1024) && "Address out of range for 8-bit mode on 27C400");
                local_addr_pins[0] = data_pins[15];
            } else {
                // Remove the first (least significant) address line, as it's
                // not used in 16-bit mode
                assert(addr < (256*1024) && "Address out of range for 16-bit mode on 27C400");
                for (int ii = 0; ii < 18; ii++) {
                    local_addr_pins[ii] = addr_pins[ii + 1];
                }
                num_addr_bits--;
            }
        } else if (rom_type == CHIP_TYPE_27C200) {
            assert(num_addr_bits == 18 && "27C200 should have 18 address bits");

            if (bit_mode == 8) {
                // Replace the first (least significant) address line with D15
                assert(addr < (256*1024) && "Address out of range for 8-bit mode on 27C200");
                local_addr_pins[0] = data_pins[15];
            } else {
                // Remove the first (least significant) address line, as it's
                // not used in 16-bit mode
                assert(addr < (128*1024) && "Address out of range for 16-bit mode on 27C200");
                for (int ii = 0; ii < 17; ii++) {
                    local_addr_pins[ii] = addr_pins[ii + 1];
                }
                num_addr_bits--;
            }
        } else if (rom_type == CHIP_TYPE_2364 && sdrr_info.pins->chip_pins == 28) {
            local_addr_pins[11] = sdrr_info.pins->cs1;    // 231024 CS1 -> 2364 A11
            local_addr_pins[12] = addr_pins[11];           // 231024 A11 -> 2364 A12
        } else if (rom_type == CHIP_TYPE_23QL512 || rom_type == CHIP_TYPE_23QL384) {
            // A15 is CE
            local_addr_pins[15] = sdrr_info.pins->ce;
        }

        for (int ii = 0; ii < num_addr_bits; ii++) {
            assert((local_addr_pins[ii] < MAX_USED_GPIOS) && "Address bit out of range for ROM");
            drive_mask |= (1ULL << local_addr_pins[ii]);
            if (addr & (1 << ii)) {
                level_mask |= (1ULL << local_addr_pins[ii]);
            }
        }
    } else {
        // Do not drive address lines
    }

    uint8_t cs1_pin = sdrr_info.pins->cs1;
    uint8_t cs2_pin = sdrr_info.pins->cs2;
    uint8_t cs3_pin = sdrr_info.pins->cs3;
    uint8_t x1_pin = sdrr_info.pins->x1;
    uint8_t x2_pin = sdrr_info.pins->x2;
    uint8_t oe_pin = sdrr_info.pins->oe;
    uint8_t ce_pin = sdrr_info.pins->ce;
    switch (rom_type) {
        case CHIP_TYPE_2364:
            if (sdrr_info.pins->chip_pins == 28) {
                // Special case the 2364 on 28 pin ROM
                cs1_pin = sdrr_info.pins->addr2[0];
            }
            break;

        case CHIP_TYPE_2316:
            // Flip CS2 and CS3 pin for all 24 pin ROMs except 2332 and 2364
            cs2_pin = sdrr_info.pins->cs3;
            cs3_pin = sdrr_info.pins->cs2;
            break;

        case CHIP_TYPE_2704:
        case CHIP_TYPE_2708:
        case CHIP_TYPE_2716:
        case CHIP_TYPE_2732:
        case CHIP_TYPE_2764:
            if (sdrr_info.pins->chip_pins == 24) {
                // Flip CS2 and CS3 pin for all 24 pin ROMs except 2332 and 2364
                cs2_pin = sdrr_info.pins->cs3;
                cs3_pin = sdrr_info.pins->cs2;
            }
            break;

        // Some cases are missing here.  Thats because /OE/CE share pins with
        // CS lines so its unecessary.  This goes for 24 and 28 pin ROMs only.

        case CHIP_TYPE_27C301:
            // /OE should be cs2
            cs1_pin = ce_pin;
            break;

        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            cs1_pin = oe_pin;
            break;

        case CHIP_TYPE_27C010:
        case CHIP_TYPE_27C020:
        case CHIP_TYPE_27C040:
        case CHIP_TYPE_SST39SF040:
            cs1_pin = ce_pin;
            cs2_pin = oe_pin;
            break;

        case CHIP_TYPE_27C080:
            // CS1 pin is A19
            cs2_pin = ce_pin;
            cs3_pin = oe_pin;
            break;

        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
            cs1_pin = ce_pin;
            cs2_pin = oe_pin;
            break;

        case CHIP_TYPE_28C16:
            // 24 pin, so swap CS2 and CS3 vs 2332.
            // CS3 is active high
            cs2_pin = sdrr_info.pins->cs3;
            cs3_pin = sdrr_info.pins->cs2;
            break;

        case CHIP_TYPE_28C64:
        case CHIP_TYPE_28C256:
            // No-op, use same pins as 231024, but CS3 active high
            break;

        case CHIP_TYPE_28C512:
            // A17, active high
            cs1_pin = ce_pin;
            cs2_pin = oe_pin;
            cs3_pin = sdrr_info.pins->addr2[17-16];
            break;

        default:
            break;
    }

    // After switch, drive /BYTE for 27C400/200
    if (rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) {
        drive_mask |= (1ULL << sdrr_info.pins->byte);
        if (bit_mode == 16) {
            level_mask |= (1ULL << sdrr_info.pins->byte);
        }
        // bit_mode == 8: /BYTE stays low (not set in level_mask)
    }

    // Add CS lines to the drive and level mask
    if (cs1 < 2) {
        drive_mask |= (1ULL << cs1_pin);
        if (cs1) {
            level_mask |= (1ULL << cs1_pin);
        }
    }
    if (cs2 < 2) {
        drive_mask |= (1ULL << cs2_pin);
        if (cs2) {
            level_mask |= (1ULL << cs2_pin);
        }
    }
    if (cs3 < 2) {
        drive_mask |= (1ULL << cs3_pin);
        if (cs3) {
            level_mask |= (1ULL << cs3_pin);
        }
    }
    if (x1 < 2) {
        drive_mask |= (1ULL << x1_pin);
        if (x1) {
            level_mask |= (1ULL << x1_pin);
        }
    }
    if (x2 < 2) {
        drive_mask |= (1ULL << x2_pin);
        if (x2) {
            level_mask |= (1ULL << x2_pin);
        }
    }

    *gpios_to_drive = drive_mask;
    *gpio_levels = level_mask;
    //TST_LOG("GPIO drive for addr 0x%08X cs1: %d cs2: %d cs3: %d x1: %d x2: %d -> drive_mask: 0x%016llX level_mask: 0x%016llX", addr, cs1, cs2, cs3, x1, x2, drive_mask, level_mask);
}

// Used to figure out the GPIO drive state for a given address and logical
// CS state, where CS is active (1) or inactive (0).
//
// When used for a multi-set ROM, the rom_index is used as an index into the
// set's ROM.
//
// addr is a word address for 16-bit ROMs.
void get_gpio_drive(
    uint8_t set_index,
    uint8_t rom_index,
    int32_t addr,
    uint8_t cs_active,
    uint64_t *gpios_to_drive,
    uint64_t *gpio_levels,
    uint8_t bit_mode
) {
    assert(cs_active <= 2 && "CS active state must be 0, 1, or 2");
    assert(addr < MAX_SUPPORTED_ADDR && "Address too large to represent as int32_t");
    assert(set_index < SDRR_NUM_SETS && "Set index out of range");
    const sdrr_rom_set_t *set = &rom_set[set_index];
    assert(rom_index < set->rom_count && "ROM index out of range");
    const sdrr_rom_info_t *rom_info = set->roms[rom_index];
    const sdrr_rom_type_t rom_type = rom_info->rom_type;
    uint8_t rom_count = set->rom_count;

    uint8_t multi_rom = 0;
    uint8_t banked_rom = 0;
    if (set->rom_count > 1) {
        // Initial sanity checks
        assert(rom_index < rom_count && "ROM index out of range for this set");
        assert(sdrr_info.pins->chip_pins == 24 && "Multi-ROM sets only supported on 24 pin ROMs");
        
        if (set->serve == SERVE_ADDR_ON_ANY_CS) {
            // Multi-ROM set, with CS/X1/X2 selecting the ROM image
            assert(rom_count <= 3 && "Multi ROM sets with more than 3 ROMs not supported");
            assert(((rom_type == CHIP_TYPE_2364) || (rom_type == CHIP_TYPE_2332)) && "Only 2364/2332 ROMs supported for multi ROM sets");
            assert(set->multi_rom_cs1_state == CS_ACTIVE_LOW && "Only active low CS1 supported for multi ROM sets");
            multi_rom = 1;
        } else {
            // Dynamically banked set, with CS acting as CS and X1/X2 ued to
            // indicate ROM number, with X1 being LSB and X2 being MSB.
            assert(rom_count <= 4 && "Dynamically banked sets with more than 4 ROMs not supported");
            banked_rom = 1;

        }
    } else {
        assert(set->serve != SERVE_ADDR_ON_ANY_CS && "Single ROM set but multi-ROM serving mode configured");
    }
    assert(!(multi_rom && banked_rom) && "ROM set cannot be both multi-ROM and banked");

    // Figure out the actual CS lines and states for the given logical CS
    // state
    uint8_t cs1 = 2;
    uint8_t cs2 = 2;
    uint8_t cs3 = 2;
    uint8_t x1 = 2;
    uint8_t x2 = 2;

    // If a 24 pin ROM, handle X1/X2 in banked/multi-rom cases.
    // X1/X2 not supported on other ROM types yet
    if (sdrr_info.pins->chip_pins == 24) {
        if (banked_rom) {
            // X1 is LSB of ROM index, X2 is MSB of ROM index
            if (sdrr_info.pins->x_jumper_pull == 1) {
                // Active high: drive high to select
                x1 = (rom_index & 0x1) ? 1 : 0;
                x2 = (rom_index & 0x2) ? 1 : 0;
            } else {
                // Active low: drive low to select (inverted)
                x1 = (rom_index & 0x1) ? 0 : 1;
                x2 = (rom_index & 0x2) ? 0 : 1;
            }
        } else if (!multi_rom) {
            if (sdrr_info.pins->x_jumper_pull == 0) {
                // Active high: drive high to select
                x1 = 1;
                x2 = 1;
            } else {
                // Active low: drive low to select
                x1 = 0;
                x2 = 0;
            }
        } else {
            // Ignore X1/X2
        }
    }

    if ((cs_active <= 1) && (!multi_rom)) {
        cs1 = get_cs_gpio_state(rom_info->cs1_state, cs_active);
        cs2 = get_cs_gpio_state(rom_info->cs2_state, cs_active);
        cs3 = get_cs_gpio_state(rom_info->cs3_state, cs_active);

        // Override CS2/CS3 for 27C080 - CE and OE are always active low
        // and won't be configured in rom_info cs states
        if (rom_type == CHIP_TYPE_27C080) {
            cs2 = get_cs_gpio_state(CS_ACTIVE_LOW, cs_active);
            cs3 = get_cs_gpio_state(CS_ACTIVE_LOW, cs_active);
        }

        if ((rom_type == CHIP_TYPE_28C16) ||
            (rom_type == CHIP_TYPE_28C64) ||
            (rom_type == CHIP_TYPE_28C256) ||
            (rom_type == CHIP_TYPE_28C512)) {
            // Override CS3 for 28Cxx6 - it's always active high as it's /W
            cs3 = get_cs_gpio_state(CS_ACTIVE_HIGH, cs_active);

        }
    } else if (multi_rom) {
        // There are 3 CS lines - CS1 (ROM 0), X1 (ROM 1) and X2 (ROM 2)
        uint8_t rom_cs = get_cs_gpio_state(set->multi_rom_cs1_state, cs_active);

        // For inactive CS we must flip it, as all pins are actually 
        uint8_t inactive_cs = get_cs_gpio_state(set->multi_rom_cs1_state, sdrr_info.pins->x_jumper_pull);

        cs1 = inactive_cs;
        x1 = inactive_cs;
        if (rom_count > 2) {
            x2 = inactive_cs;
        } else {
            x2 = 2; // Not used, so we won't drive it
        }
        if (cs_active < 2) {
            // We want to actually drive the active rom index's CS line
            // either inactive or active
            if (rom_index == 0) {
                cs1 = rom_cs;
            } else if (rom_index == 1) {
                x1 = rom_cs;
            } else if (rom_index == 2) {
                x2 = rom_cs;
            }
        } else {
            // CS not driven any direction.  For now, leave them all as inactive
        }
    } else {
        // CS not active, so we won't drive any CS lines
    }

    // Get the number of address bits for this ROM type
    uint32_t chip_size = get_rom_image_size(set_index, rom_index);
    // Round up to next power of 2 — image may be non-power-of-2 (e.g. 23QL384)
    // but the PIO address space always is.
    if (chip_size & (chip_size - 1)) {
        uint32_t pow2 = 1;
        while (pow2 < chip_size) pow2 <<= 1;
        chip_size = pow2;
    }
    uint8_t num_addr_bits = 0;
    while (chip_size > 1) {
        chip_size >>= 1;
        num_addr_bits++;
    }

    // Now get the GPIO drive state for this address and CS state
    get_gpio_drive_from_addr_cs(
        rom_type,
        addr,
        num_addr_bits,
        cs1,
        cs2,
        cs3,
        x1,
        x2,
        gpios_to_drive,
        gpio_levels,
        bit_mode
    );
}

// Turns a GPIO read into an actual, de-mangled, data byte
uint32_t get_byte_from_gpio(uint64_t gpio_in, uint8_t data_bits) {
    uint32_t data = 0;

    assert(((data_bits == 8) || (data_bits == 16)) && "Invalid number of data bits");

    for (int ii = 0; ii < data_bits; ii++) {
        assert((data_pins[ii] < MAX_USED_GPIOS) && "Data bit out of range for ROM");
        if (gpio_in & (1ULL << data_pins[ii])) {
            data |= (1 << ii);
        }
    }
    return data;
}

void setup_addr_pins(void) {
    for (int ii = 0; ii < 16; ii++) {
        addr_pins[ii] = sdrr_info.pins->addr[ii];
        TST_DBG("Address pin %d: GPIO %d", ii, addr_pins[ii]);
    }
    for (int ii = 0; ii < 8; ii++) {
        addr_pins[16 + ii] = sdrr_info.pins->addr2[ii];
        TST_DBG("Address pin %d: GPIO %d", 16 + ii, addr_pins[16 + ii]);
    }
}

void setup_data_pins(void) {
    for (int ii = 0; ii < 8; ii++) {
        data_pins[ii] = sdrr_info.pins->data[ii];
        TST_DBG("Data pin %d: GPIO %d", ii, data_pins[ii]);
    }
    for (int ii = 0; ii < 8; ii++) {
        data_pins[8 + ii] = sdrr_info.pins->data2[ii];
        TST_DBG("Data pin %d: GPIO %d", 8 + ii, data_pins[8 + ii]);
    }
}

void check_data_pins_driven(epio_t *epio, uint8_t bit_mode) {
    uint64_t driven = epio_read_driven_pins(epio);
    for (int ii = 0; ii < bit_mode; ii++) {
        uint8_t pin = data_pins[ii];
        if (pin < MAX_USED_GPIOS) {
            if (!(driven & (1ULL << pin))) {
                uint64_t level = epio_read_pin_states(epio);
                TST_LOG("Data pin %d (GPIO %d) not driven when it should be at 0x%08X GPIOs driven: 0x%016llX levels: 0x%016llX", ii, pin, get_progress(), driven, level);
                assert(0 && "Data pin not driven");
            }
        }
    }
}

void check_data_pins_undriven(epio_t *epio, uint8_t bit_mode) {
    uint64_t driven = epio_read_driven_pins(epio);
    for (int ii = 0; ii < bit_mode; ii++) {
        uint8_t pin = data_pins[ii];
        if (pin < MAX_USED_GPIOS) {
            if (driven & (1ULL << pin)) {
                uint64_t level = epio_read_pin_states(epio);
                TST_LOG("Data pin %d (GPIO %d) driven when it shouldn't be at 0x%08X GPIO driven: 0x%016llX levels: 0x%016llX", ii, pin, get_progress(), driven, level);
                assert(0 && "Data pin driven when it shouldn't be");
            }
        }
    }
}

uint8_t are_cs_active_all_high(uint8_t set_index, uint8_t rom_index) {
    assert(set_index < SDRR_NUM_SETS && "Set index out of range");
    const sdrr_rom_set_t *set = &rom_set[set_index];
    assert(rom_index < set->rom_count && "ROM index out of range");
    const sdrr_rom_info_t *rom_info = set->roms[rom_index];

    uint8_t all_high = 1;
    if (rom_info->cs1_state != CS_NOT_USED) {
        all_high &= (rom_info->cs1_state == CS_ACTIVE_HIGH);
    }
    if (rom_info->cs2_state != CS_NOT_USED) {
        all_high &= (rom_info->cs2_state == CS_ACTIVE_HIGH);
    }
    if (rom_info->cs3_state != CS_NOT_USED) {
        all_high &= (rom_info->cs3_state == CS_ACTIVE_HIGH);
    }
    return all_high;
}