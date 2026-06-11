// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 PIO/DMA autonomous RAM serving support

#include "include.h"

#include "piodma/piodma.h"

// Some possible improvements and other thoughts:
//
// - Why bother triggering WRITE data and address readers when /W goes low?
//   Just run them 24x7 alongside the READ address reader.  The important
//   thing is only to trigger the DMA when /W goes high.  May need a slight
//   delay after triggering DMA to avoid it potentially firing again quickly,
//   particularly if /W is bouncy.  Would need an algorithm change to check /W
//   goes low before re-arming.  So, perhaps, on balance, it's best to stay
//   with a separate SM re-arming them.
//
// - PIO1 SM0 technically uses different criteria to re-arm than PIO1 SM1, and
//   PIO2 SM2 (EITHER /CE or /W going inactive, vs just /W going inactive).
//   It might be possible for this to cause a problem.
//
// - For the data IO handler, a single cycle test would be to keep 001 in Y
//   and test X against that (i.e. /CE /OE active, /W inactive).  This avoids
//   the need for JMP pin, and also the need to flip the sense of /W.
//   Hopefully can rationalise this with the ROM alg.
//
// - From the HM6116 datasheet "If the /CS low transition occurs
//   simultaneously with the /WE low transition or after the /WE transition,
//   output remains in a high impedance state".  So ignore /OE if /CE and /WE
//   go low at the same time, or /CE after /WE.  This probably complicated the
//   algorithm.
//
// - Idea from Peter Stansfeld to lose an instruction to lose and instruction
//   from the data pin direction handler.
//
//      5: mov pindirs, ~null
//      .wrap_target
//      1: mov x, pins
//      2: jmp x--, 0
//      3: jmp pin, 5
//      ;  4: jmp 0    ; this instruction can be lost
//      .start
//      0: mov pindirs, null
//      .wrap
//
//   With the above idea to use 001 in Y it would become
//
//      .start
//      0: mov y, 0b001  ; Maybe 0b100, not sure
//      1: mov pindirs, null
//      .wrap_target
//      2: mov x, pins
//      3: jmp x!=y 1
//      4: mov pindirs, ~null
//      .wrap
//
//   But it probably needs to become more complicated to handle /CS going low
//   before /W - see above.

