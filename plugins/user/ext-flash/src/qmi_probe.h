// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(QMI_PROBE_H)
#define QMI_PROBE_H

#include <stdint.h>

// Brackets around each staged routine, placed by the plugin's linker script.
// A caller copies the bytes between a pair onto its stack and runs the routine
// from there.
extern const uint8_t __stage_probe_start[];
extern const uint8_t __stage_probe_end[];
extern const uint8_t __stage_qspi_start[];
extern const uint8_t __stage_qspi_end[];
extern const uint8_t __stage_op_start[];
extern const uint8_t __stage_op_end[];
extern const uint8_t __stage_m1_start[];
extern const uint8_t __stage_m1_end[];

// QMI registers.  RP2350 datasheet section 12.14.6 "List of registers".
// Only meaningful on the device.
#define QMI_BASE            0x400D0000u
#define QMI_DIRECT_CSR      (*(volatile uint32_t *)(QMI_BASE + 0x00u))
#define QMI_DIRECT_TX       (*(volatile uint32_t *)(QMI_BASE + 0x04u))
#define QMI_DIRECT_RX       (*(volatile uint32_t *)(QMI_BASE + 0x08u))
#define QMI_M0_TIMING       (*(volatile uint32_t *)(QMI_BASE + 0x0Cu))
#define QMI_M0_RFMT         (*(volatile uint32_t *)(QMI_BASE + 0x10u))
#define QMI_M0_RCMD         (*(volatile uint32_t *)(QMI_BASE + 0x14u))
#define QMI_M1_TIMING       (*(volatile uint32_t *)(QMI_BASE + 0x20u))
#define QMI_M1_RFMT         (*(volatile uint32_t *)(QMI_BASE + 0x24u))
#define QMI_M1_RCMD         (*(volatile uint32_t *)(QMI_BASE + 0x28u))

// Address translation for the first 4MB of chip select 1's window, and its
// reset value: 4MB of the device, starting at offset 0.  RP2350 datasheet
// section 12.14.6.  The bootrom's flash_reset_address_trans sets all eight
// ATRANS registers to this identity mapping, and launching an image rewrites
// ATRANS0-3 alone, which serve chip select 0.
#define QMI_ATRANS4         (*(volatile uint32_t *)(QMI_BASE + 0x44u))
#define QMI_ATRANS4_RESET   0x04000000u

// DIRECT_CSR bits.  RP2350 datasheet Table 1294.
#define QMI_CSR_EN          (1u << 0)
#define QMI_CSR_BUSY        (1u << 1)
#define QMI_CSR_ASSERT_CS0N (1u << 2)
#define QMI_CSR_ASSERT_CS1N (1u << 3)
#define QMI_CSR_TXFULL      (1u << 10)
#define QMI_CSR_RXEMPTY     (1u << 16)
#define QMI_CSR_CLKDIV_LSB  22
#define QMI_CSR_CLKDIV_BITS (0xFFu << QMI_CSR_CLKDIV_LSB)

// The clock divisor for this plugin's direct-mode transfers, the probe and the
// program.  It is the bootrom's BOOTROM_SPI_CLKDIV_DEFAULT, which the bootrom's
// own flash routines use, so these run at the speed its erases do.
//
// DIRECT_CSR otherwise holds whatever divisor the last change of XIP read mode
// left there, which is the XIP divisor.  At a 266MHz system clock that runs
// the bus at 133MHz, and direct-mode reads came back one bit late.
#define QMI_DIRECT_CLKDIV   12u

// GPIO and pad registers for the CS1 pin.  Bases from RP2350 datasheet section
// 2.2 "Address map", layouts from section 9.11, which lists the registers of
// both blocks: each GPIO has a STATUS and a CTRL word, and each pad one word
// after VOLTAGE_SELECT.  Pad bits are in section 9.6, and the isolation latch
// in section 9.7.
#define IO_BANK0_BASE       0x40028000u
#define PADS_BANK0_BASE     0x40038000u
#define GPIO_CTRL(pin)      (*(volatile uint32_t *)(IO_BANK0_BASE + 0x04u + (pin) * 0x08u))
#define GPIO_PAD(pin)       (*(volatile uint32_t *)(PADS_BANK0_BASE + 0x04u + (pin) * 0x04u))
#define PAD_IE              (1u << 6)
#define PAD_OD              (1u << 7)
#define PAD_ISO             (1u << 8)

// GPIO function select for the QMI's second chip select.  RP2350 datasheet
// table 3 "GPIO Bank 0 Functions": QMI CS1n is F9, on GPIOs 0, 8, 19 and 47.
#define GPIO_FUNC_QMI_CS1N  9u

