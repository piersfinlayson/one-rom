// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Provides ROM image functionality
//
// In particular this file handles the complex pin mapping required for USB
// access to live ROM images, mapping from logical ROM addresses to the actual
// RAM layout.  It includes per ROM type "snowflake" support analagous to (but
// inverted from) rust/gen/src/images.rs.

#include "usb_plugin.h"
#include "usb_rom.h"

// ---------------------------------------------------------------------------
// Access ROM set information
// ---------------------------------------------------------------------------
sdrr_rom_type_t app_get_active_rom_type(const usb_plugin_context_t *ctx) {
    const sdrr_rom_set_t *rom_set = app_get_active_rom_set(ctx);
    if (rom_set == NULL) {
        return INVALID_CHIP_TYPE;
    }
    const sdrr_rom_info_t *rom = rom_set->roms[0];
    return rom->rom_type;
}

const sdrr_rom_set_t *app_get_active_rom_set(const usb_plugin_context_t *ctx) {
    if ((ctx->firmware == NULL) || (ctx->runtime == NULL)) {
        ERR("Firmware or runtime info not available in context");
        return NULL;
    }

    uint8_t rom_set_index = ctx->runtime->rom_set_index;
    const onerom_metadata_header_t *metadata = ctx->firmware->metadata_header;
    if (rom_set_index >= metadata->rom_set_count) {
        ERR("Invalid ROM set index %u (only %u sets available)", rom_set_index, metadata->rom_set_count);
        return NULL;
    }

    const sdrr_rom_set_t *rom_set = &metadata->rom_sets[rom_set_index];
    return rom_set;
}

int app_get_active_rom_info(
    const usb_plugin_context_t *ctx,
    sdrr_rom_type_t *rom_type,
    uint8_t *rom_count,
    uint32_t *rom_size,
    sdrr_cs_state_t *cs1_state,
    sdrr_cs_state_t *cs2_state,
    sdrr_cs_state_t *cs3_state
) {
    const sdrr_rom_set_t *rom_set = app_get_active_rom_set(ctx);
    if (rom_set == NULL) {
        return -1;
    }

    const sdrr_rom_info_t *rom = rom_set->roms[0];
    *rom_type = rom->rom_type;
    *rom_count = rom_set->rom_count;
    *rom_size = ctx->get_chip_size_from_type(*rom_type);
    *cs1_state = rom->cs1_state;
    *cs2_state = rom->cs2_state;
    *cs3_state = rom->cs3_state;

    return 0;
}

uint32_t app_get_active_rom_size(const usb_plugin_context_t *ctx) {
    if (ctx->active_rom_set == NULL) {
        ERR("Active ROM set not available");
        return 0u;
    }
    const sdrr_rom_type_t rom_type = app_get_active_rom_type(ctx);

    if (ctx->get_chip_size_from_type == NULL) {
        ERR("Chip size from type not available in context");
        return 0u;
    }
    uint32_t rom_size = ctx->get_chip_size_from_type(rom_type);
    //DEBUG("Active ROM size: %u bytes (type 0x%02x)", rom_size, rom_type);

    return rom_size;
}

