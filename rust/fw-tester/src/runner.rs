// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Hot GPIO drive loop, shared between pio-tester and plugin-api-tester.

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::ChipType;
use onerom_fw_emulator::Emulator;

use crate::driver;
use crate::pin_cache::PinCache;
use crate::timing;

// ── Per bit mode ──────────────────────────────────────────────────────────────

/// Drive the emulated ROM bus for every address in the oracle, comparing
/// the PIO output against the expected bytes.
///
/// `gap_gpios` are absolute GPIOs the firmware declares as `GpioOverLow` — non
/// address pins that sit inside the address-read window and must be held low.
/// When non-empty, each address is read a second time with those GPIOs driven
/// to a per-address toggling pattern (gap `i` carries bit `i` of the address
/// index), so every gap sees both levels across the sweep.  The override must
/// hold the firmware's view of them at 0, so the served byte must be unchanged;
/// any divergence is counted in `forced_low_failures`, kept distinct from
/// ordinary data mismatches.  Pass `&[]` to skip the check entirely.
///
/// Returns `(reads, failures, bus_failures, forced_low_failures)`.
#[allow(clippy::too_many_arguments)]
pub fn run_mode(
    emulator: &Emulator,
    cache: &PinCache,
    oracle: &[u8],
    mode: u8,
    cycles_addr_before_cs: u32,
    cycles_cs_to_data: u32,
    set_idx: usize,
    chip_idx: usize,
    background_mask: (u64, u64),
    gap_gpios: &[u8],
) -> (u64, u64, u64, u64) {
    // Pre-compute the BYTE# mask so it can be merged into every drive_gpios
    // call.  epio_drive_gpios_ext resets every GPIO that is *not* in the
    // supplied mask to its configured pull state on each call (pull-none pins
    // go to the float mode value, which defaults to 1/high).  A one-shot
    // drive before the loop would be immediately overwritten by the first
    // phase1/phase2 call.  Merging into every call holds the level correctly
    // throughout the pass.
    let byte_mask: (u64, u64) = if let Some(gpio) = cache.byte_n_gpio {
        let bm = driver::byte_n_mask(gpio, mode);
        debug!(
            "BYTE# gpio={} mode={} mask={:#018x} levels={:#018x}",
            gpio, mode, bm.0, bm.1
        );
        bm
    } else {
        (0u64, 0u64)
    };

    // Merge BYTE# with the caller-supplied background mask to produce the
    // single constant mask applied on every GPIO drive call.
    //
    // background_mask holds GPIOs that must be kept at a fixed level:
    //   - Single chip sets:  (0, 0) — nothing extra to hold.
    //   - Multi-ROM sets:    deasserted CS lines of all non-active chips, plus
    //                        any extra primary address GPIOs held at the
    //                        current combo level.
    //   - Banked sets:       all X pins driven to the level selecting the bank.
    let const_mask = driver::merge(byte_mask, background_mask);

    let (iter_count, addr_shift) = if mode == 16 {
        (oracle.len() / 2, 1usize)
    } else {
        (oracle.len(), 0usize)
    };

    // In 16-bit mode, addr_gpios[0] is A-1, which is also D15 — a data
    // output pin in word mode.  Driving it as an address pin would interfere
    // with the data bus and cause false bus violations.  Skip it and use only
    // A0-A17 (indices [1..]) with the word index (addr_idx) as the drive
    // address, so bit 0 of addr_idx maps to A0, bit 1 to A1, etc.
    //
    // In 8-bit mode, use all address GPIOs including A-1 at index 0 (bit 0
    // of the byte address becomes the low/high byte select).
    //
    // addr_shift is retained solely for computing phys_addr for log messages,
    // which uses byte addresses in both modes.
    let addr_gpios: &[Vec<u8>] = if mode == 16 {
        &cache.addr_gpios[1..]
    } else {
        &cache.addr_gpios
    };

    debug!(
        "Mode {}bit: {} iterations, addr_shift={}, \
         cycles_addr_before_cs={}, cycles_cs_to_data={}",
        mode, iter_count, addr_shift, cycles_addr_before_cs, cycles_cs_to_data
    );

    // Pre-compute the deasserted control mask — reused on every iteration.
    let ctrl_deasserted = driver::ctrl_mask(&cache.control_lines, false);
    let ctrl_active = driver::ctrl_mask(&cache.control_lines, true);

    debug!(
        "ctrl_deasserted: mask={:#018x} levels={:#018x}",
        ctrl_deasserted.0, ctrl_deasserted.1
    );
    debug!(
        "ctrl_active:     mask={:#018x} levels={:#018x}",
        ctrl_active.0, ctrl_active.1
    );

    // The data GPIO slice used for bus-state checks.  For 16-bit mode all 16
    // pins are live.  For 8-bit mode only the low byte lane is driven by the
    // chip (BYTE# keeps D8-D15 tristated on 27C400-family devices), so we
    // limit the check to the first 8 GPIOs in the cache.
    let driven_check_gpios: &[u8] = if mode == 16 {
        &cache.data_gpios[..16]
    } else {
        &cache.data_gpios[..8.min(cache.data_gpios.len())]
    };

    // The data GPIO slice for byte extraction and mismatch logging in 8-bit
    // mode.  Even on 16-bit-capable chips the cache has 16 data GPIOs, but
    // in byte mode only D0-D7 are driven.
    let data_gpios_8 = &cache.data_gpios[..8.min(cache.data_gpios.len())];

    // CS-deasserted drive merged with const_mask so all constant levels are
    // held between reads.
    let deassert_drive = driver::merge(ctrl_deasserted, const_mask);

    // Absolute mask of the forced-low gap GPIOs.  Empty (0) when the slot has
    // no gaps, in which case the adversarial second read is skipped entirely
    // and this pass costs exactly what it did before.
    let gap_mask: u64 = gap_gpios.iter().fold(0u64, |m, &g| m | (1u64 << g));

    let mut reads = 0u64;
    let mut failures = 0u64;
    let mut bus_failures = 0u64;
    let mut forced_low_failures = 0u64;

    for addr_idx in 0..iter_count {
        // phys_addr is the byte address, used for log messages only.
        // In 16-bit mode this is addr_idx*2 (byte offset of the word).
        // In 8-bit mode addr_shift==0 so it equals addr_idx.
        let phys_addr = addr_idx << addr_shift;

        // The GPIO drive address is always addr_idx:
        // - 16-bit: addr_gpios is [1..] so bit 0 of addr_idx maps to A0. ✓
        // - 8-bit:  addr_gpios is full slice, addr_idx is the byte address. ✓
        let drive_addr = addr_idx;

        // ── Phase 1: address valid, CS inactive ──────────────────────────────
        let phase1 = driver::merge(
            driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_deasserted),
            const_mask,
        );
        if addr_idx < 2 {
            debug!(
                "addr={} phase1: mask={:#018x} levels={:#018x}",
                phys_addr, phase1.0, phase1.1
            );
        }
        emulator.drive_gpios(phase1.0, phase1.1);
        emulator.step_cycles(cycles_addr_before_cs);

        // ── Phase 2: CS asserted ─────────────────────────────────────────────
        let phase2 = driver::merge(
            driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_active),
            const_mask,
        );
        if addr_idx < 2 {
            debug!(
                "addr={} phase2: mask={:#018x} levels={:#018x}",
                phys_addr, phase2.0, phase2.1
            );
        }
        emulator.drive_gpios(phase2.0, phase2.1);
        emulator.step_cycles(cycles_cs_to_data);

        // ── Phase 3: read and compare ─────────────────────────────────────────
        let pin_states = emulator.read_pin_states();
        let driven_pins = emulator.read_driven_pins();
        if addr_idx < 2 {
            debug!("addr={} pin_states={:#018x}", phys_addr, pin_states);
            debug!("addr={} driven_pins={:#018x}", phys_addr, driven_pins);
        }

        // Data lines must be driven while CS is active.
        if !driven_check_gpios
            .iter()
            .all(|&g| driven_pins & (1u64 << g) != 0)
        {
            bus_failures += 1;
            log_bus_violation(
                set_idx,
                chip_idx,
                Some(phys_addr),
                "not all driven (CS active)",
                driven_pins,
                driven_check_gpios,
                bus_failures,
            );
        }

        if mode == 16 {
            let lo = driver::extract_byte(pin_states, &cache.data_gpios[..8]);
            let hi = driver::extract_byte(pin_states, &cache.data_gpios[8..16]);

            reads += 2;
            let exp_lo = oracle[addr_idx * 2];
            let exp_hi = oracle[addr_idx * 2 + 1];

            if lo != exp_lo {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx * 2,
                    lo,
                    exp_lo,
                    driven_pins,
                    &cache.data_gpios[..8],
                    failures,
                );
            }
            if hi != exp_hi {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx * 2 + 1,
                    hi,
                    exp_hi,
                    driven_pins,
                    &cache.data_gpios[8..16],
                    failures,
                );
            }
        } else {
            // 8-bit mode: only D0-D7 are active (D8-D15 are tristated by
            // BYTE# on 27C400-family devices).
            let byte = driver::extract_byte(pin_states, data_gpios_8);
            reads += 1;
            let expected = oracle[addr_idx];
            if byte != expected {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx,
                    byte,
                    expected,
                    driven_pins,
                    data_gpios_8,
                    failures,
                );
            }
        }

        // ── Phase 4: deassert CS and settle ──────────────────────────────────
        emulator.drive_gpios(deassert_drive.0, deassert_drive.1);
        emulator.step_cycles(timing::CYCLES_AFTER_READ);

        // Data lines must have released after CS deassert.
        let driven_after = emulator.read_driven_pins();
        if driven_check_gpios
            .iter()
            .any(|&g| driven_after & (1u64 << g) != 0)
        {
            bus_failures += 1;
            log_bus_violation(
                set_idx,
                chip_idx,
                Some(phys_addr),
                "still driven (CS deasserted)",
                driven_after,
                driven_check_gpios,
                bus_failures,
            );
        }

        // ── Phase 5: adversarial forced-low check ────────────────────────────
        // Re-read this address with the gap GPIOs driven to a per-address
        // toggling pattern (gap i ← bit i of addr_idx).  These pins carry no
        // address signal; the firmware's GpioOverLow overrides must hold its
        // view of them at 0, so the served byte must be identical to the oracle
        // regardless of what is driven here.  A divergence is the regression
        // this check exists to catch and is counted separately.
        if gap_mask != 0 {
            let gap_levels: u64 = gap_gpios.iter().enumerate().fold(0u64, |acc, (i, &g)| {
                if (drive_addr >> i) & 1 == 1 {
                    acc | (1u64 << g)
                } else {
                    acc
                }
            });
            let gap_extra = driver::merge(const_mask, (gap_mask, gap_levels));

            let gp1 = driver::merge(
                driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_deasserted),
                gap_extra,
            );
            emulator.drive_gpios(gp1.0, gp1.1);
            emulator.step_cycles(cycles_addr_before_cs);

            let gp2 = driver::merge(
                driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_active),
                gap_extra,
            );
            emulator.drive_gpios(gp2.0, gp2.1);
            emulator.step_cycles(cycles_cs_to_data);

            let gap_states = emulator.read_pin_states();
            if mode == 16 {
                let lo = driver::extract_byte(gap_states, &cache.data_gpios[..8]);
                let hi = driver::extract_byte(gap_states, &cache.data_gpios[8..16]);
                if lo != oracle[addr_idx * 2] {
                    forced_low_failures += 1;
                    log_forced_low(
                        set_idx,
                        chip_idx,
                        addr_idx * 2,
                        lo,
                        oracle[addr_idx * 2],
                        gap_gpios,
                        forced_low_failures,
                    );
                }
                if hi != oracle[addr_idx * 2 + 1] {
                    forced_low_failures += 1;
                    log_forced_low(
                        set_idx,
                        chip_idx,
                        addr_idx * 2 + 1,
                        hi,
                        oracle[addr_idx * 2 + 1],
                        gap_gpios,
                        forced_low_failures,
                    );
                }
            } else {
                let byte = driver::extract_byte(gap_states, data_gpios_8);
                if byte != oracle[addr_idx] {
                    forced_low_failures += 1;
                    log_forced_low(
                        set_idx,
                        chip_idx,
                        addr_idx,
                        byte,
                        oracle[addr_idx],
                        gap_gpios,
                        forced_low_failures,
                    );
                }
            }

            // Release CS and the gap drive, settle, before the next address.
            emulator.drive_gpios(deassert_drive.0, deassert_drive.1);
            emulator.step_cycles(timing::CYCLES_AFTER_READ);
        }
    }

    // Enumerate only the discriminating (non-commoned) control lines. A commoned
    // line (Multi primary: a CS line shared across the set) is asserted on every
    // real read and selects nothing, so toggling it independently is unphysical
    // — and, being in the CS-detect range, an asserted commoned line fires the
    // gate alone. Hold every commoned line deasserted (idle level) throughout
    // and vary only the selects, so "chip not selected" is modelled faithfully.
    let select_lines: Vec<_> = cache
        .control_lines
        .iter()
        .filter(|cl| !cl.commoned)
        .collect();
    let commoned_deasserted: (u64, u64) = cache
        .control_lines
        .iter()
        .filter(|cl| cl.commoned)
        .map(|cl| driver::ctrl_mask(std::slice::from_ref(cl), false))
        .fold((0u64, 0u64), |acc, m| driver::merge(acc, m));
    let combo_const = driver::merge(const_mask, commoned_deasserted);

    let n = select_lines.len();
    if n > 0 {
        let all_asserted: u64 = (1u64 << n) - 1;
        debug!(
            "Mode {}bit combo test: {} select line(s) ({} commoned held deasserted), \
             {} non-active combinations",
            mode,
            n,
            cache.control_lines.len() - n,
            all_asserted
        );

        for combo in 0u64..all_asserted {
            let ctrl = select_lines
                .iter()
                .enumerate()
                .map(|(i, cl)| driver::ctrl_mask(std::slice::from_ref(*cl), (combo >> i) & 1 == 1))
                .fold((0u64, 0u64), |acc, m| driver::merge(acc, m));
            let phase = driver::merge(
                driver::merge(driver::addr_mask(0, addr_gpios), ctrl),
                combo_const,
            );
            emulator.drive_gpios(phase.0, phase.1);
            emulator.step_cycles(cycles_cs_to_data);

            let driven_combo = emulator.read_driven_pins();
            if driven_check_gpios
                .iter()
                .any(|&g| driven_combo & (1u64 << g) != 0)
            {
                bus_failures += 1;
                log_bus_violation(
                    set_idx,
                    chip_idx,
                    None,
                    &format!("data driven for non-active CS combo {:#b}", combo),
                    driven_combo,
                    driven_check_gpios,
                    bus_failures,
                );
            }
        }

        emulator.drive_gpios(deassert_drive.0, deassert_drive.1);
        emulator.step_cycles(timing::CYCLES_AFTER_READ);
    }

    (reads, failures, bus_failures, forced_low_failures)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Log a data bus violation (lines unexpectedly driven or unexpectedly
/// released), capped at 5 per mode pass to avoid flooding the log for
/// systematic failures.
fn log_bus_violation(
    set: usize,
    chip: usize,
    addr: Option<usize>,
    desc: &str,
    driven_pins: u64,
    data_gpios: &[u8],
    count: u64,
) {
    if count <= 5 {
        let drive_state: String = data_gpios
            .iter()
            .rev()
            .map(|&g| {
                if driven_pins & (1u64 << g) != 0 {
                    'y'
                } else {
                    'n'
                }
            })
            .collect();
        match addr {
            Some(a) => error!(
                "BUS set={} chip={} addr=0x{:04X}: {} driven=[{}]",
                set, chip, a, desc, drive_state
            ),
            None => error!(
                "BUS set={} chip={}: {} driven=[{}]",
                set, chip, desc, drive_state
            ),
        }
    } else if count == 6 {
        error!("(further bus violations suppressed for this mode pass)");
    }
}

/// Log a byte mismatch, capped at 5 per mode pass to avoid flooding the log
/// for systematic failures.
fn log_mismatch(
    set: usize,
    chip: usize,
    addr: usize,
    got: u8,
    expected: u8,
    driven_pins: u64,
    data_gpios: &[u8],
    count: u64,
) {
    if count <= 5 {
        let drive_state: String = data_gpios
            .iter()
            .rev()
            .map(|&g| {
                if driven_pins & (1u64 << g) != 0 {
                    'y'
                } else {
                    'n'
                }
            })
            .collect();
        error!(
            "MISMATCH set={} chip={} addr=0x{:04X}: got=0x{:02X} expected=0x{:02X} driven=[{}]",
            set, chip, addr, got, expected, drive_state,
        );
    } else if count == 6 {
        error!("(further mismatches suppressed for this mode pass)");
    }
}

/// Log a forced-low override failure: a non-address GPIO that should be held
/// low by a `GpioOverLow` override changed the served byte when toggled.
/// Capped at 5 per mode pass.
fn log_forced_low(
    set: usize,
    chip: usize,
    addr: usize,
    got: u8,
    expected: u8,
    gap_gpios: &[u8],
    count: u64,
) {
    if count <= 5 {
        error!(
            "FORCED-LOW OVERRIDE set={} chip={} addr=0x{:04X}: served 0x{:02X}, \
             expected 0x{:02X} with non-address GPIO(s) {:?} toggled — \
             firmware did not hold them low",
            set, chip, addr, got, expected, gap_gpios,
        );
    } else if count == 6 {
        error!("(further forced-low failures suppressed for this mode pass)");
    }
}

// ── Timing helpers ────────────────────────────────────────────────────────────

/// Returns the number of cycles to wait between driving the address and
/// asserting CS, for the given chip type.
///
/// 27C400/27C200 chips require a longer settling time because the BYTE#
/// handling path in the PIO address-read loop adds extra cycles.
pub fn addr_before_cs_cycles(chip_type: ChipType) -> u32 {
    if chip_type.bit_modes().contains(&16) {
        timing::CYCLES_27C400_ADDR_BEFORE_CS
    } else {
        timing::CYCLES_ADDR_BEFORE_CS
    }
}

/// Returns the number of cycles to wait between asserting CS and reading the
/// data GPIOs, for the given chip type and bit mode.
///
/// 27C400/27C200 chips in 8-bit mode require additional cycles because of
/// the BYTE# pin handling path in the PIO program.
pub fn cs_to_data_cycles(chip_type: ChipType, mode: u8) -> u32 {
    if mode == 16 {
        timing::CYCLES_CS_TO_DATA
    } else if chip_type.bit_modes().contains(&16) {
        timing::CYCLES_27C400_CS_TO_DATA_BYTE
    } else {
        timing::CYCLES_CS_TO_DATA
    }
}
