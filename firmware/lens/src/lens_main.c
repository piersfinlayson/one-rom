// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM WASM API

// Causes declarations in stubbed out code, so One ROM firmware compiles
#define TEST_MAIN_C

#include <stdio.h>
#include <include.h>
#include "roms.h"
#include <test/stub.h>
#include <apio.h>
#include <epio.h>
#include <epio_wasm.h>

static epio_t *g_epio = NULL;
static uint8_t addr_pins[32];
static uint8_t data_pins[16];
static uint8_t cs1_pin, cs2_pin, cs3_pin, x1_pin, x2_pin, ce_pin, oe_pin, byte_pin;

static const sdrr_rom_info_t *get_first_rom_info(void) {
    if (sdrr_rom_set_count == 0) {
        return NULL; // No ROM sets available
    }

    // Get the first ROM image
    const sdrr_rom_set_t *rom_set_0 = &rom_set[0];
    if (rom_set_0->rom_count == 0) {
        return NULL; // No ROMs in the set
    }

    return rom_set_0->roms[0];
}

EPIO_EXPORT sdrr_rom_type_t onerom_lens_get_rom_type(void) {
    const sdrr_rom_info_t *rom = get_first_rom_info();
    if (!rom) {
        return INVALID_CHIP_TYPE; // No ROM available
    }
    return rom->rom_type;
}

static void setup_pin_mappings(void) {
    // Address pins
    for (int ii = 0; ii < 16; ii++) {
        addr_pins[ii] = sdrr_info.pins->addr[ii];
    }
    for (int ii = 0; ii < 8; ii++) {
        addr_pins[16 + ii] = sdrr_info.pins->addr2[ii];
    }
    
    // Data pins
    for (int ii = 0; ii < 8; ii++) {
        data_pins[ii] = sdrr_info.pins->data[ii];
    }
    for (int ii = 0; ii < 8; ii++) {
        data_pins[8 + ii] = sdrr_info.pins->data2[ii];
    }
    
    // Control pins
    cs1_pin = sdrr_info.pins->cs1;
    cs2_pin = sdrr_info.pins->cs2;
    cs3_pin = sdrr_info.pins->cs3;
    x1_pin = sdrr_info.pins->x1;
    x2_pin = sdrr_info.pins->x2;
    ce_pin = sdrr_info.pins->ce;
    oe_pin = sdrr_info.pins->oe;
    byte_pin = sdrr_info.pins->byte;

    if ((onerom_lens_get_rom_type() == CHIP_TYPE_27C400) && (addr_pins[0] == byte_pin)) {
        // fire-40-a JSON config was a hack - addr pin 0 is really pin 37
        addr_pins[0] = 37;
    }
}

EPIO_EXPORT int32_t onerom_lens_get_rom_size(void) {
    const sdrr_rom_info_t *rom = get_first_rom_info();
    if (!rom) {
        return -1; // No ROM available
    }

    assert(rom->rom_type < NUM_CHIP_TYPES);
    uint32_t size = chip_size_from_type[rom->rom_type];

    return size;
}

EPIO_EXPORT uint8_t onerom_lens_get_num_addr_bits(void) {
    int32_t rom_size = onerom_lens_get_rom_size();
    if (rom_size == -1) {
        return 0; // No ROM available
    }

    for (int ii = 0; ii < 20; ii++) {
        if (rom_size == (1 << ii)) {
            return ii;
        }
    }

    return 0; // Size not recognised
}

EPIO_EXPORT uint8_t onerom_lens_get_num_data_bits(void) {
    const sdrr_rom_info_t *rom = get_first_rom_info();
    if (!rom) {
        return 0; // No ROM available
    }

    assert(rom->rom_type < NUM_CHIP_TYPES);
    uint8_t data_bits = 8;
    if (rom->rom_type == CHIP_TYPE_27C400) {
        data_bits = 16;
    }

    return data_bits;
}

