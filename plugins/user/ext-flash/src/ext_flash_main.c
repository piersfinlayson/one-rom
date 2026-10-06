// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// One ROM user plugin that tests a board's external flash chip.
//
// Some Fire boards are populated with a 2MB flash chip connected to the QMI's
// second chip select, alongside the RP2354's built-in 2MB.  The plugin can be
// used to test the chip before the board is commissioned as size L.
//
// It checks the chip:
// - returns its ID when selected through the chip select GPIO in the board's
//   metadata
// - reports 2MB in its ID and is actually 2MB
// - can be programmed and read back using single-line and quad reads
// - can be erased with the D8h block erase and the 4KB sector erase
//
// If the board isn't commissioned as size L, the plugin sets the bootrom's RAM
// copy of FLASH_DEVINFO and muxes the chip select GPIO itself.
//
// Output goes to log channel 0 and input comes from channel 1.  The USB plugin
// connects both to `onerom console`.  The test starts when the user types y.
// The chip is erased at the end of a passing test.

#include "plugin.h"

#include "qmi_probe.h"
#include "recover.h"
#include "cdc_log.h"

#define VERSION_MAJOR 0u
#define VERSION_MINOR 1u
#define VERSION_PATCH 0u

ORA_DEFINE_USER_PLUGIN(
    ext_flash_main,
    VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH, 0,
    0, 8, 0         // Firmware 0.8.0 first shipped the FLASH_DEVINFO constants
);

// The size of an L board's external flash, from the FLASH_DEVINFO size field
// One ROM writes for it.  The bootrom reads a size field n as 4KB << n.
#define CHIP_SIZE (FLASH_SECTOR_SIZE << ORA_FLASH_SIZE_2MB)

// The watchdog timeout for one flash operation, twice the longest.  From the
// Winbond W25Q16JV datasheet, a 64KB block erase takes up to 2s and programming
// 64KB takes up to 256 x 3ms.
#define WD_TIMEOUT_MS 4000u

// The pause after each flash operation.  An operation parks the other core.
// That core runs the USB plugin, and the pause lets its USB traffic through.
#define SETTLE_MS 20u

// The number of quad reads of the whole chip.  A joint that only works some of
// the time can fail one read and pass another.
#define QUAD_READS 3u

// Firmware and bootrom routines and board details, looked up once at startup.
typedef struct {
    ora_enter_exclusive_mode_fn_t   enter;
    ora_exit_exclusive_mode_fn_t    exit;
    ora_yield_fn_t                  yield;
    ora_get_plugin_uptime_ms_fn_t   uptime;
    ora_log_read_fn_t               read;
    connect_internal_flash_fn_t     connect;
    flash_exit_xip_fn_t             exit_xip;
    flash_op_fn_t                   op;
    flash_flush_cache_fn_t          flush;
    flash_select_xip_read_mode_fn_t select_xip;
    uint32_t                        m0_timing;  // window 0 as the boot left it
    uint8_t                         mode;       // for the XIP restore
    uint8_t                         clkdiv;
    uint8_t                         gpio;       // chip select 1
    uint8_t                         sr2;        // external flash status register 2
} ctx_t;

static ctx_t s_ctx;

// Copy a staged routine into blob and return a pointer to run it from.
//
// fn is the routine's address in flash.  Its offset within the section is the
// same as its offset within the copy, so the functions in a section can be in
// any order.  Taking a Thumb function's address sets bit 0, so that comes off
// before the subtraction and goes back on afterwards.
//
// The link fails where a section is larger than the words the Makefile allows
// it, so the copy always fits blob.
static void *stage(
    uint32_t       *blob,
    const uint8_t  *start,
    const uint8_t  *end,
    const void     *fn
) {
    uint32_t len = ORA_STAGED_FN_SIZE(start, end);

    // Volatile destination so the compiler leaves this as a loop.  Recognised
    // as a memcpy pattern it becomes a library call, and the plugin environment
    // doesn't have a C runtime.
    const uint32_t *src = (const uint32_t *)(const void *)start;
    volatile uint32_t *dst = blob;
    for (uint32_t i = 0u; i < (len + 3u) / 4u; i++) {
        dst[i] = src[i];
    }

    uint32_t off = (uint32_t)((uintptr_t)fn & ~(uintptr_t)1u)
                 - (uint32_t)(uintptr_t)start;
    return (void *)((uintptr_t)blob + off);
}

