// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 PIO/DMA autonomous ROM serving support

#include "include.h"

#if defined(RP235X)

#include "piodma/piodma.h"

// # Introduction
//
// This file contains a completely autonomous PIO and DMA based ROM serving
// implementation.  Once started, the PIO state machines and DMA channels
// serve ROM data in response to external chip select and address lines
// without any further CPU intervention.
//
// # Algorithm Summary
//
// The implementation uses three PIO state machines and 2 DMA channels, with
// the following overall operation:
// - PIO SM0 - Chip Select/Output Data Handler
// - PIO SM1 - Address Reader
// - DMA0    - Address Forwarder
// - DMA1    - Data Byte Fetcher
// - PIO SM2 - Data Byte Writer 
//
//     CS active   Data to Outputs                 CS Inactive  Data to Inputs
//             |   |                                         |  |
//             v   v                                         v  v
// SM0 ----------+-------------------------------------------------->
//     ^         |                                                  |
//     |         | (Optional IRQ0)                                  |
//     |         v                                                  |
//     |        SM1 ------> DMA0 --------> DMA1 -------> SM2        |
//     |         |            |             |             |         |
//     |         v            v             v             v         |
//     |     Read Addr  Forward Addr  Get Data Byte  Write Data     |
//     |  (Optional Loop)                                           |
//     |                                                            v
//     <-------------------------------------------------------------
//                                                   (Not to scale)
//
// # Timings
//
// It is difficult to be sure, but based on observed data, and theoretical
// estimates, the timings are estimated as follows:
// - Address valid to correct data byte is 11-14 cycles
// - Previous data valid after address change delay 14-11 cycles (although
//   it is much less than this is CS is made inactive, which is very likely)
// - CS active to data output is 5-6 cycles
// - CS inactive to data inputs is 3 cycles
//
// Physical settling time of lines will add to this.  Also, experience has
// shown that the system is likely to introduce other, unplanned for, stalls
// and other delays.  In particular if running _anything_ else, such as having
// an SWD debug probe connected, may introduce delays and jitter due to bus
// contention.
//
// At a max rated RP2350 clock speed of 150MHz this is:
// - 73-93ns from address to data
// - 33-40ns from CS active to data output
// - 20ns from CS inactive to data inputs
//
// At 50MHz:
// - 220-280ns from address to data
// - 280-220ns from previous data valid after address change
// - 100-120ns from CS active to data output
// - 60ns from CS inactive to data inputs
//
// Overclocked to 300MHz:
// - 37-47ns from address to data
// - 17-20ns from CS active to data output
// - 10ns from CS inactive to data inputs
//
// Address to data breakdown:
// - 2 cycle delay in GPIO state reaching PIO due to input-sync
// - SM1 address read 3-4 cycles:
//   - 3 is best case scenario
//   - 6 is worst case, but this "swallows" the input-sync delay, leading to 4
// - Triggering DMA via DREQ from SM1 RX FIFO 1 cycle
// - DMAs take 2-3 cycles each:
//   - 3 cycles is likely due to single cycle stall due to contention, likely
//     with other DMA channel.
//   - Assume no stall in transfer between them.
// - SM2 data byte output 1 cycle
//
// Previous data valid after address change breakdown:
// - Inverse of address to data breakdown
//
// CS active to data output breakdown:
// - 2 cycle delay in GPIO state reaching PIO due to input-sync
// - SM0 best case is 3 cycles - mov x, pins; jmp x--, N; mov pindirs, ~null
// - SM0 worst case adds 3 cycles, 2 of which "swallow" the input-sync delay
//
// CS active to inactive breakdown:
// - 2 cycle delay in GPIO state reaching PIO due to input-sync
// - SM0 best case is 3 cycles - mov x, pins; jmp !x, N; mov pindirs, null
// - SM0 worst case add 2 cycles, but these "swallow" the input-sync delay
//
// These timings do not quite add up.  The C64 character ROM is a 2332A, with
// 350ns access time - the maximum time allowed to go from address valid to
// valid.  As we can serve this ROM successfully at around 50MHz - with our
// worse cast estimate of 280ns for this time - either our estimates are wrong,
// or the C64 VIC-II requires better of the ROM than its specification - or
// both.  Worst case it seems like our estimates may be 20% under (i.e add 25%
// to them).
// 
// Therefore 50ns operation may require the RP2350 to be clocked closer to
// 400MHz than 300Mhz.  This is still likely to be within the RP2350's
// capabilities.
//
// # Detailed Operation
//
// The detailed operation is as follows:
//
// PIO0 SM0 - CS Handler
//  - (Initially ensures data pins are inputs.)
//  - Monitors the chip select lines.
//  - When all CS lines are active, optionally triggers an IRQ to signal the
//    address read SM to read the address lines.
//  - Sets the data pins to outputs after an optional delay.  The data lines
//    will not be serving the correct byte yet.
//  - Tight loops, checking for CS going inactive.
//  - When CS goes inactive again, sets data pins back to inputs and starts
//    over.
//
// PIO0 SM1 - Address Read
//  - (One time - reads high 16 bits of ROM table address from its TX FIFO.
//    This is preloaded to the TX FIFO by the CPU before starting the PIOs.)
//  - Prepares by pushing high 16 bits of ROM table address into its OSR.
//  - Optionally waits for IRQ from CS Handler SM.
//  - After optional delay (used in non-IRQ case), reads the address lines (16
//    bits) into OSR, completing the ROM table lookup address for the byte to
//    be served.
//  - Pushes the complete 32 bit ROM table lookup address into its RX FIFO 
//    (triggering DMA Channel 0).
//  - Loops back to 2nd step (pushing high 16 bits of ROM table address into
//    OSR).
//
// DMA Channel 0 - Address Forwarder
//  - Triggered by PIO0 SM1 RX FIFO using DREQ_PIO0_RX1 (SM1 RX FIFO).
//  - Reads the 32 bit ROM table lookup address from PIO0 SM1 RX FIFO.
//  - Writes the address into DMA Channel 1 READ_ADDR or READ_ADDR_TRIG
//    register.
//
// DMA Channel 1 - Data Byte Fetcher
//  - Triggered either DMA Channel 0 writing to this channels READ_ADDR_TRIG
//    or using DREQ_PIO0_RX1 (SM1 RX FIFO) - in which case this DMA is paced
//    identically to DMA Channel 0.
//  - Reads the ROM byte from the address specified in its READ_ADDR register.
//  - Writes the byte into PIO0 SM2 TX FIFO.
//  - Waits to be re-triggered by DMA Channel 0 writing to READ_ADDR_TRIG or
//    DREQ_PIO_RX1 (SM1 RX FIFO).
//
// PIO0 SM2 - Data Byte Output
//  - Waits for a data byte to become available in its TX FIFO.
//  - When data byte available, outputs the data byte on the data pins.
//  - Loops back to waiting for next data byte.
//
// There are a number of hardware pre-requisites for this to work correctly:
// - RP2350, not the RP2040.  This implementation uses:
//   - pinsdirs as a mov destination
//   - mov using pins as a source, only moving the configured "IN" pins.
//   Neither of these are supported by the RP2040's PIOs.
// - All Chip Select (or CE/OE) lines must be connected to contiguous GPIOs.
// - Any active high chip select lines must be inverted prior to use, by
//   using GPIO input inversion (INOVER).
// - All Data lines must be connected to contiguous GPIOs.
// - All Address lines must be connected to contiguous GPIOs, and be limited
//   to a 64KB address space.  (Strictly other powers of two could be
//   supported.)
//
// In order to minimise jitter, it is advisable to ensure the following:
// - The DMA channels have high AHB5 bus priority for both reads
//   and writes using the BUS_PRIORITY register.
// - Nothing else attempts to read or write to the 4 banks of SRAM the
//   64KB ROM table is striped across.
// - If other DMAs are enabled, the DMAs within this module should have a
//   higher priority set.
// - Nothing else accesses peripherals on the AHB5 splitter during operation.
//
// Possible enhancements:
// - May want to check CS is still active before setting data pins to outputs
//   in SM2.
//
// Note that a combined PIO/CPU implementation has also been explored (see
// PIO_CONFIG_NO_DMA).  This is discussed further below, but in summary, it
// matches DMA performance, while consuming a CPU core.
//
// # Supported PIO configuration options
//
// Note where min/max clock speeds are given below they tended to vary by
// 1-2Mhz, based on the day.  Likely due to temperature variations affecting
// the host's timing.  It is unlikely the RP2350's timing varies, given it
// has a modern, extremely accurate, clock source.
//
// For these tests, the RP2350 was not overclocked - the max supported clock
// speed it known to be higher than 150MHz for these ROMs, but there is a max
// speed, particularly for character ROMs, due to the video chip requiring a
// byte to be held after CS is dectivated.
//
// # PIO_CONFIG_DEFAULT
//
// - READ_IRQ = 1
// - ADDR_READ_DELAY = 0
//
// Here the IRQ from CS handler SM is used to trigger the address read SM.
// This works well serving a C64 charaxcter ROM at higher clock speeds
// (roughly 115-150MHz).
//
// Min/Max speeds:
// - PAL C64 Char ROM: 115-150MHz
// - PAL C64 Kernal ROM: 45-150MHz
// - PAL VIC-20 Char ROM: 44-150MHz
//
// # PIO_CONFIG_SLOW_CLOCK_KERNAL
//
// - READ_IRQ = 0
// - ADDR_READ_DELAY = 1
//
// Here 1 cycles is sufficient time to allow DMA chain to avoid backing up.
// However, the VIC-II requires a 2 cycle delay from the character ROM - see
// PIO_CONFIG_SLOW_CLOCK_CHAR.
//
// Min/Max speeds:
// - PAL C64 Kernal ROM: 41-150MHz
// - PAL VIC-20 Kernal ROM: 22-150MHz
//
// # PIO_CONFIG_SLOW_CLOCK_CHAR
//
// - READ_IRQ = 0
// - ADDR_READ_DELAY = 2
//
// Add an additional cycle of delay before reading address lines to allow the
// byte to remain on the bus slightly later, as seems to be required by a
// VIC-II chip of a character ROM
//
// Min/Max speeds:
// - PAL C64 Char ROM: 51-150MHz
// - PAL VIC-20 Char ROM: 51-150MHz

// Whether to use DMA (or instead, use the CPU to read bytes).  If set,
// ADDR_READ_IRQ is ignored.
//
// This option is not maintained any may be broken.  It was implemented to test
// which was faster - DMA or CPU.  It turns out to be identical performance -
// both serve a C64 character from down to 51MHz but no further without
// glitches.  Similarly, both serve a kernal down to 41MHz.
//
// Therefore the DMA approach has been selected as superior as it frees up the
// CPU for other applications.
//
// (Actually it is possible to implement an even more pathological assembly
// CPU loop which shaves the char ROM down to 50MHz, but it's likely fragile,
// breaking if the CPU loop ever takes an extra cycle, such as when a debug
// probe is connected.)
//
// #define PIO_CONFIG_NO_DMA  1

// Fallback default configuration
#if !defined(PIO_CONFIG_ADDR_READ_IRQ) && !defined(PIO_CONFIG_ADDR_READ_DELAY) && !defined(PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY) && !defined(PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY)
#if !defined(PIO_CONFIG_DEFAULT) && !defined(PIO_CONFIG_SLOW_CLOCK_KERNAL) && !defined(PIO_CONFIG_SLOW_CLOCK_CHAR) && !defined(PIO_CONFIG_NO_DMA)
#define PIO_CONFIG_SLOW_CLOCK_CHAR  1
#endif // !PIO_CONFIG_DEFAULT && !PIO_CONFIG_SLOW_CLOCK && !PIO_CONFIG_SLOW_CLOCK_CHAR
#endif // Fallback default

// Pre-defined PIO configuration options
#if defined(PIO_CONFIG_DEFAULT)
#define PIO_CONFIG_ADDR_READ_IRQ                1
#define PIO_CONFIG_ADDR_READ_DELAY              0
#define PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY      0
#define PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY  0
#elif defined(PIO_CONFIG_SLOW_CLOCK_KERNAL)
#define PIO_CONFIG_ADDR_READ_IRQ                0
#define PIO_CONFIG_ADDR_READ_DELAY              1
#define PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY      0
#define PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY  0
#elif defined(PIO_CONFIG_SLOW_CLOCK_CHAR)
#define PIO_CONFIG_ADDR_READ_IRQ                0
#define PIO_CONFIG_ADDR_READ_DELAY              2
#define PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY      0
#define PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY  0
#elif defined(PIO_CONFIG_NO_DMA)
#define PIO_CONFIG_ADDR_READ_IRQ                0
#define PIO_CONFIG_ADDR_READ_DELAY              1
#define PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY      0
#define PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY  0
#endif // PIO_CONFIG_DEFAULT

// Whether to use IRQ from CS handler to address read SM
#if !defined(PIO_CONFIG_ADDR_READ_IRQ)
#define PIO_CONFIG_ADDR_READ_IRQ  1
#endif // PIO_CONFIG_ADDR_READ_IRQ

