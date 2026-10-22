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

#define RAM_ROM_TABLE_SIZE MAX_ROM_IMAGE_SIZE

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

// Whether the firmware asked for the bootloader on this boot.
//
// On a device enter_bootloader() never returns and the One ROM stops serving.
// The stub returns and firmware_main() carries on, so a refusal to boot leaves
// no other trace for a test to find.  Cleared by onerom_test_reset(), so it
// reads per boot.
uint8_t stub_bootloader_entered(void);

// Whether rom_reboot() was called since the last read, and its arguments.
//
// On a device the reboot follows.  The stub records the call and never
// reboots, so a test can read its arguments.  Reading clears the record,
// as does onerom_test_reset().
uint8_t stub_rom_reboot(uint8_t *bootloader_out, uint32_t *delay_ms_out);

// Make rom_reboot() fail, as it does on a device whose bootrom has no reboot
// function.  Cleared by onerom_test_reset().
void stub_set_rom_reboot_fails(uint8_t fails);

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

// Put the counter back to zero, where a device's is when TIMER0 starts.
//
// Called by onerom_test_reset() on every boot, so a scenario measuring an
// interval from the clock it is given starts where the last one started rather
// than wherever that one left it.
void stub_timer_reset(void);

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
// Only the three fields ora_gpio_query reports are modelled, plus one bit of
// function select: whether the output driver is enabled, what it is driving,
// what an input reads, and whether a PIO block has the pin.  The pin's use and
// its pulls are not, because nothing reads them back through the API.
//
// Cleared by onerom_test_reset() on every boot, so a scenario starts with the
// pads as a device's come up rather than carrying the previous one's state.
void stub_gpio_set(uint8_t gpio, uint8_t state);
uint8_t stub_gpio_is_output(uint8_t gpio);
uint8_t stub_gpio_level(uint8_t gpio);

// Hand a pin to a PIO block, or take it back.
//
// A pin whose function select names PIO is not SIO's to drive: an SIO write to
// it changes nothing, because the block drives the pad.  On a device that falls
// out of the hardware and no code tests for it, so the model does the same and
// stub_gpio_set ignores a write to a pin a block holds.  That keeps the
// knowledge here rather than in firmware that would otherwise have to ask
// whether it was under test.
//
// One ROM reaches this on a board that wires both LEDs to one pin: the LED
// engine borrows the pin for as long as a pixel takes, and the status LED
// cannot drive it until the engine hands it back.
void stub_gpio_set_pio_owned(uint8_t gpio, uint8_t owned);

// What an input pin reads.  Nothing is attached to a pin in this process, so an
// input reads low until a test says otherwise - which is what this is for: a
// pin released to input with a pull-up on the board reads high, and a test that
// cares can say so.  Has no effect while the pin is an output, which reports
// what it drives.
void stub_set_gpio_input(uint8_t gpio, uint8_t level);

// Stands in for OTP, which firmware_main() reads at boot.  A test sets what an
// ECC read and a raw read of each row return.  The two aren't kept consistent,
// as the firmware reads any one row only one way.  Every row reads 0, as an
// unwritten row does, until a test sets it.
//
// A device's reset doesn't clear OTP, so onerom_test_reset() leaves this alone
// and a test clears what it set with stub_otp_clear().
void stub_otp_set_ecc(uint16_t row, const uint16_t *values, uint32_t count);
void stub_otp_set_raw(uint16_t row, const uint32_t *values, uint32_t count);
void stub_otp_clear(void);

// Flash bytes where the firmware reads a plugin's boot slot.  A byte a test
// hasn't set reads 0xFF.  The bytes a test sets outlive onerom_test_reset() so
// the test clears them with stub_flash_clear().
uint8_t stub_flash_read8(uint32_t addr);
void stub_flash_set8(uint32_t addr, uint8_t value);
void stub_flash_clear(void);

char *stub_usb_dpram(void);
char *stub_usb_dpram_log(void);

// The firmware's USB controller reset steps since the last
// onerom_test_reset(), in order.  Each is a STUB_USB_RESET_ value.
// STUB_USB_RESET_LOG_MOVED is set in a step taken after the log moved.
//
// Copies up to max of them to steps and returns how many were recorded, up to
// STUB_USB_RESETS_MAX.
#define STUB_USB_RESET_ASSERT       1u
#define STUB_USB_RESET_RELEASE      2u
#define STUB_USB_RESET_RELEASE_WAIT 3u
#define STUB_USB_RESET_LOG_MOVED    0x80u
#define STUB_USB_RESETS_MAX         8u
uint8_t stub_usb_resets(uint8_t *steps, uint8_t max);

// Put the process-global state a device's reset would clear back to cold boot.
//
// The firmware's statics are ordinary host objects in a test build, so nothing
// restores them between the many boots one process runs.  The emulator calls
// this on every boot.  What it covers is firmware state with no counterpart in
// a plugin - see the comment on its definition.
void onerom_test_reset(void);
void stub_gpio_reset(void);

// Release every log channel.
//
// Call only from a harness that also clears its plugin's record of the channels
// it holds.
void ora_log_reset_claims(void);

#endif // TEST_STUB_H