// Base of the uncached view of the chip select 1 window.  RP2350 datasheet
// section 12.14: each chip select gets a 16MB window, chip select 0 from
// 0x10000000 and chip select 1 from 0x11000000 (ORA_FLASH_CS1_BASE_ADDR), with
// the QMI picking the matching timing and format by address decode.  Section
// 4.4.1 lists the mirrors of that space, 0x10 cached and 0x14 uncached, so the
// uncached view of window 1 starts at 0x15000000.  Read a block through the
// uncached view - a bulk read through the cached one evicts the running code
// from the 16KB XIP cache the two chip selects share.
#define XIP_CS1_NOCACHE     0x15000000u

// Serial flash commands.  Winbond W25Q16JV datasheet section 8.1 "Instruction
// Set".  Both devices here are that part: the RP2354's internal die is a
// W25Q16JVWI (RP2350 datasheet section 14.3) and the external one a W25Q16JVSS.
#define FLASH_CMD_JEDEC_ID  0x9Fu
#define FLASH_CMD_READ_SR1  0x05u
#define FLASH_CMD_READ_SR2  0x35u
#define FLASH_CMD_WRITE_EN  0x06u
#define FLASH_CMD_PAGE_PROG 0x02u

// Busy bit in status register 1, set while an erase or program is running.
#define FLASH_SR1_BUSY      (1u << 0)

// Erase and program sizes.  Winbond W25Q16JV datasheet section 6: 4KB is the
// smallest erase, 64KB the D8h block erase and 256 bytes the largest program.
#define FLASH_SECTOR_SIZE   4096u
#define FLASH_BLOCK_SIZE    65536u
#define FLASH_PAGE_SIZE     256u

// The QE bit in status register 2.  Winbond W25Q16JV datasheet section 7.1.
// Set, IO2 and IO3 are the data lines a quad read needs.  Clear, they are
// write-protect and hold.
#define FLASH_SR2_QE        (1u << 1)

// One device's ID and status register 2.
typedef struct {
    uint8_t jedec[3];  // Manufacturer, memory type, capacity
    uint8_t sr2;       // Status register 2, holding the QE bit
} qmi_device_id_t;

// The probe's result for both devices, read in one pass.  The built-in flash
// is always present, so it is the control.  If it doesn't return an ID, the
// probe is at fault and its result for the external flash can't be trusted.
typedef struct {
    qmi_device_id_t cs0;
    qmi_device_id_t cs1;
} qmi_probe_result_t;

// Read the JEDEC ID and status register 2 from both chip selects.
//
// Runs from RAM.  Direct mode makes every memory-mapped access return a bus
// error.  While it is enabled this core can't fetch from flash or take an
// interrupt.  The caller makes sure of both: exclusive mode parks the other
// core, and it masks this core's interrupts around the call.
//
// Leaves DIRECT_CSR as it found it.  It issues read commands alone, so both
// devices stay in the serial command state the bootrom left them in and XIP
// resumes on the existing M0 and M1 configuration.
//
void qmi_probe_ids(qmi_probe_result_t *out);
typedef void (*qmi_probe_ids_fn_t)(qmi_probe_result_t *out);

// Window 1 read format for a 0Bh fast read: single width throughout, an 8-bit
// opcode prefix and the 8 dummy cycles 0Bh specifies between address and data.
//
// Field layout from RP2350 datasheet Table 1298 (M0_RFMT, M1_RFMT).  DUMMY_LEN
// at 18:16 counts in units of 4 bits, and single width moves 4 bits per 4 SCK
// cycles, so 2 is those 8 cycles.  PREFIX_LEN at bit 12 set to 1 is the
// 8-bit opcode.  The width fields and SUFFIX_LEN stay zero, for single width
// without a suffix.
//
// 0Bh works on an unconfigured device, as it needs neither the QE bit nor
// continuous read mode.  It uses IO0 and IO1 alone.
#define M1_RFMT_0BH_SERIAL  ((2u << 16) | (1u << 12))
#define M1_RCMD_0BH_SERIAL  0x0Bu

// Window 1 read format for an EBh quad I/O read, the bootrom's own quad mode
// (BOOTROM_XIP_MODE_EBH_QUAD in its varm_generic_flash.c).
//
// The opcode goes out single width.  The address, the mode byte and the data
// use all four lines.  Same table as above: ADDR_WIDTH at 3:2, SUFFIX_WIDTH at
// 5:4, DUMMY_WIDTH at 7:6 and DATA_WIDTH at 9:8 are all 2 for quad.
// SUFFIX_LEN at 15:14 set to 2 is the 8-bit mode byte, which RCMD's SUFFIX
// field at 15:8 leaves 00h, so the device doesn't enter continuous read mode.
// DUMMY_LEN 4 is 16 bits, 4 cycles at quad width.  With the mode byte's 2
// cycles that is the 6 cycles the Winbond W25Q16JV datasheet requires between
// EBh's address and data.
#define M1_RFMT_EBH_QUAD    ((4u << 16) | (2u << 14) | (1u << 12) | \
                             (2u << 8) | (2u << 6) | (2u << 4) | (2u << 2))