// Mask this core's interrupts, and put them back the way they were.
//
// PRIMASK is saved and restored rather than enabled outright, so a masked
// region may sit inside another one.  Needed around every staged routine,
// since they make flash unreadable and every handler this core would run
// lives there.
static inline uint32_t irq_disable(void) {
    uint32_t primask;
    __asm volatile ("mrs %0, primask \n\t"
                    "cpsid i"
                    : "=r" (primask) :: "memory");
    return primask;
}

static inline void irq_restore(uint32_t primask) {
    __asm volatile ("msr primask, %0" :: "r" (primask) : "memory");
}

// A plugin's static RAM holds whatever was last in it when the plugin starts,
// so a static relying on zero initialisation starts with garbage.  The
// .ramfunc code is still in flash, not in the RAM it is linked to run from.
// Both are fixed here, before anything reads a static or calls into RAM.
//
// The destination pointers are volatile so the compiler doesn't turn the loops
// into memcpy and memset calls.  The plugin environment doesn't have those
// functions.
static void init_data_bss(void) {
    extern uint32_t __ramfunc_start;
    extern uint32_t __ramfunc_end;
    extern uint32_t __ramfunc_load;
    extern uint32_t __data_start;
    extern uint32_t __data_end;
    extern uint32_t __data_load;
    extern uint32_t __bss_start;
    extern uint32_t __bss_end;

    const uint32_t *src = &__ramfunc_load;
    volatile uint32_t *dst = &__ramfunc_start;
    while (dst < &__ramfunc_end) {
        *dst++ = *src++;
    }

    src = &__data_load;
    dst = &__data_start;
    while (dst < &__data_end) {
        *dst++ = *src++;
    }

    dst = &__bss_start;
    while (dst < &__bss_end) {
        *dst++ = 0;
    }
}

static void *lookup_boot(char a, char b, uint32_t flag) {
    uint32_t code = ((uint32_t)(uint8_t)b << 8) | (uint32_t)(uint8_t)a;
    return ORA_BOOTROM_LOOKUP(code, flag);
}

// Find the bootrom's RAM copy of FLASH_DEVINFO.
//
// At boot the bootrom copies OTP's FLASH_DEVINFO into it if OTP's
// FLASH_DEVINFO_ENABLE bit is set.  Otherwise it writes 0x0C00, which is a 16MB
// chip connected to chip select 0 and a size of 0 for chip select 1.  flash_op
// checks every operation against it, so erasing the chip connected to chip
// select 1 fails with NOT_PERMITTED until the copy sets chip select 1's size.
//
// RP2350 datasheet section 5.4.8.5 says the flash APIs use this copy, and that
// Secure code may update it at runtime through this pointer.  The OTP field,
// section 13.10 Table 1392, is the permanent version of the same value.
static volatile uint16_t *devinfo_ptr(void) {
    // The lookup helper at address 0x16 returns the value stored in the table
    // entry, and table entries are 16 bits wide (RP2350 datasheet section
    // 5.4.1).  A boot RAM address needs more than 16 bits, so the value
    // returned is the address of a bootrom word holding the real pointer.
    //
    // Getting this wrong doesn't show.  The first address is in ROM, so a
    // write through it does nothing, and the compiler folds the read that
    // follows into the value just stored, so the mistake reads back as a
    // success.
    uint16_t **devinfo_slot = lookup_boot('F', 'D', ORA_BOOTROM_FLAG_DATA);
    if (devinfo_slot == NULL) {
        return NULL;
    }
    return *devinfo_slot;
}

// FLASH_DEVINFO as `onerom hardware set-size` writes it for an L board with
// chip select 1 on GPIO gpio.
static uint16_t l_board_devinfo(uint8_t gpio) {
    return (uint16_t)((ORA_FLASH_SIZE_2MB << ORA_OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT)
                    | (ORA_FLASH_SIZE_2MB << ORA_OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT)
                    | ORA_OTP_FLASH_DEVINFO_D8H_ERASE_SUPPORTED
                    | (gpio & ORA_OTP_FLASH_DEVINFO_CS1_GPIO));
}

