// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Detects knock sequence on the low order byte of the address bus, then
// awaits a mode command byte and reprograms the kernal ROM accordingly.
//
// Designed to be controlled using the provided modifykernal.prg, which should
// be loaded then run on a real C64, with a One ROM serving the kernal with
// this plugin installed.
//
// Works for 2364 ROMs being served by a Fire 24 C/D/E.  (Only tested on C.)

#include "plugin.h"

// Plugin header.  Uses 0.6.9 firmware API functions.
ORA_DEFINE_USER_PLUGIN(
    c64_kernal_mod_main,
    0, 1, 1, 0,
    0, 6, 9
);

// Ring buffer for captured addresses from PIO, written by DMA.  Must be
// aligned to its size, and be a power of 2 in length.  The macro takes care
// of the alignment and size requirements.
#define RING_ENTRIES_LOG2   6   // 2^6 = 64 entries
ORA_RING_BUF_DECLARE_32BIT(ring_buf, RING_ENTRIES_LOG2);

// Knock sequence to detect: ONEROM! in ASCII
#define KNOCK_LEN 7
static const uint32_t knock_seq[KNOCK_LEN] = {
    'O', 'N', 'E', 'R', 'O', 'M', '!'
};

// The data to update in the kernal ROM when the knock sequence is detected.
// This updates the C64 boot banner.
static const uint8_t new_banner[] =
    "     **** POWERED BY ONE ROM **** "
    "\n\r\r"
    "FROM PIERS.ROCKS ";
#define NEW_BANNER_OFFSET    0x475

// Plugin entry point and main routine
void c64_kernal_mod_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    (void)plugin_type;
    (void)entry_args;

    // Get the plugin API functions we need
    ora_set_status_led_fn_t set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    ora_log_fn_t log = ora_lookup_fn(ORA_ID_LOG);
    ora_setup_address_monitor_fn_t setup_address_monitor = ora_lookup_fn(ORA_ID_SETUP_ADDRESS_MONITOR);
    ora_start_address_monitor_fn_t start_address_monitor = ora_lookup_fn(ORA_ID_START_ADDRESS_MONITOR);
    ora_init_knock_fn_t init_knock = ora_lookup_fn(ORA_ID_INIT_KNOCK);
    ora_wait_for_knock_fn_t wait_for_knock = ora_lookup_fn(ORA_ID_WAIT_FOR_KNOCK);
    ora_reprogram_ram_rom_slot_fn_t reprogram_ram_rom_slot = ora_lookup_fn(ORA_ID_REPROGRAM_RAM_ROM_SLOT);

    log("One ROM - C64 Kernal Mod example plugin");

    // Turn status LED off.  It will be turned on when the kernal has been
    // modified.
    set_status_led(0);

    // Initialize the PIO state machines and DMA channel to capture addresses
    // when CS is active.
    if (setup_address_monitor(ring_buf, RING_ENTRIES_LOG2, ORA_MONITOR_MODE_CONTROL, 32, NULL) != ORA_RESULT_OK) {
        log("Failed to set up address monitor");
        return;
    }

    // Set up the knock sequence detection.  Done before time because detecting
    // the knock will happen at line speed.
    ORA_KNOCK_DECLARE(knock, KNOCK_LEN);
    if (init_knock(knock_seq, KNOCK_LEN, 8, 32, knock) != ORA_RESULT_OK) {
        log("Failed to initialise knock sequence");
        return;
    }

    // Start the address monitor PIO state machines.
    start_address_monitor();

    // Wait for the knock and a single byte afterwards (which we ignore)
    uint32_t post_byte;
    if (wait_for_knock(
        knock,
        ring_buf,
        RING_ENTRIES_LOG2,
        ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS,
        &post_byte,
        1,
        NULL,
        NULL
    ) != ORA_RESULT_OK) {
        log("Failed waiting for knock sequence");
        return;
    }
    (void)post_byte;    // Suppress unused variable warning

    // Reprogram the specified segment of the ROM image in SRAM with the
    // updated data.  This will affect the live ROM image immediately.
    // -1 as we don't want to include the null terminator in the ROM.
    if (reprogram_ram_rom_slot(
        0,
        NEW_BANNER_OFFSET,
        new_banner,
        sizeof(new_banner)-1,
        1
    ) != ORA_RESULT_OK) {
        log("Failed to reprogram kernal");
        return;
    }

    log("Reprogramming complete - soft reset (SYS64738) or power cycle to boot updated kernal");
    set_status_led(1);
}