// Whether to delay setting data pins to outputs at the start of the address
// read SM, after any optional IRQ, and, if so, by how many PIO cycles.
//
// Counter intuitively, this is useful to ensure the data remains valid longer,
// by delaying when it is actually read.  It is hard to add delays later in the
// chain, as the DMA transfers are tightly coupled to the PIO state machines.
//
// If PIO_CONFIG_ADDR_READ_IRQ=0 then this delay is essential to allow time for
// the DMA chain to process the address read before the next one.  So, set this
// to _at least_ 1 in that case.
//
// It may be that DMA Channel 0 requires only 2 cycles most of the time, but
// occassionally requires 3 (e.g. due to bus contention from the other DMA
// channel), because a C64 kernal _almost_ fully boots with both IRQ and this
// set to 0.  But not quite!
#if !defined(PIO_CONFIG_ADDR_READ_DELAY)
#define PIO_CONFIG_ADDR_READ_DELAY  0
#endif // PIO_CONFIG_ADDR_READ_DELAY

// Whether to delay setting data pins to outputs after CS goes active, and,
// if so, by how many PIO cycles.
//
// This may not be useful in practice, as ROM specifications tend to require
// that data become valid within a certain time after CS goes active - not
// that it _doesn't_ go active for a certain time.
#if !defined(PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY)
#define PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY  0
#endif // PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY

// Whether to hold data lines as outputs for a number of cycles after CS goes
// inactive, before setting them back to inputs, and, if so, by how many PIO
// cycles.
//
// This may not be useful in practice, as ROM specifications tend not to
// require a hold time after CS goes inactive.  (They do specify a hold time
// after address changes - see PIO_CONFIG_ADDR_READ_DELAY.)
#if !defined(PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY)
#define PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY  0
#endif // PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY

// IRQ to trigger ROM read by the CS Handler SM.
#define ROM_ADDR_READ_TRIGGER_IRQ  0

// Number of data and address lines
#define NUM_DATA_LINES    8
#define NUM_ADDR_LINES    16

// PIO2 SM0 - CS Handler
//
// The program is constructed dynamically in pio_load_programs().  The overall
// algorithm is as follows:
//
// .wrap_target                      ; Start of CS loop
// 0xa063, //  mov    pindirs, null  ; set data pins to inputs
// 0xa020, //  mov    x, pins        ; read CS lines
// 0x0041, //  jmp    x--, 1         ; CS inactive, loop back to re-read CS
//                                   ; Note the decrement of x is unused -
//                                   ; but there is no jmp x instruction
// 0xc000, //  irq    set 0 [N]      ; OPTIONAL: signal CS active to address
//                                   ; read SM
//                                   ; OPTIONAL: N cycle delay before setting
//                                   ; data pins to outputs
// 0xaN42, //  nop    [N]            ; OPTIONAL: N cycle delay before setting
//                                  ROM_ADDR_READ_TRIGGER_IRQ ; data pins to outputs (if not on irq)
// 0xa06b, //  mov    pindirs, ~null ; set data pins to output
// 0xa020, //  mov    x, pins        ; read CS lines again
// 0x002Y, //  jmp    !x, Y [N]      ; CS still active, if so jump back one
//                                   ; instruction.
// 0xaN42, //  nop    [N]            ; OPTIONAL: N cycle delay before setting
//                                   ; data pins to inputs
// .wrap                             ; End of CS loop 

// There is an alternate version to handle non-contiguous CS pins:
//
// set Y, 2nd_match_value
//
// inactive:
// mov pindirs, null
//
// test_if_active:
// mov x, pins                  ; Load pins to X
// jmp !x active                ; CS = 000 Go active, could add single cycle wait to take the same time as if CS = 010
// jmp x!=y test_if_active      ; CS != 010 Check again
// ; CS = 010, so drop into active
//
// active:
// mov pindirs, ~null
//
// .wrap_target: 
// test_if_inactive:
// mov x, pins                  ; Load pins to X
// jmp !x test_if_inactive      ; CS == 000 Stay active, test again
// jmp x!=y inactive            ; CS != 010 So, go inactive
// .wrap                        ; CS = 010, so test again 

// PIO 1 SM 0 - Address Read
//
// The program is constructed dynamically in pio_load_programs().  The overall
// algorithm is as follows:
//
// ; One time setup - get high word of ROM table address from TX FIFO.  This
// ; is 0x2001 as of v0.5.5, changed to 0x2000 as of v0.5.10.
// pull   block         ; get high word of ROM table address
// mov    x, osr        ; store high word in X
//
// .wrap_target         ; Start of address read loop
// in     x, 16         ; read high address bits from X
// wait   1 irq, 0 [N]  ; OPTIONAL: wait for CS to go active (and clears IRQ)
//                      ; OPTIONAL: N cycle delay after IRQ before reading
//                      ; address
// in     pins, 16      ; read address lines (autopush)
// .wrap                ; End of address read loop

// PIO2 SM1 - Data Byte Output
//
// The program is constructed dynamically in pio_load_programs().  The overall
// algorithm is as follows:
//
// .wrap_target
// out    pins, 8       ; Auto-pulls byte from TX FIFO (from DMA Channel 1)
//                      ; and outputs on data pins
// .wrap

// Define BLOCK and SM numbers for the PIO programs
#define BLOCK_ADDR                  1
#define SM_ADDR_READ                0
#define SM_ADDR_READ_RAM_WRITE      1
#define SM_A_MINUS_1_READ           2  // Not used
#define BLOCK_DATA                  2
#define SM_DATA_OUTPUT              0
#define SM_DATA_WRITE               1
#define SM_DATA_READ_RAM_WRITE      2
#define BLOCK_MONITOR               0
#define SM_ADDR_MONITOR_CS_MONITOR  0
#define SM_ADDR_MONITOR_ADDR_READ   1
#define ADDR_MONITOR_IRQ            0 
#define DMA_CH_ADDR_MONITOR         2

// Build and load the PIO programs for ROM serving
//
// Uses the single-pass PIO assembler macros from pioasm.h
static void piorom_load_programs(piorom_config_t *config) {
    uint8_t num_addr_pins = config->num_addr_pins;
    uint8_t a_minus_1_pin = config->a_minus_1_pin;
    uint8_t a_minus_1_signal_pin = config->a_minus_1_signal_pin;
    uint8_t force_16_bit = config->force_16_bit;
    uint8_t data_base_pin = config->data_base_pin;

    // Get the high X bits of the RAM table address for preloading into the
    // address reader SM.
    uint8_t effective_addr_pins = num_addr_pins;
    if (config->bit_mode == BIT_MODE_16) {
        // For 16 bit mode, num_addr_pins is one lower than the actual number,
        // as A-1 is not included, but will still be emulated.
        effective_addr_pins += 1;
    }
    uint8_t rom_table_num_addr_bits = 32 - effective_addr_pins;
    uint32_t high_bits_mask = (1 << rom_table_num_addr_bits) - 1;
    uint32_t low_bits_mask = (1 << effective_addr_pins) - 1;
    uint32_t __attribute__((unused)) alignment_size = (1 << effective_addr_pins) / 1024;
    DEBUG("ROM table high mask: 0x%08X low mask: 0x%08X", high_bits_mask, low_bits_mask);
    if (config->rom_table_addr & low_bits_mask) {
        ERR("PIO ROM serving requires ROM table address to be %uKB aligned",
            alignment_size);
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    uint32_t rom_table_high_bits = (config->rom_table_addr >> effective_addr_pins) & high_bits_mask;
    DEBUG("ROM table high %d: 0x%08X", rom_table_num_addr_bits, rom_table_high_bits);

#if defined(DEBUG_LOGGING)
    // Log other config values
    uint8_t num_cs_pins = config->num_cs_pins;
    uint8_t cs_base_pin = config->cs_base_pin;
    uint8_t num_data_pins = config->num_data_pins;
    uint8_t addr_base_pin = config->addr_base_pin;
    uint8_t addr_read_irq = config->addr_read_irq;
    uint8_t addr_read_delay = config->addr_read_delay;
    uint8_t cs_active_delay = config->cs_active_delay;
    uint8_t cs_inactive_delay = config->cs_inactive_delay;
    uint8_t no_dma = config->no_dma;
    uint8_t contiguous_cs_pins = config->contiguous_cs_pins;
    uint8_t multi_rom_mode = config->multi_rom_mode;
    bit_modes_t bit_mode = config->bit_mode;
    uint32_t cs_pin_2nd_match = config->cs_pin_2nd_match;
    uint8_t byte_pin = config->byte_pin;
    DEBUG("PIO ROM Config:");
    DEBUG("- CS pins: %d-%d", cs_base_pin, cs_base_pin + num_cs_pins - 1);
    DEBUG("- CS invert: %s %s %s %s",
        (config->invert_cs[0] ? "Y" : "N"),
        (config->invert_cs[1] ? "Y" : "N"),
        (config->invert_cs[2] ? "Y" : "N"),
        (config->invert_cs[3] ? "Y" : "N"));
    DEBUG("- Contiguous CS pins: %s, 0x%02X", (contiguous_cs_pins ? "Y" : "N"), cs_pin_2nd_match);
    DEBUG("- Multi-ROM mode: %s", (multi_rom_mode ? "Y" : "N"));
    DEBUG("- Data pins: %d-%d", data_base_pin, data_base_pin + num_data_pins - 1);
    DEBUG("- Address pins: %d-%d", addr_base_pin, addr_base_pin + num_addr_pins - 1);
    DEBUG("- Byte mode: %d /BYTE pin: %d, A-1 pin: %d, A-1 signal pin: %d", 
        bit_mode == BIT_MODE_8 ? 8 : 16, byte_pin, a_minus_1_pin, a_minus_1_signal_pin);
    DEBUG("- Force 16 bit mode: %s", (force_16_bit ? "Y" : "N"));
    DEBUG("- Addr read IRQ: %s, DMA: %s", (addr_read_irq ? "Y" : "N"), (no_dma ? "N" : "Y"));
    DEBUG("- Delays: addr read: %u, CS active/inactive: %u/%u", addr_read_delay, cs_active_delay, cs_inactive_delay);
#if defined(DEBUG_BUILD)
    //uint16_t cs_handler_clkdiv_int = config->data_io_clkdiv_int;
    //uint8_t cs_handler_clkdiv_frac = config->data_io_clkdiv_frac;
    //uint16_t addr_reader_clkdiv_int = config->addr_reader_read_clkdiv_int;
    //uint8_t addr_reader_clkdiv_frac = config->addr_reader_read_clkdiv_frac;
    //uint16_t a_minus_1_clkdiv_int = config->a_minus_1_clkdiv_int;
    //uint8_t a_minus_1_clkdiv_frac = config->a_minus_1_clkdiv_frac;
    //uint16_t data_writer_clkdiv_int = config->data_out_clkdiv_int;
    //uint8_t data_writer_clkdiv_frac = config->data_out_clkdiv_frac;
    //DEBUG("- CS Handler CLKDIV: %d.%02d", cs_handler_clkdiv_int, cs_handler_clkdiv_frac);
    //DEBUG("- Addr Reader CLKDIV: %d.%02d", addr_reader_clkdiv_int, addr_reader_clkdiv_frac);
    //DEBUG("- A-1 Reader CLKDIV: %d.%02d", a_minus_1_clkdiv_int, a_minus_1_clkdiv_frac);
    //DEBUG("- Data Writer CLKDIV: %d.%02d", data_writer_clkdiv_int, data_writer_clkdiv_frac);
    //DEBUG("- PIO algorithm config:");
#endif // DEBUG_BUILD
#endif // DEBUG_LOGGING

    // Set up the PIO assembler
    APIO_ASM_INIT();
    
    // Clear all PIO IRQs
    APIO_CLEAR_ALL_IRQS();

    // PIO0 Programs
    //
    // Currently none for ROM serving.  Expect to rationalize RAM/ROM together
    // at some point

    // PIO1 Programs
    //
    // Address handlers
    APIO_SET_BLOCK(BLOCK_ADDR);

    // If address lines are 16+, change this block's GPIOBASE
    uint8_t base_addr_pin = config->addr_base_pin;
    if (config->addr_base_pin < 16) {
        DEBUG("PIO%d GPIOBASE 0", BLOCK_ADDR);
        APIO_GPIOBASE_0();
    } else {
        DEBUG("PIO%d GPIOBASE 16", BLOCK_ADDR);
        base_addr_pin -= 16;
        a_minus_1_pin -= 16;
        a_minus_1_signal_pin -= 16;
        APIO_GPIOBASE_16();
    }

    // PIO1 SM0 - Address reader
    //
    // Reads address lines and pushes complete ROM table lookup address to the
    // DMA chain.
    //
    // In 16 bit mode "A-1" is not used, instead the LSB is always 0, so DMA
    // can read a 16-bit value.
    APIO_SET_SM(SM_ADDR_READ);

    // The ADDR_READ_DELAY gets added either to the IRQ (if it exists) or the
    // IN instruction (if no IRQ).  In the no IRQ case it is not important on
    // which instruction we add the delay, as it doesn't affect how "old" the
    // address will be went sent to the DMA, just how _frequently_ it is read.
    if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
        // We must add 3 cycles of delay, to ensure this PIO takes a total of
        // 6 cycles, to match the data output SM's worst case.
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_X(rom_table_num_addr_bits), 4));
    } else if (!config->addr_read_irq) {
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_X(rom_table_num_addr_bits), config->addr_read_delay));
    } else {
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_X(rom_table_num_addr_bits), config->addr_read_delay));
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_WAIT_IRQ_HIGH(ROM_ADDR_READ_TRIGGER_IRQ), config->addr_read_delay));
    }

    if (config->bit_mode == BIT_MODE_8) {
        // Last belt and braces check
        if ((rom_table_num_addr_bits + num_addr_pins) != 32) {
            ERR("Internal error - invalid addr read pins");
            limp_mode(LIMP_MODE_INVALID_CONFIG);
        }

        // Read the pins and we're done
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_IN_PINS(num_addr_pins));
    } else {
        // Last belt and braces check
        if ((rom_table_num_addr_bits + num_addr_pins) != 31) {
            ERR("Internal error - invalid addr read pins");
            limp_mode(LIMP_MODE_INVALID_CONFIG);
        }

        // In 16 bit mode we do not read A-1 and stick a 0 on the end instead,
        // so DMA will read a 16-bit value.
        APIO_ADD_INSTR(APIO_IN_PINS(num_addr_pins));
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_IN_NULL(1));
    }

    // Configure the address read SM
    APIO_SM_CLKDIV_SET(
        config->addr_reader_read_clkdiv_int,
        config->addr_reader_read_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(num_addr_pins) |  // Reading the address pins (unused
                                        // as this is for mov instructions)
        APIO_AUTOPUSH |                 // Auto push when we hit threshold
        APIO_PUSH_THRESH(32) |          // Push when we have 32 bits (from
                                        // X and from address pins)
        APIO_IN_SHIFTDIR_L |    // Shift left, so address lines are in low bits
        APIO_OUT_SHIFTDIR_L     // Direction doesn't matter, as we push 32 bits
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(base_addr_pin)
    );

    // Preload the ROM table address into the X register
    APIO_TXF = rom_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);  // Pull it into OSR
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);   // Store it in X

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    //APIO_LOG_SM("Address Reader");