// Step the emulator forward by N cycles
EPIO_EXPORT void onerom_step(uint32_t cycles) {
    if (g_epio) {
        epio_step_cycles(g_epio, cycles);
    }
}

// Drive address and control lines
// addr: address to drive (up to 24 bits)
// num_addr_bits: number of address bits to drive (13, 14, 15, etc.)
// cs1, cs2, cs3: chip select states (0=low, 1=high, 2=not driven)
// x1, x2: additional control signals (0=low, 1=high, 2=not driven)
EPIO_EXPORT void onerom_drive_pins(
    uint32_t addr,
    uint8_t num_addr_bits,
    uint8_t cs1,
    uint8_t cs2,
    uint8_t cs3,
    uint8_t x1,
    uint8_t x2,
    uint8_t ce,
    uint8_t oe,
    uint8_t byte
) {
    if (!g_epio) return;
    
    uint64_t drive_mask = 0;
    uint64_t level_mask = 0;
    
    // Address lines
    for (int ii = 0; ii < num_addr_bits; ii++) {
        drive_mask |= (1ULL << addr_pins[ii]);
        if (addr & (1 << ii)) {
            level_mask |= (1ULL << addr_pins[ii]);
        }
    }
    
    // Control lines
    if (cs1 < 2) {
        if (cs1_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << cs1_pin);
            if (cs1) level_mask |= (1ULL << cs1_pin);
        }
    }
    if (cs2 < 2) {
        if (cs2_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << cs2_pin);
            if (cs2) level_mask |= (1ULL << cs2_pin);
        }
    }
    if (cs3 < 2) {
        if (cs3_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << cs3_pin);
            if (cs3) level_mask |= (1ULL << cs3_pin);
        }
    }
    if (x1 < 2) {
        if (x1_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << x1_pin);
            if (x1) level_mask |= (1ULL << x1_pin);
        }
    }
    if (x2 < 2) {
        if (x2_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << x2_pin);
            if (x2) level_mask |= (1ULL << x2_pin);
        }
    }
    if (ce < 2) {
        if (ce_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << ce_pin);
            if (ce) level_mask |= (1ULL << ce_pin);
        }
    }
    if (oe < 2) {
        if (oe_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << oe_pin);
            if (oe) level_mask |= (1ULL << oe_pin);
        }
    }
    if (byte < 2) {
        if (byte_pin < NUM_GPIOS) {
            drive_mask |= (1ULL << byte_pin);
            if (byte) level_mask |= (1ULL << byte_pin);
        }
    }
    
    epio_drive_gpios_ext(g_epio, drive_mask, level_mask);
}

