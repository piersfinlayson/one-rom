// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// Getting the board back, and finding out where it went.
//
// A flash operation runs with XIP down, this core's interrupts masked and the
// other core parked by exclusive mode.  A fault in there takes the core while
// it holds that parking, so the other core stays parked, USB stops answering
// and the board needs the BOOTSEL button.  That happened twice.
//
// Two things fix it.  The watchdog resets the board when a section does not
// finish, so it comes back by itself.  And a step marker written to a watchdog
// scratch register survives that reset, so the next boot names the call that
// never returned.

#if !defined(RECOVER_H)
#define RECOVER_H

#include <stdint.h>

// Watchdog.  RP2350 datasheet section 12.9.7 "List of registers", control bits
// in Table 1248.  SCRATCHn is at 0x0c + 4n.
#define WATCHDOG_BASE       0x400D8000u
#define WD_CTRL             (*(volatile uint32_t *)(WATCHDOG_BASE + 0x00u))
#define WD_LOAD             (*(volatile uint32_t *)(WATCHDOG_BASE + 0x04u))
#define WD_REASON           (*(volatile uint32_t *)(WATCHDOG_BASE + 0x08u))
#define WD_SCRATCH(n)       (*(volatile uint32_t *)(WATCHDOG_BASE + 0x0Cu + 4u * (n)))
#define WD_CTRL_TRIGGER     (1u << 31)
#define WD_CTRL_ENABLE      (1u << 30)

// Which scratch registers are free.  RP2350 datasheet section 5.4.8.20 puts the
// bootrom's reboot parameters in 2 and 3, and section 5.2.4 its watchdog-boot
// magic in 4 and entry point in 5.  That leaves 0, 1, 6 and 7.
#define WD_SCRATCH_MARK     0u
#define WD_SCRATCH_MAGIC    1u

// Marks the scratch value as this plugin's, rather than whatever the register
// held out of reset.
#define MARK_MAGIC          0x45585446u  // "EXTF"

// The tick the watchdog counts.  RP2350 datasheet section 12.9.2: this comes
// from the system ticks block, where RP2040 generated it inside the watchdog.
// The firmware starts the timer's tick alone, so the plugin starts this one.
#define TICKS_BASE          0x40108000u
#define TICKS_WD_CTRL       (*(volatile uint32_t *)(TICKS_BASE + 0x30u))
#define TICKS_WD_CYCLES     (*(volatile uint32_t *)(TICKS_BASE + 0x34u))
#define TICKS_CTRL_ENABLE   (1u << 0)

// Which components a watchdog timeout resets.  RP2350 datasheet section 12.9.4
// gives three levels, and section 12.9.5 says a chip-level reset clears the
// scratch registers, taking the marker with it.  So this uses the PSM level,
// bit list from Table 532, set to everything bar the two oscillators.  That
// matches the SDK's watchdog_enable.
#define PSM_BASE            0x40018000u
#define PSM_WDSEL           (*(volatile uint32_t *)(PSM_BASE + 0x08u))
#define PSM_WDSEL_ALL       0x01FFFFFFu
#define PSM_WDSEL_ROSC      (1u << 2)
#define PSM_WDSEL_XOSC      (1u << 3)

// Where a flash operation had got to.  Written before each step, so the value
// left behind names the call that did not return.
typedef enum {
    MARK_IDLE          = 0u,
    MARK_ENTERED       = 1u,  // inside the critical section, before the first call
    MARK_CONNECTED     = 2u,  // connect_internal_flash returned
    MARK_XIP_EXITED    = 3u,  // flash_exit_xip returned
    MARK_OP_DONE       = 4u,  // the erase or program returned
    MARK_CACHE_FLUSHED = 5u,  // flash_flush_cache returned
    MARK_XIP_RESTORED  = 6u,  // flash_select_xip_read_mode returned
    MARK_CANARY_CALL   = 7u,  // branched into the external flash
} mark_t;

// As mark(), plus the page a multi-page write had reached, which pins a hang to
// a page rather than the whole operation.
static inline void mark_page(mark_t step, uint32_t page) {
    WD_SCRATCH(WD_SCRATCH_MARK) = (uint32_t)step | (page << 8);
}

// Record progress.  Safe with XIP down: it is a register write built from
// macros in this header, so it fetches nothing from flash.
static inline void mark(mark_t step) {
    WD_SCRATCH(WD_SCRATCH_MARK) = (uint32_t)step;
}

// Start the watchdog's tick generator, and choose what a timeout resets.
//
// clkref_mhz sizes the divider so one tick is one microsecond, the same basis
// the firmware uses for the timer.
void watchdog_setup(uint32_t clkref_mhz);

// Arm for this many milliseconds, and note that the marker is ours.
//
// An erase can take the device 400ms, so the timeout has to clear that by
// enough for a slow but healthy erase to finish.
void watchdog_arm(uint32_t ms);

// Stop the countdown.  Called once a section finishes.
void watchdog_disarm(void);

// The marker left by a run that did not finish, or MARK_IDLE if the last run
// completed or nothing has run yet.  Clears it, so it is only reported once.
mark_t watchdog_take_mark(void);

#endif // RECOVER_H
