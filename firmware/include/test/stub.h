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

// Stands in for the pads and the SIO output registers, which ora_gpio_set
// drives and ora_gpio_query reads back on a device.  Without it a test build
// reports every pin as an input reading low, so a test can assert that a set
// was refused but never what a permitted one did - which is most of what a
// bounded GPIO hold is.
//
// Only the three fields ora_gpio_query reports are modelled: whether the output
// driver is enabled, what it is driving, and what an input reads.  The pin's
// use, its pulls and its function select are not, because nothing reads them
// back through the API.
//
// Cleared by onerom_test_reset() on every boot, so a scenario starts with the
// pads as a device's come up rather than carrying the previous one's state.
void stub_gpio_set(uint8_t gpio, uint8_t state);
uint8_t stub_gpio_is_output(uint8_t gpio);
uint8_t stub_gpio_level(uint8_t gpio);

// What an input pin reads.  Nothing is attached to a pin in this process, so an
// input reads low until a test says otherwise - which is what this is for: a
// pin released to input with a pull-up on the board reads high, and a test that
// cares can say so.  Has no effect while the pin is an output, which reports
// what it drives.
void stub_set_gpio_input(uint8_t gpio, uint8_t level);

// Put the process-global state a device's reset would clear back to cold boot.
//
// The firmware's statics are ordinary host objects in a test build, so nothing
// restores them between the many boots one process runs.  The emulator calls
// this on every boot.  What it covers is firmware state with no counterpart in
// a plugin - see the comment on its definition.
void onerom_test_reset(void);
void stub_gpio_reset(void);

// Give every log channel back.
//
// Called by a harness that also clears its plugin's own state, and only by such
// a harness: a plugin records which channels it holds itself, so releasing the
// firmware's claim without clearing the plugin's leaves the plugin believing it
// holds a channel the firmware says is free.
void ora_log_reset_claims(void);

#endif // TEST_STUB_H