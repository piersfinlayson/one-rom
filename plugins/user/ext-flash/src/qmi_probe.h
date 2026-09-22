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
extern const uint8_t __stage_bootprog_start[];
extern const uint8_t __stage_bootprog_end[];
extern const uint8_t __stage_exec_start[];
extern const uint8_t __stage_exec_end[];
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

// DIRECT_CSR bits.  RP2350 datasheet Table 1294.
#define QMI_CSR_EN          (1u << 0)
#define QMI_CSR_BUSY        (1u << 1)
#define QMI_CSR_ASSERT_CS0N (1u << 2)
#define QMI_CSR_ASSERT_CS1N (1u << 3)
#define QMI_CSR_TXFULL      (1u << 10)
#define QMI_CSR_RXEMPTY     (1u << 16)
#define QMI_CSR_CLKDIV_LSB  22

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

// Base of the cached and uncached views of the chip select 1 window.  RP2350
// datasheet section 12.14: each chip select gets a 16MB window, chip select 0
// from 0x10000000 and chip select 1 from 0x11000000, with the QMI picking the
// matching timing and format by address decode.  Section 4.4.1 gives the
// mirrors of that space, 0x10 cached and 0x14 uncached, so the uncached view of
// window 1 starts at 0x15000000.  Read a block through the uncached view - a
// bulk read through the cached one evicts the running code from the 16KB XIP
// cache the two chip selects share.
#define XIP_CS1_BASE        0x11000000u
#define XIP_CS1_NOCACHE     0x15000000u

// Serial flash commands.  Winbond W25Q16JV datasheet section 8.1 "Instruction
// Set".  Both devices here are that part: the RP2354's internal die is a
// W25Q16JVWI (RP2350 datasheet section 14.3) and the external one a W25Q16JVSS.
#define FLASH_CMD_JEDEC_ID  0x9Fu
#define FLASH_CMD_READ_SR1  0x05u
#define FLASH_CMD_READ_SR2  0x35u
#define FLASH_CMD_WRITE_EN  0x06u
#define FLASH_CMD_SECTOR_ER 0x20u
#define FLASH_CMD_PAGE_PROG 0x02u

// Busy bit in status register 1, set while an erase or program is running.
#define FLASH_SR1_BUSY      (1u << 0)

// Geometry of the region the write test uses.  Winbond W25Q16JV datasheet
// section 6: 4KB is the smallest erase and 256 bytes the largest program.
#define FLASH_SECTOR_SIZE   4096u
#define FLASH_PAGE_SIZE     256u

// The QE bit in status register 2.  Winbond W25Q16JV datasheet section 7.1.
// Set, IO2 and IO3 are the data lines a quad read needs.  Clear, they carry
// write-protect and hold.
#define FLASH_SR2_QE        (1u << 1)

// What one device answered.
typedef struct {
    uint8_t jedec[3];  // Manufacturer, memory type, capacity
    uint8_t sr2;       // Status register 2, holding the QE bit
} qmi_device_id_t;

// What the probe found.  Both devices are read in one pass.  The internal
// device is known to be present, so it is the control: when it fails to answer
// the probe is at fault, and its reading of the external device is worthless.
typedef struct {
    qmi_device_id_t cs0;
    qmi_device_id_t cs1;
} qmi_probe_result_t;

// Read the JEDEC ID and status register 2 from both chip selects.
//
// Runs from RAM.  Direct mode makes every memory-mapped access return a bus
// error, so while it is enabled this core fetches nothing from flash and takes
// no interrupt.  The caller provides both: exclusive mode parks the other core,
// and it masks this core's interrupts around the call.
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
// cycles, so 2 gives those 8 cycles.  PREFIX_LEN at bit 12 set to 1 is the
// 8-bit opcode.  The width fields and SUFFIX_LEN stay zero, which is single
// width and no suffix.
//
// 0Bh works on a device nothing has configured, since it wants neither the QE
// bit nor continuous read mode.
#define M1_RFMT_0BH_SERIAL  ((2u << 16) | (1u << 12))
#define M1_RCMD_0BH_SERIAL  0x0Bu

// Point memory window 1 at an arbitrary read configuration.
//
// The write test reads the same bytes back through several configurations,
// including the one serving the internal device, which is how it answers
// whether the external part keeps up.
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
typedef void (*flash_range_program_fn_t)(uint32_t offs, const uint8_t *data, uint32_t count);
typedef void (*flash_range_erase_fn_t)(uint32_t offs, uint32_t count, uint32_t block_size, uint8_t block_cmd);

// flash_op flag values, RP2350 datasheet section 5.4.8.9.
#define CFLASH_ASPACE_STORAGE 0x00000000u
#define CFLASH_ASPACE_RUNTIME 0x00000001u
#define CFLASH_SECLEVEL_BOOTLDR 0x00000300u
#define CFLASH_SECLEVEL_SECURE 0x00000100u
#define CFLASH_OP_ERASE       0x00000000u
#define CFLASH_OP_PROGRAM     0x00010000u