// Pause for SETTLE_MS, yielding so the other core can take exclusive mode.
static void settle(void) {
    uint32_t start = s_ctx.uptime();
    while ((s_ctx.uptime() - start) < SETTLE_MS) {
        (void)s_ctx.yield(NULL);
    }
}

// Point window 1 at a read configuration.  Runs staged, since every field bar
// the divisor may only change while the QMI is idle.
static __attribute__((noinline)) uint8_t set_m1(uint32_t rfmt, uint32_t rcmd) {
    if (s_ctx.enter() != ORA_RESULT_OK) {
        return 0u;
    }
    uint32_t blob[STAGE_M1_WORDS];
    qmi_set_m1_fn_t fn = ORA_STAGED_FN_PTR(qmi_set_m1_fn_t,
        (uint32_t)(uintptr_t)stage(blob, __stage_m1_start, __stage_m1_end,
                                   (const void *)&qmi_set_m1));
    uint32_t primask = irq_disable();
    fn(s_ctx.m0_timing, rfmt, rcmd);
    irq_restore(primask);
    s_ctx.exit();
    return 1u;
}

// Erase size bytes from offset in the external flash, through the bootrom.
//
// Returns flash_op's result, or -1 where exclusive mode was refused.
//
// The other core is parked and this core's interrupts are masked for the whole
// erase.  Serving continues on PIO and DMA out of SRAM.
static __attribute__((noinline)) int32_t erase(uint32_t offset, uint32_t size) {
    if (s_ctx.enter() != ORA_RESULT_OK) {
        return -1;
    }
    uint32_t blob[STAGE_OP_WORDS];
    ext_flash_op_critical_fn_t op = ORA_STAGED_FN_PTR(ext_flash_op_critical_fn_t,
        (uint32_t)(uintptr_t)stage(blob, __stage_op_start, __stage_op_end,
                                   (const void *)&ext_flash_op_critical));
    int32_t result;
    watchdog_arm(WD_TIMEOUT_MS);
    uint32_t primask = irq_disable();
    op(s_ctx.connect, s_ctx.exit_xip, s_ctx.op, s_ctx.flush, s_ctx.select_xip,
       CFLASH_ASPACE_STORAGE | CFLASH_SECLEVEL_SECURE | CFLASH_OP_ERASE,
       ORA_FLASH_CS1_BASE_ADDR + offset, size, NULL,
       s_ctx.mode, s_ctx.clkdiv, &result);
    irq_restore(primask);
    watchdog_disarm();
    s_ctx.exit();
    settle();
    return result;
}

// Program a 64KB block with the test's data, over the QSPI bus directly.
//
// Direct mode is used because the bootrom's flash_op needs a 256-byte page
// buffer, and the plugin's stack doesn't have room for it.  Programming doesn't
// report a result, and the readback that follows checks it.
static __attribute__((noinline)) uint8_t program_block(uint32_t offset) {
    if (s_ctx.enter() != ORA_RESULT_OK) {
        return 0u;
    }
    uint32_t blob[STAGE_QSPI_WORDS];
    qmi_cs1_program_fn_t prog = ORA_STAGED_FN_PTR(qmi_cs1_program_fn_t,
        (uint32_t)(uintptr_t)stage(blob, __stage_qspi_start, __stage_qspi_end,
                                   (const void *)&qmi_cs1_program));
    watchdog_arm(WD_TIMEOUT_MS);
    uint32_t primask = irq_disable();
    prog(offset, FLASH_BLOCK_SIZE / FLASH_PAGE_SIZE);
    irq_restore(primask);
    watchdog_disarm();
    s_ctx.exit();
    settle();
    return 1u;
}

// The bytes a comparison found wrong.
typedef struct {
    uint32_t bad;     // how many
    uint32_t offset;  // the first of them, as an offset into the chip
    uint8_t  got;
    uint8_t  want;
} mismatch_t;

