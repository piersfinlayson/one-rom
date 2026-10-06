// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// The parts of the external flash tester that run from RAM.
//
// These stay in flash.  The caller copies the one it needs onto its stack and
// runs it there, so the RAM cost is the largest single routine for the length
// of one call.  Holding them all in RAM for the life of the plugin would not
// fit: a user plugin has 1KB for its static RAM and its stack together.
//
// This file is compiled -fPIC -fno-plt, since each routine runs from wherever
// the stack put it.  Two rules follow.  Every call a staged routine makes stays
// inside its own section, so the copy takes callers and callees together and
// their relative distances survive.  And the code here doesn't read a string
// or any other constant from .rodata, which stays in flash and is unreadable
// while the routine runs.
//
// The caller copies a whole section and works out where the entry landed from
// the offset of its address within that section, so the functions can be in
// any order.

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
// copied, so the probe has a second copy rather than calling into the QSPI
// section.  Flash is cheap here and the stack is not.
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

    // The divisor may change at any time, and takes effect from the next byte.
    QMI_DIRECT_CSR = (saved & ~QMI_CSR_CLKDIV_BITS)
                   | (QMI_DIRECT_CLKDIV << QMI_CSR_CLKDIV_LSB) | QMI_CSR_EN;

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


ORA_SECTION(".stage_op") void ext_flash_op_critical(
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
) {
    // The whole sequence sits between one exit from XIP and one restore.  The
    // bootrom's flash routines need the device in its serial command state and
    // put it back afterwards, so an operation issued after XIP returns reaches
    // a device that has stopped listening.
    connect_internal_flash();
    mark(MARK_CONNECTED);
    exit_xip();
    mark(MARK_XIP_EXITED);

    *result_out = flash_op(flags, addr, size, buf);
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
// The bootrom's flash_op needs more stack than this plugin can spare once a
// 256 byte page buffer is on it.  This does the same job over the QSPI bus,
// generating the page as it clocks it out, so it doesn't need a buffer or the
// bootrom.
// ---------------------------------------------------------------------------

ORA_SECTION(".stage_qspi") __attribute__((noinline)) static uint32_t qmi_direct_begin(void) {
    uint32_t saved = QMI_DIRECT_CSR;
    QMI_DIRECT_CSR = (saved & ~QMI_CSR_CLKDIV_BITS)
                   | (QMI_DIRECT_CLKDIV << QMI_CSR_CLKDIV_LSB) | QMI_CSR_EN;
    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }
    return saved;
}

// Send a command without an address or a reply to the device connected to
// chip select 1.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static void qmi_cs1_cmd(uint8_t cmd) {
    QMI_DIRECT_CSR |= QMI_CSR_ASSERT_CS1N;
    (void)qmi_xfer(cmd);
    while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
    }
    QMI_DIRECT_CSR &= ~QMI_CSR_ASSERT_CS1N;
}

// Clock out a command and a 24-bit big-endian address, leaving the chip select
// asserted so a caller with data to send can continue.
ORA_SECTION(".stage_qspi") __attribute__((noinline)) static void qmi_cs1_cmd_addr(uint8_t cmd, uint32_t offset) {
    QMI_DIRECT_CSR |= QMI_CSR_ASSERT_CS1N;
    (void)qmi_xfer(cmd);
    (void)qmi_xfer((uint8_t)(offset >> 16));
    (void)qmi_xfer((uint8_t)(offset >> 8));
    (void)qmi_xfer((uint8_t)offset);
}

// Block until the device finishes an erase or a program.
//
// While a 25-series part is busy, a status read is the only command it
// accepts, so this polls it.  A page program takes up to 3ms, and this runs
// with interrupts masked and the other core parked.
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

ORA_SECTION(".stage_qspi") void qmi_cs1_program(uint32_t offset, uint32_t pages) {
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
            (void)qmi_xfer(ext_flash_pattern(offset + base + i));
        }
        while (QMI_DIRECT_CSR & QMI_CSR_BUSY) {
        }
        QMI_DIRECT_CSR &= ~QMI_CSR_ASSERT_CS1N;

        qmi_cs1_wait_ready();
    }

    // The device connected to chip select 0 received only read commands, and
    // this one is back in its serial command state, so XIP resumes on the M0
    // and M1 configuration already in place.
    QMI_DIRECT_CSR = saved;
    mark(MARK_XIP_RESTORED);
}