#if 0
    // Currently unused as I can't figure out how to make this work.
    // Specifically, there are no pins that can be used for signaling.
    if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
        // In 16 bit mode we need a second SM to read the A-1 line and signal
        // the data writer so it can write out the high 8 bits to the low data
        // lines if both /BYTE is active (it test that) and A-1 is high (this
        // SM measures that).

        // This SM effectively acts as a shift register, in order to delay
        // getting the A-1 value to the data writer SM at just before it needs
        // it - which is when the DMA chain completes.  I.e. we need to delay
        // the A-1 value so A-1 was read as the same time as the rest of the
        // address.   We do this by shifting from IN pin -> ISR -> OSR -> OUT
        // pin.

        // Make this SM 2, so 1 is free for RAM WRITE address reader when we
        // combine RAM into this.
        APIO_SET_SM(SM_A_MINUS_1_READ);

        // Delay cycles are inserted to make this PIO take the same number of
        // cycles as the main address reader SM, to ensure they stay aligned,
        // and we read A-1 at the same time as the rest of the address.

        // Read A-1 pin to the ISR
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_PINS(1),1));

        // Shift the OSR to our OUT pin, which is used to signal the data
        // writer SM to write the top 8-bit value to the low data pins, if
        // /BYTE is active.  I.e. if A-1 = 0, a 0 is signaled and that SM
        // writes the low 8 bits.  If A-1 = 1, a 1 is signaled and the data
        // writer SM writes the high 8 bits to the low data pins.
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_OUT_PINS(1),1));

        // Configure the A-1 reader SM
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_MOV_OSR_ISR, 1));

        APIO_SM_CLKDIV_SET(
            config->a_minus_1_clkdiv_int,
            config->a_minus_1_clkdiv_frac
        );
        APIO_SM_EXECCTRL_SET(0);
        APIO_SM_SHIFTCTRL_SET(
            APIO_IN_COUNT(1) |      // Read in just A-1
            APIO_IN_SHIFTDIR_L |    // Shift left, so bit is in LSB of ISR
            APIO_OUT_SHIFTDIR_L     // Doesn't matter, OUT always puts LSB of
                                    // OSR to lowest pin
        );
        APIO_SM_PINCTRL_SET(
            APIO_IN_BASE(a_minus_1_pin) |    // Read A-1 pin
            APIO_OUT_BASE(a_minus_1_signal_pin) |   // Signal A-1 state
            APIO_OUT_COUNT(1) |                     // Signal just one pin
            APIO_SET_BASE(a_minus_1_signal_pin)     // Used to set pindirs
        );

        // Set A-1 signal pin to output, so it can signal the data writer SM
        APIO_SM_EXEC_INSTR(APIO_SET_PIN_DIRS(1));

        // Jump to start and log
        APIO_SM_JMP_TO_START();
        APIO_LOG_SM("Address Reader A-1");
    }
#endif // 0

    //
    // PIO 1 - End of block
    //
    APIO_END_BLOCK();

    // PIO2 Programs
    //
    // Data handlers
    APIO_SET_BLOCK(BLOCK_DATA);

    // If data lines are 16+, change this block's GPIOBASE
    uint8_t base_data_pin = config->data_base_pin;
    if ((base_data_pin < 16) || (config->cs_base_pin < 16)) {
        DEBUG("PIO%d GPIOBASE 0", BLOCK_DATA);
        APIO_GPIOBASE_0();
        if ((base_data_pin + config->num_data_pins) >= 32) {
            ERR("Invalid config - data pins and CS pins cannot overlap across GPIOBASE 0/16 boundary");
            limp_mode(LIMP_MODE_INVALID_CONFIG);
        }
    } else {
        DEBUG("PIO%d block GPIOBASE to 16", BLOCK_DATA);
        base_data_pin -= 16;
        APIO_GPIOBASE_16();
    }

    // PIO2 SM0 - CS handler
    //
    // Handles detecting CS active/inactive and setting data pins to
    // outputs/inputs accordingly.  Also triggers address read SM via IRQ if
    // configured to do so.
    APIO_SET_SM(SM_DATA_OUTPUT);

    if (config->rom_type == CHIP_TYPE_23QL384) {
        // OE=GPIO8, A14=GPIO10, A15=GPIO9
        // Active: OE=1 AND (A14=0 OR A15=0)
        // Use OE as the JMP pin and IN pins A15+A14

        // Y is preloaded below with 0b11 - the value of A15+A14 when the chip
        // should be inactive

        // Set data pins to inputs
        APIO_LABEL_NEW(ql384_inactive);
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);

        // If OE is inactive, wait.  Note that OE active high is hardware
        // inverted so active always reads low
        APIO_LABEL_NEW(ql384_inactive_poll);
        APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(ql384_inactive_poll)));   // OE=1, so stay inactive until it goes active

        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_LABEL_NEW_OFFSET(ql384_active, 2);
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(ql384_active)));
        APIO_ADD_INSTR(APIO_JMP(APIO_LABEL(ql384_inactive_poll)));  // A14 & A15 are both high

        // APIO_LABEL(ql384_active)
        // OE is active, and A14 or A15 (or both) are low
        // Set data pins to outputs
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);

        // Now wait for OE to go inactive or both A14 and A15 to go high
        APIO_LABEL_NEW(ql384_active_poll);
        APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(ql384_inactive)));   // OE has gone inactive
        APIO_ADD_INSTR(APIO_MOV_X_PINS);           // Read pins to X - A14 and A15
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(ql384_active_poll)));   // If A14 or A15 goes low, stay active, otherwise go inactive
    } else if (config->contiguous_cs_pins) {
        // "Normal" case - all CS pins contiguous
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);

        APIO_LABEL_NEW(load_cs);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        if (!config->multi_rom_mode) {
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(load_cs)));
        } else {
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(load_cs)));
        }
        if (config->addr_read_irq) {
            if (!config->cs_active_delay) {
                APIO_ADD_INSTR(APIO_IRQ_SET(ROM_ADDR_READ_TRIGGER_IRQ));
            } else {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IRQ_SET(ROM_ADDR_READ_TRIGGER_IRQ), config->cs_active_delay));
            }
        } else {
            if (config->cs_active_delay) {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (config->cs_active_delay - 1)));
            }
        }
        if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
            // Read /BYTE and if low, jump to special code to only set low 8
            // data pins to outputs
            APIO_LABEL_NEW_OFFSET(byte_low_offset, 4); 
            APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(byte_low_offset)));
        }
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);
        APIO_LABEL_NEW(check_cs_gone_inactive);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_WRAP_TOP();
        if (!config->multi_rom_mode) {
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check_cs_gone_inactive)));
        } else {
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(check_cs_gone_inactive)));
        }
        if (config->cs_inactive_delay) {
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (config->cs_inactive_delay - 1)));
        }

        // Now the special /BYTE active handling for 16 bit mode.
        if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
            // Set pindirs from Y which is preloaded to 0b11111111, so only low
            // 8 data pins are set to outputs.
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_Y);
            APIO_END();
            APIO_ADD_INSTR(APIO_JMP(APIO_LABEL(check_cs_gone_inactive)));
        }
    } else {
        // Non-contiguous CS pins - need to check for 2 different possible
        // CS values
        APIO_ADD_INSTR(APIO_SET_Y(config->cs_pin_2nd_match));
        
        // inactive:
        APIO_LABEL_NEW(inactive_offset);
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);

        // test_if_active:
        APIO_LABEL_NEW(test_if_active_offset);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);

        APIO_LABEL_NEW_OFFSET(active_offset, 2);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(active_offset)));
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(test_if_active_offset)));

        // active:
        if (config->addr_read_irq) {
            if (!config->cs_active_delay) {
                APIO_ADD_INSTR(APIO_IRQ_SET(ROM_ADDR_READ_TRIGGER_IRQ));
            } else {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IRQ_SET(ROM_ADDR_READ_TRIGGER_IRQ), config->cs_active_delay));
            }
        } else {
            if (config->cs_active_delay) {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (config->cs_active_delay - 1)));
            }
        }
        APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);

        // .wrap_target:
        // test_if_inactive:
        APIO_WRAP_BOTTOM();
        APIO_LABEL_NEW(test_if_inactive_offset);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(test_if_inactive_offset)));
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(inactive_offset)));
        if (config->cs_inactive_delay) {
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (config->cs_inactive_delay - 1)));
        }
    }

    // Configure the CS handler SM
    APIO_SM_CLKDIV_SET(
        config->data_io_clkdiv_int,
        config->data_io_clkdiv_frac
    );
    if (config->bit_mode == BIT_MODE_8) {
        if (config->rom_type != CHIP_TYPE_23QL384) {
            APIO_SM_EXECCTRL_SET(0);
        } else {
            APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(config->cs_base_pin));
        }
    } else {
        APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(config->byte_pin));
    }
    if (config->rom_type != CHIP_TYPE_23QL384) {
        APIO_SM_SHIFTCTRL_SET(
            APIO_IN_COUNT(config->num_cs_pins) |
            APIO_IN_SHIFTDIR_L          // Direction left important for non-
                                        // contiguous CS pin handling
        );
        APIO_SM_PINCTRL_SET(
            APIO_OUT_COUNT(config->num_data_pins) |
            APIO_OUT_BASE(base_data_pin) |
            APIO_IN_BASE(config->cs_base_pin)
        );
    } else {
        // Use A14 and A15 as the CS pins, and they immediate follow OE
        uint8_t cs_pins = 2;
        uint8_t cs_base_pin = config->cs_base_pin+1;
        if (config->cs_base_pin == 11) {
            // fire-28-c - A14/A15 are not contiguous, so need to read them separately and combine
            cs_pins = 3;
            cs_base_pin = 10;
        }
        APIO_SM_SHIFTCTRL_SET(
            APIO_OUT_SHIFTDIR_R |       // Required for 23QL384 fire-28-c
            APIO_IN_COUNT(cs_pins) |
            APIO_IN_SHIFTDIR_L          // Direction left important for non-
                                        // contiguous CS pin handling
        );
        APIO_SM_PINCTRL_SET(
            APIO_OUT_COUNT(config->num_data_pins) |
            APIO_OUT_BASE(base_data_pin) |
            APIO_IN_BASE(cs_base_pin)
        );
    }

    if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
        // For 16 bit mode, we use the Y register to control whether we set all
        // data pins to outputs, or just the lower 8.  Hence we need to preload
        // Y with 0xFF so it can be used for this in the CS handler program.
        APIO_TXF = 0xFF;
        APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
        APIO_SM_EXEC_INSTR(APIO_MOV_Y_OSR);
    }
    if (config->rom_type == CHIP_TYPE_23QL384) {
        // Preload Y with 0b11, the value of A15+A14 when chip should be
        // inactive, if A14:A15 are contiguous.
        uint8_t set_y_val = 0b11;
        if (config->cs_base_pin == 11) {
            // fire-28-c - A15:/OE:A15.  We can assume /OE will still be 0, so
            // just need to read 3 values and set the middle one to 0 here.
            set_y_val = 0b101;
        }
        APIO_SM_EXEC_INSTR(APIO_SET_Y(set_y_val));
    }

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    //APIO_LOG_SM("CS Handler");

    // PIO2 SM1 - Data output
    //
    // Outputs a data byte received from the DMA chain on the data pins.
    APIO_SET_SM(SM_DATA_WRITE);

    // Load the data byte output program
    uint8_t bits = 8;
    if ((config->bit_mode == BIT_MODE_8) || force_16_bit) {
        if (config->bit_mode == BIT_MODE_16) {
            bits = 16;
        }
        APIO_ADD_INSTR(APIO_OUT_PINS(bits));
    } else {
        // For this approach DMA replicates 16-bit values across the FIFO, so
        // we get two copies in the OSR.
        //
        // Ideally we would signal this SM A-1 state from the A-1 reader SM,
        // but we don't have any spare pins in the shared (16-31) range to use
        // so instead we have to read A-1 directly here.  That's sub-optimal,
        // as it means reading A-1 from a different time than the rest of the
        // address.  it could be 50-75ns later, depending on the latency of the
        // DMA chain.

        bits = 16;

        // Read from the TX FIFO
        APIO_ADD_INSTR(APIO_PULL_BLOCK);

        // If /BYTE active mode (high), jump to special byte handling
        APIO_LABEL_NEW_OFFSET(byte_mode_active_offset, 3);
        APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(byte_mode_active_offset)));

        // 16-bit mode - set all 16 data pins to values from DMA
        APIO_ADD_INSTR(APIO_OUT_PINS(16));

        // If we get here, we're in /BYTE inactive mode and done.  We jump
        // rather than wrapping, as we need the byte mode active code to take
        // no more than 6 cycles (same as address reader SM) or everything gets
        // out of kilter.
        APIO_ADD_INSTR(APIO_JMP(APIO_START_LABEL()));

        // Read the A-1 signalling pin to X
        APIO_ADD_INSTR(APIO_MOV_X_PINS);

        // If X high low (meaning high 8 bits are required) jump to do that
        APIO_LABEL_NEW_OFFSET(high_byte, 4);
        APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(high_byte)));

        // Output low 8 bits
        APIO_ADD_INSTR(APIO_OUT_PINS(8));

        // Output high 8 bits to null
        APIO_ADD_INSTR(APIO_OUT_NULL(8));

        // Jump to start
        APIO_ADD_INSTR(APIO_JMP(APIO_START_LABEL()));

        // First shift low 8 bits to null
        APIO_ADD_INSTR(APIO_OUT_NULL(8));

        // Write high 8 bits to pins, then wrap to save a JMP
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_OUT_PINS(8)); 
    }

    // Configure the data byte SM
    APIO_SM_CLKDIV_SET(
        config->data_out_clkdiv_int,
        config->data_out_clkdiv_frac
    );
    if ((config->bit_mode == BIT_MODE_8) || force_16_bit) {
        APIO_SM_EXECCTRL_SET(0);
    } else {
        APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(config->byte_pin));
    }
    if ((config->bit_mode == BIT_MODE_8) || force_16_bit) {
        APIO_SM_SHIFTCTRL_SET(
            APIO_OUT_SHIFTDIR_R |   // Writes LSB of OSR
            APIO_AUTOPULL |         // Auto pull when we hit threshold
            APIO_PULL_THRESH(bits)  // Pull when we have 8 or 16 bits
        );
        APIO_SM_PINCTRL_SET(
            APIO_OUT_BASE(data_base_pin) |
            APIO_OUT_COUNT(bits)
        );
    } else {
        APIO_SM_SHIFTCTRL_SET(
            APIO_OUT_SHIFTDIR_R |    // Writes LSB of OSR
            APIO_AUTOPULL |          // Auto pull when we hit threshold
            APIO_PULL_THRESH(bits) | // Pull when we have 8 or 16 bits
            APIO_IN_COUNT(1)         // Read A-1 signal pin
        );
        APIO_SM_PINCTRL_SET(
            APIO_OUT_BASE(base_data_pin) |
            APIO_OUT_COUNT(bits) |
            APIO_IN_BASE(config->a_minus_1_pin)
        );
    }

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    //APIO_LOG_SM("Data Byte Output lower");

    //
    // PIO 2 - End of block
    //
    APIO_END_BLOCK();
}

