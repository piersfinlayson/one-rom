// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 Shared PIO routines

#include "include.h"

#if defined(RP235X)

#if defined(TEST_BUILD)
#define TEST_PIO_C
#else
#define APIO_LOG_IMPL  1
#endif // TEST_BUILD

#include "piodma/piodma.h"

int pio(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
) {
    int rc;
    if (set->roms[0]->rom_type == CHIP_TYPE_6116 || set->roms[0]->rom_type == CHIP_TYPE_62256) {
        DEBUG("PIO RAM Mode");
        rc = pioram(info, runtime, set, rom_table_addr);
    } else {
        DEBUG("PIO ROM Mode");
        rc = piorom(info, runtime, set, rom_table_addr);
    }

    return rc;
}

#endif // RP235X