// Compare len bytes from offset against the erased value, or against the
// test's data for expect, through whatever read configuration window 1 holds.
// expect differs from offset only where a read is meant to wrap.
//
// Adds to m, so several reads can share one report.  Reads a word at a time,
// through the uncached view.  The cache sits upstream of the per-window
// configuration, so a cached read after a reconfiguration could come from a
// line fetched under the previous one.
static void compare(uint32_t offset, uint32_t expect, uint32_t len, uint8_t erased,
                    mismatch_t *m) {
    const volatile uint32_t *p = (const volatile uint32_t *)(XIP_CS1_NOCACHE + offset);
    for (uint32_t i = 0u; i < len; i += 4u) {
        uint32_t want = erased ? 0xFFFFFFFFu : ext_flash_word(expect + i);
        uint32_t got = p[i / 4u];
        if (got == want) {
            continue;
        }
        for (uint32_t b = 0u; b < 4u; b++) {
            uint8_t got_byte  = (uint8_t)(got >> (b * 8u));
            uint8_t want_byte = (uint8_t)(want >> (b * 8u));
            if (got_byte != want_byte) {
                if (m->bad == 0u) {
                    m->offset = offset + i + b;
                    m->got    = got_byte;
                    m->want   = want_byte;
                }
                m->bad++;
            }
        }
    }
}

// Report a comparison's mismatches on its check's line.
static void report_mismatch(const char *check, const mismatch_t *m, uint8_t erased) {
    const char *s = (m->bad == 1u) ? "" : "s";
    if (erased) {
        cdc_log("%s: %u byte%s %s erased. The first at offset 0x%06X reads 0x%02X.",
                check, (unsigned)m->bad, s, (m->bad == 1u) ? "isn't" : "aren't",
                (unsigned)m->offset, (unsigned)m->got);
    } else {
        cdc_log("%s: %u byte%s wrong. The first at offset 0x%06X reads 0x%02X, expected 0x%02X.",
                check, (unsigned)m->bad, s,
                (unsigned)m->offset, (unsigned)m->got, (unsigned)m->want);
    }
}

// Erase a region and check it reads erased.  A failure is reported under
// check.
static uint8_t erase_and_check(const char *check, uint32_t offset, uint32_t size) {
    int32_t rc = erase(offset, size);
    if (rc != 0) {
        cdc_log("%s: the bootrom returned error %d erasing offset 0x%06X.",
                check, (int)rc, (unsigned)offset);
        return 0u;
    }
    if (!set_m1(M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL)) {
        cdc_log("%s: can't enter exclusive mode.", check);
        return 0u;
    }
    mismatch_t m = { 0u, 0u, 0u, 0u };
    compare(offset, offset, size, 1u, &m);
    if (m.bad != 0u) {
        report_mismatch(check, &m, 1u);
        return 0u;
    }
    return 1u;
}

// Check every byte reads erased, erasing first any block that doesn't.
//
// A new chip is erased, and so is the chip after a passing test, so this
// normally doesn't erase anything.  After a failed test the chip may still hold
// data.
static uint8_t check_blank(void) {
    if (!set_m1(M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL)) {
        cdc_log("Blank: can't enter exclusive mode.");
        return 0u;
    }
    for (uint32_t block = 0u; block < CHIP_SIZE; block += FLASH_BLOCK_SIZE) {
        mismatch_t m = { 0u, 0u, 0u, 0u };
        compare(block, block, FLASH_BLOCK_SIZE, 1u, &m);
        if ((m.bad != 0u) && !erase_and_check("Blank", block, FLASH_BLOCK_SIZE)) {
            return 0u;
        }
    }
    cdc_log("Blank: OK");
    return 1u;
}

// Program the whole chip, then read it back with single-line reads.
static uint8_t check_program(void) {
    for (uint32_t block = 0u; block < CHIP_SIZE; block += FLASH_BLOCK_SIZE) {
        if (!program_block(block)) {
            cdc_log("Program and read: can't enter exclusive mode.");
            return 0u;
        }
    }
    if (!set_m1(M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL)) {
        cdc_log("Program and read: can't enter exclusive mode.");
        return 0u;
    }
    mismatch_t m = { 0u, 0u, 0u, 0u };
    compare(0u, 0u, CHIP_SIZE, 0u, &m);
    if (m.bad != 0u) {
        report_mismatch("Program and read", &m, 0u);
        return 0u;
    }
    cdc_log("Program and read: OK");
    return 1u;
}