// Bootrom table lookup flag for a data entry rather than a function.  RP2350
// datasheet section 5.4.1 "Locating the API functions", which gives the lookup
// helper and the flags that select what is being asked for.
#define ORA_BOOTROM_FLAG_DATA 0x0040u

// What ext_flash_canary returns.  Arbitrary, and unlikely to turn up by
// accident, so the value coming back is evidence the routine ran rather than
// evidence something happened to be in a register.
#define EXT_FLASH_CANARY_VALUE 0xC0DEF00Du

uint32_t ext_flash_canary(void);
typedef uint32_t (*ext_flash_canary_fn_t)(void);

// The byte the write test expects at offset i of its page.
//
// 197 is coprime with 256, so the sequence visits every value once.  The
// readback then sees transitions in every bit position, which is what shows up
// a read at a clock the device cannot keep up with.
static inline uint8_t ext_flash_pattern(uint32_t i) {
    return (uint8_t)((i * 197u) + 89u);
}

// The block erase flash_range_erase may use where a region is big enough.
// Winbond W25Q16JV datasheet section 8.2 gives D8h as the 64KB block erase.
// A region smaller than a block leaves it using 4KB sector erases instead.
#define FLASH_BLOCK_SIZE      65536u
#define FLASH_BLOCK_ERASE_CMD 0xD8u

// Run one bootrom flash erase with XIP taken down around it.
//
// Pass flash_op to erase through the high-level route, or leave it NULL and
// pass range_erase to use the low-level one.  addr is an address spanning both
// chip selects for the first, and an offset from the start of flash for the
// second.
//
// Runs from RAM, under the same two conditions as qmi_probe_ids: exclusive
// mode held, this core's interrupts masked.
//
// buf must be in RAM.  It is read while flash is unreadable, so a pointer into
// the plugin's own image would fault.
//
// mode and clkdiv go to the XIP restore at the end, and should be what window 0
// was using beforehand.  That restore reprograms both windows, so a caller
// wanting window 1 set differently does it after this returns.
void ext_flash_op_critical(
    connect_internal_flash_fn_t     connect_internal_flash,
    flash_exit_xip_fn_t             exit_xip,
    flash_op_fn_t                   flash_op,
    flash_range_erase_fn_t          range_erase,
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
    flash_range_erase_fn_t          range_erase,
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


// Program `pages` consecutive pages from `offset`, over the QSPI bus directly.
//
// offset is a byte offset into the device on chip select 1, page aligned.
// Erase the sector holding it first, since programming only clears bits.  Each
// byte is generated as it is clocked out, so this needs no page buffer.
//
// Command sequence from the Winbond W25Q16JV datasheet section 8.2: write
// enable, then page program with a 24-bit address and up to 256 data bytes,
// then poll status register 1 until the busy bit clears.  Write enable is its
// own transfer because the device latches it on the rising edge of chip select.
//
// Same conditions as qmi_probe_ids: exclusive mode held, interrupts masked.
void qmi_cs1_program_sector(uint32_t offset, uint32_t pages);
typedef void (*qmi_cs1_program_sector_fn_t)(uint32_t offset, uint32_t pages);

// Program `pages` consecutive pages through the bootrom.
//
// Pass flash_op to use the high-level route, or leave it NULL and pass
// range_program to use the low-level one.  addr is an address spanning both
// chip selects for the first, and an offset from the start of flash for the
// second.
//
// page must be a 256 byte buffer in RAM.  It is written and read while flash is
// unreadable, so a pointer into the plugin's own image would fault.
//
// Same conditions as qmi_probe_ids: exclusive mode held, interrupts masked.
void ext_flash_program_sector_bootrom(
    connect_internal_flash_fn_t     connect_internal_flash,
    flash_exit_xip_fn_t             exit_xip,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    flash_op_fn_t                   flash_op,
    flash_range_program_fn_t        range_program,
    uint32_t                        flags,
    uint32_t                        addr,
    uint8_t                        *page,
    uint32_t                        pages,
    uint8_t                         mode,
    uint8_t                         clkdiv,
    int32_t                        *result_out
);
typedef void (*ext_flash_program_sector_bootrom_fn_t)(
    connect_internal_flash_fn_t     connect_internal_flash,
    flash_exit_xip_fn_t             exit_xip,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    flash_op_fn_t                   flash_op,
    flash_range_program_fn_t        range_program,
    uint32_t                        flags,
    uint32_t                        addr,
    uint8_t                        *page,
    uint32_t                        pages,
    uint8_t                         mode,
    uint8_t                         clkdiv,
    int32_t                        *result_out
);


#endif // QMI_PROBE_H
