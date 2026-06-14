// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Main header file

#ifndef SDRR_INCLUDE_H
#define SDRR_INCLUDE_H

#include <stdint.h>
#include <string.h>
#if !defined(TEST_BUILD)
#include "SEGGER_RTT.h"
#else // TEST_BUILD
#include "test/SEGGER_RTT.h"
#endif // !TEST_BUILD

// If you are not using sdrr-gen, you must define configuration options
// manually.  Exmaples are given here:
//
// #define MCU_FLASH_SIZE 65536
#define BOOT_LOGGING 1  // Enable boot logging.  Enabled/disabled via metadata
// #define DEBUG_LOGGING 1  // Enable more verbose logging
// #define OVERCLOCK 1  // Enable overclocking (may damage the part)
// #define MAIN_LOOP_LOGGING 1  // Enable main loop logging
// #define DEBUG_BUILD 1    // Enable debug checks.  If BOOT_LOGGING is
//                          // defined, DEBUG_LOGGING is also enabled.
// #define PLUGIN_LOGGING 1 // Enable logging from plugins.  Separate from
//                          // BOOT_LOGGING.
//
// sdrr-gen also provides the rom images:
//
// #define SDRR_NUM_IMAGES 1
// const uint8_t sdrr_rom_data[SDRR_NUM_IMAGES][ROM_IMAGE_SIZE] = { ... };

// Base configuration header file
#include "onerom_metadata.h"

// Include the standard SDRR header files 
#include "types.h"
#include "constants.h"
#include "registers.h"
#include "macros.h"
#include "plugin.h"
#include "functions.h"

extern const onerom_info_t onerom_info;

//
// Definition consistency checking
//
#if defined(DEBUG_BUILD) && defined(BOOT_LOGGING)
#if !defined(DEBUG_LOGGING)
#define DEBUG_LOGGING 1
#endif // !DEBUG_LOGGING
#endif // DEBUG_BUILD && BOOT_LOGGING

#if defined(MAIN_LOOP_LOGGING) && !defined(BOOT_LOGGING)
#error "MAIN_LOOP_LOGGING requires BOOT_LOGGING to be defined"
#endif // MAIN_LOOP_LOGGING/BOOT_LOGGING

#if defined(DEBUG_LOGGING) && !defined(BOOT_LOGGING)
#error "DEBUG_LOGGING requires BOOT_LOGGING to be defined"
#endif // DEBUG_LOGGING/BOOT_LOGGING

// Struct used to hold the runtime information for One ROM
extern onerom_runtime_info_t onerom_runtime_info;

// Linker variables, used by log_init()
extern uint32_t _flash_start;
extern uint32_t _flash_end;
extern uint32_t _ram_size;

#if !defined(TEST_BUILD)
#include "rp235x_inlines.h"
#else // TEST_BUILD
#include "test/stub_rp235x_inlines.h"
#endif // !TEST_BUILD

// Target frequency
#define TARGET_FREQ_MHZ    150

// PLL configuration
//   CLK_REF=12MHz
//   VCO_input=12MHz
//   fVCO=900MHz
//   SYSCLK=150MHz
//
// This is only used if the firmware failed to calculate PLL setting itself.
#define PLL_SYS_REFDIV    1
#define PLL_SYS_FBDIV     75
#define PLL_SYS_POSTDIV1  6
#define PLL_SYS_POSTDIV2  1

#endif // SDRR_INCLUDE_H