// Starts the PIO state machines for ROM serving.
static void piorom_start_pios(piorom_config_t *config) {
    // SM_A_MINUS_1_READ unused
    (void)config;
    //if ((config->bit_mode == BIT_MODE_8) || config->force_16_bit) {
        APIO_ENABLE_SMS(BLOCK_ADDR, 1 << SM_ADDR_READ);
    //} else {
    //    APIO_ENABLE_SMS(BLOCK_ADDR, ((1 << SM_ADDR_READ) | (1 << SM_A_MINUS_1_READ)));
    //}
    APIO_ENABLE_SMS(BLOCK_DATA, ((1 << SM_DATA_OUTPUT) | (1 << SM_DATA_WRITE)));
}

// Set GPIOs to PIO function for ROM serving
static void piorom_set_gpio_func(piorom_config_t *config) {
    uint8_t num_cs_pins = config->num_cs_pins;
    uint8_t cs_base_pin = config->cs_base_pin;
    uint8_t *cs_pin_invert = config->invert_cs;
    uint8_t data_base_pin = config->data_base_pin;
    uint8_t num_data_pins = config->num_data_pins;
    uint8_t force_16_bit = config->force_16_bit;
    //uint8_t addr_base_pin = config->addr_base_pin;

    APIO_GPIO_INIT();

    // Data pins
    for (int ii = data_base_pin;
        ii < (data_base_pin + num_data_pins);
        ii++) {
        APIO_GPIO_OUTPUT(ii, BLOCK_DATA);
    }
    DEBUG("Data pins %d-%d set to PIO%d", data_base_pin, data_base_pin + num_data_pins - 1, BLOCK_DATA);

    // Address pins - not required, inputs only
    // for (int ii = addr_base_pin;
    //     ii < (addr_base_pin + NUM_ADDR_LINES);
    //     ii++) {
    //     APIO_GPIO_OUTPUT(ii, BLOCK_ADDR);
    // }

    // CS pins
    //
    // We MUST set these after the address pins, as the CS pins may be part of
    // the address pin range (they are on 24 and 28 pin ROMs).
    for (int ii = 0; (ii < num_cs_pins) && (ii < 4); ii++) {
        uint8_t pin = cs_base_pin + ii;
        uint8_t invert = cs_pin_invert[ii];
        // Set to PIO function - this clears everything else - not required,
        // inputs only
        // APIO_GPIO_OUTPUT(pin, BLOCK_DATA);
        if (!invert) {
            //DEBUG("  CS pin %d active low CTRL=0x%08X", pin, GPIO_CTRL(pin));
        } else {
            // Turn CS line into active low by inverting the GPIO before the
            // PIO reads it
            APIO_GPIO_INPUT_INVERT(pin);
            DEBUG("CS pin %d inverted", pin);
        }
    }

    // Invert the /BYTE pin so it becomes active high
    if (config->bit_mode == BIT_MODE_16 && (!force_16_bit)) {
        APIO_GPIO_INPUT_INVERT(config->byte_pin);
        DEBUG("/BYTE inverted %d", config->byte_pin);
    }

    // A-1 signal pin must be set to PIO func, as well as enabling outputs
    if ((config->bit_mode == BIT_MODE_16) && (!force_16_bit)) {
        //DEBUG("A-1 signal %d PIO2", config->a_minus_1_signal_pin);
        APIO_GPIO_OUTPUT(config->a_minus_1_signal_pin, BLOCK_ADDR);
    }
}

#if !defined(TEST_BUILD)
// Setup the DMA channels for ROM serving
static void piorom_setup_dma(
    piorom_config_t *config,
    uint8_t pio_block_addr,
    uint8_t sm_addr_read,
    uint8_t pio_block_data,
    uint8_t sm_data_byte
) {
    (void)pio_block_data; // Unused
    (void)sm_data_byte;  // Unused
    (void)pio_block_addr; // Unused
    (void)sm_addr_read;   // Unused
    volatile dma_ch_reg_t *dma_reg;

    // DMA Channel 0 - Receives ROM table lookup address from PIO1 SM0 and
    // sends it onto DMA Channel 1.  Paced by PIO1 SM0 RX FIFO DREQ.
    dma_reg = DMA_CH_REG(0);
    dma_reg->read_addr = (uint32_t)&APIO1_SM_RXF(sm_addr_read);
    if (config->addr_read_irq) {
        // When address read is triggerd by IRQ, we only want a single
        // transfer per IRQ.  We need to trigger channel 1 manually.
        dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR_TRIG(1);
        dma_reg->transfer_count = 1;
    } else {
        // When address read is not triggered by IRQ, we want continuous
        // transfers to channel 1.  No triggering is necessary, as channel 1
        // will be paced by the PIO1 SM0 RX FIFO DREQ, like this channel.
        dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR(1);
        dma_reg->transfer_count = 0xffffffff;
    }
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(pio_block_addr, sm_addr_read)) |
        DMA_CTRL_TRIG_EN |
        DMA_CTRL_TRIG_DATA_SIZE_32BIT |
        DMA_CTRL_TRIG_CHAIN_TO(0);

    // DMA Channel 1 - Reads ROM data from memory and sends to PIO0 SM2.
    // Also paced by PIO0 SM1 RX FIF DREQ, so runs in lock-step with channel
    // 0.
    // Pre-load the READ_ADDR register with the first byte of the ROM table.
    // This byte will never actually get served, as the data lines will be
    // inputs, but it's more valid than setting to 0.
    dma_reg = DMA_CH_REG(1);
    dma_reg->read_addr = config->rom_table_addr;
    dma_reg->write_addr = (uint32_t)&APIO2_SM_TXF(sm_data_byte);
    uint32_t ctrl_trig = DMA_CTRL_TRIG_EN | DMA_CTRL_TRIG_CHAIN_TO(0);
    if (config->bit_mode == BIT_MODE_16) {
        DEBUG("DMA1 16-bit");
        ctrl_trig |= DMA_CTRL_TRIG_DATA_SIZE_16BIT;
    } else {
        DEBUG("DMA1 8-bit");
        ctrl_trig |= DMA_CTRL_TRIG_DATA_SIZE_8BIT;
    }
    if (config->addr_read_irq) {
        // When address read is triggerd by IRQ, we only want a single
        // transfer per IRQ.  We need to re-trigger channel 1 manually.
        dma_reg->transfer_count = 1;
        ctrl_trig |= DMA_CTRL_TRIG_TREQ_SEL(DMA_CTRL_TRIG_TREQ_PERM);
    } else {
        // When address read is not triggered by IRQ, we want continuous
        // transfers.
        dma_reg->transfer_count = 0xffffffff;
        ctrl_trig |= DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(pio_block_addr, sm_addr_read));
    }
    dma_reg->ctrl_trig = ctrl_trig;

#if 0
    //
    // Temporary duplicate DMA channel for 16-bit mode
    //
    dma_reg = DMA_CH_REG(2);
    dma_reg->read_addr = (uint32_t)&APIO1_SM_RXF(1);
    dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR(3);
    dma_reg->transfer_count = 0xffffffff;
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(1, 1)) |
        DMA_CTRL_TRIG_EN |
        DMA_CTRL_TRIG_DATA_SIZE_32BIT |
        DMA_CTRL_TRIG_CHAIN_TO(2);
    dma_reg = DMA_CH_REG(3);
    dma_reg->read_addr = config->rom_table_addr;
    dma_reg->write_addr = (uint32_t)&APIO2_SM_TXF(2);
    dma_reg->transfer_count = 0xffffffff;
    dma_reg->ctrl_trig = 
        DMA_CTRL_TRIG_EN | 
        DMA_CTRL_TRIG_DATA_SIZE_8BIT |
        DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(1, 1)) |
        DMA_CTRL_TRIG_CHAIN_TO(2);
#endif

    // Set DMA Read as high priority on the AHB5 bus for both:
    // - Reads (from RAM and PIO RX FIFO)
    // - Writes (to PIO TX FIFO and DMA READ_ADDR)
    BUSCTRL_BUS_PRIORITY |=
        BUSCTRL_BUS_PRIORITY_DMA_R_BIT |
        BUSCTRL_BUS_PRIORITY_DMA_W_BIT;
}
#else // TEST_BUILD
void piorom_setup_dma(
    piorom_config_t *config,
    uint8_t pio_block_addr,
    uint8_t sm_addr_read,
    uint8_t pio_block_data,
    uint8_t sm_data_byte
) {
    (void)config;
    (void)pio_block_addr;
    (void)sm_addr_read;
    (void)pio_block_data;
    (void)sm_data_byte;
    STUB_LOG("piorom_setup_dma");
}
#endif // !TEST_BUILD

// Get lowest data GPIO from the pin info
static uint8_t get_lowest_data_gpio(
    const sdrr_info_t *info
) {
    uint8_t lowest = MAX_USED_GPIOS;
    for (int ii = 0; ii < 8; ii++) {
        if (info->pins->data[ii] < lowest) {
            lowest = info->pins->data[ii];
        }
    }
    return lowest;
}

