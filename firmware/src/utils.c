// One ROM utils

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"

// This function checks the state of the image select pins, and returns an
// integer value, as if the sel pins control bit 0, 1, 2, 3, etc in order of
// that integer.  The first sel pin in the array is bit 0, the second bit 1, 
// etc.
uint32_t check_sel_pins(uint32_t *sel_mask) {
    uint32_t num_sel_pins, sel_value;
    uint64_t orig_sel_mask, gpio_value, sel_flip_bits;

    // Setup the pins first.  Do this first to allow any pull-ups to settle
    // before reading.
    num_sel_pins = setup_sel_pins(&orig_sel_mask, &sel_flip_bits);
    if (num_sel_pins == 0) {
        DEBUG("No image select pins");
        disable_sel_pins();
        *sel_mask = 0;
        return 0;
    }

    // Read the actual GPIO value, masked appropriately
    gpio_value = get_sel_value(orig_sel_mask, sel_flip_bits);

    // SEGGER doesn't cope well with 64-bit values, so split into two 32-bit
    // parts for logging
    DEBUG("Sel GPIO val: 0x%08lX%08lX #: %lu mask 0x%08lX%08lX", 
        (uint32_t)(gpio_value >> 32), (uint32_t)gpio_value,
        (unsigned long)num_sel_pins,
        (uint32_t)(orig_sel_mask >> 32), (uint32_t)orig_sel_mask);

    disable_sel_pins();

    // Now turn the GPIO value into a SEL value, with the bits consecutive
    // starting from bit 0, based on which pin the SEL value is.  At the same
    // time we have to update sel_mask, to match.  This gives us an integer
    // which can be used as an index into the rom set
    *sel_mask = 0;
    sel_value = 0;
    for (int ii = 0; ii < MAX_IMG_SEL_PINS; ii++) {
        uint8_t pin = HW->gpio_sel[ii];
        if (pin < MAX_GPIOS) {
            if (gpio_value & (1ULL << pin)) {
                sel_value |= (1 << ii);
            }
            *sel_mask |= (1ULL << ii);
        }
    }

    DEBUG("Sel value: 0x%08X mask: 0x%08X", sel_value, *sel_mask);

    // Store the value of the pins
    RUNTIME->image_sel = sel_value;

    return sel_value;
}

// Read in firmware overrides from the selected ROM set
void process_firmware_overrides(const onerom_rom_slot_t *slot) {
    const onerom_firmware_overrides_t *overrides = slot->firmware_overrides;
    if ((overrides != NULL) && (overrides != (void*)0xFFFFFFFF)) {
        if (overrides->override_present[0] & (1 << 2)) {
            RUNTIME->fire_freq = overrides->fire_freq;
            LOG("Fire freq override: %d", RUNTIME->fire_freq);
        }
        if (overrides->override_present[0] & (1 << 3)) {
            RUNTIME->overclock_enabled = overrides->override_value[0] & (1 << 1) ? 1 : 0;
            LOG("Fire overclock override: %d", RUNTIME->overclock_enabled);
        }
        if (overrides->override_present[0] & (1 << 4)) {
            RUNTIME->fire_vreg = overrides->fire_vreg;
            LOG("Fire VREG override: %d", RUNTIME->fire_vreg);
        }
        if (overrides->override_present[0] & (1 << 5)) {
            RUNTIME->status_led_enabled = overrides->override_value[0] & (1 << 2) ? 1 : 0;
            LOG("Status LED override: %d", RUNTIME->status_led_enabled);
        }
        if (overrides->override_present[0] & (1 << 6)) {
            RUNTIME->swd_enabled = overrides->override_value[0] & (1 << 3) ? 1 : 0;
            LOG("SWD enabled override: %d", RUNTIME->swd_enabled);
        }
    }
}

