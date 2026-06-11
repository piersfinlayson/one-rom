// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 Shared PIO routines

#include "include.h"

#if defined(TEST_BUILD)
#define TEST_PIO_C
#else
#define APIO_LOG_IMPL  1
#endif // TEST_BUILD

#include "piodma/piodma.h"

int pio(void) {
    int rc;

    if (0) {
        DEBUG("PIO RAM Mode");
        uint32_t rom_table_addr = (uint32_t)(uintptr_t)RUNTIME->rom_table;
        rc = pioram(INFO, RUNTIME, rom_table_addr);
    } else {
        DEBUG("PIO ROM Mode");
        rc = piorom2();
    }

    return rc;
}