// Get lowest address GPIO from the pin info.
//
// For 24 pin ROMs this includes CS lines and X pins.
// For 28 pin ROMs this include CS lines.
static uint8_t get_lowest_addr_gpio(
    const sdrr_info_t *info,
    uint32_t img_size,
    const uint8_t cs_base_pin,
    sdrr_rom_type_t rom_type

) {
    uint8_t lowest = MAX_USED_GPIOS;
    uint8_t chip_pins = info->pins->chip_pins;

    for (int ii = 0; ii < 16; ii++) {
        if ((ii == 0) && (rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200)) {
            // Skip A-1 pin for 16-bit capable chips, as the algorithm handles
            // the lowest bit (only required in /BYTE low mode) separately.
            continue;
        }
 
        if (info->pins->addr[ii] < lowest) {
            lowest = info->pins->addr[ii];
        }
    }

    if (chip_pins != 24) {
        // For 28 pin ROMs, we check the image size to decide how many address
        // pins to consider, not the ROM type.  This is because some version of
        // Studio uses 256KB images for <231024 ROMS.  However, since 0.6.4,
        // only a 231024 _needs_ a 256KB image.
        if (img_size > (128*1024)) {
            // Consider addr2 pins, but only when serving a > 64KB image, as
            // for 28 pin ROMs, these are only used in this case.
            for (int ii = 0; ii < 8; ii++) {
                if (rom_type == CHIP_TYPE_27C301) {
                    if (info->pins->cs2 == 16 && ii == 0) {
                        // cs2=16 = fire-32-a
                        // Ignore A16 for 27C301, as this is actually /OE.
                        DEBUG("Ignoring A16 (GPIO %d) for 27C301", info->pins->addr2[ii]);
                        continue;
                    }
                    if (info->pins->cs2 == 12) {
                        // fire-32-b
                        if (ii == 2 || ii == 1) {
                            // fire-32-b - also ignore A17 and A18
                            DEBUG("Ignoring A%d (GPIO %d) for 27C301 fire-32-b",
                                ii+16,
                                info->pins->addr2[ii]
                            );
                            continue;
                        }
                    }
                }

                if (info->pins->addr2[ii] < lowest) {
                    lowest = info->pins->addr2[ii];
                }
            }
        }
    }

    if (chip_pins == 24) {
        // Consider X pins
        if (info->pins->x1 < lowest) {
            lowest = info->pins->x1;
        }
        if (info->pins->x2 < lowest) {
            lowest = info->pins->x2;
        }
    }

    if ((chip_pins == 24) ||
        ((chip_pins == 28) && (img_size >= (256*1024)))) {
        // Consider CS pins - only need to check the base as this will be the
        // lowest.
        //
        // 24 pin One ROMs always include CS pins.  However, 28 pins only
        // include them if the image size is 256KB.  check_config() checks
        // that image size is either 64KB or 256Kb so the > test here is
        // sufficient.
        if (cs_base_pin < lowest) {
            lowest = cs_base_pin;
        }
    } else if (rom_type == CHIP_TYPE_23QL512 || rom_type == CHIP_TYPE_23QL384) {
        // A15 is actually in the CE location.
        if (info->pins->ce < lowest) {
            lowest = info->pins->ce;
        }
    } else if (rom_type == CHIP_TYPE_SST39SF040) {
        // Add one to the lowest address pin - as the usual A18 is first, but
        // on this chip type, on fire-32-b, A18 is last
        lowest += 1; 
    }

    return lowest;
}

// Handle non-contiguous CS pins - changes configuration so that a different
// CS PIO algorithm is used.
//
// Args:
// - config: PIO ROM serving configuration
// - num_cs_pins: Number of CS pins originally detected
// - lowest_cs: Lowest CS pin number
// - low_cs: Highest of the bottom set of contiguous CS pins
// - high_cs: Lowest of the top set of contiguous CS pins
static void piorom_handle_non_contiguous_cs_pins(
    piorom_config_t *config,
    uint8_t num_cs_pins,
    uint8_t lowest_cs,
    uint8_t low_cs,
    uint8_t high_cs
) {
    DEBUG("Handle non-contig pins num_cs_pins=%d lowest_cs=%d low_cs=%d high_cs=%d",
        num_cs_pins,
        lowest_cs,
        low_cs,
        high_cs
    );
    if (!config->contiguous_cs_pins) {
        ERR("Multiple non-contiguous CS pin ranges not supported");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return;
    }

    // We have non-contiguous CS pins.  Only support a single pin break.
    if ((high_cs - low_cs) != 2) {
        ERR("Non-contiguous CS pins with break of more than 1 pin not supported");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return;
    }

    config->contiguous_cs_pins = 0;
    config->num_cs_pins = num_cs_pins+1;
    config->cs_pin_2nd_match = 1 << (low_cs - lowest_cs + 1);
}