// Checks for minimal metadata presence and validity.
//
// If this function returns non-zero, subsequent code can assume the top-level
// metadata structure is present and valid, as are hardware_info and
// firmware_config and can be read from safely.
uint8_t metadata_valid(void) {
    // Check if the metadata structure exists
    if (METADATA == NULL || METADATA == (void*)0xFFFFFFFF) {
        return 0;
    }

    if (METADATA->turbo_boot) {
        return 1;
    }

    // Make sure the header and version are as expected.
    for (int ii = 0; ii < 16; ii++) {
        if (METADATA->magic[ii] != ONEROM_METADATA_MAGIC[ii]) {
            return 0;
        }
    }
    if (METADATA->version != CURRENT_METADATA_VERSION) {
        return 0;
    }

    // Make sure the hardware info pointer is valid
    if (HW == NULL || HW == (void*)0xFFFFFFFF) {
        return 0;
    }

    // Make sure the firmware config pointer is valid
    if (FW == NULL || FW == (void*)0xFFFFFFFF) {
        return 0;
    }

    // Check we have a hardware revision string
    if (HW->hw_rev == NULL || HW->hw_rev == (void*)0xFFFFFFFF) {
        return 0;
    }
    LOG("Board: %s", HW->hw_rev);

    if (HW->rp235x != RUNTIME->rp235x) {
        ERR("Runtime RP235X %c vs metadata %c", RUNTIME->rp235x == RP235XA ? 'A' : 'B', HW->rp235x ? 'A' : 'B');
        return 0;
    }
    LOG("MCU: RP235X%c", RUNTIME->rp235x == RP235XA ? 'A' : 'B');

    return 1;
}

void update_runtime_from_metadata(void) {
    RUNTIME->swd_enabled = METADATA->swd_enabled;
    RUNTIME->boot_logging = METADATA->boot_logging;
}

#if REAL_HARDWARE
void limp_mode(limp_mode_pattern_t pattern) {
    LOG("Limp mode %d", pattern);

    RUNTIME->limp_mode = pattern;

    uint32_t on_time, off_time;

    if (!RUNTIME->status_led_enabled && RUNTIME->status_led_enabled) {
        DEBUG("Enabled status LED");
        setup_status_led();
    }

    if (pattern >= NUM_LIMP_MODE_PATTERNS) {
        DEBUG("!!! Invalid limp mode pattern");
        pattern = LIMP_MODE_NONE;
    }

    on_time = limp_mode_patterns[pattern].on_time;
    off_time = limp_mode_patterns[pattern].off_time;

    while (1) {
        blink_pattern(on_time, off_time, 1);
    }
}
#endif // REAL_HARDWARE

//
// Functions to handle copying functions to and executing them from RAM
//

#if !defined(REAL_HARDWARE)
// Copies a function from flash to RAM
void copy_func_to_ram(void (*fn)(void), uint32_t ram_addr, size_t size) {
    // Copy the function to RAM
    memcpy((void*)ram_addr, (void*)((uint32_t)fn & ~1), size);
}

void execute_ram_func(uint32_t ram_addr) {
    // Execute the function in RAM
    void (*ram_func)() = (void(*)(void))(ram_addr | 1);
    ram_func();
}
#endif // REAL_HARDWARE

// Simple delay function
void delay(volatile uint32_t count) {
    while(count--);
}

// Pull in the RAM ROM image start/end locations from the linker
extern uint32_t _ram_rom_image_start[];
extern uint32_t _ram_rom_image_end[];

// Get the index of the selected ROM by from the image select jumper values
//
// Returns the index
uint8_t get_rom_slot_index(uint32_t sel_pins, uint32_t sel_mask, uint8_t plugins) {
    uint8_t rom_sel, rom_index;

    // Shift the sel pins to read from 0.  Do this by shifting each present
    // bit (usig sel_mask) the appropriate number of bits right
    rom_sel = 0;
    int bit_pos = 0;
    for (int ii = 0; ii < 32; ii++) {
        if (sel_mask & (1 << ii)) {
            if (sel_pins & (1 << ii)) {
                rom_sel |= (1 << bit_pos);
            }
            bit_pos++;
        }
    }

    // See what plugins we have
    uint8_t num_plugins = plugins & 1 ? 1 : 0;
    num_plugins += plugins & 2 ? 1 : 0;

    // Get the rom set count
    uint8_t rom_slot_count = METADATA->rom_slot_count;
    if (rom_slot_count <= num_plugins) {
        ERR("No ROM sets");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return 0;
    }

    // Calculate the ROM image index based on the selection bits and number of
    // images installed in this firmware.  For example, if image 4 was selected
    // but there are only 3 images, it will select image 1.
    rom_index = (rom_sel % (rom_slot_count - num_plugins)) + num_plugins;

    if (rom_index >= rom_slot_count) {
        ERR("ROM set cal error: %d", rom_index);
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return 0;
    }

    LOG("ROM sel/index %d/%d", rom_sel, rom_index);

    return rom_index;
}

