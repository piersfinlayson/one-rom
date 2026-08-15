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
    LOG("%s", log_divider);
    LOG("%s v%d.%d.%d.%d %s", product, INFO->major_version, INFO->minor_version, INFO->patch_version, INFO->build_number, project_url);
    LOG("%s %s", copyright, author);
#if defined(DEBUG_BUILD)
    LOG("Built: %s (DEBUG)", INFO->build_date);
#else // !DEBUG_BUILD
    LOG("Built: %s", INFO->build_date);
#endif // DEBUG_BUILD
    LOG("Commit: %s", INFO->commit);

    DEBUG("onerom_info: 0x%08lX", (unsigned long)(uintptr_t)INFO);
    DEBUG("RAM ROM table: 0x%08lX", (unsigned long)(uintptr_t)&_ram_rom_image_start);
    DEBUG("runtime_info: 0x%08lX", (unsigned long)(uintptr_t)RUNTIME);
    DEBUG("RTT CB: 0x%08lX", (unsigned long)(uintptr_t)INFO->rtt);
    DEBUG("%s", log_divider);
    DEBUG("RT Fire Freq: 0x%04X", RUNTIME->fire_freq);
    DEBUG("RT Overclock Enabled: 0x%02X", RUNTIME->overclock_enabled);
    DEBUG("RT Status LED Enabled: 0x%02X", RUNTIME->status_led_enabled);
    DEBUG("RT SWD Enabled: 0x%02X", RUNTIME->swd_enabled);

    LOG("%s", log_divider);
    platform_logging();

    LOG("%s", log_divider);
}

void log_roms() {
    LOG("# of ROM sets: %d", METADATA->rom_slot_count);

    for (uint8_t ii = 0; ii < METADATA->rom_slot_count; ii++) {
        const onerom_rom_slot_t *slot = &METADATA->rom_slots[ii];

        LOG("Set #%d: %d ROM(s), size: %lu bytes", ii, slot->rom_count, (unsigned long)slot->size);

#if defined(DEBUG_LOGGING)
        for (uint8_t jj = 0; jj < slot->rom_count; jj++) {
            const onerom_rom_info_t *rom = slot->roms[jj];
            DEBUG("  Chip #%d: %s, %s", jj, rom->filename ? rom->filename : "<unknown>", rom->rom_type);
        }
#endif // DEBUG_LOGGING
    }
}
#endif // BOOT_LOGGING

#if REAL_HARDWARE
// These write the boot channel whenever they are called.  Whether boot logging
// is enabled is decided by the caller: LOG() and DEBUG() test it, ERR() and the
// plugin logging calls do not.
//
// A line ends CRLF because the log reaches a serial terminal over USB CDC, and
// a terminal in raw mode does no translation of its own - a bare LF there drops
// a row without returning to column 0, so each line starts where the last one
// ended.  A debug probe is unaffected: it writes to a cooked tty, which supplies
// the CR itself, and the device's own CR arrives as a duplicate move to column
// 0, which changes nothing.  That is also why the test stub in
// firmware/test/stub_rp235x.c ends its lines with a bare LF - it writes to
// stdout, which does the translation for it.
//
// The USB plugin's banner ends its lines the same way, and has to be changed
// with this - see LOG_BANNER_LINE_END in plugins/system/usb/src/usb_log.c.
void __attribute__((noinline)) do_log_v(const char* msg, va_list* args) {
    onerom_rtt_vprintf(ONEROM_RTT_CH_BOOT, msg, args);
    onerom_rtt_printf(ONEROM_RTT_CH_BOOT, "\r\n");
}

void do_err_log_prefix() {
    onerom_rtt_printf(ONEROM_RTT_CH_BOOT, "ERROR: ");
}

#if defined(DEBUG_LOGGING)
void do_debug_log_prefix() {
    onerom_rtt_printf(ONEROM_RTT_CH_BOOT, "DBG: ");
}
#endif // DEBUG_LOGGING

#if defined(BOOT_LOGGING)
void __attribute__((noinline)) do_log(const char* msg, ...) {
    // Turbo boot skips boot logging, to reach serving as quickly as possible.
    if (!TURBO) {
        va_list args;
        va_start(args, msg);
        do_log_v(msg, &args);
        va_end(args);
    }
}

void __attribute__((noinline)) err_log(const char* msg, ...) {
    // Do error logging even if turbo booting
    do_err_log_prefix();
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
}
#endif // BOOT_LOGGING
#endif // REAL_HARDWARE