// # Introduction
//
// This file contains a completely autonomous PIO and DMA based RAM serving
// implementation.  Once started, the PIO state machines and DMA channels
// serve RAM data for both reads and writes in response to external chip
// select, output enable, write enable and address lines without any further
// CPU intervention.
//
// Unlike a ROM chip, a RAM chip has a /W (Write Enable, active low) pin,
// which switches between READ (like a ROM) and WRITE (data is written to the
// device) modes.
//
// # Algorithm Summary
//
// The implementation uses six PIO state machines across three PIO blocks and
// four DMA channels, with the following overall operation:
//
// Direction Control:
// - PIO2 SM0 - Data Pin Direction Handler
//
// READ Path:
// - PIO1 SM0 - Address Reader
// - DMA0     - Address Forwarder
// - DMA1     - Data Byte Fetcher
// - PIO2 SM1 - Data Byte Writer
//
// WRITE Path:
// - PIO0 SM0 - Write Enable Detector
// - PIO1 SM1 - Address Reader
// - PIO2 SM2 - Data Byte Reader
// - DMA2     - Address Forwarder
// - DMA3     - Data Byte Writer
//
// PIO blocks:
// - PIO0 - Write Enable Handler
// - PIO1 - Address Handlers
// - PIO2 - Data Pin Handlers
//
// DMA channels:
// 0/1 - READ path
// 2/3 - WRITE path
//
//                         Data Direction Control
//                         ======================
//
//                        <--------------------------------------------
//                        |                                           ^
//   Loops continuously   |                                           |
//                        v  /OE AND /CE active        /W inactive    |
// PIO2_SM0 --------------+----------------------+-------------------->
//     ^                  |                      |    Set data pins
//     |       /OE OR /CE |            /W active |     to outputs
//     |         inactive |                      |
//     |                  v                      v
//     <------------------<-----------------------
//             Sets Data Pins to Inputs
//
//                        READ Path (Continuous Loop)
//                        ===========================
//
//   PIO1_SM0 Loops continuously
//
//    ---> PIO1_SM0 --+-----> DMA0 --------> DMA1 -------> PIO2_SM1
//    ^        ^      |        ^              ^                |
//    |        |      |        |              |                v
//    |    Read Addr  |  Forward Addr    Get Data Byte    Write Data Pins
//    |               |
//    |               v
//    <----------------
//
//                        WRITE Path (On /W Trigger)
//                        ==========================
//
//   PIO0_SM0 Loops continuously
//
//   /CE AND /W active
//         |
//         v               IRQ                  /W inactive
//     PIO0_SM0 ---------+--->--> PIO1_SM1 ---+-------------> DMA2
//         ^             |   ^        ^       |   Read Addr    |
//         |             |   |        |       |  via RX FIFO   |
//   ------>             |   |    Read Addr   |                |
//   ^                   |   |                v                | Forward
//   |                   |   <-----------------                | Address
//   |                   |                                     |
//   |                   |                                     |
//   |                   | IRQ                  /W inactive    v
//   |                   +--->--> PIO2_SM2 ---+-------------> DMA3
//   |                   |   ^        ^       |     Data       |
//   |                   |   |        |       |  via RX FIFO   |
//   |                   |   | Read Data Pins |                |
//   |                   |   |                v                |
//   |                   |   <-----------------                v
//   |                   |                                Store Data
//   |                   |                                  in RAM
//   |                   v
//   |                   <-------------------
//   |                   |                  ^
//   |       Re-arm      v                  |
//   <-------------------+------------------>
//    /CE OR /W inactive   /CE AND /W active
//
// (Diagrams not to scale)
//
// # Detailed Operation
//
// The detailed operation is as follows:
//
// ## Data Direction Control
//
// PIO2 SM0 - Data Pin Direction Handler
//  - Continuously monitors /CE, /OE and /W pins.
//  - Sets data pins to inputs when /CE inactive OR /OE inactive OR /W
//    active.
//  - Sets data pins to outputs only when /CE AND /OE active AND /W
//    inactive.
//
// ## READ Path
//
// PIO1 SM0 - Address Reader (READ)
//  - (One time - reads high bits of RAM table address from TX FIFO,
//    preloaded by CPU before starting.)
//  - Continuously reads address lines.
//  - Combines high RAM table address bits with current address pins.
//  - Pushes complete 32-bit RAM table lookup address to RX FIFO
//    (triggering DMA0).
//  - Loops continuously to serve next address with slight delay to
//    avoid overwhelming DMA chain.
//
// DMA0 - Address Forwarder (READ)
//  - Triggered by PIO1 SM0 RX FIFO using DREQ_PIO1_RX0.
//  - Reads 32-bit RAM table lookup address from PIO1 SM0 RX FIFO.
//  - Writes address into DMA1 READ_ADDR_TRIG register, re-arming DMA1.
//
// DMA1 - Data Byte Fetcher (READ)
//  - Triggered by DMA0 writing to READ_ADDR_TRIG.
//  - Reads RAM byte from address specified in READ_ADDR register.
//  - Writes byte into PIO2 SM1 TX FIFO.
//  - Waits to be re-triggered by DMA0.
//
// PIO2 SM1 - Data Byte Writer (READ)
//  - Waits for data byte in TX FIFO (from DMA1).
//  - When available, outputs byte on data pins.
//  - Loops back to wait for next byte.
//  - (Direction control handled separately by PIO2 SM0.)
//
// ## WRITE Path
//
// PIO0 SM0 - Write Enable Detector
//  - Continuously monitors /CE and /W pins.
//  - When both go low (write enabled), performs debounce check by
//    reading multiple times (PIORAM_WRITE_ACTIVE_CHECK_COUNT).
//  - Once confirmed low, triggers single IRQ to signal write operation to
//    trigger both address and data reader SMs.
//  - Waits for either /CE OR /W to go high before re-arming.  Has NOPs
//    inserted, to avoid potentially re-arming too quickly on bouncy /W
//    signals.
//
// PIO1 SM1 - Address Reader (WRITE)
//  - (One time - reads high bits of RAM table address from TX FIFO,
//    preloaded by CPU before starting.)
//  - Triggered by PIO0 SM0 IRQ, write enable detection (same IRQ as PIO2 SM2
//    data byte writer).
//  - Waits for IRQ from PIO0 SM0 (write enable detection).
//  - Loops reading address lines until /W goes high.
//  - When /W goes high, pushes last read address to RX FIFO (triggering
//    DMA2) and loops back to wait for next IRQ.
//  - Perfectly synchronised with PIO2 SM2 data byte writer to sample and
//    output at the same time.
//
// PIO2 SM2 - Data Byte Reader (WRITE)
//  - Triggered by PIO0_SM0 IRQ, write enable detection (same IRQ as PIO1 SM1
//    address reader).
//  - Waits for IRQ from PIO0 SM0 (write enable detection).
//  - Loops reading data pins until /W goes high.
//  - When /W goes high, pushes last read data byte to RX FIFO (for DMA3)
//    and loops back to wait for next IRQ.
//  - Synchronized with address reader to sample at same time.
//  - Perfectly synchronised with PIO1 SM1 address reader to sample and
//    output at the same time.
//
// DMA2 - Address Forwarder (WRITE)
//  - Triggered by PIO1 SM1 RX FIFO using DREQ_PIO1_RX1.
//  - Reads 32-bit RAM table address from PIO1 SM1 RX FIFO.
//  - Writes address into DMA3 WRITE_ADDR_TRIG register, triggereing DMA3.
//
// DMA3 - Data Byte Writer (WRITE)
//  - Triggered by DMA2 writing to WRITE_ADDR_TRIG.
//  - Reads data byte from PIO2 SM2 RX FIFO.
//  - Writes byte to RAM table at address specified by DMA2.
//  - Waits to be re-triggered.
//
// There are a number of hardware pre-requisites for this to work:
// - RP2350, not RP2040 (uses pindirs as mov destination and mov pins as
//   source with IN pin masking).
// - All /CE, /OE and /W pins must be readable by all PIOs (always true
//   for inputs on RP2350).
// - All Data lines must be connected to contiguous GPIOs.
// - All Address lines must be connected to contiguous GPIOs.
// - Address space limited to powers of two (typically 2KB for 6116).
//
// To minimize jitter:
// - DMA channels should have high AHB5 bus priority using BUS_PRIORITY.
// - Avoid other SRAM access to banks containing RAM table.
// - These DMAs should have higher priority than others if present.
// - Minimize peripheral access on AHB5 during operation.
//
// # PIO Allocation
//
// There are a number of constraints over PIO allocation:
// - There are 3 PIO blocks total.
// - Each PIO block has 4 state machines.
// - Only one PIO block can control specific pin outputs.
//
// We have these requirements:
// - The only pins which need output control are the data pins.
// - We need 6 PIOs total.
// - 2 PIOs need to control data pin outputs (one to write data, one to set
//   to inputs/outputs).
//
// The PIO assignment was chosen to logically split the functionality, while
// meeting the above constraints.  There are other ways it could have been
// arranged - in particular it would be possible to collapse PIO blocks 0 and
// 1 together (or even 0 and 2 together), freeing up a whole PIO block for
// other uses if necessary.

