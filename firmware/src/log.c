// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM logging

#include "include.h"

#if defined(BOOT_LOGGING)
extern uint32_t _onerom_runtime_info_start;
extern uint32_t _ram_rom_image_start[];

// Logging function to output various debug information via RTT
void log_init(void) {
    LOG(log_divider);
    LOG("%s v%d.%d.%d.%d %s", product, INFO->major_version, INFO->minor_version, INFO->patch_version, INFO->build_number, project_url);
    LOG("%s %s", copyright, author);
#if defined(DEBUG_BUILD)
    LOG("Built: %s (DEBUG)", INFO->build_date);
#else // !DEBUG_BUILD
    LOG("Built: %s", INFO->build_date);
#endif // DEBUG_BUILD
    LOG("Commit: %s", INFO->commit);

    DEBUG("onerom_info: 0x%08X", (uint32_t)(uintptr_t)INFO);
    DEBUG("RAM ROM table: 0x%08X", (uint32_t)(uintptr_t)&_ram_rom_image_start);
    DEBUG("runtime_info: 0x%08X", (uint32_t)(uintptr_t)RUNTIME);
    DEBUG("RTT CB: 0x%08X", (uint32_t)(uintptr_t)INFO->rtt);
    DEBUG(log_divider);
    DEBUG("RT Fire Freq: 0x%04X", RUNTIME->fire_freq);
    DEBUG("RT Overclock Enabled: 0x%02X", RUNTIME->overclock_enabled);
    DEBUG("RT Status LED Enabled: 0x%02X", RUNTIME->status_led_enabled);
    DEBUG("RT SWD Enabled: 0x%02X", RUNTIME->swd_enabled);

    LOG(log_divider);
    platform_logging();

    LOG(log_divider);
}

void log_roms() {
    if (BOOT_LOGGING_EN) {
        LOG("# of ROM sets: %d", METADATA->rom_slot_count);

        for (uint8_t ii = 0; ii < METADATA->rom_slot_count; ii++) {
            const onerom_rom_slot_t *slot = &METADATA->rom_slots[ii];

            LOG("Set #%d: %d ROM(s), size: %d bytes", ii, slot->rom_count, slot->size);

#if defined(DEBUG_LOGGING)
            for (uint8_t jj = 0; jj < slot->rom_count; jj++) {
                const onerom_rom_info_t *rom = slot->roms[jj];
                DEBUG("  Chip #%d: %s, %s", jj, rom->filename ? rom->filename : "<unknown>", rom->rom_type);
            }
#endif // DEBUG_LOGGING
        }
    }
}
#endif // BOOT_LOGGING

#if REAL_HARDWARE
void __attribute__((noinline)) do_log_v(const char* msg, va_list* args) {
    if (BOOT_LOGGING_EN) {
        SEGGER_RTT_vprintf(0, msg, args);
        SEGGER_RTT_printf(0, "\n");
    }
}

void do_err_log_prefix() {
    if (BOOT_LOGGING_EN) {
        SEGGER_RTT_printf(0, "ERROR: ");
    }
}

#if defined(DEBUG_LOGGING)
void do_debug_log_prefix() {
    if (BOOT_LOGGING_EN) {
        SEGGER_RTT_printf(0, "DBG: ");
    }
}
#endif // DEBUG_LOGGING

#if defined(BOOT_LOGGING)
void __attribute__((noinline)) do_log(const char* msg, ...) {
    if (BOOT_LOGGING_EN && !TURBO) {
        va_list args;
        va_start(args, msg);
        do_log_v(msg, &args);
        va_end(args);
    }
}

void __attribute__((noinline)) err_log(const char* msg, ...) {
    // Do error logging even if turbo booting
    if (BOOT_LOGGING_EN) {
        do_err_log_prefix();
        va_list args;
        va_start(args, msg);
        do_log_v(msg, &args);
        va_end(args);
    }
}
#endif // BOOT_LOGGING
#endif // REAL_HARDWARE