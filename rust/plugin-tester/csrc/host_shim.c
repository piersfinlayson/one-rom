// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Host-side shim standing in for the parts of a plugin's device environment
// that do not exist when the plugin is compiled natively and run against the
// firmware emulator.
//
// Three kinds of thing live here:
//
// 1. The symbols the plugin's linker script would otherwise define.  On device
//    __nv_storage_start points at a reserved flash sector and the
//    __flash_erase_fn_* pair brackets a position-independent blob in the
//    plugin's .text; here they are ordinary host objects.
//
// 2. The emulation seams the ORA host-test macros route through.
//
// 3. The entry point the harness calls to start the plugin.
//
// This file is specific to one plugin (host-control): it names rbcp_main, and
// two plugins cannot share a binary in any case — each defines its own
// ora_plugin_header and its own file-scope state.

#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <plugin.h>

#include "flash_erase.h"
#include "host_shim.h"

// ---------------------------------------------------------------------------
// Firmware entry points we link against (from libonerom-test.a)
// ---------------------------------------------------------------------------

// firmware/src/plugin.c — the plugin API lookup the firmware hands a plugin.
void *ora_fn_lookup(api_id_t id);

// firmware/src/piodma/pioplugin.c — translates a device SRAM address into a
// pointer valid in this process.  Only meaningful once the emulator has called
// set_host_sram_ptr (i.e. after setup_epio).
uint8_t *sram_to_host(uint32_t addr);

// ---------------------------------------------------------------------------
// The plugin under test
// ---------------------------------------------------------------------------

void rbcp_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
);

// The plugin's own header, as the firmware would read it before launching the
// plugin.  Useful to the harness for asserting the plugin's declared version
// and min_fw_version.
extern const ora_plugin_header_t ora_plugin_header;

// ---------------------------------------------------------------------------
// Linker symbols the plugin expects
// ---------------------------------------------------------------------------

// Must match NV_STORAGE_SIZE in the plugin.  Declared const on the plugin's
// side (it reads flash); writable here so the harness can seed it and check
// what a commit wrote.
#define SHIM_NV_STORAGE_SIZE 4096u

uint8_t __nv_storage_start[SHIM_NV_STORAGE_SIZE];

// Where this process pretends its NV region sits within the device's flash.
// Arbitrary, but not zero: a plugin that forgot to add the region's offset
// would then still land on it.  Sector-aligned, as a reserved region is.
#define SHIM_NV_FLASH_OFFSET 0x0003F000u

// Stand-ins for the linker symbols bracketing the position-independent erase
// routine.  Their *addresses* are what the plugin copies from, and their
// difference is meaningless here — these are unrelated objects, and the linker
// may lay them out in either order — which is exactly why the plugin asks
// ORA_STAGED_FN_SIZE rather than subtracting them.
//
// The size is deliberately small, so a staging slot can be sized either side
// of NV_STORAGE_SIZE + this, exercising both branches of the too-small-slot
// check in nv_poke_begin_impl.  The bytes the plugin copies are never
// executed: ORA_STAGED_FN_PTR hands it the real `flash_erase_critical`
// compiled for this host instead.
#define SHIM_ERASE_FN_SIZE 256u

uint8_t __flash_erase_fn_start[SHIM_ERASE_FN_SIZE];
uint8_t __flash_erase_fn_end[1];

// ---------------------------------------------------------------------------
// ORA host-test seams
// ---------------------------------------------------------------------------

// ORA_SRAM_PTR.  On device this is the identity; here the plugin's process is
// not the device's address space, so hand the address to the firmware's own
// translation.  Only valid once the emulator has called set_host_sram_ptr,
// which setup_epio does — before that sram_to_host returns into the firmware's
// own allocation rather than epio's, and the plugin would write somewhere the
// PIO does not serve from.
void *ora_host_test_sram_ptr(uint32_t addr) {
    return sram_to_host(addr);
}

// ORA_TEST_YIELD.  Installed by the harness thread that runs the plugin; the
// plugin calls this from inside a busy-wait, and the hook hands control back
// so the emulation can be advanced.  A NULL hook means nothing is driving the
// emulation, so the wait can never end: say so rather than spinning forever.
static void (*s_yield_hook)(void);

void ora_host_test_set_yield_hook(void (*hook)(void)) {
    s_yield_hook = hook;
}