//
// Config options
//

// Number of checks to confirm /W is active.  Can we used to debounce noisy /W
// signals, or brief /W low glitches.
#define PIORAM_WRITE_ACTIVE_CHECK_MAX 8  // Too high and we'll run out of instructions
#define PIORAM_WRITE_ACTIVE_CHECK_MIN 1
#ifndef PIORAM_WRITE_ACTIVE_CHECK_COUNT
#define PIORAM_WRITE_ACTIVE_CHECK_COUNT 2
#endif // PIORAM_WRITE_ACTIVE_CHECK_COUNT

#ifndef PIORAM_WRITE_TRIGGER_IRQ_DELAY
// Number of cycles to delay after triggering RAM WRITE IRQ before checking
// whether /W has gone high.  This provides time for the data and address
// reader SMs to get into a state where they can check /W as well.
#define PIORAM_WRITE_TRIGGER_IRQ_DELAY 4
#endif // PIORAM_WRITE_TRIGGER_IRQ_DELAY

// The IRQ number used to trigger RAM WRITE handling.  The PIO block used for
// this IRQ is the PIO block where the Data read handler SM is located (i.e.
// the SM that triggers the IRQ when /W goes low).
#define RAM_WRITE_TRIGGER_IRQ  3

// Configuration structure for PIO RAM serving
typedef struct pioram_config {
    // CS pin configuration for READ (/CE and /OE)
    uint8_t read_cs_base_pin;
    uint8_t num_read_cs_pins;  // Should be 2 for 6116

    // CS pin configuration for WRITE (/CE and /W)
    uint8_t write_cs_base_pin;
    uint8_t num_write_cs_pins;  // Should be 2 for 6116

    // /W pin number
    uint8_t write_pin;
    uint8_t pad0[3];
    
    // Data pins (Q0-Q7)
    uint8_t data_base_pin;
    uint8_t num_data_pins;  // 8 for 6116
    
    // Address pins (A0-A10)
    uint8_t addr_base_pin;
    uint8_t num_addr_pins;  // 11 for 6116 (2KB)
    
    // RAM table base address in SRAM
    uint32_t ram_table_addr;

    // Clock dividers for each SM
    uint16_t data_read_handler_clkdiv_int;
    uint8_t data_read_handler_clkdiv_frac;
    uint8_t pad1;
    
    uint16_t addr_reader_read_clkdiv_int;
    uint8_t addr_reader_read_clkdiv_frac;
    uint8_t pad2;
    
    uint16_t addr_reader_write_clkdiv_int;
    uint8_t addr_reader_write_clkdiv_frac;
    uint8_t pad3;

    uint16_t data_io_clkdiv_int;
    uint8_t data_io_clkdiv_frac;
    uint8_t pad4;
    
    uint16_t data_out_clkdiv_int;
    uint8_t data_out_clkdiv_frac;
    uint8_t pad5;
    
    uint16_t data_in_clkdiv_int;
    uint8_t data_in_clkdiv_frac;
    uint8_t pad6;
} pioram_config_t;

// Function prototypes
static void pioram_load_programs(pioram_config_t *config);
static void pioram_setup_dma(pioram_config_t *config);
static void pioram_set_gpio_func(pioram_config_t *config);
static void pioram_start_pios(void);