// Figures out the pin layout for this board, which is used by the address and
// data bit mapping functions below.
pb_status_t app_retrieve_pin_layout(
    const usb_plugin_context_t *ctx,
    rom_pin_layout_t           *layout
) {
    if (ctx->firmware == NULL) {
        ERR("Firmware info not available in context");
        return PB_STATUS_NOT_FOUND;
    }

    // Zero out layout
    memset(layout, 0, sizeof(*layout));

    // Get pins and ROM type information
    const sdrr_pins_t *pins = ctx->firmware->pins;
    layout->chip_pins = pins->chip_pins;
    uint8_t rom_count;
    uint32_t rom_size;
    int rc = app_get_active_rom_info(
        ctx,
        &layout->rom_type,
        &rom_count,
        &rom_size,
        &layout->cs1_state,
        &layout->cs2_state,
        &layout->cs3_state
    );
    if (rc) {
        ERR("Failed to get active ROM info");
        return PB_STATUS_NOT_FOUND;
    }

    if (rom_count < 1) {
        ERR("Zero ROM count set");
        return PB_STATUS_NOT_FOUND;
    }
    if (rom_count > 1) {
        ERR("Multi-ROM sets not supported yet");
        return PB_STATUS_NOT_PERMITTED;
    }

    // Derive address and data bits
    while (rom_size > 1) {
        rom_size >>= 1;
        layout->num_addr_bits++;
    }

    // Data bits is always 8 - even for a 27C400 as we handle it in 8-bit mode
    // here
    layout->num_data_bits = 8;

    // Populate address GPIOs
    layout->min_addr_pin = 0xFF;
    for (int ii = 0; ii < 16; ii++) {
        layout->addr_gpio[ii] = pins->addr[ii];
        layout->addr_gpio[ii + 16] = pins->addr2[ii];
    }

    // Populate data GPIOs.
    layout->min_data_pin = 0xFF;
    for (int ii = 0; ii < 8; ii++) {
        layout->data_gpio[ii] = pins->data[ii];
        layout->data_gpio[ii + 8] = pins->data2[ii];
    }

    // Populate X, CS and CS states
    layout->x1 = pins->x1;
    layout->x2 = pins->x2;
    layout->cs1 = pins->cs1;
    layout->cs2 = pins->cs2;
    layout->cs3 = pins->cs3;
    if (layout->chip_pins == 24) {
        if ((layout->rom_type != CHIP_TYPE_2332) && (layout->rom_type != CHIP_TYPE_2364)) {
            // Flip CS2 and CS3 for all 24 pin types except 2332 and 2364,
            // which have them in the "normal" order
            layout->cs2 = pins->cs3;
            layout->cs3 = pins->cs2;
        }
    } else if ((layout->chip_pins == 32) || (layout->chip_pins == 40)) {
        layout->cs1 = pins->ce;
        layout->cs2 = pins->oe;
    }

    // Figure out how many address bits are actually present in the address
    // space
    uint8_t addr_bits_in_space = 0;
    if (layout->chip_pins == 24) {
        // Always uses all 13 address bits (plus X1/X2/CS1 - see later)
        addr_bits_in_space = 13;
    } else if (layout->chip_pins == 28) {
        // All 28 pin chips use all 16 address bits in calculation, athough
        // the 231024 also adds two CS lines in there (later)
        addr_bits_in_space = 16;
    } else if (layout->chip_pins == 32) {
        // 32 pin types use all 19 address bits (512KB space)
        addr_bits_in_space = 19;
    } else if (layout->chip_pins == 40) {
        // 40 pin uses all 19 address bits (512KB space)
        addr_bits_in_space = 19;
    } else {
        ERR("Unsupported chip pin count %u", layout->chip_pins);
        return PB_STATUS_NOT_FOUND;
    }

    // Based on that, find the minimum address pin number
    for (int ii = 0; ii < addr_bits_in_space; ii++) {
        if (layout->addr_gpio[ii] < layout->min_addr_pin) layout->min_addr_pin = layout->addr_gpio[ii];
    }

    // Find the minimum data pin number
    for (int ii = 0; ii < layout->num_data_bits; ii++) {
        if (layout->data_gpio[ii] < layout->min_data_pin) layout->min_data_pin = layout->data_gpio[ii];
    }

    // Finally, H=handle X1/X2 pins (24 pin only) and other specials.
    if (layout->chip_pins == 24) {
        // While 24 pin ROMs use whatever CS lines they have in the address
        // apart from CS1, these are already included by virtue of using all
        // 13 address lines
        if (pins->x1 < layout->min_addr_pin) layout->min_addr_pin = pins->x1;
        if (pins->x2 < layout->min_addr_pin) layout->min_addr_pin = pins->x2;
        if (pins->cs1 < layout->min_addr_pin) layout->min_addr_pin = pins->cs1;

        // 2732 has A11 and CS2 transposed, so correct for that here
        if (layout->rom_type == CHIP_TYPE_2732) {
            // CS2 and CS3 have been flipped by the time we get here, so we
            // need to flip A11 with CS3
            uint8_t cs3_pin = pins->cs3;
            layout->cs3 = layout->addr_gpio[11];
            layout->addr_gpio[11] = cs3_pin;
        }
    } else if (layout->chip_pins == 28) {
        // Only the 231024 ROM has CS lines in the address space
        if (layout->rom_type == CHIP_TYPE_231024) {
            // While 231024 only has a single CS line, both CS lines are used
            // in the (256KB) address space
            if (pins->cs2 < layout->min_addr_pin) layout->min_addr_pin = pins->cs2;
            if (pins->cs1 < layout->min_addr_pin) layout->min_addr_pin = pins->cs1;
        }
    } else if (layout->chip_pins == 32 && layout->rom_type == CHIP_TYPE_27C301) {
        // 27C301 shifts A16 from the start of the range to the end
        layout->min_addr_pin++;
        layout->addr_gpio[16] += 19;
    } else if (layout->chip_pins == 40 && layout->rom_type == CHIP_TYPE_27C400) {
        // Althought A0(A-1) is not really at the configured GPIO on the
        // fire-40-a board, it is effectively, as the PIO algorithm works
        // around this.  So leave the mapping as is.
    }

    return PB_STATUS_OK;
}

