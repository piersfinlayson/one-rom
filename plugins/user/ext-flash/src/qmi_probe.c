// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// The parts of the external flash probe that run from RAM.
//
// These stay in flash.  The caller copies the one it needs onto its stack and
// runs it there, so the RAM cost is the largest single routine for the length
// of one call.  Holding them all in RAM for the life of the plugin would not
// fit: a user plugin has 1KB for its static RAM and its stack together.
//
// This file is compiled -fPIC -fno-plt, since each routine runs from wherever
// the stack put it.  Two rules follow.  Every call a staged routine makes stays
// inside its own section, so the copy takes callers and callees together and
// their relative distances survive.  And nothing here reads a string or any
// other constant, since .rodata stays in flash, which is unreadable while the
// routine runs.
//
// The caller copies a whole section and works out where the entry landed from
// the offset of its address within that section, so the order functions appear
// in is free.

#include <stdint.h>

#include <plugin.h>

#include "qmi_probe.h"
#include "recover.h"

// Exchange one byte in direct mode.
//
// noinline, like the other helpers here.  A staged section is copied whole onto
// the stack, so inlined copies cost stack the caller needs.
//
// Every byte clocked out clocks one in, so a command byte leaves an RX entry to
// pop whatever it holds.  DIRECT_TX.NOPUSH would suppress it, but pushing and
// popping in lockstep keeps one path for the command and its data, whatever
// depth the FIFOs have.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static uint8_t qmi_xfer(uint8_t tx) {
    while (QMI_DIRECT_CSR & QMI_CSR_TXFULL) {
    }
    QMI_DIRECT_TX = (uint32_t)tx;
    while (QMI_DIRECT_CSR & QMI_CSR_RXEMPTY) {
    }
    return (uint8_t)QMI_DIRECT_RX;
}

// The probe's own copy of qmi_xfer.
//
// A staged routine may only call into its own section, since that is what gets
// copied, so the probe carries a second copy rather than reaching into the
// QSPI section.  Flash is cheap here and the stack is not.
ORA_SECTION(".stage_probe") __attribute__((noinline)) static uint8_t probe_xfer(uint8_t tx) {
    while (QMI_DIRECT_CSR & QMI_CSR_TXFULL) {
    }
    QMI_DIRECT_TX = (uint32_t)tx;
    while (QMI_DIRECT_CSR & QMI_CSR_RXEMPTY) {
    }
    return (uint8_t)QMI_DIRECT_RX;
}

// Issue one command on one chip select and read its reply.
//
// assert_bit selects the device.  The chip select drops once BUSY goes low,
// since the last byte is still shifting while it is set and an early drop
// truncates the frame.
ORA_SECTION(".stage_probe") __attribute__((noinline)) static void qmi_read_reg(
    uint32_t assert_bit,
    uint8_t  cmd,
    uint8_t *out,
    uint32_t len
) {
    QMI_DIRECT_CSR |= assert_bit;

    (void)probe_xfer(cmd);
    for (uint32_t i = 0u; i < len; i++) {
        out[i] = probe_xfer(0x00u);
    }

    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }
    QMI_DIRECT_CSR &= ~assert_bit;
}

ORA_SECTION(".stage_probe") void qmi_probe_ids(qmi_probe_result_t *out) {
    uint32_t saved = QMI_DIRECT_CSR;

    // Enter direct mode at the divisor already in DIRECT_CSR.  The reset value
    // of 6 suits any of these parts at any clock this firmware runs at, and a
    // probe gains nothing from going faster.
    QMI_DIRECT_CSR = saved | QMI_CSR_EN;

    // A memory-mapped transfer already under way when direct mode was enabled
    // keeps running, and asserting a chip select across it corrupts both.
    // Direct mode blocks new ones, so this wait ends.
    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }

    qmi_read_reg(QMI_CSR_ASSERT_CS0N, FLASH_CMD_JEDEC_ID, out->cs0.jedec, 3u);
    qmi_read_reg(QMI_CSR_ASSERT_CS0N, FLASH_CMD_READ_SR2, &out->cs0.sr2, 1u);
    qmi_read_reg(QMI_CSR_ASSERT_CS1N, FLASH_CMD_JEDEC_ID, out->cs1.jedec, 3u);
    qmi_read_reg(QMI_CSR_ASSERT_CS1N, FLASH_CMD_READ_SR2, &out->cs1.sr2, 1u);

    // qmi_read_reg has already deasserted both chip selects, and must have.
    // ASSERT_CS0N and ASSERT_CS1N drive the pin with EN set or clear, so one
    // left behind would hold a device selected against every memory-mapped
    // access that follows.
    QMI_DIRECT_CSR = saved;
    mark(MARK_XIP_RESTORED);
}


