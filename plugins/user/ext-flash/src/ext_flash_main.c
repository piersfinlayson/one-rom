// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// One ROM user plugin that reports what is on the QSPI bus.
//
// A Fire board with an RP2354 carries its 2MB of flash as a second die on the
// QMI's first chip select.  Some of those boards also carry a second 2MB part
// on the QMI's second chip select, which the firmware has never used and which
// no board has been verified with.  This plugin says whether that part is
// there, what state the bootrom left the QSPI interface in, and whether the
// part can be read at 0x11000000.
//
// It is a diagnostic.  It leaves OTP alone, and the read-only build leaves both
// flash devices alone too.  The write-test build erases and programs one sector
// of the external device, and touches the internal one never.
//
// It leaves behind the second chip select's GPIO and the QMI's second memory
// window, and only where it found a device to point them at.

#include "plugin.h"

#include "qmi_probe.h"
#include "recover.h"
#include "cdc_log.h"

ORA_DEFINE_USER_PLUGIN(
    ext_flash_main,
    0, 1, 0, 0,     // Plugin version
    0, 7, 2         // Minimum One ROM firmware version required (log read API)
);

// GPIO to try when the board says nothing about which pin carries the QMI's
// second chip select.
//
// A constant rather than the hardware metadata's gpio_ext_flash_cs, so the
// plugin reports the same pin whatever a board declares, and a board declaring
// none can still be probed by hand.  Every board in the tree that declares the
// pin declares 47.  An OTP-programmed board names the pin itself, and s_cs1
// carries that instead.
#define CS1_GPIO_DEFAULT 47u

// The second chip select as the board presented it, before this plugin touched
// anything.
//
// An OTP-programmed board arrives with FLASH_DEVINFO set and the pin already
// muxed by the bootrom.  An unprogrammed one arrives with CS1_SIZE zero and
// neither done.  Both exist, so the plugin follows what it reads rather than
// assuming either.
static struct {
    uint16_t boot_value;  // FLASH_DEVINFO as the boot left it
    uint8_t  gpio;        // the pin carrying the second chip select
    uint8_t  from_otp;    // set when the bootrom configured it from OTP
} s_cs1 = { 0u, CS1_GPIO_DEFAULT, 0u };

// How long a flash operation gets before the watchdog decides it is not coming
// back.  Winbond W25Q16JV datasheet section 9.6 gives a sector erase up to
// 400ms, so this clears that by enough that a slow but healthy erase finishes.
#define WD_TIMEOUT_MS 2000u

// Room on the stack for each staged routine.  Checked against the real size at
// every call, so a routine that outgrows its allowance says so rather than
// running off the end of the copy.