// Read the whole chip back with quad reads, QUAD_READS times.
static uint8_t check_quad(void) {
    if (!set_m1(M1_RFMT_EBH_QUAD, M1_RCMD_EBH_QUAD)) {
        cdc_log("Quad read: can't enter exclusive mode.");
        return 0u;
    }
    for (uint32_t read = 1u; read <= QUAD_READS; read++) {
        mismatch_t m = { 0u, 0u, 0u, 0u };
        compare(0u, 0u, CHIP_SIZE, 0u, &m);
        if (m.bad != 0u) {
            cdc_log("Quad read: %u byte%s wrong on read %u of %u. The first at offset 0x%06X reads 0x%02X, expected 0x%02X.",
                    (unsigned)m.bad, (m.bad == 1u) ? "" : "s",
                    (unsigned)read, (unsigned)QUAD_READS,
                    (unsigned)m.offset, (unsigned)m.got, (unsigned)m.want);
            if ((s_ctx.sr2 & FLASH_SR2_QE) == 0u) {
                cdc_log("The chip's QE bit is clear.");
            }
            return 0u;
        }
    }
    cdc_log("Quad read: OK");
    return 1u;
}

// Check the chip isn't larger than 2MB by where its addresses wrap.
//
// A serial flash ignores address bits above its size, so on a 2MB chip offset
// 2MB reads offset 0.  A smaller chip fails Program and read instead, because
// the top of the data overwrites the bottom.
//
// The read goes through window 1.  Window 1 reaches offset 2MB only while
// ATRANS4 holds its identity mapping.
static uint8_t check_size(void) {
    if (QMI_ATRANS4 != QMI_ATRANS4_RESET) {
        cdc_log("Size: can't check. QMI ATRANS4 is 0x%08X.", (unsigned)QMI_ATRANS4);
        return 0u;
    }
    if (!set_m1(M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL)) {
        cdc_log("Size: can't enter exclusive mode.");
        return 0u;
    }
    mismatch_t m = { 0u, 0u, 0u, 0u };
    compare(CHIP_SIZE, 0u, FLASH_SECTOR_SIZE, 0u, &m);
    if (m.bad != 0u) {
        cdc_log("Size: offset 0x%06X doesn't read the same data as offset 0x000000. The chip is larger than 2MB.",
                (unsigned)CHIP_SIZE);
        return 0u;
    }
    cdc_log("Size: OK");
    return 1u;
}

// Erase the first sector, and check the one after it still holds its data.
static uint8_t check_sector_erase(void) {
    if (!erase_and_check("Sector erase", 0u, FLASH_SECTOR_SIZE)) {
        return 0u;
    }
    mismatch_t m = { 0u, 0u, 0u, 0u };
    compare(FLASH_SECTOR_SIZE, FLASH_SECTOR_SIZE, FLASH_SECTOR_SIZE, 0u, &m);
    if (m.bad != 0u) {
        report_mismatch("Sector erase", &m, 0u);
        return 0u;
    }
    cdc_log("Sector erase: OK");
    return 1u;
}

// Erase the whole chip a 64KB block at a time, checking each block.
//
// The bootrom sends an aligned 64KB erase as D8h when FLASH_DEVINFO's
// D8H_ERASE_SUPPORTED bit is set.  Every block holds data, so a D8h erase that
// doesn't work, or erases only a sector, leaves data behind.
static uint8_t check_block_erase(void) {
    for (uint32_t block = 0u; block < CHIP_SIZE; block += FLASH_BLOCK_SIZE) {
        if (!erase_and_check("Block erase", block, FLASH_BLOCK_SIZE)) {
            return 0u;
        }
    }
    cdc_log("Block erase: OK");
    return 1u;
}

// Run the test once.  Returns non-zero on a pass.
//
// Each check prints a line with OK or the fault.  The test stops at the first
// failed check.  The chip is erased at the end of a passing test.
static uint8_t run_test(void) {
    return check_blank()
        && check_program()
        && check_quad()
        && check_size()
        && check_sector_erase()
        && check_block_erase();
}