#define M1_RCMD_EBH_QUAD    0xEBu

// Point memory window 1 at an arbitrary read configuration.
//
// The test reads the same bytes back in more than one read format.
//
// Runs from RAM.  RP2350 datasheet Table 1297 allows the divisor to change at
// any time, and requires the QMI be idle for every other field.
void qmi_set_m1(uint32_t timing, uint32_t rfmt, uint32_t rcmd);
typedef void (*qmi_set_m1_fn_t)(uint32_t timing, uint32_t rfmt, uint32_t rcmd);

// Bootrom flash routines, looked up by their two-character codes.
typedef void (*connect_internal_flash_fn_t)(void);
typedef void (*flash_exit_xip_fn_t)(void);
typedef void (*flash_flush_cache_fn_t)(void);
typedef void (*flash_select_xip_read_mode_fn_t)(uint8_t mode, uint8_t clkdiv);
typedef int32_t (*flash_op_fn_t)(uint32_t flags, uint32_t addr, uint32_t size, uint8_t *buf);

// flash_op flag values, RP2350 datasheet section 5.4.8.9.
#define CFLASH_ASPACE_STORAGE  0x00000000u
#define CFLASH_SECLEVEL_SECURE 0x00000100u
#define CFLASH_OP_ERASE        0x00000000u

// Bootrom table lookup flag for a data entry rather than a function.  RP2350
// datasheet section 5.4.1 "Locating the API functions" describes the lookup
// helper and its flags.
#define ORA_BOOTROM_FLAG_DATA 0x0040u

// The 32-bit word the test programs at a word-aligned offset into the chip.
//
// Each word is its own offset scrambled by a multiply, so the bits change from
// word to word on every data line.  The multiplier is odd, so two offsets
// never produce the same word, and data at the wrong address reads back wrong.
static inline uint32_t ext_flash_word(uint32_t offset) {
    return ((offset >> 2) * 0x9E3779B1u) ^ 0x5A5A5A5Au;
}

// The byte the test programs at an offset into the chip.  A word's bytes run
// from its least significant, as a 32-bit read of the chip returns them.
static inline uint8_t ext_flash_pattern(uint32_t offset) {
    return (uint8_t)(ext_flash_word(offset & ~3u) >> ((offset & 3u) * 8u));
}

// Run one bootrom flash_op with XIP taken down around it.
//
// addr is in the address space that spans both chip selects, so chip select 1
// starts at 0x11000000.  An erase of a 64KB-aligned 64KB region goes out as a
// D8h block erase when FLASH_DEVINFO's D8H_ERASE_SUPPORTED bit is set, and as
// 4KB sector erases otherwise (the bootrom's varm_checked_flash.c).
//
// Runs from RAM, under the same two conditions as qmi_probe_ids: exclusive
// mode held, this core's interrupts masked.
//
// buf must be in RAM, or NULL for an erase.  It is read while flash is
// unreadable, so a pointer into the plugin's own image would fault.
//
// mode and clkdiv go to the XIP restore at the end, and should be what window 0
// was using beforehand.  That restore reprograms both windows, so a caller
// wanting window 1 set differently does it after this returns.
void ext_flash_op_critical(
    connect_internal_flash_fn_t     connect_internal_flash,
    flash_exit_xip_fn_t             exit_xip,
    flash_op_fn_t                   flash_op,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    uint32_t                        flags,
    uint32_t                        addr,
    uint32_t                        size,
    uint8_t                        *buf,
    uint8_t                         mode,
    uint8_t                         clkdiv,
    int32_t                        *result_out
);
typedef void (*ext_flash_op_critical_fn_t)(
    connect_internal_flash_fn_t     connect_internal_flash,
    flash_exit_xip_fn_t             exit_xip,
    flash_op_fn_t                   flash_op,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    uint32_t                        flags,
    uint32_t                        addr,
    uint32_t                        size,
    uint8_t                        *buf,
    uint8_t                         mode,
    uint8_t                         clkdiv,
    int32_t                        *result_out
);


// Program `pages` consecutive pages from `offset` with the test's data, over
// the QSPI bus directly.
//
// offset is a page-aligned byte offset into the device connected to chip
// select 1.  Erase the pages first, since programming only clears bits.  Each
// byte is generated as it is clocked out, so this doesn't need a page buffer.
//
// Command sequence from the Winbond W25Q16JV datasheet section 8.2: write
// enable, then page program with a 24-bit address and up to 256 data bytes,
// then poll status register 1 until the busy bit clears.  Write enable is its
// own transfer because the device latches it on the rising edge of chip select.
//
// Same conditions as qmi_probe_ids: exclusive mode held, interrupts masked.
void qmi_cs1_program(uint32_t offset, uint32_t pages);
typedef void (*qmi_cs1_program_fn_t)(uint32_t offset, uint32_t pages);

#endif // QMI_PROBE_H
