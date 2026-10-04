// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#pragma once
#include <stdint.h>
#include <epio.h>

void *ffi_runtime_info_ptr(void);
uint32_t ffi_runtime_info_size(void);
void *ffi_system_plugin_context(void);
void *ffi_user_plugin_context(void);
uint8_t ffi_limp_mode(void);
uint8_t ffi_pios_enabled(void);
uint8_t ffi_image_sel(void);
uint8_t ffi_board_size(void);

// Plugin launch is compiled out of a test build, so a test sets
// FIRMWARE_STATE_PLUGINS_STARTED itself.
uint32_t ffi_firmware_states(void);
void ffi_set_plugins_started(void);

// The generation recorded in the metadata header, and a way to change it.
//
// On a device the two differ routinely - see firmware/src/utils.c.  A host
// build compiles both from one tree and only ever sees them agree, so these
// let a test put a different generation in front of the firmware before it
// boots.
uint32_t ffi_metadata_generation(void);
void ffi_set_metadata_generation(uint32_t generation);

// A ROM slot's address in a device's flash, a way to move it, its size, and
// whether it lies within the flash at the chip sizes OTP configures.
//
// A host build's slot data is a host pointer, so the firmware reads a slot's
// flash address from a table in gen-config.c - see test/stub_rp235x_inlines.h.
// A test that moves a slot plays the CLI's part, which writes a device's
// metadata.  The address outlives a boot, so the test puts it back.  `index`
// is the firmware's ROM slot index, plugin slots included.
uint32_t ffi_rom_slot_flash_addr(uint8_t index);
void ffi_set_rom_slot_flash_addr(uint8_t index, uint32_t addr);
uint32_t ffi_rom_slot_size(uint8_t index);
uint8_t ffi_rom_slot_in_flash(uint8_t index);

// The firmware's check of the plugin in ROM slot `index`, as boot and launch
// run it.  The test configs don't have plugin slots, so a test runs the check
// on a ROM slot it has moved.
uint8_t ffi_check_plugin_valid(
    uint8_t index,
    ora_plugin_type_t expected_type,
    uint8_t plugin_index
);

// ROM slots 0 and 1 as a system plugin and a user plugin with their headers in
// host memory, and the metadata's own slots put back.
//
// The plugin slots read their flash addresses from the metadata's table so the
// metadata must have at least two slots.
void ffi_install_plugin_slots(void);
void ffi_restore_rom_slots(void);

// Write a valid header to plugin slot `index` with its entry point at device
// address `entry`.
void ffi_set_plugin_header(
    uint8_t index,
    uint32_t entry,
    uint8_t overrides1,
    uint8_t properties1
);

// The serving algorithms and address window the current ROM slot is running.
//
// A test needs the address state machine's sampled pin window to know whether a
// chip select transition changes the SRAM index (window covers it) or only
// gates the data output drivers (it does not) — the two have very different
// CS-to-valid-data costs.  Reported from the live slot config rather than
// re-derived host side, so it cannot drift from what the firmware is running.
typedef struct ffi_serving_alg_t {
    uint8_t addr_alg;       // onerom_alg_addr_t
    uint8_t cs_alg;         // onerom_alg_cs_t
    uint8_t data_alg;       // onerom_alg_data_t
    uint8_t addr_window_base;   // first GPIO the address SM samples
    uint8_t addr_window_pins;   // how many GPIOs it samples
} ffi_serving_alg_t;

// Returns 1 and fills `out` when a ROM slot is being served, 0 otherwise.
uint8_t ffi_serving_alg(ffi_serving_alg_t *out);
void ffi_epio_setup_sram(epio_t *epio);
void ffi_epio_setup_dma_chain(epio_t *epio, uint8_t word_size);
void ffi_epio_arm_monitor(epio_t *epio);
void ffi_led_frame(void);
uint32_t ffi_led_last_pixel(uint32_t *count_out);
uint8_t ffi_led_next_deadline(uint32_t *ms_out);
void ffi_led_reset(void);
void ffi_set_logging(uint8_t enabled);