// Build and load the PIO programs for RAM serving
//
// Uses the single-pass PIO assembler macros from pioasm.h
static void pioram_load_programs(pioram_config_t *config) {
    // Get the high X bits of the RAM table address for preloading into the
    // address reader SMs.
    uint8_t ram_table_num_addr_bits = 32 - config->num_addr_pins;
    uint32_t high_bits_mask = (1 << ram_table_num_addr_bits) - 1;
    uint32_t low_bits_mask = (1 << config->num_addr_pins) - 1;
    uint32_t __attribute__((unused)) alignment_size = (1 << config->num_addr_pins) / 1024;
    DEBUG("Checking RAM table address 0x%08X is %uKB aligned", config->ram_table_addr, alignment_size);
    DEBUG("High bits mask: 0x%08X, low bits mask: 0x%08X", high_bits_mask, low_bits_mask);
    if (config->ram_table_addr & low_bits_mask) {
        ERR("PIO RAM serving requires RAM table address to be %uKB aligned",
            alignment_size);
        limp_mode(LIMP_MODE_INVALID_CONFIG);
    }
    uint32_t ram_table_high_bits = (config->ram_table_addr >> config->num_addr_pins) & high_bits_mask;
    DEBUG("RAM table high %d bits: 0x%08X", ram_table_num_addr_bits, ram_table_high_bits);

#if defined(DEBUG_LOGGING)
    // Log other config values
    uint8_t read_cs_base_pin = config->read_cs_base_pin;
    uint8_t write_cs_base_pin = config->write_cs_base_pin;
    uint8_t num_read_cs_pins = config->num_read_cs_pins;
    uint8_t num_write_cs_pins = config->num_write_cs_pins;
    uint8_t write_pin = config->write_pin;
    uint8_t data_base_pin = config->data_base_pin;
    uint8_t num_data_pins = config->num_data_pins;
    uint8_t addr_base_pin = config->addr_base_pin;
    uint8_t num_addr_pins = config->num_addr_pins;
    uint16_t data_read_handler_clkdiv_int = config->data_read_handler_clkdiv_int;
    uint8_t data_read_handler_clkdiv_frac = config->data_read_handler_clkdiv_frac;
    uint16_t addr_reader_read_clkdiv_int = config->addr_reader_read_clkdiv_int;
    uint8_t addr_reader_read_clkdiv_frac = config->addr_reader_read_clkdiv_frac;
    uint16_t addr_reader_write_clkdiv_int = config->addr_reader_write_clkdiv_int;
    uint8_t addr_reader_write_clkdiv_frac = config->addr_reader_write_clkdiv_frac;
    uint16_t data_io_clkdiv_int = config->data_io_clkdiv_int;
    uint8_t data_io_clkdiv_frac = config->data_io_clkdiv_frac;
    uint16_t data_out_clkdiv_int = config->data_out_clkdiv_int;
    uint8_t data_out_clkdiv_frac = config->data_out_clkdiv_frac;
    uint16_t data_in_clkdiv_int = config->data_in_clkdiv_int;
    uint8_t data_in_clkdiv_frac = config->data_in_clkdiv_frac;
    DEBUG("PIO RAM Serving Config:");
    DEBUG("- /OE /CE pins: %d-%d", read_cs_base_pin, read_cs_base_pin + num_read_cs_pins - 1);
    DEBUG("- /CE /W pins: %d-%d", write_cs_base_pin, write_cs_base_pin + num_write_cs_pins - 1);
    DEBUG("- /W pin: %d", write_pin);
    DEBUG("- Data pins: %d-%d", data_base_pin, data_base_pin + num_data_pins - 1);
    DEBUG("- Address pins: %d-%d", addr_base_pin, addr_base_pin + num_addr_pins - 1);
    DEBUG("- Data Read Handler CLKDIV: %d.%02d", data_read_handler_clkdiv_int, data_read_handler_clkdiv_frac);
    DEBUG("- Addr Reader READ CLKDIV: %d.%02d", addr_reader_read_clkdiv_int, addr_reader_read_clkdiv_frac);
    DEBUG("- Addr Reader WRITE CLKDIV: %d.%02d", addr_reader_write_clkdiv_int, addr_reader_write_clkdiv_frac);
    DEBUG("- Data IO CLKDIV: %d.%02d", data_io_clkdiv_int, data_io_clkdiv_frac);
    DEBUG("- Data OUT CLKDIV: %d.%02d", data_out_clkdiv_int, data_out_clkdiv_frac);
    DEBUG("- Data IN CLKDIV: %d.%02d", data_in_clkdiv_int, data_in_clkdiv_frac);
#endif // DEBUG_LOGGGING

    // Set up the PIO assembler
    APIO_ASM_INIT();
    
    // Clear all PIO IRQs
    APIO_CLEAR_ALL_IRQS();

    // PIO0 Programs
    //
    // Combined data/address handlers
    APIO_SET_BLOCK(0);

    // SM0 - Data read handler - triggers data read chain on /CE and /W low
    //
    // Reads both /CE and /W together.  When both are low, triggers first the
    // WRITE address reader, then the data input reader.
    //
    // Re-arms once either /CE or /W goes high.
    APIO_SET_SM(0);

    APIO_LABEL_NEW(start_write_enabled_check);
    // This algorithm will check /CE and /W this number of times when it goes
    // low, to make sure it's really low.
    uint8_t data_read_check_count = PIORAM_WRITE_ACTIVE_CHECK_COUNT;
    if (data_read_check_count > PIORAM_WRITE_ACTIVE_CHECK_MAX) {
        data_read_check_count = PIORAM_WRITE_ACTIVE_CHECK_MAX;
        ERR("PIORAM WE ACTIVE CHECK COUNT too high, limiting to %d", PIORAM_WRITE_ACTIVE_CHECK_MAX);
    } else if (data_read_check_count < PIORAM_WRITE_ACTIVE_CHECK_MIN) {
        data_read_check_count = 1;
        ERR("PIORAM WE ACTIVE CHECK COUNT too low, setting to 1");
    }
    for (int ii = 0; ii < data_read_check_count; ii++) {
        // Read /CE and /W
        APIO_ADD_INSTR(APIO_MOV_X_PINS);
        
        // If either /CE or /W is high, check again
        APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(start_write_enabled_check)));
    }

    // Trigger RAM WRITE IRQ. Triggers both addr and data readers
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IRQ_SET(RAM_WRITE_TRIGGER_IRQ), PIORAM_WRITE_TRIGGER_IRQ_DELAY)); 

    // Wait for either /CE or /W to go high
    APIO_LABEL_NEW(check_write_disabled);
    APIO_ADD_INSTR(APIO_MOV_X_PINS);

    // If both /CE and /W still low, keep waiting, otherwise jump to start
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check_write_disabled)));

    // Set the various SM register values
    APIO_SM_CLKDIV_SET(
        config->data_read_handler_clkdiv_int,
        config->data_read_handler_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(config->num_write_cs_pins) |   // Reading /CE and /W
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(config->write_cs_base_pin)      // /CE and /W pins
    );

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Trigger Data and Address Reader (RAM WRITE)");

    //
    // PIO 0 - End of block
    //
    APIO_END_BLOCK();

    // PIO 1 Programs
    //
    // Address Readers
    APIO_SET_BLOCK(1);

    // PIO 1 - Address Readers
    // 
    // SM0 - Address Reader (RAM READ)
    //
    // Constantly serves bytes to the READ DMA chain
    APIO_SET_SM(0);

    // Preload high bits of RAM table address to X - done via TX FIFO before
    // starting as SET(X) only supports 5 bits.

    // Pull high bits from X
    APIO_ADD_INSTR(APIO_IN_X(ram_table_num_addr_bits));

    // Read address lines and push to RX FIFO, so READ DMA chain serves the
    // byte.  We add a delay after this, to avoid overloading the DMA chain.
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_PINS(config->num_addr_pins), 2));   // Autopush

    // SM configuration
    APIO_SM_CLKDIV_SET(
        config->addr_reader_read_clkdiv_int,
        config->addr_reader_read_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(config->num_addr_pins) |
        APIO_AUTOPUSH |          // Auto push when we hit threshold
        APIO_PUSH_THRESH(32) |   // Push when we have total of 32 bits (a full address)
        APIO_IN_SHIFTDIR_L |
        APIO_OUT_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(config->addr_base_pin)
    );

    // Preload the X register to the high bits of the RAM table address
    APIO_TXF = ram_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Address Reader (RAM READ)");

    // PIO1 - Address Readers
    //
    // SM1 - Address Reader (RAM WRITE)
    //
    // Wait for Data read handler to trigger via IRQ - this indicates /CE and
    // /W went low.
    //
    // Loop reading the address until /W goes high.
    //
    // When /W goes high, push the last read address to the RX FIFO.  This
    // triggers the WRITE DMA chain.
    //
    // The data reader SM is triggered at the same time (actually one cycle
    // later), runs independently , and similarly waits for /W to go high.  As
    // they are both started at around the same time, and take roughly the same
    // time to loop, the data to write should be in the WRITE DMA chain by the
    // time the DMA gets the address and writes the byte.
    APIO_SET_SM(1);

    // Preload high 16 bits of RAM table address to X - done via TX FIFO
    // before starting as SET(X) only supports 5 bits.

    // (SM does not start here.). Push combined RAM table address and lower
    // order address bits when /W goes high.
    APIO_LABEL_NEW(addr_write_valid);
    APIO_ADD_INSTR(APIO_PUSH_BLOCK);

    // Wait for address reader IRQ from Data read handler
    APIO_START();
    APIO_ADD_INSTR(APIO_WAIT_IRQ_HIGH_PREV(3));

    // Pull high bits from X
    APIO_WRAP_BOTTOM();
    APIO_ADD_INSTR(APIO_IN_X(ram_table_num_addr_bits));

    // Read address lines.
    APIO_ADD_INSTR(APIO_IN_PINS(config->num_addr_pins));

    // Jump when /W goes high
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(addr_write_valid)));

    // SM configuration
    APIO_SM_CLKDIV_SET(
        config->addr_reader_write_clkdiv_int,
        config->addr_reader_write_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(
        APIO_JMP_PIN(config->write_pin)
    );
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(config->num_addr_pins) |
        APIO_IN_SHIFTDIR_L |
        APIO_OUT_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(config->addr_base_pin)
    );

    // Preload the X register to the high bits of the RAM table address
    APIO_TXF = ram_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Address Reader (RAM WRITE)");

    //
    // PIO 1 - End of block
    //
    APIO_END_BLOCK();

    // PIO 2 Programs
    //
    // Data Handlers
    APIO_SET_BLOCK(2);

    // PIO 2 - Data Handlers
    //
    // SM0 - Data Input/Output handler
    //
    // Start by setting data pins to inputs
    APIO_SET_SM(0);
    APIO_LABEL_NEW(data_io_write_enabled);

    // Set data pins to inputs
    APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);

    // Test for /CE and /OE active
    APIO_WRAP_BOTTOM();
    APIO_ADD_INSTR(APIO_MOV_X_PINS);
    APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_START_LABEL()));    // /CE or /OE inactive.  Have to jump
                                                    // to start and set pins to inputs cos
                                                    // this part of the loop is also used
                                                    // when pins may already be outputs.

    // /CE and /OE low - both active.  Check /W state next
    APIO_LABEL_NEW_OFFSET(data_io_set_outputs, 2);           // Point to set data pins as outputs
    APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(data_io_set_outputs))); // /W disabled, do enable
    APIO_ADD_INSTR(APIO_JMP(APIO_LABEL(data_io_write_enabled)));   // /W enabled, don't enable
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);                    // Set data pins to outputs

    // Configure SM
    APIO_SM_CLKDIV_SET(
        config->data_io_clkdiv_int,
        config->data_io_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(
        APIO_JMP_PIN(config->write_pin)
    );
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(config->num_read_cs_pins) |    // /OE amd /CE
        APIO_IN_SHIFTDIR_L                           // Direction doesn't matter
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(config->read_cs_base_pin) |     // /OE and /CE
        APIO_OUT_COUNT(config->num_data_pins) |
        APIO_OUT_BASE(config->data_base_pin)
    );

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Data IO Handler");

    //
    // PIO2 - Data Handlers
    //
    // SM1 - Data output (RAM READ)
    //
    // Just waits until 8 bits are made available by the READ DMA chain, then
    // writes them to the data pin outputs (whether they are set to outputs
    // or not).
    APIO_SET_SM(1);
    APIO_ADD_INSTR(APIO_OUT_PINS(config->num_data_pins)); // Autopull, blocks until all bits available

    APIO_SM_CLKDIV_SET(
        config->data_out_clkdiv_int,
        config->data_out_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_OUT_SHIFTDIR_R |    // Writes LSB of OSR
        APIO_AUTOPULL |          // Auto pull when we hit threshold
        APIO_PULL_THRESH(config->num_data_pins)  // Pull when we have all data bits
    );
    APIO_SM_PINCTRL_SET(
        APIO_OUT_COUNT(config->num_data_pins) |
        APIO_OUT_BASE(config->data_base_pin)
    );

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Data Reader (RAM READ)");

    // PIO2 - Data Handlers
    //
    // SM2 - Data input (RAM WRITE)
    APIO_SET_SM(2);
    APIO_LABEL_NEW(data_in_valid);
    APIO_ADD_INSTR(APIO_PUSH_BLOCK);              // Push data to RX FIFO for DMA
    APIO_START();
    APIO_ADD_INSTR(APIO_WAIT_IRQ_HIGH_NEXT(3));   // Wait for RAM WRITE IRQ
    APIO_WRAP_BOTTOM();
    APIO_ADD_INSTR(APIO_NOP);                     // Synchronise with address reader which takes 2 cycles to read
    APIO_ADD_INSTR(APIO_MOV_ISR_PINS);            // Read at same time as address pins
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(data_in_valid)));   // Jump when /W goes high 

    APIO_SM_CLKDIV_SET(
        config->data_in_clkdiv_int,
        config->data_in_clkdiv_frac
    );
    APIO_SM_EXECCTRL_SET(
        APIO_JMP_PIN(config->write_pin)
    );
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(config->num_data_pins) |
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(config->data_base_pin)
    );

    // Jump to start and log
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Data Reader (RAM WRITE)");

    //
    // PIO 2 - End of block
    //
    APIO_END_BLOCK();
}