ORA_SECTION(".stage_m1") void qmi_set_m1(uint32_t timing, uint32_t rfmt, uint32_t rcmd) {
    QMI_M1_TIMING = timing;
    QMI_M1_RFMT   = rfmt;
    QMI_M1_RCMD   = rcmd;
}


// The routine the execute test writes into the external flash and calls there.
//
// Returns a value with no other reason to appear, so the caller can tell the
// routine ran from what it got back.  Calls nothing and reads nothing outside
// itself, since everything else sits at addresses that mean nothing once these
// bytes are in the external device.
//
// used and noinline because nothing calls it here - it is reached only by its
// address, and the compiler would otherwise drop it.
ORA_SECTION(".stage_exec") __attribute__((used, noinline))
uint32_t ext_flash_canary(void) {
    return EXT_FLASH_CANARY_VALUE;
}

ORA_SECTION(".stage_op") void ext_flash_op_critical(
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
) {
    // The whole sequence sits between one exit from XIP and one restore.  The
    // bootrom's flash routines need the device in its serial command state and
    // put it back afterwards, so an operation issued after XIP returns reaches
    // a device that has stopped listening.
    connect_internal_flash();
    mark(MARK_CONNECTED);
    exit_xip();
    mark(MARK_XIP_EXITED);

    if (flash_op != NULL) {
        *result_out = flash_op(flags, addr, size, buf);
    } else {
        // Returns void, so a failure shows up in the readback rather than in a
        // status.
        range_erase(addr, size, FLASH_BLOCK_SIZE, FLASH_BLOCK_ERASE_CMD);
        *result_out = 0;
    }
    mark(MARK_OP_DONE);

    flush_cache();
    mark(MARK_CACHE_FLUSHED);

    // Restores the mode the bootrom discovered, on both windows, which is why
    // the caller reprograms window 1 afterwards rather than before.
    select_xip(mode, clkdiv);
    mark(MARK_XIP_RESTORED);
}


// ---------------------------------------------------------------------------
// Direct mode program
//
// The bootrom's flash_op wants more stack than this plugin can spare once a
// 256 byte page buffer is on it.  This does the same job over the QSPI bus,
// generating the page as it clocks it out, so it needs no buffer and no
// bootrom.
// ---------------------------------------------------------------------------

ORA_SECTION(".stage_qspi") __attribute__((noinline)) static uint32_t qmi_direct_begin(void) {
    uint32_t saved = QMI_DIRECT_CSR;
    QMI_DIRECT_CSR = saved | QMI_CSR_EN;
    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }
    return saved;
}

// Send a command with no address and no reply, on chip select 1.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static void qmi_cs1_cmd(uint8_t cmd) {
    QMI_DIRECT_CSR |= QMI_CSR_ASSERT_CS1N;
    (void)qmi_xfer(cmd);
    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }
    QMI_DIRECT_CSR &= ~QMI_CSR_ASSERT_CS1N;
}

// Clock out a command and a 24-bit big-endian address, leaving the chip select
// asserted so a caller with data to send can carry straight on.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static void qmi_cs1_cmd_addr(uint8_t cmd, uint32_t offset) {
    QMI_DIRECT_CSR |= QMI_CSR_ASSERT_CS1N;
    (void)qmi_xfer(cmd);
    (void)qmi_xfer((uint8_t)(offset >> 16));
    (void)qmi_xfer((uint8_t)(offset >> 8));
    (void)qmi_xfer((uint8_t)offset);
}