// Print a chip's size from the capacity byte of its JEDEC ID.  Most serial
// flash parts set that byte to log2 of their size in bytes.  Returns the size,
// or 0 where the byte isn't a valid size.
static uint32_t report_chip_size(uint8_t capacity) {
    if ((capacity < 10u) || (capacity > 31u)) {
        cdc_log("Chip size: unknown");
        return 0u;
    }
    uint32_t size = 1u << capacity;
    const char *not_2mb = (size == CHIP_SIZE) ? "" : ", not 2MB";
    if (capacity >= 20u) {
        cdc_log("Chip size: %uMB%s", (unsigned)(1u << (capacity - 20u)), not_2mb);
    } else {
        cdc_log("Chip size: %uKB%s", (unsigned)(1u << (capacity - 10u)), not_2mb);
    }
    return size;
}

// A device is present when its ID holds a mix of ones and zeros.  Without a
// device SD1 floats or is held, so all ones and all zeros both mean the line
// wasn't driven.
static uint8_t device_present(const qmi_device_id_t *id) {
    uint8_t and_all = (uint8_t)(id->jedec[0] & id->jedec[1] & id->jedec[2]);
    uint8_t or_all  = (uint8_t)(id->jedec[0] | id->jedec[1] | id->jedec[2]);
    return (and_all != 0xFFu) && (or_all != 0x00u);
}

// Read both chips' IDs, with the chip select GPIO muxed to chip select 1.
//
// If the bootrom muxed the GPIO from OTP, it is left alone.  Otherwise the
// plugin muxes it: output enabled, input enabled so the pad reads back, and
// the isolation latch cleared last so the pin stays quiet until the mux behind
// it is settled.  If the ID read doesn't find a chip, the GPIO is put back as
// it was.
//
// Returns 0 where exclusive mode was refused.
static __attribute__((noinline)) uint8_t probe(qmi_probe_result_t *result) {
    uint8_t gpio = s_ctx.gpio;
    uint32_t ctrl_before = GPIO_CTRL(gpio);
    uint32_t pad_before  = GPIO_PAD(gpio);
    if ((ctrl_before & 0x1Fu) != GPIO_FUNC_QMI_CS1N) {
        GPIO_PAD(gpio) = (pad_before & ~PAD_OD) | PAD_IE;
        GPIO_CTRL(gpio) = GPIO_FUNC_QMI_CS1N;
        GPIO_PAD(gpio) &= ~PAD_ISO;
    }

    // Serving runs from SRAM on PIO and DMA, so it keeps running while flash is
    // unreadable.  The other core runs the USB plugin from flash, and exclusive
    // mode parks it.
    if (s_ctx.enter() != ORA_RESULT_OK) {
        return 0u;
    }
    uint32_t blob[STAGE_PROBE_WORDS];
    qmi_probe_ids_fn_t fn = ORA_STAGED_FN_PTR(qmi_probe_ids_fn_t,
        (uint32_t)(uintptr_t)stage(blob, __stage_probe_start, __stage_probe_end,
                                   (const void *)&qmi_probe_ids));
    uint32_t primask = irq_disable();
    fn(result);
    irq_restore(primask);
    s_ctx.exit();

    if (!device_present(&result->cs1)) {
        GPIO_CTRL(gpio) = ctrl_before;
        GPIO_PAD(gpio) = pad_before;
    }
    return 1u;
}