#if !defined(TEST_BUILD)
// Setup DMA channels for RAM serving
//
// See `dma.c` for notes on RP2350 DMA usage.
static void pioram_setup_dma(pioram_config_t *config) {
    volatile dma_ch_reg_t *dma_reg;
    
    //
    // READ Chain DMAs
    //
    
    // DMA0 - Address Forwarder (READ)
    dma_reg = DMA_CH_REG(0);
    dma_reg->read_addr = (uint32_t)&APIO1_SM_RXF(0);         // Read from RAM READ address reader RX FIFO
    dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR_TRIG(1);  // Write to DMA1 to re-arm it
    dma_reg->transfer_count = 0xffffffff;                   // Re-arm self
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |                                  // Enable DMA
        DMA_CTRL_TRIG_IRQ_QUIET |                           // No IRQs
        DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(1, 0)) |  // Triggered by RAM READ address reader RX FIFO
        DMA_CTRL_TRIG_DATA_SIZE_32BIT |                     // Read a 32-bit RAM READ target address
        DMA_CTRL_TRIG_CHAIN_TO(0);                          // Disable chaining
    
    // DMA1 - Data Fetcher (READ)
    dma_reg = DMA_CH_REG(1);
    dma_reg->read_addr = config->ram_table_addr;            // Placeholder value, written to by DMA0
    dma_reg->write_addr = (uint32_t)&APIO2_SM_TXF(1);        // Write to RAM READ data writer TX FIFO
    dma_reg->transfer_count = 1;                            // Run once, then require re-arming by DMA0 writing to ADDR_TRIG register
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |                                  // Enable DMA
        DMA_CTRL_TRIG_IRQ_QUIET |                           // No IRQs
        DMA_CTRL_TRIG_TREQ_SEL(DMA_CTRL_TRIG_TREQ_PERM) |   // Triggered by arming
        DMA_CTRL_TRIG_DATA_SIZE_8BIT |                      // Write 8-bit RAM READ data
        DMA_CTRL_TRIG_CHAIN_TO(0);                          // Disable chaining
    
    //
    // WRITE Chain DMAs
    //
    
    // DMA2 - Address Forwarder (WRITE)
    dma_reg = DMA_CH_REG(2);
    dma_reg->read_addr = (uint32_t)&APIO1_SM_RXF(1);         // Read from RAM WRITE address reader RX FIFO
    dma_reg->write_addr = (uint32_t)&DMA_CH_WRITE_ADDR_TRIG(3);  // Trigger DMA3 to store the data byte
    dma_reg->transfer_count = 0xffffffff;                   // Re-arm self
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |                                  // Enable DMA
        DMA_CTRL_TRIG_IRQ_QUIET |                           // No IRQs
        DMA_CTRL_TRIG_PRIORITY_HIGH |                       // High priority
        DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(1, 1)) |  // Triggered by RAM WRITE address reader RX FIFO
        DMA_CTRL_TRIG_DATA_SIZE_32BIT |                     // Read a 32-bit RAM WRITE target address
        DMA_CTRL_TRIG_CHAIN_TO(2);                          // Disable chaining
    
    // DMA3 - Data Writer (WRITE)
    dma_reg = DMA_CH_REG(3);
    dma_reg->read_addr = (uint32_t)&APIO2_SM_RXF(2);         // Read from RAM WRITE data reader RX FIFO
    dma_reg->write_addr = config->ram_table_addr;           // Placeholder, gets overwritten by DMA2
    dma_reg->transfer_count = 1;
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |                                  // Enable DMA
        DMA_CTRL_TRIG_IRQ_QUIET |                           // No IRQs
        DMA_CTRL_TRIG_PRIORITY_HIGH |                       // High priority
        DMA_CTRL_TRIG_DATA_SIZE_8BIT |                      // Store 8-bit RAM WRITE data
        DMA_CTRL_TRIG_TREQ_SEL(DMA_CTRL_TRIG_TREQ_PERM) |   // Triggered by arming
        DMA_CTRL_TRIG_CHAIN_TO(3);                          // Disable chaining
    
    // Set DMA high priority (over CPU access).  It would be possible 
    BUSCTRL_BUS_PRIORITY |=
        BUSCTRL_BUS_PRIORITY_DMA_R_BIT |
        BUSCTRL_BUS_PRIORITY_DMA_W_BIT;
}

