// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Test stub header file

#if !defined(TEST_STUB_H)
#define TEST_STUB_H

#include "stdio.h"
#include "assert.h"
#include "stdlib.h"
#include "test/SEGGER_RTT.h"
#include "types.h"

#define STUB_LOG stub_log
#define STUB_ASSERT(X, ...) STUB_LOG(__VA_ARGS__); assert(X)
#define STUB_EXIT(X)        STUB_LOG("Exiting with code %d", X); exit(X)

extern limp_mode_pattern_t limp_mode_value;

#define _ram_rom_image_start test_ram_rom_image_table

#define RAM_ROM_TABLE_SIZE (512 * 1024)

void stub_log(const char* msg, ...);

// Variadic-forwarding forms, for callers that are themselves variadic and so
// have a va_list rather than a pack to pass on.  Calling stub_log(msg) from
// such a caller drops its arguments, leaving vprintf to read whatever happens
// to be on the stack — every value in the log line is then meaningless.
void stub_log_v(const char* msg, va_list args);
void stub_log_prefix_v(const char* prefix, const char* msg, va_list args);
uint64_t *get_ram_rom_image_table_aligned(void);
uint8_t stub_set_sel_image(uint8_t image_index);
void stub_set_rp_variant(uint8_t is_b);

#endif // TEST_STUB_H