// Construct the PIO ROM serving configuration from the SDRR and ROM set info
static void piorom_finish_config(
    piorom_config_t *config,
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
) {
    const sdrr_rom_info_t *rom = set->roms[0];
    uint8_t is_16_bit_capable = 0;
    if (rom->rom_type == CHIP_TYPE_27C400 || rom->rom_type == CHIP_TYPE_27C200) {
        is_16_bit_capable = 1;
        if (runtime->force_16_bit) {
            config->force_16_bit = 1;
        }
    }

    // Figure out number of CS pins from ROM type
    switch (rom->rom_type) {
        case CHIP_TYPE_2364:
            if (set->serve != SERVE_ADDR_ON_ANY_CS) {
                config->num_cs_pins = 1;
            } else {
                if ((set->rom_count < 2) || (set->rom_count > 3)) {
                    ERR("PIO ROM serving invalid multi-ROM count %d for 2364",
                        set->rom_count);
                    limp_mode(LIMP_MODE_INVALID_CONFIG);
                    config->num_cs_pins = 1;
                } else {
                    // Always use CS AND both X pins, even when serving dual
                    // ROM sets.  This ensures X2 is also hardware inverted,
                    // which is required to serve the correct byte.
                    config->num_cs_pins = 3;
                    config->multi_rom_mode = 1;
                }
            }
            break;

        case CHIP_TYPE_2332:
        case CHIP_TYPE_23256:
        case CHIP_TYPE_23512:
            config->num_cs_pins = 2;
            break;

        case CHIP_TYPE_2316:
        case CHIP_TYPE_23128:
            config->num_cs_pins = 3;
            break;

        case CHIP_TYPE_2704:
        case CHIP_TYPE_2708:
        case CHIP_TYPE_2716:
        case CHIP_TYPE_2732:
        case CHIP_TYPE_2764:
        case CHIP_TYPE_27128:
        case CHIP_TYPE_27256:
        case CHIP_TYPE_27512:
        case CHIP_TYPE_28C16:
        case CHIP_TYPE_28C64:
        case CHIP_TYPE_28C256:
        case CHIP_TYPE_28C512:
            config->num_cs_pins = 2;
            break;

        case CHIP_TYPE_231024:
        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            config->num_cs_pins = 1;
            break;

        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
            config->num_cs_pins = 2;
            break;

        case CHIP_TYPE_SST39SF040:
        case CHIP_TYPE_27C010:
        case CHIP_TYPE_27C020:
        case CHIP_TYPE_27C040:
        case CHIP_TYPE_27C301:
            config->num_cs_pins = 2;
            break;

        case CHIP_TYPE_27C080:
            config->num_cs_pins = 3;
            break;


        default:
            ERR("PIO ROM serving invalid ROM type %d", rom->rom_type);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            config->num_cs_pins = 1;
            break;
    }

    // Figure out CS pin base
    uint8_t series_23 = 0;
    switch (rom->rom_type) {
        // 23 series ROMs - use CS lines
        case CHIP_TYPE_2364:
            // Special case for handling multi-ROM serving
            if (config->multi_rom_mode) {
                // For 2 or 3 ROMs always use CS, X1 and X2.
                // The base pin is the lowest of these.
                // Strictly contiguity of X2 with the others is not required
                // when serving 2 ROM sets, but required for simplicity - it
                // would be perverse to develop hardware to support 2 ROM sets
                // but not 3 ROM sets.
                series_23 = 1;
                uint8_t lowest = info->pins->cs1;
                if (info->pins->x1 < lowest) {
                    lowest = info->pins->x1;
                }
                if (config->num_cs_pins == 3) {
                    if (info->pins->x2 < lowest) {
                        lowest = info->pins->x2;
                    }
                }
                config->cs_base_pin = lowest;

                // For now, insist on contiguity (it may be possible to lift
                // this restriction)
                if ((info->pins->x1 > (info->pins->cs1 + 1)) ||
                    (info->pins->x1 < (info->pins->cs1 - 1))) {
                    ERR("PIO ROM serving non-contiguous CS/X1 pins not supported");
                    limp_mode(LIMP_MODE_INVALID_CONFIG);
                }
                if (config->num_cs_pins == 3) {
                    if ((info->pins->x2 > (info->pins->x1 + 1)) ||
                        (info->pins->x2 < (info->pins->x1 - 1))) {
                        ERR("PIO ROM serving non-contiguous CS/X1/X2 pins not supported");
                        limp_mode(LIMP_MODE_INVALID_CONFIG);
                    }
                }
                break;
            }
            // GCC notices the following comment and allows compilation
            // fall through 
        case CHIP_TYPE_2316:
        case CHIP_TYPE_2332:
        case CHIP_TYPE_23128:
        case CHIP_TYPE_23256:
        case CHIP_TYPE_23512:
        case CHIP_TYPE_231024:
        case CHIP_TYPE_27C080:
            // Figure out base CS pin from SDRR info
            series_23 = 1;
            uint8_t num_cs_pins = config->num_cs_pins;

            // Collect and sort the active CS pins ascending
            uint8_t cs1_pin = info->pins->cs1;
            uint8_t cs2_pin = info->pins->cs2;
            uint8_t cs3_pin = info->pins->cs3;
            if (rom->rom_type == CHIP_TYPE_27C080) {
                // 27C080, we use A19/pin 1 as CS1, then the other CS lines
                // are CE/OE
                cs2_pin = info->pins->ce;
                cs3_pin = info->pins->oe;
            }
            uint8_t pins[3] = { cs1_pin, cs2_pin, cs3_pin };
            for (uint8_t i = 1; i < num_cs_pins; i++) {
                for (uint8_t j = i; j > 0 && pins[j-1] > pins[j]; j--) {
                    uint8_t tmp = pins[j-1];
                    pins[j-1] = pins[j];
                    pins[j] = tmp;
                }
            }

            config->cs_base_pin = pins[0];

            if (num_cs_pins > 1) {
                uint8_t gap_count = 0;
                for (uint8_t i = 1; i < num_cs_pins; i++) {
                    if (pins[i] != pins[i-1] + 1) {
                        gap_count++;
                    }
                }

                if (gap_count == 1) {
                    // Find which pair has the gap
                    uint8_t low = pins[0];
                    uint8_t high = pins[1];
                    for (uint8_t i = 1; i < num_cs_pins; i++) {
                        if (pins[i] != pins[i-1] + 1) {
                            low = pins[i-1];
                            high = pins[i];
                            break;
                        }
                    }
                    piorom_handle_non_contiguous_cs_pins(config, num_cs_pins,
                        pins[0], low, high);
                } else if (gap_count > 1) {
                    ERR("Multiple non-contiguous CS pin ranges not supported");
                    limp_mode(LIMP_MODE_INVALID_CONFIG);
                    break;
                }
            }
            break;

        // 27 series ROMs - use OE/CE lines
        case CHIP_TYPE_2704:
        case CHIP_TYPE_2708:
        case CHIP_TYPE_2716:
        case CHIP_TYPE_2732:
        case CHIP_TYPE_2764:
        case CHIP_TYPE_27128:
        case CHIP_TYPE_27256:
        case CHIP_TYPE_27512:
        case CHIP_TYPE_27C010:
        case CHIP_TYPE_27C020:
        case CHIP_TYPE_SST39SF040:
        case CHIP_TYPE_27C040:
        case CHIP_TYPE_27C301:
        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
        case CHIP_TYPE_28C16:
        case CHIP_TYPE_28C64:
        case CHIP_TYPE_28C256:
        case CHIP_TYPE_28C512:
            ;
            // Use OE/CE instead of CS pins
            uint8_t ce_pin = info->pins->ce;
            uint8_t oe_pin = info->pins->oe;
            if (rom->rom_type == CHIP_TYPE_27C301) {
                // Don't use OE, use CS2 pin for 27C301
                oe_pin = info->pins->cs2;
            }

            config->cs_base_pin = oe_pin;
            if (ce_pin == (config->cs_base_pin + 1)) {
                // OK
            } else if (ce_pin == (config->cs_base_pin - 1)) {
                config->cs_base_pin = ce_pin;
            } else if (ce_pin > (config->cs_base_pin + 1)) {
                if (rom->rom_type == CHIP_TYPE_27C400 || rom->rom_type == CHIP_TYPE_27C200) {
                    // Non-contiguous not supported for 27C400/200 as the chip
                    // select detect algorithm is more complex, due to the
                    // need to spot /BYTE
                    ERR("PIO ROM serving non-contiguous OE/CE pins not supported for 27C400/200");
                    limp_mode(LIMP_MODE_INVALID_CONFIG);
                }
                piorom_handle_non_contiguous_cs_pins(
                    config,
                    config->num_cs_pins,
                    config->cs_base_pin,
                    config->cs_base_pin,
                    ce_pin
                );
            } else {
                // ce is less than oe
                if (rom->rom_type == CHIP_TYPE_27C400 || rom->rom_type == CHIP_TYPE_27C200) {
                    ERR("PIO ROM serving non-contiguous OE/CE pins not supported for 27C400/200");
                    limp_mode(LIMP_MODE_INVALID_CONFIG);
                }
                piorom_handle_non_contiguous_cs_pins(
                    config,
                    config->num_cs_pins,
                    ce_pin,
                    ce_pin,
                    config->cs_base_pin
                );
                config->cs_base_pin = ce_pin;
            }
            break;

        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            config->cs_base_pin = info->pins->cs2;
            break;

        default:
            ERR("PIO ROM serving invalid ROM type %d", rom->rom_type);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            config->num_cs_pins = 1;
            break;
    }

    // Find any CS lines which need to be inverted.  Make sure to make CS
    // lines against the pin numbers - the lower pin number is first, whether
    // that is CS1 or CS2 (or CS3).
    //
    // This isn't required for 27 series ROMs, as both OE and CE are active
    // low.
    //
    // Where non-contiguous CS pins are used, we may check non CS pins here.
    // That's OK as they won't match an actual CS pin.
    if (series_23) {
        if (!config->multi_rom_mode) {
            for (int ii = 0; (ii < config->num_cs_pins) && (ii < 4); ii++) {
                if (info->pins->cs1 == (config->cs_base_pin + ii)) {
                    if (rom->cs1_state == CS_ACTIVE_HIGH) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                } else if (info->pins->cs2 == (config->cs_base_pin + ii)) {
                    if (rom->cs2_state == CS_ACTIVE_HIGH) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                } else if (info->pins->cs3 == (config->cs_base_pin + ii)) {
                    if (rom->cs3_state == CS_ACTIVE_HIGH) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                }
            }

            // For the ROM type 2316 specifically, we have to swap CS2
            // and CS3 inversions around, as 2316 CS3 is 2332 CS2, and the
            // 2332 is how we refer to the pins.
            if (rom->rom_type == CHIP_TYPE_2316) {
                uint8_t cs2_offset = info->pins->cs2 - config->cs_base_pin;
                uint8_t cs3_offset = info->pins->cs3 - config->cs_base_pin;
                uint8_t temp = config->invert_cs[cs2_offset];
                config->invert_cs[cs2_offset] = config->invert_cs[cs3_offset];
                config->invert_cs[cs3_offset] = temp;
            }
        } else {
            // In multi-ROM mode, CS1, X1 and potentially X2 are CS lines.
            // Also, invert logic is reversed compared to the normal case, as
            // _any_ CS line active is supported.
            for (int ii = 0; (ii < config->num_cs_pins) && (ii < 3); ii++) {
                if (info->pins->cs1 == (config->cs_base_pin + ii)) {
                    if (rom->cs1_state == CS_ACTIVE_LOW) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                } else if (info->pins->x1 == (config->cs_base_pin + ii)) {
                    // Inversion is per CS1
                    if (rom->cs1_state == CS_ACTIVE_LOW) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                } else if (info->pins->x2 == (config->cs_base_pin + ii)) {
                    // Inversion is per CS1
                    if (rom->cs1_state == CS_ACTIVE_LOW) {
                        config->invert_cs[ii] = 1;
                    } else {
                        config->invert_cs[ii] = 0;
                    }
                }
            }
        }
    }
    if (rom->rom_type == CHIP_TYPE_27C080) {
        // 27C080 is 1MB.  We can only serve half of it.  A19 is used as a CS
        // pin to decide whether this is the half we serve, so can be active
        // low (serve bottom half) or active high (serve top half).
        if (rom->cs1_state == CS_ACTIVE_HIGH) {
            config->invert_cs[0] = 1;
        } else {
            config->invert_cs[0] = 0;
        }
    }
    if ((rom->rom_type == CHIP_TYPE_2364) && (info->pins->chip_pins == 28)) {
        // Special case the 2364 on 28 pin ROM case:
        // - Use A16 as the CS pins, and invert it if required.
        config->cs_base_pin = info->pins->addr2[0];
        if (rom->cs1_state == CS_ACTIVE_HIGH) {
            config->invert_cs[0] = 1;
        } else {
            config->invert_cs[0] = 0;
        }
    }
    if (rom->rom_type == CHIP_TYPE_23QL512 || rom->rom_type == CHIP_TYPE_23QL384) {
        // For 23QL384/512, CS2 is being used as CS1 so special case the inversion test
        if (rom->cs1_state == CS_ACTIVE_HIGH) {
            config->invert_cs[0] = 1;
        } else {
            config->invert_cs[0] = 0;
        }
    }

    // Figure out base address pin from SDRR info
    uint32_t img_size = set->size;
    config->addr_base_pin = get_lowest_addr_gpio(info, img_size, config->cs_base_pin, rom->rom_type);
    if (is_16_bit_capable) {
        // !!! Currently set this to the actual A-1 pin in the data range.  If
        // we were using the A-1 reader SM, it would need to be in the address
        // reader pin range (connected to a second pin, like 37)
        config->a_minus_1_pin = 15;

        // !!! Currentlu hardcoded to fire-40-a unused pin, and unused anyway
        // as the data PIO block can't access this pin.
        config->a_minus_1_signal_pin = 45;
    }

    // Figure out base data pin from SDRR info
    config->data_base_pin = get_lowest_data_gpio(info);

    // Set the ROM table address
    config->rom_table_addr = rom_table_addr;

    // Set the number of address lines from ROM pins
    if (info->pins->chip_pins == 24) {
        config->num_addr_pins = 16;
    } else if (info->pins->chip_pins == 28) {
        // For 28 pin ROMs, we check the image size to decide how many address
        // pins to consider, not the ROM type.  This is because some version of
        // Studio uses 256KB images for <231024 ROMS.  However, since 0.6.4,
        // only a 231024 _needs_ a 256KB image.
        if (img_size > (128*1024)) {
            config->num_addr_pins = 18; // Includes OE/CE (OE also A16 for 231024)
        } else if (img_size > (64*1024)) {
            // 23QL512.  Doesn't include OE which is the first pin.
            config->num_addr_pins = 17;
        } else {
            config->num_addr_pins = 16; // Doesn't include OE/CE
        }
    } else if (info->pins->chip_pins == 32) {
        config->num_addr_pins = 19; // Doesn't include any CS pins
    } else {
        config->num_addr_pins = 19; // Doesn't include OE/CE/BYTE
        config->byte_pin = info->pins->byte;
    }

    // Handle 8/16 bit mode for 40 pin ROMs
    config->bit_mode = BIT_MODE_8;
    if (is_16_bit_capable) {
        DEBUG("Enable 16-bit mode");
        config->bit_mode = BIT_MODE_16;
        if (config->addr_read_delay > 0) {
            // PIO algorithm for 16 bits takes 1 extra cycle - so we can
            // shave one off the delay
            config->addr_read_delay -= 1;
        }

        // A-1 is not used as part of the address space for 16 bit ROMs.
        // get_lowest_addr_gpio() skips A-1 when determining the
        // addr_base_pin, so we don't need to adjust that here.
        config->num_addr_pins -= 1;

        // Set number of data pins
        config->num_data_pins = 16;
    }
    runtime->bit_mode = config->bit_mode;

    // Final checks
    if ((config->rom_table_addr == 0) || (config->rom_table_addr == 0xFFFFFFFF)) {
        ERR("PIO Invalid ROM table address");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->cs_base_pin >= 26) {
        ERR("PIO Invalid CS pin(s)");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->data_base_pin >= 26) {
        ERR("PIO Invalid data pins");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->addr_base_pin >= 26) {
        ERR("PIO Invalid address pins");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->addr_read_irq > 1) {
        ERR("PIO Invalid addr_read_irq");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->addr_read_delay > 32) {
        ERR("PIO Invalid addr_read_delay");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    if (config->cs_active_delay > 32) {
        ERR("PIO Invalid cs_active_delay");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
}

// Default PIO ROM serving configuration
static const piorom_config_t int_piorom_config = {
    .num_cs_pins = 0,
    .invert_cs = {0, 0, 0},
    .cs_base_pin = 255,
    .data_base_pin = 255,
    .num_data_pins = NUM_DATA_LINES,
    .addr_base_pin = 255,
    .num_addr_pins = NUM_ADDR_LINES,
    .addr_read_irq = PIO_CONFIG_ADDR_READ_IRQ,
    .addr_read_delay = PIO_CONFIG_ADDR_READ_DELAY,
    .cs_active_delay = PIO_CONFIG_CS_TO_DATA_OUTPUT_DELAY,
    .cs_inactive_delay = PIO_CONFIG_CS_INACTIVE_DATA_HOLD_DELAY,
#if defined(PIO_CONFIG_NO_DMA) && !PIO_CONFIG_NO_DMA
    .no_dma = 1,
#else // !PIO_CONFIG_NO_DMA
    .no_dma = 0,
#endif // PIO_CONFIG_NO_DMA
    .byte_pin = 255,
    .a_minus_1_pin = 255,
    .a_minus_1_signal_pin = 255,
    .force_16_bit = 0,
    .rom_type = INVALID_CHIP_TYPE,
    .rom_table_addr = 0,
    .addr_reader_read_clkdiv_int = 1,
    .addr_reader_read_clkdiv_frac = 0,
    .pad2 = 0,
    .a_minus_1_clkdiv_int = 1,
    .a_minus_1_clkdiv_frac = 0,
    .pad3 = 0,
    .data_io_clkdiv_int = 1,
    .data_io_clkdiv_frac = 0,
    .pad4 = 0,
    .data_out_clkdiv_int = 1,
    .data_out_clkdiv_frac = 0,
    .pad5 = 0,
    .contiguous_cs_pins = 1,
    .multi_rom_mode = 0,
    .bit_mode = BIT_MODE_8,
    .pad6 = 0,
    .cs_pin_2nd_match = 255
};

// Apply any ROM set specific overrides to the PIO ROM serving configuration.
void piorom_overrides(
    const sdrr_rom_set_t *set,
    piorom_config_t *config
) {
    config->rom_type = set->roms[0]->rom_type;
    if ((set->extra_info) &&
        (set->serve_config != NULL) &&
        (set->serve_config != (void*)0xFFFFFFFF)) {
            const uint8_t *serve_config = (const uint8_t*)set->serve_config;

            // Current supported PIO serve override format:
            // Byte 0: 0xFE (signature)
            // Byte 1: addr_read_irq
            // Byte 2: addr_read_delay
            // Byte 3: cs_active_delay
            // Byte 4: cs_inactive_delay
            // Byte 5: no_dma
            // Byte 6: 0xFE (end signature)
            // Byte 7: 0xFF (padding)
            if ((serve_config[0] == 0xFE) &&
                (serve_config[1] != 0xFF) && 
                (serve_config[2] != 0xFF) && 
                (serve_config[3] != 0xFF) && 
                (serve_config[4] != 0xFF) && 
                (serve_config[5] != 0xFF) &&
                (serve_config[6] == 0xFE) &&
                (serve_config[7] == 0xFF)) {
                config->addr_read_irq = serve_config[1];
                config->addr_read_delay = serve_config[2];
                config->cs_active_delay = serve_config[3];
                config->cs_inactive_delay = serve_config[4];
                config->no_dma = serve_config[5];
                LOG("PIO found valid overriding serve config: 0x%02X 0x%02X 0x%02X 0x%02X 0x%02X",
                    config->addr_read_irq,
                    config->addr_read_delay,
                    config->cs_active_delay,
                    config->cs_inactive_delay,
                    config->no_dma
                );
            } else {
                ERR("PIO ROM serving invalid serve_config signature");
                DEBUG("  Bytes: 0x%02X 0x%02X 0x%02X 0x%02X 0x%02X 0x%02X 0x%02X 0x%02X",
                    serve_config[0],
                    serve_config[1],
                    serve_config[2],
                    serve_config[3],
                    serve_config[4],
                    serve_config[5],
                    serve_config[6],
                    serve_config[7]
                );
                limp_mode(LIMP_MODE_INVALID_CONFIG);
            }
    }
}

static void piorom_force_unused_addr_pins_to_zero(
    const sdrr_info_t *info,
    const sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    const piorom_config_t *config
) {
    (void)runtime;

    // Force any unused address pins to 0.  This is quite complicated:
    // - For 24 pin ROMs, CS and X pins are in the same range.  For 23 series
    //   24 pin ROMs, CS and address lines are always used.  X pins are only
    //   used to multi-ROM and dynamic banked configurations.  In the multi-ROM
    //   case, X2 is only used for a 3 ROM set.
    // - For 28 pin ROMs, CS pins are part of the address space.
    // - 32 pin ROMs.  CS lines are not part of the address space.
    // - 40 pin ROMs are easy - all address pins are always used.
    if (config->multi_rom_mode) {
        if (set->rom_count < 3) {
            if (info->pins->x2 < MAX_USED_GPIOS) {
                // Multi-ROM CS works opposite to regular CS.  CS pins are
                // inverted if necessary so that non zero = serve.  As X2
                // is not being used, it should never contribute to serving,
                // so should always be low.
                DEBUG("Force X2 pin %d low", info->pins->x2);
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->x2);
            }
        }
    }

    switch (set->roms[0]->rom_type) {
        case CHIP_TYPE_2316:
        case CHIP_TYPE_2332:
        case CHIP_TYPE_2364:
        case CHIP_TYPE_23512:
            // No NC pins - any spare A lines are used as CS lines.
            break;

        case CHIP_TYPE_231024:
            // No NC pins - all address lines used.  /OE becomes A16
            break;

        case CHIP_TYPE_2704:
            // Physical pin 22 = A9.  NC 
            if (info->pins->addr[9] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[9]);
            }
            // Also 2708 pins:
        
        // Fall through
        case CHIP_TYPE_2708:
            // Physical pins 19 = A10 and 21 = A12.  NC.
            // 2364 A11 = /CE so ignore
            if (info->pins->addr[10] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[10]);
            }

        // Fall through
        case CHIP_TYPE_2716:
            // Physical pin 21 is Vpp.
            if (info->pins->addr[12] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[12]);
            }
            if (info->pins->chip_pins == 28) {
                // A11 is unused.
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[11]);
            }

        // Fall through
        case CHIP_TYPE_2732:
            // For 24 pin ROMs, physical pin 21 is A11 (not A12), and is used.
            // No NC.
            if (info->pins->chip_pins == 28) {
                // A12, A13, A14 and A15 are unused.  Assume these pins aren't
                // invalid, as they shouldn't be for a 28 pin ROM
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[12]);
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[13]);
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[14]);
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[15]);
            }
            break;

        case CHIP_TYPE_28C16:
            // Physical pin 21 is /WRITE
            if (info->pins->addr[12] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_HIGH(info->pins->addr[12]);
            }
            break;

        case CHIP_TYPE_28C64:
            // Pin 1 /BUSY (A15)
            // PIN 27 /WRITE (A14)
            if (info->pins->addr[14] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_HIGH(info->pins->addr[14]);
            }
            if (info->pins->addr[15] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[15]);
            }

        // Fall through
        case CHIP_TYPE_2764:
            // Pin 26 = NC = A13
            if (info->pins->addr[13] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[13]);
            }

        // Fall through
        case CHIP_TYPE_27128:
            // Pin 27 = PGM = A14
            if (info->pins->addr[14] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[14]);
            }

        // Fall through
        case CHIP_TYPE_23128:
        case CHIP_TYPE_23256:
        case CHIP_TYPE_27256:
            // Pin 1 = A15 = Vpp
            if (info->pins->addr[15] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[15]);
            }
            break;

        case CHIP_TYPE_28C256:
            // Pin 27 /WRITE
            if (info->pins->addr[14] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_HIGH(info->pins->addr[14]);
            }
            break;

        case CHIP_TYPE_27512:
            // No NC pins - all address lines used.
            break;

        case CHIP_TYPE_27C301:
            // As per 27C010

        // Fall through
        case CHIP_TYPE_27C010:
            // Pin 30 = NC = A17
            if (info->pins->addr2[17-16] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr2[17-16]);
            }

        // Fall through
        case CHIP_TYPE_27C020:
            // Pin 31 = PGM = A18
            if (info->pins->addr2[18-16] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr2[18-16]);
            }
            break;

        case CHIP_TYPE_28C512:
            // Pin 31 = NC (A18)
            // Pin 30 = /WRITE (A17)
            // Pin 2 = NC (A16)
            // Pin 1 = NC (A19 = no-op)
            if (info->pins->addr2[18-16] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr2[18-16]);
            }
            if (info->pins->addr2[17-16] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_HIGH(info->pins->addr2[17-16]);
            }
            if (info->pins->addr2[16-16] < MAX_USED_GPIOS) {
                APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr2[16-16]);
            }
            break;

        case CHIP_TYPE_SST39SF040:
        case CHIP_TYPE_27C040:
            // No NC pins - all address lines used.
            break;

        case CHIP_TYPE_27C080:
            // No NC pins - all address lines used.
            break;

        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
            // No NC pins - all address lines used.
            break;

        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            // Actual addr 15 is NC.  (/CE is used as A15)
            APIO_GPIO_FORCE_INPUT_LOW(info->pins->addr[15]);
            break;

        default:
            ERR("Invalid ROM Type # %d", set->roms[0]->rom_type);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
}