// Set GPIOs to PIO function for RAM serving
static void pioram_set_gpio_func(pioram_config_t *config) {
    (void)config;

    // CS pins - not required, as always inputs, and all PIOs can access inputs
    // all the time
    // GPIO_CTRL(10) = GPIO_CTRL_FUNC_PIO2; // /OE
    // GPIO_CTRL(11) = GPIO_CTRL_FUNC_PIO2; // /CE
    // GPIO_CTRL(12) = GPIO_CTRL_FUNC_PIO2; // /W

    // Address pins - not required, as always inputs
    // for (int ii = 13; ii <= 23; ii++) {
    //     GPIO_CTRL(ii) = GPIO_CTRL_FUNC_PIO1;
    // }

    // Data pins
    for (int ii = 0; ii < 8; ii++) {
        GPIO_CTRL(ii) = GPIO_CTRL_FUNC_PIO2;
    }
}
#else // TEST_BUILD
static void pioram_setup_dma(pioram_config_t *config) {
    (void)config;
    STUB_LOG("pioram_setup_dma");
}
static void pioram_set_gpio_func(pioram_config_t *config) {
    (void)config;
    STUB_LOG("pioram_set_gpio_func");
}
#endif // !defined(TEST_BUILD)

// Start all PIO state machines
static void pioram_start_pios(void) {
    APIO_ENABLE_SMS(0, 0x1);  // Enable SM0
    APIO_ENABLE_SMS(1, 0x3);  // Enable SM0 and SM1
    APIO_ENABLE_SMS(2, 0x7);  // Enable SM0,
    DEBUG("RAM PIOs started");
}