EPIO_EXPORT void onerom_drive_addr(
    uint32_t addr,
    uint8_t cs_active,
    uint8_t bit_mode
) {
    if (!g_epio) return;

    if ((bit_mode != 8) && (bit_mode != 16)) {
        printf("Invalid bit mode: %d\n", bit_mode);
        return; // Invalid bit mode
    }

    sdrr_rom_type_t rom_type = onerom_lens_get_rom_type();
    assert(rom_type != INVALID_CHIP_TYPE);
    assert(rom_type < NUM_CHIP_TYPES);

    if ((bit_mode == 16) && (rom_type != CHIP_TYPE_27C400)) {
        printf("16-bit mode only supported for 27C400 ROMs\n");
        return;
    }

    uint8_t cs_val = cs_active ? 0 : 1; // Active low

    uint8_t addr_bits = onerom_lens_get_num_addr_bits();
    assert(addr_bits <= 19);
    int32_t rom_size = onerom_lens_get_rom_size();
    assert(rom_size != -1);
    
    switch (rom_type) {
        case CHIP_TYPE_2316:
        case CHIP_TYPE_23128:
            onerom_drive_pins(
                addr,
                addr_bits,
                cs_val,
                cs_val,
                cs_val,
                2,
                2,
                2,
                2,
                2
            );
            break;

        case CHIP_TYPE_2332:
        case CHIP_TYPE_23256:
        case CHIP_TYPE_23512:
            onerom_drive_pins(
                addr,
                addr_bits,
                cs_val,
                cs_val,
                2,
                2,
                2,
                2,
                2,
                2
            );
            break;

        case CHIP_TYPE_2364:
        case CHIP_TYPE_231024:
            onerom_drive_pins(
                addr,
                addr_bits,
                cs_val,
                2,
                2,
                2,
                2,
                2,
                2,
                2
            );
            break;

        case CHIP_TYPE_2704:
        case CHIP_TYPE_2708:
        case CHIP_TYPE_2716:
        case CHIP_TYPE_2732:
        case CHIP_TYPE_2764:
        case CHIP_TYPE_27128:
        case CHIP_TYPE_27256:
        case CHIP_TYPE_27512:
        case CHIP_TYPE_27C010:
        case CHIP_TYPE_27C301:
        case CHIP_TYPE_27C020:
        case CHIP_TYPE_27C040:
        case CHIP_TYPE_27C080:
            onerom_drive_pins(
                addr,
                addr_bits,
                2,
                2,
                2,
                2,
                2,
                cs_val,
                cs_val,
                2
            );
            break;

        case CHIP_TYPE_27C400:
            ;
            uint32_t new_addr = bit_mode == 16 ? addr << 1 : addr;
            onerom_drive_pins(
                new_addr,
                addr_bits,
                2,
                2,
                2,
                2,
                2,
                cs_val,
                cs_val,
                bit_mode == 16 ? 1 : 0
            );
            break;

        default:
            assert(0 && "Unsupported ROM type");
            break;
    }
}

// Stop driving all GPIOs
EPIO_EXPORT void onerom_release_pins(void) {
    if (g_epio) {
        epio_drive_gpios_ext(g_epio, 0, 0);
    }
}

// Read the data pins and return as a byte
EPIO_EXPORT uint32_t onerom_read_data(uint8_t data_bits) {
    if (!g_epio) return 0;
    
    uint64_t gpio_state = epio_read_pin_states(g_epio);
    uint32_t data = 0;
    
    for (int ii = 0; ii < data_bits; ii++) {
        if (gpio_state & (1ULL << data_pins[ii])) {
            data |= (1 << ii);
        }
    }
    
    return data;
}

// Get the GPIO number for a specific address pin
EPIO_EXPORT uint8_t onerom_get_addr_pin(uint8_t bit) {
    if (bit < 32) {
        return addr_pins[bit];
    }
    return 0xFF;
}

// Get the GPIO number for a specific data pin
EPIO_EXPORT uint8_t onerom_get_data_pin(uint8_t bit) {
    if (bit < 16) {
        return data_pins[bit];
    }
    return 0xFF;
}

EPIO_EXPORT int onerom_get_pio_disassembly(
    char *buffer,
    size_t buffer_size
) {
    int wrote, total_written = 0;

    if (!g_epio) return -1;

    wrote = epio_disassemble_sm(g_epio, 0, 0, buffer, buffer_size);
    if (wrote < 0) {
        return -1;
    }
    buffer += wrote-1;
    buffer_size -= wrote-1;
    total_written += wrote-1;

    // Add 2 \ns between SMs if we have space
    if (buffer_size > 2) {
        *buffer++ = '\n';
        *buffer++ = '\n';
        buffer_size -= 2;
        total_written += 2;
    } else {
        return -1;
    }

    wrote = epio_disassemble_sm(g_epio, 0, 1, buffer, buffer_size);
    if (wrote < 0) {
        return -1;
    }
    buffer += wrote-1;
    buffer_size -= wrote-1;
    total_written += wrote-1;

    // Add 2 \ns between SMs if we have space
    if (buffer_size > 2) {
        *buffer++ = '\n';
        *buffer++ = '\n';
        buffer_size -= 2;
        total_written += 2;
    } else {
        return -1;
    }

    wrote = epio_disassemble_sm(g_epio, 0, 2, buffer, buffer_size);
    if (wrote < 0) {
        return -1;
    }
    buffer += wrote;
    buffer_size -= wrote;
    total_written += wrote;

    return total_written;
}

