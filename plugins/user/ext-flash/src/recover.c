// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include <stdint.h>

#include <plugin.h>

#include "recover.h"

void watchdog_setup(uint32_t clkref_mhz) {
    // One tick per microsecond.  The datasheet requires the generator be
    // stopped before its cycle count changes, so clear ENABLE first in case
    // something has started it.
    TICKS_WD_CTRL &= ~TICKS_CTRL_ENABLE;
    TICKS_WD_CYCLES = clkref_mhz;
    TICKS_WD_CTRL |= TICKS_CTRL_ENABLE;

    // Everything bar the oscillators.  Leaving the watchdog to trigger a
    // chip-level reset instead would clear the scratch registers, and the
    // marker in them is the whole reason for arming it.
    PSM_WDSEL = PSM_WDSEL_ALL & ~(PSM_WDSEL_ROSC | PSM_WDSEL_XOSC);

    WD_CTRL &= ~WD_CTRL_ENABLE;
}

void watchdog_arm(uint32_t ms) {
    WD_CTRL &= ~WD_CTRL_ENABLE;
    WD_SCRATCH(WD_SCRATCH_MAGIC) = MARK_MAGIC;
    mark(MARK_ENTERED);
    WD_LOAD = ms * 1000u;
    WD_CTRL |= WD_CTRL_ENABLE;
}

void watchdog_disarm(void) {
    WD_CTRL &= ~WD_CTRL_ENABLE;
    mark(MARK_IDLE);
    WD_SCRATCH(WD_SCRATCH_MAGIC) = 0u;
}

mark_t watchdog_take_mark(void) {
    if (WD_SCRATCH(WD_SCRATCH_MAGIC) != MARK_MAGIC) {
        return MARK_IDLE;
    }
    mark_t step = (mark_t)WD_SCRATCH(WD_SCRATCH_MARK);
    WD_SCRATCH(WD_SCRATCH_MAGIC) = 0u;
    WD_SCRATCH(WD_SCRATCH_MARK) = 0u;
    return step;
}