// Configure and start the Autonomous PIO/DMA ROM serving implementation.
int piorom(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
) {
    piorom_config_t *config = &runtime->piorom_config;

    DEBUG("%s", log_divider);

    memcpy(config, &int_piorom_config, sizeof(piorom_config_t));

    // Apply any ROM set overrides
    piorom_overrides(set, config);

    piorom_finish_config(config, info, runtime, set, rom_table_addr);

    // Bring PIO0 and DMA out of reset
    APIO_ENABLE_PIOS();
    DMA_ENABLE();

    // Setup the DMA channels:
    // - Adress PIO block 1
    // - Data PIO block 2
    // - SM0 is the address read SM
    // - SM1 is the data byte output SM
#if !defined(TEST_BUILD)
    if (!config->no_dma) {
        piorom_setup_dma(config, BLOCK_ADDR, SM_ADDR_READ, BLOCK_DATA, SM_DATA_WRITE);
    }
#endif // !TEST_BUILD

    // Configure GPIOs for PIO function
    // - 2 CS pins
    // - CS pins start at GPIO 10
    // - CS active high/low config
    // - Data pins start at GPIO 0
    // - Address pins start at GPIO 8
    piorom_set_gpio_func(config);

    // Force any address pins to zero as required
    piorom_force_unused_addr_pins_to_zero(info, runtime, set, config);

    // Load and configure the PIO programs
    // - 2 CS pins
    // - CS pins start at GPIO 10
    // - Data pins start at GPIO 0
    // - Address pins start at GPIO 8
    piorom_load_programs(config);

    if (!config->no_dma) {
#if !defined(TEST_BUILD)
        if (runtime->rom_dma_copy) {
            DEBUG("DMA copy words remaining: 0x%08X", dma_copy_status());
        }
#endif // !TEST_BUILD

        // Start the PIOs.  This kicks off the autonomous ROM serving.
        DEBUG("Start PIOs");
        piorom_start_pios(config);

#if !defined(DEBUG_BUILD)
        // Done
        return 0;
#else // DEBUG_BUILD
        while (1) {
            uint32_t read_addr1 = DMA_CH_REG(1)->read_addr;
            DEBUG("DMA1 Read Addr: 0x%08X",
                read_addr1);
            DEBUG("PIO1 FIFO Status 0x%08X", APIO1_FSTAT);
            DEBUG("PIO2 FIFO Status 0x%08X", APIO2_FSTAT);

            // Delay to avoid swamping RTT
            for (volatile int ii = 0; ii < 100000; ii++);
        }
#endif // !DEBUG_BUILD
    } else {
#if !defined(TEST_BUILD)
        DEBUG("PIO ROM serving running without DMA - CPU active loop");

        register volatile uint32_t *ctrl asm("r0") = &APIO0_CTRL;
        register volatile uint32_t *rxf1 asm("r2") = &APIO0_SM_RXF(1);
        register volatile uint32_t *txf2 asm("r3") = &APIO0_SM_TXF(2);
        register volatile uint32_t *irq  asm("r4") = &APIO0_IRQ_FORCE;
        register uint32_t irq_set asm("r5") = 0x1;  // Set IRQ 0

        asm volatile (
            // Enable SM0/1/2
            "movs r1, #0x7\n"
            "str  r1, [r0]\n"

            // 6 cycle version with IRQ triggering SM1 to read address -
            // essentially pacing SM1, avoiding addr reads backing up
            "1:\n"
            "ldr  r0, [r2]\n"       // Read address from SM1 RX (1 cycle + 1 stall)
            "ldrb r1, [r0]\n"       // Read byte from that address (1 cycle)
            "str  r5, [r4]\n"       // Set IRQ triggering SM1 to re-read (1 cycle)
            "str  r1, [r3]\n"       // Write byte to SM2 TX (1 cycle)
            "b    1b\n"             // Loop (1 cycle)

            /*
            // 5 cycle version, eliminating read address stall with branch
            "ldr  r0, [r2]\n"
            "1:\n"
            "ldrb r1, [r0]\n"
            "str  r5, [r4]\n"
            "str  r1, [r3]\n"
            "ldr  r0, [r2]\n"
            "b    1b\n"
            */

            // Pathological 5 cycle version, requires no IRQ detection in SM1.
            // Shaves char ROM ser ing down to 50MHz.
            //"1:\n"
            //"str  r1, [r3]\n"       // Write byte to SM2 TX (1 cycle)
            //"ldr  r0, [r2]\n"       // Read address from SM1 RX (1 cycle + 1 stall)
            //"ldrb r1, [r0]\n"       // Read byte from that address (1 cycle)
            //"b    1b\n"             // Loop (1 cycle)

            :
            : "r"(ctrl), "r"(rxf1), "r"(txf2), "r"(irq), "r"(irq_set)
            : "r1", "memory"
        );
#else // TEST_BUILD
        ERR("PIO ROM serving without DMA not supported in test build");
#endif // !TEST_BUILD
    }

    return 0;
}

#if !defined(TEST_BUILD)

static void pio_setup_address_monitor_pios() {
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint8_t num_cs_pins = piorom_config->num_cs_pins;
    uint8_t cs_base_pin = piorom_config->cs_base_pin;
    uint8_t num_addr_pins = piorom_config->num_addr_pins;
    uint8_t base_addr_pin = piorom_config->addr_base_pin;

    // For multi-ROM mode, we ignore X1 and X2 and only montior the "first"
    // ROM.  We also only support 2364s.
    if (piorom_config->multi_rom_mode) {
        num_cs_pins = 1;
        cs_base_pin = sdrr_info.pins->cs1;
    }

    APIO_ASM_INIT();
    APIO_SET_BLOCK(BLOCK_MONITOR);
    APIO_GPIOBASE_0();

    //
    // SM 0: CS Monitor
    //
    APIO_SET_SM(SM_ADDR_MONITOR_CS_MONITOR);

    if (piorom_config->contiguous_cs_pins) {
        // All CS pins contiguous - CS active == zero
        APIO_WRAP_BOTTOM();
        APIO_LABEL_NEW(cs_inactive);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        if (!piorom_config->multi_rom_mode) {
            // CS active == zero
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
        } else {
            // CS active == non-zero (pins inverted)
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
        }

        APIO_ADD_INSTR(APIO_IRQ_SET(ADDR_MONITOR_IRQ));

        APIO_LABEL_NEW(cs_active);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_WRAP_TOP();
        if (!piorom_config->multi_rom_mode) {
            // CS inactive == non-zero
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_active)));
        } else {
            // CS inactive == zero (pins inverted)
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_active)));
        }
    } else {
        // Non-contiguous CS pins - CS active == zero OR cs_pin_2nd_match
        APIO_ADD_INSTR(APIO_SET_Y(piorom_config->cs_pin_2nd_match));

        APIO_LABEL_NEW(cs_inactive);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_LABEL_NEW_OFFSET(check2, 2);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check2)));
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_LABEL_NEW_OFFSET(check3, 2);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check3)));
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_LABEL_NEW_OFFSET(check4, 2);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check4)));
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_LABEL_NEW_OFFSET(cs_active, 2);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_active)));
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));

        // cs_active:
        APIO_ADD_INSTR(APIO_IRQ_SET(ADDR_MONITOR_IRQ));

        APIO_WRAP_BOTTOM();
        APIO_LABEL_NEW(test_if_inactive);
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(test_if_inactive)));
        APIO_WRAP_TOP();
        APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
        // cs_pin_2nd_match still active - wrap back to test_if_inactive
    }

    APIO_SM_CLKDIV_SET(1, 0);
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(num_cs_pins) |
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(cs_base_pin)
    );
    APIO_SM_JMP_TO_START();

    //
    // SM 1: Address read monitor
    //
    APIO_SET_SM(SM_ADDR_MONITOR_ADDR_READ);

    APIO_ADD_INSTR(APIO_WAIT_IRQ_HIGH(ADDR_MONITOR_IRQ));
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_IN_PINS(num_addr_pins));

    APIO_SM_CLKDIV_SET(1, 0);
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_AUTOPUSH        |
        APIO_PUSH_THRESH(num_addr_pins) |
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(base_addr_pin)
    );
    APIO_SM_JMP_TO_START();

    APIO_END_BLOCK();

    return;
}

static void pio_setup_address_monitor_dma(
    uint8_t dma_ch,
    uint8_t block,
    uint8_t sm_addr_read,
    volatile uint32_t *ring_buf,
    uint8_t ring_size_log2,
    uint8_t data_size
) {
    uint32_t dma_data_size;
    if (data_size == 8) {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_8BIT;
    } else if (data_size == 16) {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_16BIT;
    } else {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_32BIT;
    }

    // SM1 RX FIFO -> ring_buf circular write
    volatile dma_ch_reg_t *dma_reg = DMA_CH_REG(dma_ch);
    dma_reg->read_addr = (uint32_t)&APIO0_SM_RXF(sm_addr_read);
    dma_reg->write_addr = (uint32_t)ring_buf;
    dma_reg->transfer_count = 0xffffffff;
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |
        dma_data_size |
        DMA_CTRL_RING_SIZE(ring_size_log2) |
        DMA_CTRL_RING_SEL |
        DMA_CTRL_INCR_WRITE |
        DMA_CTRL_TRIG_CHAIN_TO(dma_ch) |
        DMA_CTRL_TRIG_TREQ_SEL(
            APIO_DREQ_PIO_X_SM_Y_RX(
                block,
                sm_addr_read
            )
        );
}