// Copy a staged routine into blob and return a pointer to run it from.
//
// fn is the routine's address in flash.  Its offset within the section is the
// same as its offset within the copy, which is what makes the order of
// functions in the section free.  Taking a Thumb function's address sets bit 0,
// so that comes off before the subtraction and goes back on afterwards.
//
// Returns NULL when the section does not fit the space the caller allowed.
static void *stage(
    uint32_t       *blob,
    uint32_t        blob_words,
    const uint8_t  *start,
    const uint8_t  *end,
    const void     *fn
) {
    uint32_t len = ORA_STAGED_FN_SIZE(start, end);
    if (len > blob_words * 4u) {
        cdc_log("EXTF: staged routine is %u bytes, allowed %u",
              (unsigned)len, (unsigned)(blob_words * 4u));
        return NULL;
    }

    // Volatile destination so the compiler leaves this as a loop.  Recognised
    // as a memcpy pattern it emits a library call, and the plugin environment
    // has no C runtime.
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
// region may sit inside another one.  Needed around the probe, since direct
// mode makes flash unreadable and every handler this core would run lives
// there.
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

// The firmware hands a plugin whatever was last in its static RAM, so a static
// relying on zero initialisation starts with garbage, and .ramfunc still sits
// in flash where a call cannot reach it.  Both are fixed here, before anything
// reads a static or calls into RAM.
//
// The destination pointers are volatile to keep the compiler from spotting
// memcpy and memset patterns and calling libraries the plugin environment
// lacks.
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

// One letter for a transfer width, as the RFMT width fields encode it.
static char width_char(uint32_t rfmt, uint32_t lsb) {
    switch ((rfmt >> lsb) & 0x3u) {
        case 0u:  return 'S';
        case 1u:  return 'D';
        case 2u:  return 'Q';
        default:  return '?';
    }
}

// Report one memory window's read configuration.
//
// The raw registers go out alongside the interpretation, which covers the
// fields distinguishing the four read modes the bootrom chooses between.  The
// raw value is what to check if the device turns out to be in a fifth.
static void log_window(const char *name, uint32_t timing, uint32_t rfmt, uint32_t rcmd) {
    cdc_log("EXTF: %s timing=0x%08X rfmt=0x%08X rcmd=0x%08X",
          name, (unsigned)timing, (unsigned)rfmt, (unsigned)rcmd);
    cdc_log("EXTF: %s  opcode=0x%02X clkdiv=%u addr=%c data=%c dummy=%u prefix=%u",
          name,
          (unsigned)(rcmd & 0xFFu),
          (unsigned)(timing & 0xFFu),
          width_char(rfmt, 2u),
          width_char(rfmt, 8u),
          (unsigned)((rfmt >> 16) & 0x7u),
          (unsigned)((rfmt >> 12) & 0x1u));
}


static void log_device(const char *name, const qmi_device_id_t *id) {
    cdc_log("EXTF: %s jedec=%02X %02X %02X sr2=0x%02X qe=%u",
          name,
          (unsigned)id->jedec[0], (unsigned)id->jedec[1], (unsigned)id->jedec[2],
          (unsigned)id->sr2,
          (unsigned)((id->sr2 & FLASH_SR2_QE) ? 1u : 0u));
}

// A device answered when its ID holds a mix of ones and zeros.  An absent
// device leaves SD1 floating and an unconnected one holds it, so all-ones and
// all-zeros both mean nothing drove the line.
static uint8_t device_present(const qmi_device_id_t *id) {
    uint8_t and_all = (uint8_t)(id->jedec[0] & id->jedec[1] & id->jedec[2]);
    uint8_t or_all  = (uint8_t)(id->jedec[0] | id->jedec[1] | id->jedec[2]);
    return (and_all != 0xFFu) && (or_all != 0x00u);
}




// Where the test works.  Offset 0 of the external device, which is where a ROM
// image would land, and which the probe has just shown reads erased.
#define TEST_OFFSET  0u
#define TEST_ADDR    (XIP_CS1_BASE + TEST_OFFSET)

// Where chip select 1 starts in the space flash_range_erase and
// flash_range_program count offsets from.  Those take an offset from the start
// of flash (RP2350 datasheet section 5.4.8.11) and say nothing about chip
// selects, so this is the offset that would reach it if the storage map puts
// the second device 16MB up, as flash_op's address space does.  Whether it does
// is what the R command is for.
#define CS1_STORAGE_OFFSET 0x1000000u



// Somewhere in RAM for the critical section to leave flash_op's return in.
static int32_t s_op_result;

typedef struct {
    connect_internal_flash_fn_t     connect;
    flash_exit_xip_fn_t             exit_xip;
    flash_op_fn_t                   op;
    flash_flush_cache_fn_t          flush;
    flash_select_xip_read_mode_fn_t select_xip;
    flash_range_program_fn_t        range_program;
    flash_range_erase_fn_t          range_erase;
} boot_fns_t;

static void *lookup_boot(char a, char b, uint32_t flag) {
    uint32_t code = ((uint32_t)(uint8_t)b << 8) | (uint32_t)(uint8_t)a;
    return ORA_BOOTROM_LOOKUP(code, flag);
}

// Find the bootrom's RAM copy of FLASH_DEVINFO.
//
// flash_op bounds-checks every operation against this word.  An OTP-programmed
// board has it set at boot.  An unprogrammed one reads as 16MB on chip select 0
// and nothing on chip select 1, so anything aimed at chip select 1 comes back
// NOT_PERMITTED until the copy is written.
//
// Writing the boot RAM copy changes that without burning OTP.  RP2350
// datasheet section 5.4.8.5 says the flash APIs use this copy, and that Secure
// code may update it at runtime through this pointer.  The OTP field, section
// 13.10 Table 1392, is the permanent version of the same value.
//
// Field layout is that table: CS1_SIZE at 15:12 decoded as 4KiB << value,
// CS0_SIZE at 11:8 the same way, D8H_ERASE_SUPPORTED at 7, CS1_GPIO at 5:0.
static volatile uint16_t *devinfo_ptr(void) {
    // The lookup helper at address 0x16 returns the value stored in the table
    // entry, and table entries are 16 bits wide (RP2350 datasheet section
    // 5.4.1).  A boot RAM address needs more than 16 bits, so what comes back
    // is the address of a bootrom word holding the real pointer - two steps to
    // the value rather than one.
    //
    // Getting this wrong is quiet.  That first address is in ROM, so a write
    // through it goes nowhere, and the compiler folds the read that follows
    // into the value just stored, so the mistake reads back as a success.
    uint16_t **devinfo_slot = lookup_boot('F', 'D', ORA_BOOTROM_FLAG_DATA);
    if (devinfo_slot == NULL) {
        cdc_log("EXTF: flash_devinfo16_ptr not found");
        return 0u;
    }
    volatile uint16_t *devinfo = *devinfo_slot;
    cdc_log("EXTF: devinfo slot=0x%08X ptr=0x%08X",
          (unsigned)(uintptr_t)devinfo_slot, (unsigned)(uintptr_t)devinfo);
    if (devinfo == NULL) {
        cdc_log("EXTF: devinfo pointer is null");
        return 0u;
    }
    return devinfo;
}

// Read what the board says about the second chip select, and say so.
//
// This is the whole diagnostic now that both kinds of board are in the wild:
// one line naming which one is on the desk.  Runs before anything here changes
// the pin or the word.
static void cs1_classify(void) {
    volatile uint16_t *devinfo = devinfo_ptr();
    if (devinfo == NULL) {
        cdc_log("EXTF: cs1 unknown - no devinfo, trying gpio%u",
              (unsigned)CS1_GPIO_DEFAULT);
        return;
    }

    s_cs1.boot_value = *devinfo;
    s_cs1.from_otp   = ((s_cs1.boot_value >> 12) != 0u) ? 1u : 0u;

    // A board that names its own pin is followed.  Overriding it with the
    // default would mux the wrong pin on a board wired differently.
    s_cs1.gpio = s_cs1.from_otp ? (uint8_t)(s_cs1.boot_value & 0x3Fu)
                                : (uint8_t)CS1_GPIO_DEFAULT;

    if (s_cs1.from_otp) {
        cdc_log("EXTF: cs1 from OTP - devinfo 0x%04X, gpio%u, %uKB",
              (unsigned)s_cs1.boot_value, (unsigned)s_cs1.gpio,
              (unsigned)(4u << (s_cs1.boot_value >> 12)));
    } else {
        cdc_log("EXTF: cs1 unset in OTP - devinfo 0x%04X, plugin will use gpio%u",
              (unsigned)s_cs1.boot_value, (unsigned)s_cs1.gpio);
    }
}

// Tell the bootrom's flash routines that the second device exists.
//
// Only where OTP has not already said so.  Writing over an OTP value would
// move the bound the bootrom checks against, silently, on any board whose
// external device is not the 2MB part this assumes.
static uint16_t enable_cs1_devinfo(void) {
    volatile uint16_t *devinfo = devinfo_ptr();
    if (devinfo == NULL) {
        return 0u;
    }

    if (s_cs1.from_otp) {
        cdc_log("EXTF: devinfo 0x%04X set by OTP - left alone",
              (unsigned)*devinfo);
        return *devinfo;
    }

    uint16_t before = *devinfo;
    // 15:12 CS1_SIZE, 9 meaning 4KiB << 9 = 2MB.  5:0 CS1_GPIO.
    *devinfo = (uint16_t)((before & 0x0FC0u) | (9u << 12) | s_cs1.gpio);
    cdc_log("EXTF: devinfo 0x%04X -> 0x%04X", (unsigned)before, (unsigned)*devinfo);
    return *devinfo;
}

// Run one flash operation, holding the MCU for its duration.
//
// An erase can take the device 400ms, and the other core is parked and this
// core's interrupts masked throughout.  Serving continues, on PIO and DMA out
// of SRAM, and the USB plugin stalls for that long.
static int32_t run_flash_op(
    const boot_fns_t *fns,
    ora_enter_exclusive_mode_fn_t enter,
    ora_exit_exclusive_mode_fn_t exit,
    uint8_t use_flash_op,
    uint32_t flags,
    uint32_t addr,
    uint32_t size,
    uint8_t *buf,
    uint8_t mode,
    uint8_t clkdiv
) {
    if (enter() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed");
        return -1;
    }
    uint32_t blob[STAGE_OP_WORDS];
    ext_flash_op_critical_fn_t op = ORA_STAGED_FN_PTR(ext_flash_op_critical_fn_t,
        (uint32_t)(uintptr_t)stage(blob, STAGE_OP_WORDS,
                                   __stage_op_start, __stage_op_end,
                                   (const void *)&ext_flash_op_critical));
    watchdog_arm(WD_TIMEOUT_MS);
    uint32_t primask = irq_disable();
    op(fns->connect, fns->exit_xip,
       use_flash_op ? fns->op : NULL,
       use_flash_op ? NULL : fns->range_erase,
       fns->flush, fns->select_xip,
       flags, addr, size, buf, mode, clkdiv, &s_op_result);
    irq_restore(primask);
    watchdog_disarm();
    exit();
    return s_op_result;
}

// How much room is left between the stack pointer and the top of this plugin's
// statics.
//
// Worth logging before a critical section.  Running out in there takes this
// core while it holds exclusive mode, so the other core stays parked, USB
// stops answering and the board needs BOOTSEL.  A number in the log beforehand
// is what makes that visible.
static uint32_t stack_headroom(void) {
    extern uint32_t __bss_end;
    uint32_t sp;
    __asm volatile ("mov %0, sp" : "=r" (sp));
    return sp - (uint32_t)(uintptr_t)&__bss_end;
}

// Fill the whole test sector with the pattern, over the QSPI bus directly.
//
// A whole sector rather than a page, since the read checks that follow are
// worth as much as the data they cover, and 4KB at speed says far more than
// 256 bytes.
//
// Direct mode rather than the bootrom, whose flash_op needs a page buffer and
// more stack than is left once that buffer is on it.  See
// qmi_cs1_program_sector.
static int32_t program_sector_direct(
    ora_enter_exclusive_mode_fn_t enter,
    ora_exit_exclusive_mode_fn_t exit
) {
    if (enter() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed");
        return -1;
    }
    uint32_t blob[STAGE_QSPI_WORDS];
    qmi_cs1_program_sector_fn_t prog = ORA_STAGED_FN_PTR(qmi_cs1_program_sector_fn_t,
        (uint32_t)(uintptr_t)stage(blob, STAGE_QSPI_WORDS,
                                   __stage_qspi_start, __stage_qspi_end,
                                   (const void *)&qmi_cs1_program_sector));
    watchdog_arm(WD_TIMEOUT_MS);
    uint32_t primask = irq_disable();
    prog(TEST_OFFSET, FLASH_SECTOR_SIZE / FLASH_PAGE_SIZE);
    irq_restore(primask);
    watchdog_disarm();
    exit();
    return 0;
}


// Compare the test page against what it should hold, through whatever window 1
// is currently pointed at.
//
// Reads go through the uncached view.  The cache sits upstream of the
// per-window configuration, so a cached read after a reconfiguration could come
// from a line fetched under the previous one.
static uint32_t verify_page(uint8_t expect_erased, uint32_t *first_bad) {
    const volatile uint8_t *p = (const volatile uint8_t *)(XIP_CS1_NOCACHE + TEST_OFFSET);
    uint32_t bad = 0u;
    *first_bad = FLASH_SECTOR_SIZE;
    for (uint32_t i = 0u; i < FLASH_SECTOR_SIZE; i++) {
        uint8_t want = expect_erased ? 0xFFu : ext_flash_pattern(i);
        if (p[i] != want) {
            if (bad == 0u) {
                *first_bad = i;
            }
            bad++;
        }
    }
    return bad;
}

// Set window 1, then check the page through it.
static void check_through(
    const char *name,
    ora_enter_exclusive_mode_fn_t enter,
    ora_exit_exclusive_mode_fn_t exit,
    uint32_t timing,
    uint32_t rfmt,
    uint32_t rcmd
) {
    if (enter() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed setting m1");
        return;
    }
    uint32_t blob[STAGE_M1_WORDS];
    qmi_set_m1_fn_t set_m1 = ORA_STAGED_FN_PTR(qmi_set_m1_fn_t,
        (uint32_t)(uintptr_t)stage(blob, STAGE_M1_WORDS,
                                   __stage_m1_start, __stage_m1_end,
                                   (const void *)&qmi_set_m1));
    uint32_t primask = irq_disable();
    set_m1(timing, rfmt, rcmd);
    irq_restore(primask);
    exit();

    uint32_t first_bad;
    uint32_t bad = verify_page(0u, &first_bad);
    if (bad == 0u) {
        cdc_log("EXTF: read %s clkdiv=%u opcode=0x%02X - all %u ok",
              name, (unsigned)(timing & 0xFFu), (unsigned)(rcmd & 0xFFu),
              (unsigned)FLASH_SECTOR_SIZE);
    } else {
        const volatile uint8_t *p =
            (const volatile uint8_t *)(XIP_CS1_NOCACHE + TEST_OFFSET);
        cdc_log("EXTF: read %s clkdiv=%u opcode=0x%02X - %u BAD, first at %u got 0x%02X want 0x%02X",
              name, (unsigned)(timing & 0xFFu), (unsigned)(rcmd & 0xFFu),
              (unsigned)bad, (unsigned)first_bad,
              (unsigned)p[first_bad], (unsigned)ext_flash_pattern(first_bad));
    }
}

// Everything the commands need, worked out once.
typedef struct {
    boot_fns_t                    fns;
    ora_enter_exclusive_mode_fn_t enter;
    ora_exit_exclusive_mode_fn_t  exit;
    uint32_t                      m0_timing;
    uint32_t                      m0_rfmt;
    uint32_t                      m0_rcmd;
    uint8_t                       mode;       // for the XIP restore
    uint8_t                       clkdiv;
    uint8_t                       ready;
} test_ctx_t;

static test_ctx_t s_ctx;

// Point window 1 at a read configuration.  Runs staged, since every field bar
// the divisor may only change while the QMI is idle.
static void set_m1(uint32_t timing, uint32_t rfmt, uint32_t rcmd) {
    uint32_t blob[STAGE_M1_WORDS];
    qmi_set_m1_fn_t fn = ORA_STAGED_FN_PTR(qmi_set_m1_fn_t,
        (uint32_t)(uintptr_t)stage(blob, STAGE_M1_WORDS,
                                   __stage_m1_start, __stage_m1_end,
                                   (const void *)&qmi_set_m1));
    if (s_ctx.enter() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed setting m1");
        return;
    }
    uint32_t primask = irq_disable();
    fn(timing, rfmt, rcmd);
    irq_restore(primask);
    s_ctx.exit();
}


// Look up what the write commands need and note the state they have to put
// back.  Called once, before any command runs.
static void write_test_init(
    ora_enter_exclusive_mode_fn_t enter,
    ora_exit_exclusive_mode_fn_t exit
) {
    s_ctx.enter = enter;
    s_ctx.exit  = exit;

    s_ctx.fns.connect    = lookup_boot('I', 'F', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.exit_xip   = lookup_boot('E', 'X', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.op         = lookup_boot('F', 'O', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.flush      = lookup_boot('F', 'C', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.select_xip = lookup_boot('X', 'M', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.range_program = lookup_boot('R', 'P', ORA_BOOTROM_FLAG_ARM_SEC);
    s_ctx.fns.range_erase   = lookup_boot('R', 'E', ORA_BOOTROM_FLAG_ARM_SEC);
    if (s_ctx.fns.connect == NULL || s_ctx.fns.exit_xip == NULL ||
        s_ctx.fns.op == NULL || s_ctx.fns.flush == NULL ||
        s_ctx.fns.select_xip == NULL) {
        cdc_log("EXTF: bootrom flash routines not all found");
        return;
    }

    if (enable_cs1_devinfo() == 0u) {
        return;
    }

    // Window 0's configuration is what serves the internal device.  The last
    // read check applies it verbatim to the external one, which asks whether
    // the external part keeps up.
    //
    // Captured here, before any command disturbs the QMI.
    s_ctx.m0_timing = QMI_M0_TIMING;
    s_ctx.m0_rfmt   = QMI_M0_RFMT;
    s_ctx.m0_rcmd   = QMI_M0_RCMD;
    s_ctx.clkdiv    = (uint8_t)(s_ctx.m0_timing & 0xFFu);

    // The XIP restore at the end of each critical section needs the mode
    // number the bootrom chose, which its opcode identifies.  RP2350 datasheet
    // section 5.4.8.14 lists them: 0 is 03h serial, 1 is 0Bh serial, 2 is BBh
    // dual-IO and 3 is EBh quad-IO.
    switch (s_ctx.m0_rcmd & 0xFFu) {
        case 0xEBu: s_ctx.mode = 3u; break;
        case 0xBBu: s_ctx.mode = 2u; break;
        case 0x0Bu: s_ctx.mode = 1u; break;
        default:    s_ctx.mode = 0u; break;
    }

    s_ctx.ready = 1u;
    cdc_log("EXTF: write commands ready, xip restore mode=%u clkdiv=%u",
          (unsigned)s_ctx.mode, (unsigned)s_ctx.clkdiv);
}

// Erase the test sector and say whether it reads erased afterwards.
//
// Storage address space, secure, at the chip select 1 window address.  This
// needs FLASH_DEVINFO to describe the second device, from OTP or from the
// plugin's write.  Verified on fire-40-a: with neither in place it returns
// NOT_PERMITTED, and
// with chip select 1 declared in the boot RAM copy it succeeds.
static void cmd_erase_by(uint8_t use_flash_op) {
    int32_t rc = run_flash_op(&s_ctx.fns, s_ctx.enter, s_ctx.exit, use_flash_op,
                              CFLASH_ASPACE_STORAGE | CFLASH_SECLEVEL_SECURE | CFLASH_OP_ERASE,
                              use_flash_op ? TEST_ADDR : (CS1_STORAGE_OFFSET + TEST_OFFSET),
                              FLASH_SECTOR_SIZE, NULL,
                              s_ctx.mode, s_ctx.clkdiv);
    cdc_log("EXTF: erase rc=%d (headroom was %u)", (int)rc,
          (unsigned)stack_headroom());
    if (rc != 0) {
        return;
    }

    set_m1(s_ctx.m0_timing, M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL);
    uint32_t first_bad;
    uint32_t bad = verify_page(1u, &first_bad);
    cdc_log("EXTF: erased check %u bytes still set", (unsigned)bad);
}

static void cmd_erase(void) {
    cdc_log("EXTF: erase via flash_op, stack headroom %u",
          (unsigned)stack_headroom());
    cmd_erase_by(1u);
}

static void cmd_erase_range(void) {
    cdc_log("EXTF: erase via flash_range_erase, stack headroom %u",
          (unsigned)stack_headroom());
    cmd_erase_by(0u);
}

// Program the sector through the bootrom, by whichever of the two routes the
// caller names.
//
// The page buffer is a local because this plugin has 1KB for its static RAM and
// its stack together, and a 256 byte static crowds the stack the firmware's log
// formatter needs.  Nothing here logs while it is live.
static int32_t program_sector_bootrom(uint8_t use_flash_op) {
    uint8_t page[FLASH_PAGE_SIZE];

    if (s_ctx.enter() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed");
        return -1;
    }
    uint32_t blob[STAGE_BOOTPROG_WORDS];
    ext_flash_program_sector_bootrom_fn_t prog =
        ORA_STAGED_FN_PTR(ext_flash_program_sector_bootrom_fn_t,
            (uint32_t)(uintptr_t)stage(blob, STAGE_BOOTPROG_WORDS,
                                       __stage_bootprog_start, __stage_bootprog_end,
                                       (const void *)&ext_flash_program_sector_bootrom));
    watchdog_arm(WD_TIMEOUT_MS);
    uint32_t primask = irq_disable();
    prog(
        s_ctx.fns.connect, s_ctx.fns.exit_xip, s_ctx.fns.flush,
        s_ctx.fns.select_xip,
        use_flash_op ? s_ctx.fns.op : NULL,
        use_flash_op ? NULL : s_ctx.fns.range_program,
        CFLASH_ASPACE_STORAGE | CFLASH_SECLEVEL_SECURE | CFLASH_OP_PROGRAM,
        use_flash_op ? TEST_ADDR : (CS1_STORAGE_OFFSET + TEST_OFFSET),
        page, FLASH_SECTOR_SIZE / FLASH_PAGE_SIZE,
        s_ctx.mode, s_ctx.clkdiv, &s_op_result);
    irq_restore(primask);
    watchdog_disarm();
    s_ctx.exit();
    return s_op_result;
}

static void cmd_program_flash_op(void) {
    cdc_log("EXTF: program via flash_op, stack headroom %u",
          (unsigned)stack_headroom());
    int32_t rc = program_sector_bootrom(1u);
    cdc_log("EXTF: program rc=%d", (int)rc);
}

static void cmd_program_range(void) {
    cdc_log("EXTF: program via flash_range_program, stack headroom %u",
          (unsigned)stack_headroom());
    int32_t rc = program_sector_bootrom(0u);
    cdc_log("EXTF: program rc=%d", (int)rc);
}

static void cmd_program(void) {
    cdc_log("EXTF: program direct, stack headroom %u", (unsigned)stack_headroom());
    int32_t rc = program_sector_direct(s_ctx.enter, s_ctx.exit);
    cdc_log("EXTF: program rc=%d", (int)rc);
}


// Read the page back at a range of clock divisors, in the internal device's own
// read format.
//
// Sweeping down until it breaks says where the margin is, which decides
// whether the external device can be driven as hard as the internal one, and
// what happens to it when a slot asks for a faster system clock.
//
// The divisor is system clocks per SCK period, so a smaller one is a faster
// bus.  At a 150MHz system clock, 2 is 75MHz and 1 is 150MHz, past the 133MHz
// the W25Q16JV is rated for, so a failure at 1 is the part behaving as
// specified.
// Write a routine into the external flash, run it from there, and erase it.
//
// This asks whether the RP2350 will fetch instructions from the second chip
// select, which is a different question from whether it will read data.  The
// call goes to 0x11000000 rather than the uncached view the readback checks
// use, because fetching through the cache is how code would really run.
//
// ext_flash_op_critical flushes the cache at the end of every operation it
// performs, so the bytes just programmed are not answered from a stale line.
// Program the canary routine into the test sector's first page.
//
// The page buffer lives here rather than in cmd_execute, so the erases and the
// call into external flash do not carry it on the stack.
static __attribute__((noinline)) int32_t program_canary(uint32_t len) {
    uint8_t page[FLASH_PAGE_SIZE];

    // Pad with the erased value, since programming only clears bits.
    for (uint32_t i = 0u; i < FLASH_PAGE_SIZE; i++) {
        page[i] = (i < len) ? __stage_exec_start[i] : 0xFFu;
    }

    return run_flash_op(&s_ctx.fns, s_ctx.enter, s_ctx.exit, 1u,
                        CFLASH_ASPACE_STORAGE | CFLASH_SECLEVEL_SECURE | CFLASH_OP_PROGRAM,
                        TEST_ADDR, FLASH_PAGE_SIZE, page,
                        s_ctx.mode, s_ctx.clkdiv);
}

// Run the bootrom flash path on the device information the boot left behind.
//
// That is the path the firmware will take once external flash ships, and until
// now nothing has run it: the plugin writes FLASH_DEVINFO at startup, so every
// other command is testing the plugin's write as much as the bootrom.  This
// one puts the boot value back for the duration.
//
// On an OTP-programmed board both operations should succeed, which is what says
// the OTP field alone is enough.  On an unprogrammed board both should come
// back NOT_PERMITTED, which is what says the write is doing the work.
static void cmd_bootrom_only(void) {
    volatile uint16_t *devinfo = devinfo_ptr();
    if (devinfo == NULL) {
        cdc_log("EXTF: devinfo unavailable");
        return;
    }

    uint16_t plugin_value = *devinfo;
    *devinfo = s_cs1.boot_value;
    cdc_log("EXTF: bootrom-only test, devinfo 0x%04X for the duration (was 0x%04X)",
          (unsigned)s_cs1.boot_value, (unsigned)plugin_value);

    int32_t erc = run_flash_op(&s_ctx.fns, s_ctx.enter, s_ctx.exit, 1u,
                               CFLASH_ASPACE_STORAGE | CFLASH_SECLEVEL_SECURE | CFLASH_OP_ERASE,
                               TEST_ADDR, FLASH_SECTOR_SIZE, NULL,
                               s_ctx.mode, s_ctx.clkdiv);
    cdc_log("EXTF: bootrom-only erase rc=%d", (int)erc);

    int32_t prc = program_sector_bootrom(1u);
    cdc_log("EXTF: bootrom-only program rc=%d", (int)prc);

    *devinfo = plugin_value;
    cdc_log("EXTF: devinfo back to 0x%04X", (unsigned)plugin_value);

    cdc_log("EXTF: bootrom path %s the plugin writing devinfo",
          ((erc == 0) && (prc == 0)) ? "works without" : "needs");
}

static void cmd_execute(void) {
    uint32_t len = ORA_STAGED_FN_SIZE(__stage_exec_start, __stage_exec_end);
    uint32_t off = (uint32_t)((uintptr_t)&ext_flash_canary & ~(uintptr_t)1u)
                 - (uint32_t)(uintptr_t)__stage_exec_start;

    cdc_log("EXTF: execute test, routine %u bytes at offset %u",
          (unsigned)len, (unsigned)off);

    if (len > FLASH_PAGE_SIZE) {
        cdc_log("EXTF: routine is larger than a page");
        return;
    }

    cdc_log("EXTF: erasing, stack headroom %u", (unsigned)stack_headroom());
    cmd_erase_by(1u);

    int32_t rc = program_canary(len);
    cdc_log("EXTF: execute test program rc=%d", (int)rc);
    if (rc != 0) {
        return;
    }

    // Compare what landed against what was sent, so a bad result from the call
    // can be told apart from a bad write.
    const volatile uint8_t *stored =
        (const volatile uint8_t *)(XIP_CS1_BASE + TEST_OFFSET);
    uint32_t bad = 0u;
    for (uint32_t i = 0u; i < len; i++) {
        if (stored[i] != __stage_exec_start[i]) {
            bad++;
        }
    }
    cdc_log("EXTF: routine reads back with %u bytes wrong", (unsigned)bad);

    ext_flash_canary_fn_t fn = ORA_STAGED_FN_PTR(ext_flash_canary_fn_t,
                                                 XIP_CS1_BASE + TEST_OFFSET + off);
    cdc_log("EXTF: calling, stack headroom %u", (unsigned)stack_headroom());

    // A branch into external flash can fault or stall the bus, and neither
    // shows up as a log line.  The watchdog turns either into a reset carrying
    // the marker, instead of a board that has to be BOOTSELled.
    watchdog_arm(WD_TIMEOUT_MS);
    mark(MARK_CANARY_CALL);
    uint32_t got = fn();
    watchdog_disarm();
    cdc_log("EXTF: called 0x%08X, got 0x%08X, wanted 0x%08X",
          (unsigned)(XIP_CS1_BASE + TEST_OFFSET + off),
          (unsigned)got, (unsigned)EXT_FLASH_CANARY_VALUE);
    cdc_log("EXTF: execute from external flash %s",
          (got == EXT_FLASH_CANARY_VALUE) ? "works" : "gave the wrong answer");

    cmd_erase_by(1u);
}

static void cmd_verify(void) {
    // The bootrom's own choice first, as the baseline that must pass.
    check_through("0Bh serial", s_ctx.enter, s_ctx.exit,
                  s_ctx.m0_timing, M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL);

    for (uint32_t div = 6u; div >= 1u; div--) {
        uint32_t timing = (s_ctx.m0_timing & ~0xFFu) | div;
        check_through("m0 fmt", s_ctx.enter, s_ctx.exit,
                      timing, s_ctx.m0_rfmt, s_ctx.m0_rcmd);
    }

    // Leave window 1 where the firmware would want it rather than at whatever
    // the sweep finished on.
    check_through("restore", s_ctx.enter, s_ctx.exit,
                  s_ctx.m0_timing, s_ctx.m0_rfmt, s_ctx.m0_rcmd);
}


// Report the QSPI state and identify both devices.
//
// Read-only bar the second chip select's GPIO, which has to carry the chip
// select before the probe can drive it.  That GPIO goes back if nothing
// answers, so a board without an external device is left as found.
//
// Returns non-zero when a device answered on chip select 1.
static uint8_t probe_and_report(
    ora_enter_exclusive_mode_fn_t enter_exclusive,
    ora_exit_exclusive_mode_fn_t exit_exclusive
) {
    // What the bootrom and the firmware between them left the QMI in.
    //
    // Window 1 is the interesting one.  The bootrom's flash scan calls
    // flash_select_xip_read_mode, which RP2350 datasheet section 5.4.8.14 says
    // configures both windows, so window 1 should already match window 0 even
    // though this firmware never wrote to it.  That claim is why this is logged
    // before anything here touches the QMI.
    uint32_t m0_timing = QMI_M0_TIMING;
    uint32_t m0_rfmt   = QMI_M0_RFMT;
    uint32_t m0_rcmd   = QMI_M0_RCMD;
    log_window("m0", m0_timing, m0_rfmt, m0_rcmd);
    log_window("m1", QMI_M1_TIMING, QMI_M1_RFMT, QMI_M1_RCMD);

    // Whatever the pin carries when the plugin arrives, reported rather than
    // predicted.  Several things reach this pin before the plugin does - the
    // bootrom on an OTP-programmed board, and the firmware's own setup
    // otherwise - so what it reads is the answer.
    uint32_t ctrl_before = GPIO_CTRL(s_cs1.gpio);
    uint32_t pad_before  = GPIO_PAD(s_cs1.gpio);
    cdc_log("EXTF: gpio%u ctrl=0x%08X pad=0x%08X func=%u iso=%u",
          (unsigned)s_cs1.gpio, (unsigned)ctrl_before, (unsigned)pad_before,
          (unsigned)(ctrl_before & 0x1Fu),
          (unsigned)((pad_before & PAD_ISO) ? 1u : 0u));

    // The probe drives the second chip select through the QMI, so the pin has
    // to carry it.  Output enabled, input enabled so the pad reads back, and
    // the isolation latch cleared last so the pin stays quiet until the mux
    // behind it is settled.  Where the bootrom has already done all that, it
    // is left alone, so the probe says which of the two configured it.
    if ((ctrl_before & 0x1Fu) == GPIO_FUNC_QMI_CS1N) {
        cdc_log("EXTF: gpio%u already carries cs1", (unsigned)s_cs1.gpio);
    } else {
        GPIO_PAD(s_cs1.gpio) = (pad_before & ~PAD_OD) | PAD_IE;
        GPIO_CTRL(s_cs1.gpio) = GPIO_FUNC_QMI_CS1N;
        GPIO_PAD(s_cs1.gpio) &= ~PAD_ISO;
        cdc_log("EXTF: gpio%u muxed to cs1 by the plugin", (unsigned)s_cs1.gpio);
    }

    // Serving runs out of SRAM on PIO and DMA, so taking flash away for the
    // length of the probe leaves it running.  The other core runs the USB
    // plugin out of flash, and exclusive mode is what parks it.
    qmi_probe_result_t result;
    if (enter_exclusive() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed");
        return 0u;
    }
    uint32_t blob[STAGE_PROBE_WORDS];
    qmi_probe_ids_fn_t probe = ORA_STAGED_FN_PTR(qmi_probe_ids_fn_t,
        (uint32_t)(uintptr_t)stage(blob, STAGE_PROBE_WORDS,
                                   __stage_probe_start, __stage_probe_end,
                                   (const void *)&qmi_probe_ids));
    uint32_t primask = irq_disable();
    probe(&result);
    irq_restore(primask);
    exit_exclusive();

    log_device("cs0", &result.cs0);
    log_device("cs1", &result.cs1);

    // The internal device is known to be present, so it is the control.  A
    // silent cs0 means the probe is wrong, and what it said about cs1 is
    // worthless.
    if (!device_present(&result.cs0)) {
        cdc_log("EXTF: cs0 did not answer - probe is at fault, ignore cs1");
        GPIO_CTRL(s_cs1.gpio) = ctrl_before;
        GPIO_PAD(s_cs1.gpio) = pad_before;
        return 0u;
    }

    if (!device_present(&result.cs1)) {
        cdc_log("EXTF: no device on cs1 - not fitted, or not reaching gpio%u",
              (unsigned)s_cs1.gpio);
        GPIO_CTRL(s_cs1.gpio) = ctrl_before;
        GPIO_PAD(s_cs1.gpio) = pad_before;
        return 0u;
    }

    cdc_log("EXTF: cs1 device found");

    // Point window 1 at a read the device can service untouched, and read
    // through it.  A sensible ID over direct mode says the part is there and
    // talks.  This says the QMI can reach it the way the ROM image copy would.
    if (enter_exclusive() != ORA_RESULT_OK) {
        cdc_log("EXTF: enter exclusive mode failed before m1 setup");
        return 0u;
    }
    uint32_t m1_blob[STAGE_M1_WORDS];
    qmi_set_m1_fn_t set_m1 = ORA_STAGED_FN_PTR(qmi_set_m1_fn_t,
        (uint32_t)(uintptr_t)stage(m1_blob, STAGE_M1_WORDS,
                                   __stage_m1_start, __stage_m1_end,
                                   (const void *)&qmi_set_m1));
    uint32_t m1_primask = irq_disable();
    set_m1(QMI_M0_TIMING, M1_RFMT_0BH_SERIAL, M1_RCMD_0BH_SERIAL);
    irq_restore(m1_primask);
    exit_exclusive();

    log_window("m1'", QMI_M1_TIMING, QMI_M1_RFMT, QMI_M1_RCMD);

    // Read through the uncached view, which leaves the running code in the 16KB
    // cache the two chip selects share.
    const volatile uint32_t *cs1 = (const volatile uint32_t *)XIP_CS1_NOCACHE;
    cdc_log("EXTF: 0x%08X: %08X %08X %08X %08X",
          (unsigned)XIP_CS1_NOCACHE,
          (unsigned)cs1[0], (unsigned)cs1[1], (unsigned)cs1[2], (unsigned)cs1[3]);

    return 1u;
}

// Wait for a command byte and run it.
//
// Every write command runs from a keypress.  An erase holds the other core
// parked for as long as the device takes, up to 400ms, and a boot that did
// several back to back left the host unable to talk to the USB interface
// afterwards.  One operation per keypress keeps each stall on its own and its
// result readable before the next starts.
//
// Input arrives on log channel 1, where the USB plugin puts what `onerom
// console` sends.  The host-control plugin normally consumes that channel, and
// only one user plugin loads at a time, so the channel is free while this one
// runs.
static void console_loop(
    ora_lookup_fn_t ora_lookup_fn,
    ora_enter_exclusive_mode_fn_t enter,
    ora_exit_exclusive_mode_fn_t exit
) {
    ora_log_open_read_fn_t open_read = ora_lookup_fn(ORA_ID_LOG_OPEN_READ);
    ora_log_read_fn_t      log_read  = ora_lookup_fn(ORA_ID_LOG_READ);
    if (open_read == NULL || log_read == NULL) {
        cdc_log("EXTF: log read API not available - no console");
        return;
    }
    if (open_read(ORA_LOG_CHANNEL_1) != ORA_RESULT_OK) {
        cdc_log("EXTF: could not claim log channel 1");
        return;
    }

    write_test_init(enter, exit);

    // Polling the channel spins this core, and yielding in the loop is what
    // lets the other one take exclusive mode.  It returns at once while nothing
    // has asked.
    ora_yield_fn_t yield = ora_lookup_fn(ORA_ID_YIELD);

    cdc_log("EXTF: console ready - '?' for commands");

    for (;;) {
        if (yield != NULL) {
            (void)yield(NULL);
        }

        uint8_t  buf[8];
        uint32_t got = 0u;
        if (log_read(ORA_LOG_CHANNEL_1, buf, sizeof(buf), &got) != ORA_RESULT_OK) {
            continue;
        }
        for (uint32_t i = 0u; i < got; i++) {
            switch (buf[i]) {
                case '?':
                    cdc_log("EXTF: ? help, p probe, x hang on purpose");
                    cdc_log("EXTF: e erase, g program direct, v verify, w erase+program+verify+erase");
                    cdc_log("EXTF: E erase via flash_range_erase, X execute from external flash");
                    cdc_log("EXTF: G program via flash_op, R program via flash_range_program");
                    cdc_log("EXTF: b erase+program on the boot's own devinfo");
                    break;
                case 'p':
                    probe_and_report(enter, exit);
                    break;
                case 'x':
                    // Prove the watchdog before anything relies on it.
                    //
                    // Deliberately outside exclusive mode, with interrupts live
                    // and XIP up, so a watchdog that fails to fire leaves this
                    // core spinning while the other keeps USB and the board
                    // reprogrammable.  Inside a critical section the same test
                    // would need the button it exists to save.
                    cdc_log("EXTF: hanging on purpose - watchdog should reset in %ums",
                          (unsigned)WD_TIMEOUT_MS);
                    watchdog_arm(WD_TIMEOUT_MS);
                    mark(MARK_OP_DONE);
                    for (;;) {
                    }
                case 'e':
                    if (s_ctx.ready) { cmd_erase(); }
                    break;
                case 'E':
                    if (s_ctx.ready) { cmd_erase_range(); }
                    break;
                case 'g':
                    if (s_ctx.ready) { cmd_program(); }
                    break;
                case 'G':
                    if (s_ctx.ready) { cmd_program_flash_op(); }
                    break;
                case 'R':
                    if (s_ctx.ready) { cmd_program_range(); }
                    break;
                case 'v':
                    if (s_ctx.ready) { cmd_verify(); }
                    break;
                case 'X':
                    if (s_ctx.ready) { cmd_execute(); }
                    break;
                case 'b':
                    if (s_ctx.ready) { cmd_bootrom_only(); }
                    break;
                case 'w':
                    if (s_ctx.ready) {
                        cdc_log("EXTF: --- write test ---");
                        cmd_erase();
                        cmd_program();
                        cmd_verify();
                        cmd_erase();
                        cdc_log("EXTF: --- write test end ---");
                    }
                    break;
                default:
                    break;
            }
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

    // Channel 0 is what the USB plugin drains to CDC, and claiming it needs no
    // particular firmware build, unlike ora_log().
    if (!cdc_log_init(ora_lookup_fn, "ext-flash")) {
        return;
    }

    ora_enter_exclusive_mode_fn_t enter_exclusive =
        ora_lookup_fn(ORA_ID_ENTER_EXCLUSIVE_MODE);
    ora_exit_exclusive_mode_fn_t exit_exclusive =
        ora_lookup_fn(ORA_ID_EXIT_EXCLUSIVE_MODE);
    if (enter_exclusive == NULL || exit_exclusive == NULL) {
        cdc_log("EXTF: exclusive mode not available");
        return;
    }

    // Whatever the last run left behind, before anything here overwrites it.
    // A value other than idle means that run never reached its end and the
    // watchdog brought the board back.
    mark_t died_at = watchdog_take_mark();

    cdc_log("EXTF: start");

    if (died_at != MARK_IDLE) {
        cdc_log("EXTF: previous run did not finish - step %u page %u, reason 0x%08X",
              (unsigned)((uint32_t)died_at & 0xFFu),
              (unsigned)((uint32_t)died_at >> 8),
              (unsigned)WD_REASON);
    }

    ora_get_clkref_mhz_fn_t get_clkref = ora_lookup_fn(ORA_ID_GET_CLKREF_MHZ);
    if (get_clkref == NULL) {
        cdc_log("EXTF: clkref unavailable - no watchdog, a fault will need BOOTSEL");
    } else {
        watchdog_setup(get_clkref());
        cdc_log("EXTF: watchdog armed for %ums around flash operations",
              (unsigned)WD_TIMEOUT_MS);
    }

    cs1_classify();

    if (!probe_and_report(enter_exclusive, exit_exclusive)) {
        return;
    }

    // A part that has never been written reads back erased.  That is the
    // expected result on a new board: the read path works and the device is
    // empty.
    cdc_log("EXTF: probe done - gpio%u and m1 left configured",
          (unsigned)s_cs1.gpio);

    console_loop(ora_lookup_fn, enter_exclusive, exit_exclusive);

    while (1) {
        __asm volatile ("wfi");
    }
}