void ora_host_test_yield(void) {
    if (s_yield_hook == NULL) {
        fprintf(stderr,
                "onerom plugin harness: plugin yielded with no hook installed — "
                "nothing can advance the emulation, so this wait would never "
                "end.  Install the hook on the thread that runs the plugin.\n");
        abort();
    }
    s_yield_hook();
}

// ---------------------------------------------------------------------------
// Device facts, and a flash model to make them mean something
//
// The plugin's NV commit path reaches past the plugin API to the chip it runs
// on: the bootrom's function table, the QMI clock divisor, the base of mapped
// flash, and the Thumb-bit pointer to the erase routine it staged in SRAM.
// Each goes through its own ORA macro (see "Device facts" in ora/api.h), and
// each lands here.
//
// What the stand-ins model is a flash sector: erase sets it to 0xFF, program
// writes it.  That is enough for a scenario to assert what a commit did — and
// enough for one to notice that it erased the wrong range, or programmed
// before erasing.  The arguments of every call are recorded, so a scenario can
// also assert *how* the device asked, not only what came out.
// ---------------------------------------------------------------------------

// Increments on every flash call, so the log records the order they arrived in
// and not merely that they did.
static uint32_t s_seq;

static uint32_t next_seq(void) {
    return ++s_seq;
}

static ora_host_test_flash_log_t s_flash_log;

const ora_host_test_flash_log_t *ora_host_test_flash_log(void) {
    return &s_flash_log;
}

void ora_host_test_reset_flash_log(void) {
    s_flash_log = (ora_host_test_flash_log_t){0};
    // A device is running from XIP when a commit starts; that is what the
    // erase sequence has to take it out of and put it back into.
    s_flash_log.xip_active = 1;
    s_seq = 0;
}

// The XIP clock divisor a device would have configured.  Any non-zero value
// will do; the point is that the plugin reads it before disabling XIP and
// hands that same value back to the routine that restores it.
#define SHIM_XIP_CLKDIV 4u

uint8_t ora_host_test_xip_clkdiv(void) {
    return SHIM_XIP_CLKDIV;
}

uint32_t ora_host_test_flash_offset(const void *addr) {
    // Only the plugin's own NV region has a meaningful offset here.
    if ((const uint8_t *)addr == __nv_storage_start) {
        return SHIM_NV_FLASH_OFFSET;
    }
    fprintf(stderr,
            "onerom plugin harness: ORA_FLASH_OFFSET of a pointer that is not "
            "the NV region — this process models one flash region, not the "
            "device's whole address map.\n");
    abort();
}

uint32_t ora_host_test_staged_fn_size(const void *start, const void *end) {
    // Deliberately ignores both: they are unrelated objects here, and their
    // difference is what this seam exists to avoid computing.
    (void)start;
    (void)end;
    return SHIM_ERASE_FN_SIZE;
}

// --- the bootrom stand-ins ------------------------------------------------

static void shim_connect_internal_flash(void) {
    s_flash_log.connect_calls++;
    s_flash_log.connect_seq = next_seq();
}

static void shim_flash_exit_xip(void) {
    s_flash_log.exit_xip_calls++;
    s_flash_log.exit_xip_seq = next_seq();
    s_flash_log.xip_active = 0;
}

static void shim_flash_range_erase(uint32_t offs, uint32_t count, uint32_t block_size,
                                   uint8_t block_cmd) {
    s_flash_log.erase_calls++;
    s_flash_log.erase_seq = next_seq();
    s_flash_log.erase_offs = offs;
    s_flash_log.erase_count = count;
    s_flash_log.erase_block_size = block_size;
    s_flash_log.erase_block_cmd = block_cmd;
    if (!s_flash_log.xip_active && offs == SHIM_NV_FLASH_OFFSET
        && count <= SHIM_NV_STORAGE_SIZE) {
        memset(__nv_storage_start, 0xFF, count);
    } else {
        s_flash_log.bad_erase = 1;
    }
}

static void shim_flash_flush_cache(void) {
    s_flash_log.flush_calls++;
    s_flash_log.flush_seq = next_seq();
}