// Block until the device finishes an erase or a program.
//
// A 25-series part answers only a status read while it is busy, so polling is
// how to tell.  An erase can take it 400ms, and this runs with interrupts
// masked and the other core parked.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static void qmi_cs1_wait_ready(void) {
    uint8_t status;
    do {
        QMI_DIRECT_CSR |= QMI_CSR_ASSERT_CS1N;
        (void)qmi_xfer(FLASH_CMD_READ_SR1);
        status = qmi_xfer(0x00u);
        while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
        }
        QMI_DIRECT_CSR &= ~QMI_CSR_ASSERT_CS1N;
    } while (status & FLASH_SR1_BUSY);
}

ORA_SECTION(".stage_qspi") void qmi_cs1_program_sector(uint32_t offset, uint32_t pages) {
    uint32_t saved = qmi_direct_begin();
    mark(MARK_XIP_EXITED);

    for (uint32_t pg = 0u; pg < pages; pg++) {
        uint32_t base = pg * FLASH_PAGE_SIZE;
        mark_page(MARK_OP_DONE, pg);

        // The device latches write enable on the rising edge of chip select and
        // clears it when the program completes, so it goes as its own frame
        // ahead of the program command.
        qmi_cs1_cmd(FLASH_CMD_WRITE_EN);

        qmi_cs1_cmd_addr(FLASH_CMD_PAGE_PROG, offset + base);
        for (uint32_t i = 0u; i < FLASH_PAGE_SIZE; i++) {
            (void)qmi_xfer(ext_flash_pattern(base + i));
        }
        while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
        }
        QMI_DIRECT_CSR &= ~QMI_CSR_ASSERT_CS1N;

        qmi_cs1_wait_ready();
    }

    // The device on chip select 0 saw read commands alone, and this one is back
    // in its serial command state, so XIP resumes on the M0 and M1
    // configuration already in place.
    QMI_DIRECT_CSR = saved;
    mark(MARK_XIP_RESTORED);
}


// Program a run of pages through the bootrom.
//
// One routine for both bootrom routes, since they differ only in the call at
// the centre.  Pass flash_op to use the high-level route, or leave it NULL and
// pass flash_range_program to use the low-level one.
//
// addr means different things to the two.  flash_op takes an address in the
// space that spans both chip selects, so chip select 1 starts at 0x11000000
// (RP2350 datasheet section 5.4.8.9).  flash_range_program takes an offset from
// the start of flash (section 5.4.8.11), and the datasheet says nothing about
// which chip select an offset reaches, which is what this is here to find out.
//
// page is refilled here between calls rather than by the caller, because the
// caller lives in flash and flash stays unreadable throughout.
ORA_SECTION(".stage_bootprog") void ext_flash_program_sector_bootrom(
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
) {
    connect_internal_flash();
    mark(MARK_CONNECTED);
    exit_xip();
    mark(MARK_XIP_EXITED);

    int32_t rc = 0;
    for (uint32_t pg = 0u; pg < pages; pg++) {
        uint32_t base = pg * FLASH_PAGE_SIZE;
        mark_page(MARK_OP_DONE, pg);

        for (uint32_t i = 0u; i < FLASH_PAGE_SIZE; i++) {
            page[i] = ext_flash_pattern(base + i);
        }

        if (flash_op != NULL) {
            rc = flash_op(flags, addr + base, FLASH_PAGE_SIZE, page);
            if (rc != 0) {
                break;
            }
        } else {
            // Returns void, so a failure here shows up in the readback rather
            // than in a status.
            range_program(addr + base, page, FLASH_PAGE_SIZE);
        }
    }
    *result_out = rc;

    flush_cache();
    mark(MARK_CACHE_FLUSHED);
    select_xip(mode, clkdiv);
    mark(MARK_XIP_RESTORED);
}