ora_result_t pio_setup_address_monitor(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
) {
    (void)mode;
    (void)reserved;

    uint32_t bytes_per_entry_log2 = __builtin_ctz(data_size / 8); // 8->0, 16->1, 32->2
    uint32_t ring_size_log2 = ring_entries_log2 + bytes_per_entry_log2;
    uint32_t ring_size = 1u << ring_size_log2;

    // Check ring_buf is valid and aligned to ring size
    if (ring_buf == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (((uintptr_t)ring_buf % ring_size) != 0) {
        return ORA_RESULT_INVALID_SIZE;
    }

    pio_setup_address_monitor_pios();
    pio_setup_address_monitor_dma(
        DMA_CH_ADDR_MONITOR,
        BLOCK_MONITOR,
        SM_ADDR_MONITOR_ADDR_READ,
        ring_buf,
        ring_size_log2,
        data_size
    );

    return ORA_RESULT_OK;
}

static ora_result_t get_addr_pin(uint8_t i, uint8_t *pin_out) {
    uint8_t pin = (i < 16) ? sdrr_info.pins->addr[i]
                            : sdrr_info.pins->addr2[i - 16];
    if (pin >= MAX_USED_GPIOS) {
        return ORA_RESULT_INTERNAL_ERROR;
    }
    *pin_out = pin;
    return ORA_RESULT_OK;
}

uint32_t pio_map_addr_to_phys(uint32_t logical_addr) {
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint8_t base = piorom_config->addr_base_pin;
    uint8_t num  = piorom_config->num_addr_pins;
    uint32_t physical = 0;

    for (uint8_t b = 0; b < num; b++) {
        if (logical_addr & (1u << b)) {
            uint8_t pin;
            if (get_addr_pin(b, &pin) == ORA_RESULT_OK) {
                physical |= (1u << (pin - base));
            }
        }
    }

    // In multi-ROM mode CS1 is part of the SRAM address space, and active
    // CS1 = bit SET (inverted).  Always OR in the CS1 bit so back-channel
    // writes land in the correct half of SRAM that the host is reading from.
    if (piorom_config->multi_rom_mode) {
        uint8_t cs1_pin = sdrr_info.pins->cs1;
        if (cs1_pin < MAX_USED_GPIOS) {
            physical |= (1u << (cs1_pin - base));
        }
    }

    return physical;
}

uint32_t pio_map_data_to_phys(uint32_t logical_data) {
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint8_t base = piorom_config->data_base_pin;
    uint32_t physical = 0;

    for (uint8_t b = 0; b < 8; b++) {
        if (logical_data & (1u << b)) {
            uint8_t pin = sdrr_info.pins->data[b];
            if (pin < MAX_USED_GPIOS) {
                physical |= (1u << (pin - base));
            }
        }
    }
    return physical;
}

ora_result_t pio_demangle_addr(
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
) {
    if (logical_addr_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint8_t base = piorom_config->addr_base_pin;
    uint8_t num  = piorom_config->num_addr_pins;

    if (check_control_pins) {
        uint8_t x1  = sdrr_info.pins->x1;
        uint8_t x2  = sdrr_info.pins->x2;
        uint8_t cs1 = sdrr_info.pins->cs1;

        if (!piorom_config->multi_rom_mode) {
            // CS1 active low, not inverted - reject if CS1 inactive (set)
            if ((cs1 < MAX_USED_GPIOS) && (physical_addr & (1u << (cs1 - base)))) {
                return ORA_RESULT_CONTROL_PIN_ACTIVE;
            }
        } else {
            // Pins are inverted - CS1 active = bit SET, X pins active = bit SET
            // Reject if CS1 inactive (clear)
            if ((cs1 < MAX_USED_GPIOS) && !(physical_addr & (1u << (cs1 - base)))) {
                return ORA_RESULT_CONTROL_PIN_ACTIVE;
            }
            // Reject if any X pin active (set)
            if ((x1 < MAX_USED_GPIOS) && (physical_addr & (1u << (x1 - base)))) {
                return ORA_RESULT_CONTROL_PIN_ACTIVE;
            }
            if ((x2 < MAX_USED_GPIOS) && (physical_addr & (1u << (x2 - base)))) {
                return ORA_RESULT_CONTROL_PIN_ACTIVE;
            }
        }
    }

    // 23QL512 not supported here, nor are other chip types like the 231024,
    // 2732 - any snowflake chip type
    // TODO - lift restriction
    uint32_t logical = 0;
    for (uint8_t b = 0; b < num; b++) {
        uint8_t pin;
        if (get_addr_pin(b, &pin) == ORA_RESULT_OK) {
            if (physical_addr & (1u << (pin - base))) {
                logical |= (1u << b);
            }
        }
    }

    *logical_addr_out = logical;
    return ORA_RESULT_OK;
}

uint8_t pio_demangle_data(uint8_t physical_data) {
    uint8_t base = sdrr_runtime_info.piorom_config.data_base_pin;
    uint8_t logical = 0;
    for (uint8_t b = 0; b < 8; b++) {
        uint8_t pin = sdrr_info.pins->data[b];
        if (pin < MAX_USED_GPIOS) {
            if (physical_data & (1u << (pin - base))) {
                logical |= (1u << b);
            }
        }
    }
    return logical;
}

ora_result_t pio_init_knock(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint8_t data_size,
    ora_knock_t *knock
) {
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    if (knock_seq == NULL || knock == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (knock_len == 0 || knock_bits == 0 || knock_bits > piorom_config->num_addr_pins) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint8_t base = piorom_config->addr_base_pin;
    uint8_t pin;
    ora_result_t result;

    knock->mask = 0;
    for (uint8_t i = 0; i < knock_bits; i++) {
        result = get_addr_pin(i, &pin);
        if (result != ORA_RESULT_OK) {
            return result;
        }
        knock->mask |= (1u << (pin - base));
    }

    for (uint8_t k = 0; k < knock_len; k++) {
        knock->matches[k] = 0;
        for (uint8_t i = 0; i < knock_bits; i++) {
            result = get_addr_pin(i, &pin);
            if (result != ORA_RESULT_OK) {
                return result;
            }
            if (knock_seq[k] & (1u << i)) {
                knock->matches[k] |= (1u << (pin - base));
            }
        }
    }

    // Calculate CS and X pin masks for filtering during knock detection and
    // payload collection
    uint32_t cs_mask = 0;
    uint32_t x_mask = 0;

    // TODO - handle non 2364 chip types with different CS pin arrangements
    uint8_t cs1_pin = sdrr_info.pins->cs1;
    if (cs1_pin < MAX_USED_GPIOS) {
        cs_mask = 1u << (cs1_pin - base);
    }
    if (!piorom_config->multi_rom_mode) {
        knock->multi_rom_mode = 0;
    } else {
        uint8_t x1_pin = sdrr_info.pins->x1;
        uint8_t x2_pin = sdrr_info.pins->x2;
        if (x1_pin < MAX_USED_GPIOS) {
            x_mask |= 1u << (x1_pin - base);
        }
        if (x2_pin < MAX_USED_GPIOS) {
            x_mask |= 1u << (x2_pin - base);
        }
        knock->multi_rom_mode = 1;
    }

    knock->len  = knock_len;
    knock->bits = knock_bits;
    knock->data_size = data_size;
    knock->cs_mask = cs_mask;
    knock->x_mask = x_mask;

    return ORA_RESULT_OK;
}

__attribute__((always_inline)) static inline uint8_t debounce(
    uint32_t entry,
    const ora_knock_t *knock
) {
    // Primary CS debouncing is now done in the address monitor PIO SM
    //if (!knock->multi_rom_mode) {
    //    if (knock->cs_mask && (entry & knock->cs_mask)) return 1;     // CS inactive - bit set (active low, not inverted)
    //} else {
    //    if (knock->cs_mask && !(entry & knock->cs_mask)) return 1;  // CS inactive - bit clear after inversion
    //}

    // So the only thing needed here is filtering if X pin(s) active
    if (knock->x_mask && (entry & knock->x_mask)) return 1;     // X pin active
    return 0;
}

// Written as a macro to allow multiple data sizes
#define KNOCK_DETECT_LOOP(TYPE) do {                                        \
    volatile TYPE *rp = (volatile TYPE *)read_ptr;                          \
    volatile TYPE *rb = (volatile TYPE *)ring_buf;                          \
    while (knock_pos < knock->len) {                                        \
        volatile TYPE *wp = (volatile TYPE *)                               \
            DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;                    \
        while (rp != wp) {                                                  \
            uint32_t entry = (uint32_t)*rp;                                 \
            if (++rp >= rb + ring_entries) rp = rb;                         \
            if (debounce_cs) {                                              \
                if (debounce(entry, knock)) continue;                       \
            }                                                               \
            if ((entry & knock->mask) == knock->matches[knock_pos]) {       \
                knock_pos++;                                                \
                if (knock_pos >= knock->len) break;                         \
            } else {                                                        \
                knock_pos = ((entry & knock->mask) == knock->matches[0])    \
                    ? 1 : 0;                                                \
            }                                                               \
        }                                                                   \
    }                                                                       \
    read_ptr = (volatile uint32_t *)rp;                                     \
} while (0)

#define PAYLOAD_COLLECT_LOOP(TYPE) do {                                     \
    volatile TYPE *rp = (volatile TYPE *)read_ptr;                          \
    volatile TYPE *rb = (volatile TYPE *)ring_buf;                          \
    while (payload_pos < payload_len) {                                     \
        volatile TYPE *wp = (volatile TYPE *)                               \
            DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;                    \
        while (rp != wp && payload_pos < payload_len) {                     \
            uint32_t entry = (uint32_t)*rp;                                 \
            if (++rp >= rb + ring_entries) rp = rb;                         \
            if (debounce_cs) {                                              \
                if (debounce(entry, knock)) continue;                       \
            }                                                               \
            payload_out[payload_pos++] = entry;                             \
        }                                                                   \
    }                                                                       \
    read_ptr = (volatile uint32_t *)rp;                                     \
} while (0)

ora_result_t pio_wait_for_knock(
    const ora_knock_t *knock,
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    uint32_t flags,
    uint32_t *payload_out,
    uint8_t payload_len,
    volatile uint32_t *start_pos,
    volatile uint32_t **next_read_out
) {
    // Discard any captures that occurred before we were called.  Do this first
    // to avoid missing bytes, even before testing for a start_pos.
    volatile uint32_t *read_ptr = (volatile uint32_t *)DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;
    if (start_pos != NULL) {
        // We have a start_pos so use that instead.
        read_ptr = start_pos;
    }

    // Next check the args
    if (knock == NULL || ring_buf == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (payload_len > 0 && payload_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint32_t ring_entries = 1u << ring_entries_log2;
    uint8_t debounce_cs = (flags & ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS) != 0;

    // Knock detection loop
    uint8_t knock_pos = 0;
    switch (knock->data_size) {
        case 8:  KNOCK_DETECT_LOOP(uint8_t);  break;
        case 16: KNOCK_DETECT_LOOP(uint16_t); break;
        default: KNOCK_DETECT_LOOP(uint32_t); break;
    }

    // Payload collection
    uint8_t payload_pos = 0;
    switch (knock->data_size) {
        case 8:  PAYLOAD_COLLECT_LOOP(uint8_t);  break;
        case 16: PAYLOAD_COLLECT_LOOP(uint16_t); break;
        default: PAYLOAD_COLLECT_LOOP(uint32_t); break;
    }

    if (next_read_out != NULL) {
        *next_read_out = read_ptr;
    }
    return ORA_RESULT_OK;
}

ora_result_t pio_reprogram_ram_rom_slot(
    uint8_t slot,
    uint32_t offset,
    const uint8_t *data,
    uint32_t len,
    uint8_t allow_active
) {
    if (data == NULL || len == 0) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Get the SRAM address and size of the target slot
    uint32_t addr, size;
    ora_result_t result = ora_get_ram_slot_info(slot, &addr, &size, NULL);
    if (result != ORA_RESULT_OK) {
        return result;
    }

    // Check the write stays within the slot
    if (offset + len > size) {
        return ORA_RESULT_INVALID_ARG;
    }

    // If allow_active is not set, refuse to write to the currently active slot
    if (!allow_active) {
        uint8_t active_slot;
        result = ora_get_active_ram_slot(&active_slot);
        if (result == ORA_RESULT_OK && active_slot == slot) {
            return ORA_RESULT_SLOT_ACTIVE;
        }
    }

    // Remap logical addresses and data bytes to their physical representations
    // and write to the target slot in SRAM
    uint8_t *sram = (uint8_t *)addr;
    for (uint32_t i = 0; i < len; i++) {
        uint32_t physical_addr = pio_map_addr_to_phys(offset + i);
        uint8_t  physical_data = pio_map_data_to_phys(data[i]);
        sram[physical_addr] = physical_data;
    }

    return ORA_RESULT_OK;
}

ora_result_t pio_start_address_monitor(void) {
    APIO_ENABLE_SMS(BLOCK_MONITOR, ((1 << SM_ADDR_MONITOR_CS_MONITOR) | (1 << SM_ADDR_MONITOR_ADDR_READ)));

    return ORA_RESULT_OK;
}

volatile uint32_t * volatile *pio_get_address_monitor_ring_write_pos(void) {
    return (volatile uint32_t * volatile *)&DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;
}

uint8_t pio_get_effective_addr_pins(void) {
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint8_t effective_addr_pins = piorom_config->num_addr_pins;
    if (piorom_config->bit_mode == BIT_MODE_16) {
        effective_addr_pins += 1;
    }
    return effective_addr_pins;
}

uint32_t pio_get_rom_region_size(void) {
    return 1u << pio_get_effective_addr_pins();
}

ora_result_t pio_switch_rom_region(uint32_t new_region_addr) {
    // Input validation is the caller's responsibility. ora_set_active_ram_slot
    // validates the slot index and derives a correct address via
    // ora_get_ram_slot_info before calling this function.
    uint8_t effective_addr_pins = pio_get_effective_addr_pins();
    uint8_t rom_table_num_addr_bits = 32 - effective_addr_pins;
    uint32_t high_bits_mask = (1u << rom_table_num_addr_bits) - 1;
    uint32_t rom_table_high_bits = (new_region_addr >> effective_addr_pins) & high_bits_mask;

    // Update the ROM table address in the config to keep it consistent with
    // reality.
    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    piorom_config->rom_table_addr = new_region_addr;

    // Avoid unused variable warnings from APIO implementation causing
    // compile errors.
    // Update the X register in the address read SM with the new RAM table
    // base.  This delays the address read SM by a single cycle, but is an
    // atomic switch.
    APIO_ASM_INIT();
    APIO_SET_BLOCK(BLOCK_ADDR);
    APIO_SET_SM(SM_ADDR_READ);
    APIO_TXF = rom_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);

    // This is the point at which the SRAM region switch takes effect.
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);

    return ORA_RESULT_OK;
}

#endif // !TEST_BUILD

#endif // RP235X