static void shim_flash_select_xip_read_mode(uint8_t mode, uint8_t clkdiv) {
    s_flash_log.select_xip_calls++;
    s_flash_log.select_xip_seq = next_seq();
    s_flash_log.select_xip_mode = mode;
    s_flash_log.select_xip_clkdiv = clkdiv;
    s_flash_log.xip_active = 1;
}

static void shim_flash_range_program(uint32_t offs, const uint8_t *data, uint32_t count) {
    s_flash_log.program_calls++;
    s_flash_log.program_seq = next_seq();
    s_flash_log.program_offs = offs;
    s_flash_log.program_count = count;
    if (s_flash_log.xip_active && offs == SHIM_NV_FLASH_OFFSET
        && count <= SHIM_NV_STORAGE_SIZE) {
        // Real flash can only clear bits; a program over unerased storage
        // leaves the AND of the two.  Modelling that rather than a plain copy
        // is what makes an erase the device skipped visible in the result.
        for (uint32_t i = 0; i < count; i++) {
            __nv_storage_start[i] &= data[i];
        }
    } else {
        s_flash_log.bad_program = 1;
    }
}

void *ora_host_test_bootrom_lookup(uint32_t code, uint32_t mask) {
    (void)mask;
    switch (code) {
        case ORA_SHIM_ROM_CODE('I', 'F'): return (void *)shim_connect_internal_flash;
        case ORA_SHIM_ROM_CODE('E', 'X'): return (void *)shim_flash_exit_xip;
        case ORA_SHIM_ROM_CODE('R', 'E'): return (void *)shim_flash_range_erase;
        case ORA_SHIM_ROM_CODE('F', 'C'): return (void *)shim_flash_flush_cache;
        case ORA_SHIM_ROM_CODE('X', 'M'): return (void *)shim_flash_select_xip_read_mode;
        case ORA_SHIM_ROM_CODE('R', 'P'): return (void *)shim_flash_range_program;
        default: return NULL;
    }
}

// The plugin copies `flash_erase_critical` into a RAM slot and calls it there.
// Those bytes are not executable in this process, so hand back the routine
// itself, compiled for this host — which means the sequence under test is the
// plugin's real one, not a re-description of it.
//
// The address is checked rather than ignored: the plugin is required to stage
// the routine immediately above the staging buffer, and a scenario would
// otherwise not notice if it called through a pointer to somewhere else.
void *ora_host_test_staged_fn_ptr(uint32_t addr) {
    s_flash_log.staged_fn_addr = addr;
    return (void *)flash_erase_critical;
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

// Under ORA_HOST_TEST the plugin's ORA_RING_BUF_DECLARE_32BIT declares a
// pointer rather than an array, so the harness can place the ring inside the
// SRAM the emulator serves from — the capture DMA writes there and nowhere
// else.  The name is the plugin's own; this shim is plugin-specific anyway.
extern volatile uint32_t *ring_buf;

void ora_host_test_set_ring_buf(volatile uint32_t *p) {
    ring_buf = p;
}

// ---------------------------------------------------------------------------
// NV storage access, for the harness
// ---------------------------------------------------------------------------

uint8_t *ora_host_test_nv_storage(void) {
    return __nv_storage_start;
}

uint32_t ora_host_test_nv_storage_size(void) {
    return SHIM_NV_STORAGE_SIZE;
}

// ---------------------------------------------------------------------------
// Plugin entry
// ---------------------------------------------------------------------------

// Start the plugin.  Does not return: the plugin's main loop is infinite by
// design, so the harness runs this on a thread it is willing to abandon.
//
// The entry arguments describe the plugin's static RAM and stack on a device.
// This plugin voids both that and its plugin-type argument, and on a host its
// data and stack are the host's, so a zeroed struct is passed rather than an
// invented layout that nothing would honour.
void ora_host_test_run_plugin(void) {
    static const ora_entry_args_t args = {0};
    rbcp_main(ora_fn_lookup, ORA_PLUGIN_TYPE_USER, &args);
}

// Version fields from the plugin's own header, so the harness can report which
// build of the plugin it exercised.
uint32_t ora_host_test_plugin_version(void) {
    return ((uint32_t)ora_plugin_header.major_version << 24)
         | ((uint32_t)ora_plugin_header.minor_version << 16)
         | ((uint32_t)ora_plugin_header.patch_version << 8)
         | (uint32_t)ora_plugin_header.build_version;
}
