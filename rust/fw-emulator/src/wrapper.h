// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

/*
 * src/wrapper.h — bindgen entry point
 *
 * Pulls in every header that declares the functions listed in build.rs.
 * Include paths are supplied by build.rs via -I clang_args:
 *
 *   $c_root/src/
 *   $c_root/include/
 *   $c_root/test/       <- for ffi.h
 */

// Include main firmwware header - note TEST_BUILD must be defined by build
// arguments
#include "include.h"

/* epio_from_apio, epio_drive_gpios_ext, epio_read_pin_states,
   epio_step_cycles, epio_free — and the Epio / Apio types.      */
#include "epio.h"

/* stub_set_sel_image */
#include "stub.h"

/* ffi_limp_mode, ffi_pios_enabled, ffi_image_sel,
   ffi_epio_setup_sram, ffi_epio_setup_dma_chain */
#include "ffi.h"

/*
 * firmware_main is not in a dedicated header.
 * Forward-declare it here
 */
int firmware_main(void);

/*
 * Forward-declare ora_fn_lookup it here
 */
void *ora_fn_lookup(api_id_t id);