// Get control pin GPIO numbers
EPIO_EXPORT uint8_t onerom_get_cs1_pin(void) { return cs1_pin; }
EPIO_EXPORT uint8_t onerom_get_cs2_pin(void) { return cs2_pin; }
EPIO_EXPORT uint8_t onerom_get_cs3_pin(void) { return cs3_pin; }
EPIO_EXPORT uint8_t onerom_get_x1_pin(void) { return x1_pin; }
EPIO_EXPORT uint8_t onerom_get_x2_pin(void) { return x2_pin; }
EPIO_EXPORT uint8_t onerom_get_ce_pin(void) { return ce_pin; }
EPIO_EXPORT uint8_t onerom_get_oe_pin(void) { return oe_pin; }
EPIO_EXPORT uint8_t onerom_get_byte_pin(void) { return byte_pin; }

// Initialize the One ROM emulator
// Returns epio_t* handle on success, NULL on failure.  This handle can be used
// to directly access epio functions if needed.
EPIO_EXPORT epio_t *onerom_init(void) {
    if (g_epio) {
        return 0; // Already initialized
    }
    
    // Set up pin mappings
    setup_pin_mappings();
    
    // Run the firmware to set up the PIO programs
    firmware_main();
    
    // Check PIOs were enabled
    if (_apio_emulated_pio.pios_enabled != 1) {
        return 0;
    }
    
    // Start the PIO emulator
    g_epio = epio_from_apio();
    if (!g_epio) {
        return 0;
    }
    
    // Double check data lines are controllable as outputs by PIO block 2
    for (int ii = 0; ii < 8; ii++) {
        if (!epio_block_can_control_gpio_output(g_epio, 2, sdrr_info.pins->data[ii])) {
            printf("!!Data pin %d (GPIO %d) not controlled by PIO block 2\n", ii, sdrr_info.pins->data[ii]);
        }
    }
    for (int ii = 0; ii < 8; ii++) {
        if (sdrr_info.pins->data2[ii] < 255) {
            if (!epio_block_can_control_gpio_output(g_epio, 2, sdrr_info.pins->data2[ii])) {
                printf("!!Data pin %d (GPIO %d) not controlled by PIO block 2\n", ii+8, sdrr_info.pins->data2[ii]);
            }
        }
    }

    // Configure the DMA chain
    uint8_t bit_mode = 8;
    if (onerom_lens_get_rom_type() == CHIP_TYPE_27C400) {
        bit_mode = 16;
    }
    epio_dma_setup_read_pio_chain(
        g_epio,
        0,
        1,
        0,
        4,
        2,
        1,
        4,
        bit_mode
    );
    
    // Load ROM data into SRAM
    uint64_t *source = get_ram_rom_image_table_aligned();
    epio_sram_set(g_epio, 0x20000000, (uint8_t *)source, 512*1024);

    // Disassemble PIOs
    char buffer[4096];
    int wrote;
    wrote = epio_disassemble_sm(g_epio, 1, 0, buffer, sizeof(buffer));
    assert(wrote >= 0);
    printf("%s\n", buffer);
    wrote = epio_disassemble_sm(g_epio, 1, 1, buffer, sizeof(buffer));
    assert(wrote >= 0);
    printf("%s\n", buffer);
    wrote = epio_disassemble_sm(g_epio, 2, 0, buffer, sizeof(buffer));
    assert(wrote >= 0);
    printf("%s\n", buffer);
    wrote = epio_disassemble_sm(g_epio, 2, 1, buffer, sizeof(buffer));
    assert(wrote >= 0);
    printf("%s\n", buffer);
    wrote = epio_disassemble_sm(g_epio, 2, 2, buffer, sizeof(buffer));
    assert(wrote >= 0);
    printf("%s\n", buffer);
    
    return g_epio;
}
