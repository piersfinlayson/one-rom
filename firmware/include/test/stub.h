// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Test stub header file

#if !defined(TEST_STUB_H)
#define TEST_STUB_H

#include "stdio.h"
#include "assert.h"
#include "stdlib.h"
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

// Stands in for TIMER0's free-running microsecond counter, which
// ora_get_plugin_uptime_ms() reads on a device.  There is no TIMER0 in this process
// and its address is not mapped, so the harness owns the count instead - which
// also lets a test put the clock exactly where it wants it, including either
// side of the 49.7 day wrap, rather than waiting for wall time to get there.
//
// The two halves are read separately, as they are on a device.  Each read
// consumes one step of a scripted sequence of counter values, so a test can
// make the counter move between the firmware's two reads of the high half and
// exercise the retry that assembles a consistent pair.  Once the script runs
// out the last value stands, so a single-valued script simply holds still.
uint32_t stub_timer_raw_hi(void);
uint32_t stub_timer_raw_lo(void);

// Park the counter at a single value, which every subsequent half-read sees.
void stub_set_timer_us(uint64_t us);
void stub_advance_timer_us(uint64_t delta_us);

// Script the counter's value across successive half-reads, one entry consumed
// per read.  count must be between 1 and STUB_TIMER_SCRIPT_MAX.
#define STUB_TIMER_SCRIPT_MAX 16u
void stub_set_timer_raw_script(const uint64_t *values, uint32_t count);

#endif // TEST_STUB_H