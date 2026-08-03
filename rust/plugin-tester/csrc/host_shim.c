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

#include <plugin.h>

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

// Stands in for the position-independent erase routine the plugin copies into
// a RAM slot and calls.  Deliberately small, so a staging slot can be sized
// either side of NV_STORAGE_SIZE + this, exercising both branches of the
// too-small-slot check in nv_poke_begin_impl.  Never executed: the commit path
// is out of scope until the flash seams land (phase 6).
uint8_t __flash_erase_fn_start[256];
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