#if REAL_HARDWARE

void preload_rom_image(void) {
    // Get ROM slot information
    const onerom_rom_slot_t *slot = RUNTIME->current_rom_slot;
    uint32_t img_size = slot->size;
    uint32_t *img_src = (uint32_t *)(slot->data);
    uint32_t *img_dst = _ram_rom_image_start;

    // Set up the runtime ROM table pointer and size now, before any early
    // return.
    RUNTIME->rom_table = (void *)img_dst;
    RUNTIME->rom_table_size = img_size;

    if (img_src == (uint32_t *)0xFFFFFFFF) {
        LOG("No RAM image");
        return;
    }

    const char *filename = "";
    if (BOOT_LOGGING_EN) {
        if (slot->roms[0]->filename != NULL) {
            filename = slot->roms[0]->filename;
        }
        LOG("ROM preload %s from 0x%08X to 0x%08X size 0x%08X bytes",
            filename, (uint32_t)(uintptr_t)img_src, (uint32_t)(uintptr_t)img_dst, img_size);
    }

    if ((((uint32_t)img_src % 4) != 0) || (((uint32_t)img_dst % 4) != 0)) {
        ERR("ROM src/dest unaligned: 0x%08X 0x%08X", (uint32_t)img_src, (uint32_t)img_dst);
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    dma_copy((uint32_t)img_src, (uint32_t)img_dst, (img_size + 3) / 4);
    LOG("DMA preload initiated from 0x%08X to 0x%08X size 0x%08X %s",
        (uint32_t)(uintptr_t)img_src, (uint32_t)(uintptr_t)img_dst, img_size, filename);

    LOG("Slot ROM count: %d", slot->rom_count);

    return;
}

#else // !REAL_HARDWARE

void preload_rom_image(void) {
    // Get ROM slot information
    const onerom_rom_slot_t *slot = RUNTIME->current_rom_slot;
    uint64_t *ram_table_ptr = get_ram_rom_image_table_aligned();
    uint32_t img_size = slot->size;
    uint64_t *img_src = (uint64_t *)(slot->data);

    // Set up the runtime ROM table pointer and size now, before any early return.  For
    RUNTIME->rom_table_size = img_size;
    RUNTIME->rom_table = (void *)(uintptr_t)0x20000000;

    if (img_src == (uint64_t *)0xFFFFFFFF) {
        LOG("No RAM image");
        return;
    }

    uint64_t *img_dst = ram_table_ptr;

        const char *filename = "";
    if (BOOT_LOGGING_EN) {
        if (slot->roms[0]->filename != NULL) {
            filename = slot->roms[0]->filename;
        }
        LOG("ROM preload %s from 0x%08X to 0x%08X size 0x%08X bytes",
            filename, (uint32_t)(uintptr_t)img_src, (uint32_t)(uintptr_t)img_dst, img_size);
    }

    // Set image (either single ROM or multiple ROMs) has been fully
    // pre-processed before embedding in the flash.
    memcpy(img_dst, img_src, img_size);
    LOG("CPU preload complete from 0x%08X to 0x%08X size 0x%08X %s",
        (uint32_t)(uintptr_t)img_src, (uint32_t)(uintptr_t)img_dst, img_size, filename);

    LOG("Slot ROM count: %d", slot->rom_count);
    // For testing, pretend we wrote to 0x20000000 (start of SRAM). The tester
    // copies the data from where it was actually written to the PIO emulator.
    RUNTIME->rom_table = (void *)(uintptr_t)0x20000000;

    return;
}

#endif // REAL_HARDWARE