// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM globals

#include "include.h"

#if defined(REAL_HARDWARE)
// Linker variable containing location of metadata header
extern char _metadata_start;
#else // !REAL_HARDWARE
#include "gen-config.c"
#endif // REAL_HARDWARE

// Pointer to the SEGGER RTT CB
extern SEGGER_RTT_CB _SEGGER_RTT;

// Build time/date string
const char onerom_build_date[] = __DATE__ " " __TIME__;

// Main One ROM runtime info structure, located in RAM and updated at
// runtime.  Pointed to by onerom_info, which is located at a known point in
// flash.
_Static_assert(sizeof(RUNTIME_INFO_MAGIC) == 5, "RUNTIME_INFO_MAGIC must be 4 bytes + NULL TERMINATOR");
#if REAL_HARDWARE
#define SECTION_ONEROM_RUNTIME_INFO __attribute__((section(".onerom_runtime_info")))
#else // !REAL_HARDWARE
#define SECTION_ONEROM_RUNTIME_INFO
#endif // !REAL_HARDWARE
onerom_runtime_info_t onerom_runtime_info SECTION_ONEROM_RUNTIME_INFO = {
    .magic = { RUNTIME_INFO_MAGIC[0], RUNTIME_INFO_MAGIC[1], RUNTIME_INFO_MAGIC[2], RUNTIME_INFO_MAGIC[3] },
    .version = RUNTIME_INFO_VERSION,
    .runtime_info_size = sizeof(onerom_runtime_info_t),
    .rp235x = RP235XA,  // Updated later based on querying hardware
    .image_sel = 0xFF,
    .rom_slot_index = 0xFF,
    .rom_table = NULL,
    .rom_table_size = 0,
    .overclock_enabled = 0,
    .status_led_enabled = 0,
    .swd_enabled = 0,
    .fire_vreg = FIRE_VREG_STOCK,
    .fire_freq = FIRE_FREQ_NONE,
    .sysclk_mhz = TARGET_FREQ_MHZ,
    .timer0_irq_0_handler = NULL,
    .usbctrl_irq_handler = NULL,
    .limp_mode = LIMP_MODE_NONE,
    .peri_en = 0,
    .bit_mode = BIT_MODE_8,
    .boot_logging = 0,
    .system_plugin_context = NULL,
    .user_plugin_context = NULL,
    .current_rom_slot = NULL,
    .addr_pio_block_info = 0,
    .addr_pio_sm_info = 0,
    .cs_data_pio_block_info = 0,
    .cs_data_pio_sm_info = 0,
    .dma_pio_ch = 0,
    .current_ram_slot = 0xFF
};

// Main One ROM build info structure, located at known point in flash
_Static_assert(sizeof(ONEROM_INFO_MAGIC) == 5, "ONEROM_INFO_MAGIC must be 4 bytes + NULL TERMINATOR");
#if REAL_HARDWARE
__attribute__((section(".onerom_info")))
#endif // !REAL_HARDWARE
const onerom_info_t onerom_info = {
    .magic = { ONEROM_INFO_MAGIC[0], ONEROM_INFO_MAGIC[1], ONEROM_INFO_MAGIC[2], ONEROM_INFO_MAGIC[3] },
    .major_version = ONEROM_VERSION_MAJOR,
    .minor_version = ONEROM_VERSION_MINOR,
    .patch_version = ONEROM_VERSION_PATCH,
    .build_number = ONEROM_BUILD_NUMBER,
    .build_date = onerom_build_date,
    .commit = ONEROM_GIT_COMMIT,
    .version = ONEROM_INFO_VERSION,
    .metadata = (const struct onerom_metadata_header_t *)&_metadata_start,
    .rtt = &_SEGGER_RTT,
    .runtime = &onerom_runtime_info,
    .reserved = {
        0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff
    },
};