// Convert a logical address (i.e. the address as seen by the ROM) to an offset
// into the RAM table
static pb_status_t app_logical_addr_to_offset(
    uint32_t logical_addr,
    const rom_pin_layout_t *layout,
    uint32_t *offset_out,
    const usb_plugin_context_t *ctx
) {
    if (ctx->runtime == NULL) {
        ERR("Runtime info not available in context");
        return PB_STATUS_NOT_FOUND;
    }

    uint32_t rom_table_size = ctx->runtime->rom_table_size;

    uint32_t offset_addr = 0u;
    for (int ii = 0; ii < layout->num_addr_bits; ii++) {
        if (layout->addr_gpio[ii] < 0xFF) {
            uint8_t bit = (logical_addr >> ii) & 1u;
            offset_addr |= ((uint32_t)bit << (layout->addr_gpio[ii] - layout->min_addr_pin));
        }
    }

    if (layout->chip_pins == 24) {
        // TODO: Handle X1/X2 for multi-ROM and dynamically banked ROM sets
    }
    if (offset_addr >= rom_table_size) {
        ERR("Offset address 0x%08x out of bounds for ROM table", offset_addr);
        return PB_STATUS_INVALID_ADDRESS;
    }

    *offset_out = offset_addr;
    DEBUG("Logical address 0x%08x maps to offset 0x%08x in ROM table", logical_addr, offset_addr);

    return PB_STATUS_OK;
}

// Get the logical byte value at a logical ROM address by reading the raw byte
// from the appropriate RAM location and remapping it to a logical byte.
pb_status_t app_get_logical_byte_from_logical_addr(
    uint32_t logical_addr,
    uint32_t  *value_out,
    const rom_pin_layout_t *layout,
    const usb_plugin_context_t *ctx
) {
    if (ctx->runtime == NULL) {
        ERR("Runtime info not available in context");
        return PB_STATUS_NOT_FOUND;
    }

    // Get the physical RAM address
    uint32_t offset_addr;
    pb_status_t st = app_logical_addr_to_offset(logical_addr, layout, &offset_addr, ctx);
    if (st != PB_STATUS_OK) {
        return st;
    }

    // Get the raw byte from RAM
    const uint8_t *rom_data = (const uint8_t *)(uintptr_t)(ctx->runtime->rom_table);
    uint8_t raw = rom_data[offset_addr];

    // Reverse the data pin permutation: raw bit (data_gpio[ii] - min_data_pin) → logical bit ii
    uint32_t logical_value = 0u;
    for (int ii = 0; ii < layout->num_data_bits; ii++) {
        if (layout->data_gpio[ii] < 0xFF) {
            uint8_t bit = (raw >> (layout->data_gpio[ii] - layout->min_data_pin)) & 1u;
            logical_value |= ((uint32_t)bit << ii);
        }
    }

    *value_out = logical_value;
    return PB_STATUS_OK;
}

// Writes a logical byte value to a logical ROM address by remapping it to a
// raw byte and writing that to the appropriate RAM location.
pb_status_t app_set_logical_byte_at_logical_addr(
    uint32_t                    logical_addr,
    uint8_t                     logical_value,
    const rom_pin_layout_t     *layout,
    const usb_plugin_context_t *ctx
) {
    if (ctx->runtime == NULL) {
        ERR("Runtime info not available in context");
        return PB_STATUS_NOT_FOUND;
    }

    uint32_t offset_addr;
    pb_status_t st = app_logical_addr_to_offset(logical_addr, layout, &offset_addr, ctx);
    if (st != PB_STATUS_OK) {
        return st;
    }

    // Reverse the data pin permutation: logical bit ii → raw bit (data_gpio[ii] - min_data_pin)
    uint8_t raw = 0u;
    for (int ii = 0; ii < layout->num_data_bits; ii++) {
        if (layout->data_gpio[ii] < 0xFF) {
            uint8_t bit = (logical_value >> ii) & 1u;
            raw |= (uint8_t)(bit << (layout->data_gpio[ii] - layout->min_data_pin));
        }
    }

    // Write the raw byte to RAM
    //DEBUG("Writing logical value 0x%02x to logical address 0x%08x (offset 0x%08x, raw value 0x%02x)", logical_value, logical_addr, offset_addr, raw);
    uint8_t  *rom_data = (uint8_t *)(uintptr_t)(ctx->runtime->rom_table);
    rom_data[offset_addr] = raw;
    return PB_STATUS_OK;
}
