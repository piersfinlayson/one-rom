// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM macros

#if !defined(MACROS_H)
#define MACROS_H

// Whether boot logging emits, tested at runtime.  Set from the metadata's
// boot_logging flag during startup - see update_runtime_from_metadata().
#if !defined(BOOT_LOGGING)
#define BOOT_LOGGING_EN 0
#elif defined(TEST_BUILD)
// Under emulation boot logging goes to the harness's stdout, and the harness
// owns the switch (see include.h).  The metadata flag is not the emulator's to
// set, so honouring it here would make the firmware's own logging unreachable
// from a test.
#define BOOT_LOGGING_EN logging_enabled
#else // BOOT_LOGGING && !TEST_BUILD
#define BOOT_LOGGING_EN RUNTIME->boot_logging
#endif // !BOOT_LOGGING

// Logging macros
//
// LOG() and DEBUG() are boot logging, so both emit only when boot logging is
// enabled.  The runtime test belongs here rather than inside do_log(), so that
// a disabled boot log does not evaluate its arguments.
//
// ERR() carries no runtime test.  An error reaches the boot channel whatever
// the boot logging setting, because whoever hits one is the least likely to
// have turned logging on beforehand.
#if defined(BOOT_LOGGING)
#define LOG_INIT()   log_init()
#define LOG(X, ...)  do { if (BOOT_LOGGING_EN) { do_log(X, ##__VA_ARGS__); } } while (0)
#define ERR(X, ...)  err_log(X, ##__VA_ARGS__)
#else // BOOT_LOGGING
#define LOG_INIT()
#define LOG(X, ...)
#define ERR(X, ...)
#endif // BOOT_LOGGING
#if defined(DEBUG_LOGGING)
#define DEBUG(X, ...)  do { if (BOOT_LOGGING_EN) { do_log(X, ##__VA_ARGS__); } } while (0)
#else // DEBUG_LOGGING
#define DEBUG(X, ...)
#endif // DEBUG_LOGGING

#define INFO        (&onerom_info)
#define RUNTIME     (&onerom_runtime_info)

// Use of these macros requires metadata_present() to have been caled and
// returned true.
#define METADATA        INFO->metadata
#define HW              METADATA->hw
#define FW              METADATA->fw
#define ROM_SLOTS       METADATA->rom_slots
#define TURBO           METADATA->turbo_boot

// Macro to retreive maximum number of GPIOs on this board
#define MAX_GPIOS   max_gpios[RUNTIME->rp235x]

#define CURRENT_SLOT    RUNTIME->current_rom_slot

// Macro to retrieve number of address pins
#define NUM_ADDR_PINS   (CURRENT_SLOT->alg->alg_addr->num_addr_pins)

// Macro to retrieve base address pin
#define BASE_ADDR_PIN   (CURRENT_SLOT->alg->alg_addr->base_addr_pin)

// Macro to retrieve number of data pins
#define NUM_DATA_PINS   (CURRENT_SLOT->alg->alg_data->num_data_pins)

// Macro to retrieve base data pin
#define BASE_DATA_PIN   (CURRENT_SLOT->alg->alg_data->base_data_pin)

// Get the current ROM slot's algorithm's DMA bit mode (8 or 16)
#define BIT_MODE    (CURRENT_SLOT->alg->alg_dma->bit_mode)

// Macro to test if building for real hardware.  When building an emulated
// version, the test enivronment typically provides stub functions to replace
// those which interact with real hardware.
#if defined(TEST_BUILD)
#define REAL_HARDWARE    0
#else // !TEST_BUILD
#define REAL_HARDWARE    1
#endif // TEST_BUILD

#if !defined(STATIC_ASSERT)
#if !defined(TEST_BUILD) && !defined(__INTELLISENSE__)
#include <assert.h>
#define STATIC_ASSERT(X, MSG)   static_assert(X, MSG)
#else // TEST_BUILD
#define STATIC_ASSERT(X, MSG)
#endif // !TEST_BUILD
#endif // !STATIC_ASSERT

#define STORE_PIO_BLOCK_INFO(block) ((block) << 6)
#define GET_PIO_BLOCK_INFO(info) (((info) >> 6) & 0x03)
#define STORE_PIO_BLOCK_INSTR_LEN(len) ((len) & 0x3F)
#define GET_PIO_BLOCK_INSTR_LEN(info) ((info) & 0x3F)
#define STORE_PIO_SM_INFO(sm) (1 << (sm))
#define CHECK_PIO_SM_INFO(info, sm) ((info) & (1 << (sm)))
#define STORE_PIO_IRQ_INFO(irq) (1 << (irq+4))
#define CHECK_PIO_IRQ_INFO(info, irq) ((info) & (1 << (irq+4)))
#define STORE_DMA_CH_INFO(ch) (1 << (ch))
#define CHECK_DMA_CH_INFO(info, ch) ((info) & (1 << (ch)))

#endif // MACROS_H