// Extern RAM/ROM image start symbol from linker script.  Used because,
// currently main() does not provide the correct address to pioram().
extern uint32_t _ram_rom_image_start[];

// Top-level RAM serving function
int pioram(
    const onerom_info_t *info,
    onerom_runtime_info_t *runtime,
    uint32_t ram_table_addr
) {
    (void)info;
    (void)runtime;

    DEBUG("%s", log_divider);

    ram_table_addr = (uint32_t)(uintptr_t)_ram_rom_image_start;

    pioram_config_t config = {
        .read_cs_base_pin = 10,     // /OE + /CE, fire-24-d
        .num_read_cs_pins = 2,
        .write_cs_base_pin = 11,    // /CE + /W, fire-24-d
        .num_write_cs_pins = 2,
        .write_pin = 12,            // /W pin, fire-24-d
        .data_base_pin = 0,         // fire-24-d
        .num_data_pins = 8,
        .addr_base_pin = 13,        // fire-24-d
        .num_addr_pins = 11,        // 6116 has A0-A10
        .ram_table_addr = ram_table_addr,
        .data_read_handler_clkdiv_int = 1,
        .data_read_handler_clkdiv_frac = 0,
        .addr_reader_read_clkdiv_int = 1,
        .addr_reader_read_clkdiv_frac = 0,
        .addr_reader_write_clkdiv_int = 1,
        .addr_reader_write_clkdiv_frac = 0,
        .data_io_clkdiv_int = 1,
        .data_io_clkdiv_frac = 0,
        .data_out_clkdiv_int = 1,
        .data_out_clkdiv_frac = 0,
        .data_in_clkdiv_int = 1,
        .data_in_clkdiv_frac = 0,
    };
    
    // Bring PIOs and DMA out of reset
    APIO_ENABLE_PIOS();
    DMA_ENABLE();
    
    // Setup DMA channels
    pioram_setup_dma(&config);
    
    // Configure GPIOs
    pioram_set_gpio_func(&config);

    // Load PIO programs
    pioram_load_programs(&config);
    
    // Start PIOs
    pioram_start_pios();
    DEBUG("PIOs started");
    DEBUG("%s", log_divider);

#define PIO_DEBUG_LOOP 1
#if defined(PIO_DEBUG_LOOP)
    // Output PIO and DMA debug information periodically
    uint32_t last_read_addr = 0xFFFFFFFF;
    uint32_t last_write_addr = 0xFFFFFFFF;
    uint8_t read_addr_still_unchanged = 0;
    uint8_t write_addr_still_unchanged = 0;
    while (1) {
        // See if any PIO FIFOs are full
        uint32_t volatile pio_fstats[3] = {
            APIO0_FSTAT,
            APIO1_FSTAT,
            APIO2_FSTAT
        };
        for (int ii = 0; ii < 3; ii++) {
            uint32_t pio_fstat = pio_fstats[ii];
            for (int jj = 0; jj < 4; jj++) {
                uint8_t rxfull_bit = 0 + jj;
                uint8_t txfull_bit = 16 + jj;
                if (pio_fstat & (1 << rxfull_bit)) {
                    DEBUG("!!! PIO%d SM%d RXFULL set", ii, jj);
                }
                if (pio_fstat & (1 << txfull_bit)) {
                    DEBUG("!!! PIO%d SM%d TXFULL set", ii, jj);
                }
            }
        }

        // Check the DMA read/write RAM table addresses are changing.
        // The odd log here is acceptable - but constant unchanging read or
        // write addresses suggest a problem (for example, host has crashed).
        // As such we only log if at least the last three checks have been
        // the same.
        volatile dma_ch_reg_t *dma1 = DMA_CH_REG(1);
        volatile dma_ch_reg_t *dma3 = DMA_CH_REG(3);
        uint32_t new_read_addr = dma1->read_addr;
        uint32_t new_write_addr = dma3->write_addr;
        if (new_read_addr == last_read_addr) {
            if (read_addr_still_unchanged > 1) {
                DEBUG("!!! RAM READ address unchanged: 0x%08X", new_read_addr);
            }
            read_addr_still_unchanged++;
        } else {
            read_addr_still_unchanged = 0;
        }
        if (new_write_addr == last_write_addr) {
            if (write_addr_still_unchanged > 1) {
                DEBUG("!!! RAM WRITE address unchanged: 0x%08X", new_write_addr);
            }
            write_addr_still_unchanged++;
        } else {
            write_addr_still_unchanged = 0;
        }
        last_read_addr = new_read_addr;
        last_write_addr = new_write_addr;

        // Delay before next check
        #define PIO_DEBUG_LOOP_DELAY 1000000
        for (volatile int i = 0; i < PIO_DEBUG_LOOP_DELAY; i++);
    }
#endif // PIO_DEBUG_LOOP

    // Low power loop
    while (1) {
        APIO_ASM_WFI();
    }

    return 0;
}