// Look up the routines the test uses, check the board and identify the chip.
//
// Prints each result.  Returns non-zero when the test can run.  Otherwise it
// prints the reason, and FAIL if the reason is the board or the chip.
static uint8_t setup(ora_lookup_fn_t lookup) {
    s_ctx.enter  = lookup(ORA_ID_ENTER_EXCLUSIVE_MODE);
    s_ctx.exit   = lookup(ORA_ID_EXIT_EXCLUSIVE_MODE);
    s_ctx.yield  = lookup(ORA_ID_YIELD);
    s_ctx.uptime = lookup(ORA_ID_GET_PLUGIN_UPTIME_MS);
    ora_get_clkref_mhz_fn_t clkref = lookup(ORA_ID_GET_CLKREF_MHZ);
    ora_get_sysclk_mhz_fn_t sysclk = lookup(ORA_ID_GET_SYSCLK_MHZ);
    ora_get_metadata_uint_fn_t get_uint = lookup(ORA_ID_GET_METADATA_UINT);
    if ((s_ctx.enter == NULL) || (s_ctx.exit == NULL) || (s_ctx.yield == NULL) ||
        (s_ctx.uptime == NULL) || (clkref == NULL) || (sysclk == NULL) ||
        (get_uint == NULL)) {
        cdc_log("Can't find the plugin API calls the test needs.");
        return 0u;
    }

    s_ctx.connect    = lookup_boot('I', 'F', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.exit_xip   = lookup_boot('E', 'X', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.op         = lookup_boot('F', 'O', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.flush      = lookup_boot('F', 'C', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.select_xip = lookup_boot('X', 'M', ORA_BOOTROM_FLAG_ARM_SEC);
    volatile uint16_t *devinfo = devinfo_ptr();
    if ((s_ctx.connect == NULL) || (s_ctx.exit_xip == NULL) || (s_ctx.op == NULL) ||
        (s_ctx.flush == NULL) || (s_ctx.select_xip == NULL) || (devinfo == NULL)) {
        cdc_log("Can't find the bootrom's flash routines.");
        return 0u;
    }

    watchdog_setup(clkref());

    // The pin `onerom hardware set-size` writes into FLASH_DEVINFO comes from
    // the same board definition as this.
    uint32_t gpio;
    if (get_uint(ORA_METADATA_KEY_GPIO_EXT_FLASH_CS, &gpio) != ORA_RESULT_OK) {
        cdc_log("Can't read the external flash chip select from the board's metadata.");
        return 0u;
    }
    if (gpio == ORA_GPIO_NONE) {
        cdc_log("This board doesn't support external flash.");
        return 0u;
    }
    s_ctx.gpio = (uint8_t)gpio;
    cdc_log("Chip select: GPIO %u", (unsigned)gpio);

    // Window 0's configuration reads the built-in flash.  The read checks use
    // its timing, captured here before anything changes the QMI.
    //
    // The XIP restore at the end of each flash operation needs the bootrom's
    // number for window 0's mode, found from its opcode.  RP2350 datasheet
    // section 5.4.8.14 lists them: 0 is 03h serial, 1 is 0Bh serial, 2 is BBh
    // dual-IO and 3 is EBh quad-IO.
    s_ctx.m0_timing = QMI_M0_TIMING;
    s_ctx.clkdiv    = (uint8_t)(s_ctx.m0_timing & 0xFFu);
    switch (QMI_M0_RCMD & 0xFFu) {
        case 0xEBu: s_ctx.mode = 3u; break;
        case 0xBBu: s_ctx.mode = 2u; break;
        case 0x0Bu: s_ctx.mode = 1u; break;
        default:    s_ctx.mode = 0u; break;
    }

    // The firmware sets the flash clock from the CPU clock, and the read checks
    // use it.  A divisor of 0 means 256 (RP2350 datasheet Table 1297).
    uint32_t cpu_mhz = sysclk();
    uint32_t divisor = (s_ctx.clkdiv == 0u) ? 256u : s_ctx.clkdiv;
    cdc_log("CPU clock: %uMHz", (unsigned)cpu_mhz);
    cdc_log("Flash clock: %uMHz", (unsigned)(cpu_mhz / divisor));

    // The board size as the CLI reads it: M for a chip select 1 size of 0, L
    // for 2MB and other for anything else.
    uint16_t value = *devinfo;
    uint16_t cs1_size = (uint16_t)((value >> ORA_OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT)
                                   & ORA_OTP_FLASH_DEVINFO_SIZE_BITS);
    uint16_t want = l_board_devinfo(s_ctx.gpio);
    if (cs1_size == 0u) {
        cdc_log("Board size: M");
    } else {
        cdc_log("Board size: %s",
                (cs1_size == ORA_FLASH_SIZE_2MB) ? "L" : "other");
        if (value != want) {
            cdc_log("OTP sets FLASH_DEVINFO to 0x%04X, not 0x%04X.",
                    (unsigned)value, (unsigned)want);
            cdc_log("FAIL");
            return 0u;
        }
    }

    qmi_probe_result_t result;
    if (!probe(&result)) {
        cdc_log("Can't enter exclusive mode.");
        return 0u;
    }

    // The built-in flash is always present, so it is the control.  If it
    // doesn't return an ID, the probe is at fault and its result for chip
    // select 1 can't be trusted.
    if (!device_present(&result.cs0)) {
        cdc_log("The built-in flash doesn't respond.");
        return 0u;
    }
    if (!device_present(&result.cs1)) {
        cdc_log("The external flash doesn't respond.");
        cdc_log("FAIL");
        return 0u;
    }
    s_ctx.sr2 = result.cs1.sr2;

    cdc_log("Chip ID: %02X %02X %02X",
            (unsigned)result.cs1.jedec[0], (unsigned)result.cs1.jedec[1],
            (unsigned)result.cs1.jedec[2]);
    if (report_chip_size(result.cs1.jedec[2]) != CHIP_SIZE) {
        cdc_log("FAIL");
        return 0u;
    }

    // On an M board, set the bootrom's RAM copy as set-size sets OTP, so the
    // bootrom's flash routines can reach the chip.  Chip select 0's size is
    // left as the boot set it.
    if (cs1_size == 0u) {
        uint16_t cs0 = (uint16_t)(ORA_OTP_FLASH_DEVINFO_SIZE_BITS
                                  << ORA_OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT);
        *devinfo = (uint16_t)((value & cs0) | (want & ~cs0));
    }

    return 1u;
}

// Throw away whatever has been typed, so a key pressed during a test doesn't
// start the next one.
static void drain_input(void) {
    uint8_t  buf[8];
    uint32_t got;
    do {
        got = 0u;
        (void)s_ctx.read(ORA_LOG_CHANNEL_1, buf, sizeof(buf), &got);
    } while (got != 0u);
}

// Wait for a y.  Everything else typed is ignored, including the line ending
// `onerom console` sends after it.
//
// Yields throughout, so the other core can take exclusive mode.
static void wait_for_y(void) {
    for (;;) {
        (void)s_ctx.yield(NULL);

        uint8_t  buf[8];
        uint32_t got = 0u;
        if (s_ctx.read(ORA_LOG_CHANNEL_1, buf, sizeof(buf), &got) != ORA_RESULT_OK) {
            continue;
        }
        for (uint32_t i = 0u; i < got; i++) {
            if ((buf[i] == 'y') || (buf[i] == 'Y')) {
                return;
            }
        }
    }
}

// Loop forever, yielding so the other core can take exclusive mode.
static void idle(void) {
    for (;;) {
        if (s_ctx.yield != NULL) {
            (void)s_ctx.yield(NULL);
        }
    }
}

void ext_flash_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    (void)plugin_type;
    (void)entry_args;

    init_data_bss();

    // Channel 0 is what the USB plugin sends to `onerom console`.
    if (!cdc_log_init(ora_lookup_fn, "ext-flash")) {
        return;
    }

    cdc_log("One ROM External Flash Tester v%u.%u.%u",
            VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH);

    // watchdog_take_mark returns MARK_IDLE unless a flash operation didn't
    // finish and the watchdog reset the board.
    mark_t died_at = watchdog_take_mark();
    if (died_at != MARK_IDLE) {
        cdc_log("The last test didn't finish. The watchdog reset the board at step %u, page %u.",
                (unsigned)((uint32_t)died_at & 0xFFu),
                (unsigned)((uint32_t)died_at >> 8));
    }

    if (!setup(ora_lookup_fn)) {
        idle();
    }

    // Channel 1 holds what is typed at `onerom console`.
    ora_log_open_read_fn_t open_read = ora_lookup_fn(ORA_ID_LOG_OPEN_READ);
    s_ctx.read = ora_lookup_fn(ORA_ID_LOG_READ);
    if ((open_read == NULL) || (s_ctx.read == NULL) ||
        (open_read(ORA_LOG_CHANNEL_1) != ORA_RESULT_OK)) {
        cdc_log("Can't read console input.");
        idle();
    }
    drain_input();

    cdc_log("This test erases, writes and reads the external flash.");
    cdc_log("Takes 20-25s with a new flash chip and One ROM clocked at 150MHz.");
    cdc_log("Type y then Enter to proceed.");
    for (;;) {
        wait_for_y();
        cdc_log("%s", run_test() ? "PASS" : "FAIL");
        drain_input();
        cdc_log("Type y then Enter to repeat.");
    }
}
