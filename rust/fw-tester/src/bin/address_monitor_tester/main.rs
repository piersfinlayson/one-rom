// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Address-Monitor Tester
//!
//! Drives the firmware's address-monitor plugin API (`setup_address_monitor`,
//! `init_knock`, `start_address_monitor`, `wait_for_knock`,
//! `get_address_monitor_ring_write_pos`) against the PIO/DMA emulator and
//! verifies that CS-active bus accesses are captured into the ring buffer and
//! that a knock sequence is detected.  The monitor is what an RBCP plugin is
//! built on; this exercises the monitor itself, independent of RBCP semantics.
//!
//! Layer 1 (capture pipeline) is deterministic: it drives one CS-active access
//! and asserts the ring write pointer advanced and the captured word demangles
//! to the driven address.  Layer 2 drives a full `"!RBCP!"` knock through the
//! real (blocking) `wait_for_knock`, fed by the yield hook, with a watchdog
//! timeout so a broken capture path fails the case rather than hanging.
//! Layer 3 starts the monitor in standby.  Accesses must be captured with the
//! data pins undriven, and once standby is off, with them driven.  Layer 4
//! checks the CS monitor's arming and release timing - see [`min_cs_act`].
//!
//! Addresses are driven on, and checked in, the *observed* (bus) address space
//! — the lines the device actually monitors, which on the 40-pin variant
//! exclude the ROM's least-significant address line.  That is the space
//! host-to-device command signalling travels in.  A 16-bit-capable chip is
//! exercised in both `/BYTE` modes, and in byte mode Layer 1 additionally
//! drives A-1 (the byte-within-word select) both ways and requires the captured
//! address to be identical — A-1 must never leak into command decode.
//!
//! Env: `CONFIG` (config JSON), `BOARD` (e.g. `fire-24-a`), optional
//! `BASE_DIR`, `ONEROM_LOG=1`.  Exits 0 if all cases pass, 1 otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::driver;
use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_gen::{ChipConfig, Config};
use onerom_metadata::onerom_addr_monitor_pio_t as MonitorPio;
use onerom_metadata::onerom_pio_block_claim_t as PioClaim;
use onerom_metadata::{MaybeKnown, RomSlotType};

use onerom_fw_tester::geometry;
use onerom_fw_tester::min_cs_act;
use onerom_fw_tester::pin_cache::PinCache;

// ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS (a #define, not surfaced by bindgen).
const WAIT_FLAG_DEBOUNCE_CS: u32 = 0x0000_0001;

// Ring geometry: 64 32-bit entries (256 bytes), placed near the top of SRAM in
// the region the plugin's ring/stack occupy on real hardware (above the ROM
// table).
const RING_ENTRIES_LOG2: u8 = 6;
const RING_DATA_SIZE: u8 = 32;
const RING_BASE: u32 = 0x2008_1000;

// Knock sequence "!RBCP!" matched against A0-A7.
const KNOCK: [u32; 6] = [
    b'!' as u32,
    b'R' as u32,
    b'B' as u32,
    b'C' as u32,
    b'P' as u32,
    b'!' as u32,
];
const KNOCK_BITS: u8 = 8;

// Watchdog for the blocking wait_for_knock case.
const KNOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// A correctly sized, 4-byte-aligned `ora_knock_t` backing buffer.
struct KnockBuf {
    words: Vec<u32>,
}

impl KnockBuf {
    fn new(knock_len: usize) -> Self {
        let bytes = std::mem::size_of::<ffi::ora_knock_t>() + knock_len * 4;
        KnockBuf {
            words: vec![0u32; bytes.div_ceil(4)],
        }
    }
    fn ptr(&mut self) -> *mut ffi::ora_knock_t {
        self.words.as_mut_ptr() as *mut ffi::ora_knock_t
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let board_str = std::env::var("BOARD").expect("BOARD env var must be set (e.g. fire-24-a)");
    let board =
        Board::try_from_str(&board_str).unwrap_or_else(|| panic!("Unknown board '{}'", board_str));
    let log_enabled = std::env::var("ONEROM_LOG")
        .map(|v| v == "1")
        .unwrap_or(false);
    let config_path = std::env::var("CONFIG").expect("CONFIG env var must be set");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{}': {}", base_dir_str, e));
    let config_json = std::fs::read_to_string(base_dir.join(&config_path))
        .unwrap_or_else(|e| panic!("Failed to read config '{}': {}", config_path, e));
    let config: Config = serde_json::from_str(&config_json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{}': {}", config_path, e));

    // Each ROM slot's CS configuration, from metadata built the same way as
    // the firmware's.
    let fw_version = geometry::firmware_version(board, log_enabled)
        .unwrap_or_else(|e| panic!("Failed to read the firmware version: {e}"));
    let header = geometry::build_header(&config, board, fw_version, &base_dir)
        .unwrap_or_else(|e| panic!("Failed to build metadata for '{config_path}': {e}"));
    let slot_cs: Vec<_> = header
        .rom_slots
        .iter()
        .filter(|slot| {
            !matches!(
                slot.slot_type,
                MaybeKnown::Known(
                    RomSlotType::RomSlotTypePluginSystem
                        | RomSlotType::RomSlotTypePluginUser
                        | RomSlotType::RomSlotTypePluginPio
                )
            )
        })
        .map(|slot| slot.alg.as_ref().map(|alg| alg.alg_cs.clone()))
        .collect();

    let mut passed = 0u32;
    let mut failed = 0u32;

    for (idx, chip_set) in config.chip_sets.iter().enumerate() {
        // Single sets exercise the monitor's core path; Multi/Banked add X-pin
        // handling covered by the wider matrix's dedicated configs.
        let chip = match chip_set.chips.first() {
            Some(c) => c,
            None => continue,
        };
        let chip_type = chip.chip_type.resolved();
        let min_cs_act_ns = min_cs_act::slot_min_cs_act_ns(&config, chip_set);
        let sysclk_mhz = min_cs_act::slot_sysclk_mhz(chip_set.firmware_overrides.as_ref());
        let label = format!("set {} ({})", idx, chip_type.name());

        // Skip chip types the address monitor cannot handle yet.  This list is
        // maintained by hand, never inferred from firmware behaviour: if the
        // monitor's own error path decided what to skip, a firmware bug could
        // silently drop a case and still report green.  A dropped case here is
        // a deliberate, reviewed choice; when the monitor gains support, the
        // entry is removed and the case starts being exercised.
        if let Some(reason) = monitor_skip_reason(chip_type) {
            println!("SKIP  {label}: {reason}");
            continue;
        }

        // A set index beyond what the board's sel pins can express wraps to a
        // lower image, so this case would drive the bus for one chip set while
        // the firmware served another.  Unlike the PIO tester there is no
        // oracle substitution to make that meaningful here, so say so and move
        // on rather than test the wrong ROM under this set's label.
        let max_images = 1usize << board.sel_pins().len();
        if idx >= max_images {
            println!(
                "SKIP  {label}: board has {} sel pin(s) (max {max_images} images), so this \
                 set is not selectable",
                board.sel_pins().len()
            );
            continue;
        }

        // force_16_bit configs ignore /BYTE entirely (AlgData0, word_size 16),
        // so there is no byte mode to exercise and the shared A-1/D15 pin is
        // always a firmware output.
        let force_16_bit = chip_set
            .firmware_overrides
            .as_ref()
            .and_then(|fw| fw.fire.as_ref())
            .map(|f| f.force_16_bit)
            .unwrap_or(false);

        let cs = match slot_cs.get(idx) {
            Some(Some(alg_cs)) => min_cs_act::Cs::from_metadata(alg_cs),
            _ => Err("the metadata doesn't have this slot's CS configuration".to_string()),
        };
        let cs = match cs {
            Ok(cs) => cs,
            Err(e) => {
                println!("FAIL  {label}: {e}");
                failed += 1;
                continue;
            }
        };

        let case = Case {
            board,
            chip_type,
            chip: chip.clone(),
            sel: idx as u8,
            force_16_bit,
            cs,
            min_cs_act_ns,
            sysclk_mhz,
            log_enabled,
        };
        match run_case(case) {
            Ok(timing) => {
                println!("PASS  {label}{timing}");
                passed += 1;
            }
            Err(e) => {
                println!("FAIL  {label}: {e}");
                failed += 1;
            }
        }
    }

    println!("address-monitor: {passed} passed, {failed} failed  [{board_str} / {config_path}]");
    std::process::exit(if failed == 0 { 0 } else { 1 });
}

/// Chip types the address monitor cannot handle yet, and why.  Returns the skip
/// reason, or `None` for a chip that should be exercised.
///
/// This is deliberately an explicit, hand-maintained list keyed on the chip
/// type — not derived from any firmware return value — so coverage is never
/// silently dropped by a firmware bug.  Each entry is removed when the monitor
/// gains support, at which point the case is exercised again.
fn monitor_skip_reason(chip_type: ChipType) -> Option<&'static str> {
    let _ = chip_type;
    // The list is currently empty: the monitor handles every CS algorithm a
    // chip type can resolve to, including ALG_CS_2 (qualifier-based CS, which
    // 23QL384 resolves to on every board).  Kept so a future limitation is
    // recorded as a deliberate, reviewed skip rather than a silent gap.
    None
}

/// One chip set under test.
struct Case {
    board: Board,
    chip_type: ChipType,
    chip: ChipConfig,
    sel: u8,
    force_16_bit: bool,
    cs: min_cs_act::Cs,
    /// The minimum CS-active time the slot boots with, in nanoseconds.
    min_cs_act_ns: Option<u16>,
    /// The clock the slot runs at in MHz, from the config.
    sysclk_mhz: u16,
    log_enabled: bool,
}

/// Run one case on a detached worker thread, with a watchdog on the main
/// thread.  The whole case (boot, setup, every layer) runs on the worker
/// because the [`Emulator`] is not `Send` and the only step that can block —
/// `wait_for_knock` — must be interruptible.  Layer 1 runs first and returns
/// deterministically, so broken firmware fails fast without reaching the
/// blocking path; the timeout only bites if capture works for a single access
/// but knock detection never completes.  On timeout the worker is signalled to
/// park at its next yield and abandoned (it holds only leaked emulator state).
///
/// Returns the slot's minimum CS-active time for the PASS line, or an empty
/// string where it doesn't have one.
fn run_case(case: Case) -> Result<String, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let r = run_case_inner(&case, &stop_worker);
        let _ = tx.send(r);
    });

    match rx.recv_timeout(KNOCK_TIMEOUT) {
        Ok(r) => {
            let _ = handle.join();
            r
        }
        Err(_) => {
            // Signal the worker to park at its next yield, then abandon it.
            stop.store(true, Ordering::Relaxed);
            Err("case timed out — capture path is not delivering entries".to_string())
        }
    }
}

fn run_case_inner(case: &Case, stop: &Arc<AtomicBool>) -> Result<String, String> {
    let chip_type = case.chip_type;
    let word_size: u8 = if matches!(chip_type, ChipType::Chip27C400 | ChipType::Chip27C200) {
        16
    } else {
        8
    };

    let emu = boot(case.board, case.sel, word_size, case.log_enabled)?;
    let cache = PinCache::build(chip_type, &case.chip, case.board);

    // From the config, with the device's clock as a cross-check.
    let sysclk_mhz = u32::from(case.sysclk_mhz);
    let device_mhz = emu.sysclk_mhz();
    if device_mhz != sysclk_mhz {
        return Err(format!(
            "the device runs at {device_mhz}MHz, not the config's {sysclk_mhz}MHz"
        ));
    }
    let min_cycles = case
        .min_cs_act_ns
        .map(|ns| min_cs_act::min_cycles(ns, sysclk_mhz));
    let cs_active = min_cs_act::access_cycles(case.cs, min_cycles);

    setup_monitor(&emu)?;

    // Command signalling travels on the lines the device observes, which on the
    // 40-pin variant exclude the ROM's least-significant address line.  Ask the
    // firmware how many low lines it does not observe rather than deriving it
    // from the chip type, so this tracks whatever the firmware actually does.
    let (r, unobserved) = emu.get_unobserved_addr_bits();
    if r != OraResult::Ok {
        return Err(format!("get_unobserved_addr_bits returned {r:?}"));
    }
    if (unobserved as usize) >= cache.addr_gpios.len() {
        return Err(format!(
            "firmware reports {unobserved} unobserved address bits, but the chip has only {} address lines",
            cache.addr_gpios.len()
        ));
    }
    let observed_gpios = &cache.addr_gpios[unobserved as usize..];

    // /BYTE modes to exercise.  A 16-bit-capable chip is driven both ways: the
    // monitor watches the word address lines and must behave identically in
    // word mode (/BYTE high) and byte mode (/BYTE low).  A force_16_bit config
    // ignores /BYTE, so only the word pass is meaningful; a chip with no /BYTE
    // line gets a single pass with an empty mask.
    let modes: &[u8] = match cache.byte_n_gpio {
        None => &[0],
        Some(_) if case.force_16_bit => &[16],
        Some(_) => &[16, 8],
    };

    for &mode in modes {
        let byte_bg = match cache.byte_n_gpio {
            Some(g) if mode != 0 => driver::byte_n_mask(g, mode),
            _ => (0, 0),
        };

        // Layer 1: one CS-active access must produce exactly one ring entry
        // that demangles to the driven observed address.
        layer1_capture(&emu, &cache, observed_gpios, byte_bg, mode, cs_active)?;

        // In byte mode the host drives A-1 on a line the monitor must not
        // observe.  Only valid here: in word mode that same physical pin is
        // D15, a firmware output, and must not be driven.
        if mode == 8 && unobserved > 0 {
            layer1_a_minus_1_invariance(
                &emu,
                &cache,
                observed_gpios,
                byte_bg,
                unobserved,
                cs_active,
            )?;
        }

        // A chip whose select folds in address lines (ALG_CS_2) must not be
        // captured in the range it does not serve.
        if let Some(qual) = chip_type.deselect_when_address_all_high() {
            layer1_deselected_range(
                &emu,
                &cache,
                observed_gpios,
                byte_bg,
                qual,
                unobserved,
                mode,
                cs_active,
            )?;
        }

        // Layer 2: the real wait_for_knock must detect "!RBCP!" and collect the
        // trailing GROUP/CMD payload, driven through the yield hook.
        layer2_knock(&emu, &cache, unobserved, byte_bg, stop, mode, cs_active)?;
    }
    check_unclaimed_pio(&emu)?;

    // Layer 3 boots again, as it starts the monitor in standby.  Layer 4 boots
    // again for a monitor of its own.
    drop(emu);
    layer3_standby(case, word_size, modes[0], cs_active)?;
    layer4_min_cs_act(case, word_size, modes[0], min_cycles)?;

    Ok(match (case.min_cs_act_ns, min_cycles) {
        (Some(ns), Some(cycles)) => {
            format!(": minimum CS-active {ns}ns, {cycles} cycles at {sysclk_mhz}MHz")
        }
        _ => String::new(),
    })
}

fn boot(board: Board, sel: u8, word_size: u8, log_enabled: bool) -> Result<Emulator, String> {
    Emulator::set_logging(log_enabled);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(sel);
    let mut emu = Emulator::boot();
    // Confirm the firmware selected the image this case is about.  Without
    // this check a mis-selected image is invisible: the case runs against a
    // different ROM entirely and reports whatever that one does under this
    // case's label.
    if emu.sel_image() != sel {
        return Err(format!(
            "firmware selected image {} but this case is image {sel} — the case would \
             have tested the wrong ROM",
            emu.sel_image()
        ));
    }
    if emu.limp_mode() {
        return Err("firmware entered limp mode".to_string());
    }
    emu.poison_unclaimed_pio();
    emu.setup_epio(word_size);
    Ok(emu)
}

/// Fail if the firmware wrote or ran an unclaimed PIO instruction.
fn check_unclaimed_pio(emu: &Emulator) -> Result<(), String> {
    match emu.unclaimed_pio_used() {
        Some(block) => Err(format!(
            "the firmware wrote or ran an unclaimed instruction in PIO block {block}"
        )),
        None => Ok(()),
    }
}

/// Setting up the monitor fails if its PIO space can't be claimed, and leaves
/// the claims unchanged.  Each case marks every instruction in one or both
/// blocks as claimed.  With only the address block full, the CS state
/// machine's claim succeeds first and must be released.
fn setup_monitor_without_space(
    emu: &Emulator,
    ring: *mut u32,
    mode: ffi::ora_monitor_mode_t,
) -> Result<(), String> {
    let saved = (emu.pio_instr_claims(), emu.pio_block_claims());
    for (label, blocks) in [
        ("address block full", &[1usize][..]),
        ("both blocks full", &[1, 2]),
    ] {
        let mut instrs = saved.0;
        for &block in blocks {
            instrs[block] = u32::MAX;
        }
        emu.set_pio_instr_claims(instrs);
        // SAFETY: as for the real setup in setup_monitor.
        let r = unsafe {
            emu.setup_address_monitor_with_options(
                ring,
                RING_ENTRIES_LOG2,
                mode,
                RING_DATA_SIZE,
                std::ptr::null(),
            )
        };
        let after = (emu.pio_instr_claims(), emu.pio_block_claims());
        emu.set_pio_instr_claims(saved.0);
        if r != OraResult::InUse {
            return Err(format!(
                "setup_address_monitor with {label} returned {r:?}, expected InUse"
            ));
        }
        if after != (instrs, saved.1) {
            return Err(format!(
                "setup_address_monitor with {label} left claims {after:x?}, expected {:x?}",
                (instrs, saved.1)
            ));
        }
        if emu.addr_monitor_pio() != onerom_metadata::ADDR_MONITOR_PIO_NONE {
            return Err(format!(
                "setup_address_monitor with {label} set addr_monitor_pio to {:#04x}",
                emu.addr_monitor_pio()
            ));
        }
    }
    Ok(())
}

/// The state machines recorded in `addr_monitor_pio` are claimed, with one run
/// of instructions in each of their blocks.  A second setup changes nothing.
fn check_monitor_claims(
    emu: &Emulator,
    before: ([u32; 3], u16),
    ring: *mut u32,
    mode: ffi::ora_monitor_mode_t,
) -> Result<(), String> {
    let pio = emu.addr_monitor_pio();
    let field = |mask: u8, shift: u32| u32::from(pio & mask) >> shift;
    let sms = [
        (
            field(
                MonitorPio::ADDR_MONITOR_PIO_ADDR_BLOCK,
                MonitorPio::ADDR_MONITOR_PIO_ADDR_BLOCK_SHIFT,
            ),
            field(
                MonitorPio::ADDR_MONITOR_PIO_ADDR_SM,
                MonitorPio::ADDR_MONITOR_PIO_ADDR_SM_SHIFT,
            ),
        ),
        (
            field(
                MonitorPio::ADDR_MONITOR_PIO_CS_BLOCK,
                MonitorPio::ADDR_MONITOR_PIO_CS_BLOCK_SHIFT,
            ),
            field(
                MonitorPio::ADDR_MONITOR_PIO_CS_SM,
                MonitorPio::ADDR_MONITOR_PIO_CS_SM_SHIFT,
            ),
        ),
    ];
    let after = (emu.pio_instr_claims(), emu.pio_block_claims());
    let mut want_sms = before.1;
    for &(block, sm) in &sms {
        if block >= 3 {
            return Err(format!(
                "addr_monitor_pio is {pio:#04x}, with no monitor state machines recorded"
            ));
        }
        want_sms |= (1 << sm) << (block * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT);
    }
    if after.1 != want_sms {
        return Err(format!(
            "state machine claims {:#06x}, want {want_sms:#06x} for monitor {pio:#04x}",
            after.1
        ));
    }
    for block in 0..3 {
        let added = after.0[block] & !before.0[block];
        if (after.0[block] & before.0[block]) != before.0[block] {
            return Err(format!(
                "block {block}: setting up the monitor removed instruction claims"
            ));
        }
        let run = added.checked_shr(added.trailing_zeros()).unwrap_or(0);
        if run & run.wrapping_add(1) != 0 {
            return Err(format!(
                "block {block}: instructions {added:#010x} claimed for the monitor aren't one run"
            ));
        }
        let has_sm = sms.iter().any(|&(b, _)| b as usize == block);
        if has_sm != (added != 0) {
            return Err(format!(
                "block {block}: unexpected instruction claims {added:#010x} for the monitor"
            ));
        }
    }

    // SAFETY: as for the real setup in setup_monitor.
    let r = unsafe {
        emu.setup_address_monitor_with_options(
            ring,
            RING_ENTRIES_LOG2,
            mode,
            RING_DATA_SIZE,
            std::ptr::null(),
        )
    };
    if r != OraResult::InUse {
        return Err(format!(
            "second setup_address_monitor returned {r:?}, expected InUse"
        ));
    }
    if (
        emu.pio_instr_claims(),
        emu.pio_block_claims(),
        emu.addr_monitor_pio(),
    ) != (after.0, after.1, pio)
    {
        return Err("second setup_address_monitor changed the claims".to_string());
    }
    Ok(())
}

/// The state machines the emulated PIO has enabled, per block.
fn enabled_sms() -> [u8; 3] {
    // SAFETY: `_apio_emulated_pio` is a plain C global, and the emulator is
    // single threaded.
    unsafe { (*core::ptr::addr_of!(ffi::_apio_emulated_pio)).enabled_sms }
}

/// Arm the seam, configure and start the address monitor, and init the knock.
fn setup_monitor(emu: &Emulator) -> Result<(), String> {
    // Before setup the monitor has no state machines, so starting it enables
    // none.
    let before = enabled_sms();
    emu.start_address_monitor();
    if enabled_sms() != before {
        return Err(format!(
            "start_address_monitor before setup enabled state machines: {:?} to {:?}",
            before,
            enabled_sms()
        ));
    }

    emu.arm_monitor();
    let ring = emu.sram_host_ptr(RING_BASE);

    // Options the firmware must refuse, each before it touches any PIO or DMA
    // state, so the real setup below starts from nothing.  Only the
    // validation is observable here: there are no DMA registers under
    // emulation for the priority bit to land in.
    let mode = ffi::ora_monitor_mode_t_ORA_MONITOR_MODE_CONTROL;
    let mut unsized_opts = Emulator::address_monitor_options(
        ffi::ora_address_monitor_priority_t_ORA_ADDRESS_MONITOR_PRIORITY_HIGH,
    );
    unsized_opts.size = 0;
    let bad_priority = Emulator::address_monitor_options(
        ffi::ora_address_monitor_priority_t_ORA_ADDRESS_MONITOR_PRIORITY_HIGH + 1,
    );
    let refused: &[(&str, &ffi::ora_address_monitor_options_t, OraResult)] = &[
        ("size 0", &unsized_opts, OraResult::InvalidSize),
        (
            "priority out of range",
            &bad_priority,
            OraResult::InvalidArg,
        ),
    ];
    for (label, opts, expected) in refused {
        // SAFETY: as below, and `opts` outlives the call.
        let r = unsafe {
            emu.setup_address_monitor_with_options(
                ring,
                RING_ENTRIES_LOG2,
                mode,
                RING_DATA_SIZE,
                *opts,
            )
        };
        if r != *expected {
            return Err(format!(
                "setup_address_monitor with {label} returned {r:?}, expected {expected:?}"
            ));
        }
    }

    setup_monitor_without_space(emu, ring, mode)?;
    let before = (emu.pio_instr_claims(), emu.pio_block_claims());

    // The real setup, asking for high priority so the accepted path is the
    // one a plugin that wants it takes.
    let high = Emulator::address_monitor_options(
        ffi::ora_address_monitor_priority_t_ORA_ADDRESS_MONITOR_PRIORITY_HIGH,
    );
    // SAFETY: `ring` is a valid ring buffer within epio SRAM (from
    // sram_host_ptr), live for the monitor's lifetime, and `high` outlives
    // the call.
    let r = unsafe {
        emu.setup_address_monitor_with_options(ring, RING_ENTRIES_LOG2, mode, RING_DATA_SIZE, &high)
    };
    if r != OraResult::Ok {
        return Err(format!("setup_address_monitor returned {r:?}"));
    }
    check_monitor_claims(emu, before, ring, mode)?;
    emu.update_from_apio();
    emu.start_address_monitor();
    emu.update_from_apio();
    Ok(())
}

/// Drive one CS-active read of observed address `addr`: settle the address with
/// CS deasserted, assert CS, then deassert.  Mirrors a real ROM access cycle.
///
/// As on a real host, the address changes on the cycle chip select goes
/// inactive, so a capture that reads the address after that fails.
///
/// `addr` is placed on the *observed* address lines (bit 0 on the
/// least-significant observed line).  `background` is held across every phase
/// and carries the /BYTE level plus, in byte mode, any A-1 level being driven.
/// Chip select is held active for `cs_active` cycles.
///
/// Returns the GPIOs driven at the end of the CS-active phase.
fn drive_access(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    addr: usize,
    cs_active: u32,
) -> u64 {
    let a = driver::addr_mask(addr, observed_gpios);
    let cs_on = driver::ctrl_mask(&cache.control_lines, true);
    let cs_off = driver::ctrl_mask(&cache.control_lines, false);

    let settle = driver::merge(driver::merge(a, cs_off), background);
    emu.drive_gpios(settle.0, settle.1);
    emu.step_cycles(8);

    let active = driver::merge(driver::merge(a, cs_on), background);
    emu.drive_gpios(active.0, active.1);
    emu.step_cycles(cs_active);
    let driven = emu.read_driven_pins();

    let end_addr = !addr & ((1usize << observed_gpios.len()) - 1);
    let end = driver::merge(
        driver::merge(driver::addr_mask(end_addr, observed_gpios), cs_off),
        background,
    );
    emu.drive_gpios(end.0, end.1);
    emu.step_cycles(8);

    driven
}

/// Drive one access and return the observed address the monitor captured for
/// it.  Fails if no ring entry was produced or it would not demangle.
fn capture_one(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    addr: usize,
    cs_active: u32,
) -> Result<u32, String> {
    let slot = emu.get_address_monitor_ring_write_pos();
    if slot.is_null() {
        return Err("get_address_monitor_ring_write_pos returned NULL".to_string());
    }
    let before = unsafe { *slot };

    drive_access(emu, cache, observed_gpios, background, addr, cs_active);

    let after = unsafe { *slot };
    if after == before {
        return Err(
            "capture pipeline produced no ring entry (write pointer did not advance) — \
             address-monitor SMs are not feeding the capture DMA"
                .to_string(),
        );
    }

    // The entry that was written sits at `before`; demangle and return it.
    // Observed (bus) space, not byte space — that is what signalling uses.
    let phys = unsafe { *before };
    let (r, observed) = emu.demangle_observed_addr(phys, true);
    if r != OraResult::Ok {
        return Err(format!("captured entry failed to demangle: {r:?}"));
    }
    Ok(observed)
}

fn layer1_capture(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    mode: u8,
    cs_active: u32,
) -> Result<(), String> {
    let addr = KNOCK[0] as usize; // '!'
    let observed = capture_one(emu, cache, observed_gpios, background, addr, cs_active)?;
    if (observed & 0xFF) as usize != addr {
        return Err(format!(
            "{}: captured address 0x{:02X} != driven 0x{:02X}",
            mode_label(mode),
            observed & 0xFF,
            addr
        ));
    }
    Ok(())
}

/// A chip type with `deselect_when_address_all_high` (23QL384) folds those
/// address lines into its chip-select decision: with all of them high it is
/// deselected and serves nothing, so the monitor must capture nothing there.
/// A capture in that range would feed the plugin an address the chip never
/// answered, and — because command decode masks off the low bits — one
/// indistinguishable from a real command.
///
/// Drives one access inside the deselected range and requires no ring entry,
/// then one just outside it (a single qualifier line dropped low) and requires
/// a capture of exactly that address, so a monitor that has simply stopped
/// capturing cannot pass.
#[allow(clippy::too_many_arguments)]
fn layer1_deselected_range(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    qual_indices: &[u8],
    unobserved: u8,
    mode: u8,
    cs_active: u32,
) -> Result<(), String> {
    // Qualifier indices are chip address-line numbers; the driven address is in
    // observed space, which omits `unobserved` low lines.  A qualifier below
    // that cut is not drivable here — it cannot happen (the qualifiers are a
    // chip's top address lines) but do not guess if it ever does.
    if qual_indices.iter().any(|&i| i < unobserved) {
        return Err(format!(
            "chip has a chip-select qualifier on address line {:?}, below the {unobserved} \
             unobserved line(s) — cannot drive it in observed space",
            qual_indices.iter().min()
        ));
    }
    let bit = |i: u8| 1usize << (i - unobserved);

    let deselected: usize = qual_indices.iter().fold(0, |acc, &i| acc | bit(i));
    // Drop the lowest qualifier line to land just below the deselected range.
    let lowest = *qual_indices
        .iter()
        .min()
        .expect("qualifier list is non-empty");
    let selected: usize = deselected & !bit(lowest);

    let slot = emu.get_address_monitor_ring_write_pos();
    if slot.is_null() {
        return Err("get_address_monitor_ring_write_pos returned NULL".to_string());
    }

    let before = unsafe { *slot };
    drive_access(
        emu,
        cache,
        observed_gpios,
        background,
        deselected,
        cs_active,
    );
    if unsafe { *slot } != before {
        return Err(format!(
            "{}: monitor captured an access at observed address 0x{deselected:X}, which is \
             inside the chip's deselected range — the chip serves nothing there, so nothing \
             may reach the ring",
            mode_label(mode)
        ));
    }

    let observed = capture_one(emu, cache, observed_gpios, background, selected, cs_active)?;
    if observed as usize != selected {
        return Err(format!(
            "{}: captured address 0x{observed:X} != driven 0x{selected:X} just below the \
             deselected range",
            mode_label(mode)
        ));
    }
    Ok(())
}

/// In byte mode the host drives A-1 (the byte-within-word select) to pick a
/// half of the addressed word.  A-1 is not one of the lines the device
/// observes, so the monitor must capture the same observed address whichever
/// way it is driven — this is exactly the bit that must not leak into command
/// decode.  Drive the same word address with A-1 low and then high and require
/// both captures to be identical.
fn layer1_a_minus_1_invariance(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    byte_bg: (u64, u64),
    unobserved: u8,
    cs_active: u32,
) -> Result<(), String> {
    let addr = KNOCK[0] as usize;
    // The unobserved low lines, driven as a little-endian value below them.
    let unobserved_gpios = &cache.addr_gpios[..unobserved as usize];

    let mut seen: Vec<u32> = Vec::new();
    for level in 0..(1usize << unobserved) {
        let a_minus_1 = driver::addr_mask(level, unobserved_gpios);
        let background = driver::merge(byte_bg, a_minus_1);
        let observed = capture_one(emu, cache, observed_gpios, background, addr, cs_active)?;
        if (observed & 0xFF) as usize != addr {
            return Err(format!(
                "byte mode, A-1={level}: captured address 0x{:02X} != driven 0x{:02X}",
                observed & 0xFF,
                addr
            ));
        }
        seen.push(observed);
    }

    if seen.windows(2).any(|w| w[0] != w[1]) {
        return Err(format!(
            "byte mode: A-1 leaked into the observed address — captures differed across \
             A-1 levels ({seen:#X?}); the monitor must not observe the byte-within-word select"
        ));
    }
    Ok(())
}

/// The monitor started in standby.
///
/// Starting the monitor leaves the CS state machine stopped, so accesses are
/// captured with the data pins undriven.  Once standby is off the monitor keeps
/// capturing and the data pins are driven during an access.
fn layer3_standby(case: &Case, word_size: u8, mode: u8, cs_active: u32) -> Result<(), String> {
    let emu = boot(case.board, case.sel, word_size, case.log_enabled)?;
    let cache = PinCache::build(case.chip_type, &case.chip, case.board);
    let r = emu.set_standby(1, 0);
    if r != OraResult::Ok {
        return Err(format!("set_standby(1, 0) returned {r:?}"));
    }
    setup_monitor(&emu)?;

    let (r, unobserved) = emu.get_unobserved_addr_bits();
    if r != OraResult::Ok {
        return Err(format!("get_unobserved_addr_bits returned {r:?}"));
    }
    let observed_gpios = &cache.addr_gpios[unobserved as usize..];
    let background = match cache.byte_n_gpio {
        Some(g) if mode != 0 => driver::byte_n_mask(g, mode),
        _ => (0, 0),
    };
    let lanes = if mode == 16 { 16 } else { 8 };
    let data_mask = cache.data_gpios[..lanes.min(cache.data_gpios.len())]
        .iter()
        .fold(0u64, |m, &g| m | (1u64 << g));

    for standby in [true, false] {
        let when = if standby {
            "monitor started in standby"
        } else {
            "standby turned off"
        };
        if !standby {
            let r = emu.set_standby(0, 0);
            if r != OraResult::Ok {
                return Err(format!("set_standby(0, 0) returned {r:?}"));
            }
        }
        layer1_capture(&emu, &cache, observed_gpios, background, mode, cs_active)
            .map_err(|e| format!("{when}: {e}"))?;
        let driven = drive_access(
            &emu,
            &cache,
            observed_gpios,
            background,
            KNOCK[1] as usize,
            cs_active,
        ) & data_mask;
        if standby && driven != 0 {
            return Err(format!(
                "{when}: data pins {driven:#018x} driven with chip select asserted"
            ));
        }
        if !standby && driven != data_mask {
            return Err(format!(
                "{when}: data pins {:#018x} not driven with chip select asserted",
                data_mask & !driven
            ));
        }
    }
    check_unclaimed_pio(&emu)
}

/// The CS monitor's arming and release timing.  See [`min_cs_act`].
fn layer4_min_cs_act(
    case: &Case,
    word_size: u8,
    mode: u8,
    min_cycles: Option<u32>,
) -> Result<(), String> {
    let emu = boot(case.board, case.sel, word_size, case.log_enabled)?;
    let cache = PinCache::build(case.chip_type, &case.chip, case.board);
    setup_monitor(&emu)?;

    let (r, unobserved) = emu.get_unobserved_addr_bits();
    if r != OraResult::Ok {
        return Err(format!("get_unobserved_addr_bits returned {r:?}"));
    }
    let observed_gpios = &cache.addr_gpios[unobserved as usize..];
    let background = match cache.byte_n_gpio {
        Some(g) if mode != 0 => driver::byte_n_mask(g, mode),
        _ => (0, 0),
    };
    min_cs_act::check(
        &emu,
        &cache,
        observed_gpios,
        background,
        1 << RING_ENTRIES_LOG2,
        case.cs,
        min_cycles,
    )?;
    check_unclaimed_pio(&emu)
}

/// Label for a /BYTE mode, for error messages.
fn mode_label(mode: u8) -> &'static str {
    match mode {
        16 => "word mode (/BYTE high)",
        8 => "byte mode (/BYTE low)",
        _ => "8-bit ROM",
    }
}

/// `unobserved` is taken rather than a borrowed slice so the yield hook can
/// re-derive the observed lines from the cache pointer it already holds.
fn layer2_knock(
    emu: &Emulator,
    cache: &PinCache,
    unobserved: u8,
    background: (u64, u64),
    stop: &Arc<AtomicBool>,
    mode: u8,
    cs_active: u32,
) -> Result<(), String> {
    // Sequence the hook plays: the six knock bytes, then a GROUP/CMD payload
    // (NOP = 0x00/0x00) which wait_for_knock collects after detection.
    let mut schedule: Vec<usize> = KNOCK.iter().map(|&b| b as usize).collect();
    schedule.push(0x00); // GROUP
    schedule.push(0x00); // CMD

    let stop_hook = Arc::clone(stop);
    // Raw pointers so the yield hook can re-enter the emulator while
    // wait_for_knock also holds it.  Single-threaded on this worker; the
    // emulator outlives the call.
    let emu_ptr = emu as *const Emulator as usize;
    let cache_ptr = cache as *const PinCache as usize;
    let mut cursor = 0usize;
    emu.set_yield_hook(move || {
        if stop_hook.load(Ordering::Relaxed) {
            std::thread::park();
            return;
        }
        // SAFETY: single-threaded re-entrant use; pointers valid for the call.
        let emu = unsafe { &*(emu_ptr as *const Emulator) };
        let cache = unsafe { &*(cache_ptr as *const PinCache) };
        if cursor < schedule.len() {
            let observed_gpios = &cache.addr_gpios[unobserved as usize..];
            drive_access(
                emu,
                cache,
                observed_gpios,
                background,
                schedule[cursor],
                cs_active,
            );
            cursor += 1;
        } else {
            emu.step_cycles(8);
        }
    });

    let mut payload = [0u32; 2];
    let mut knock_buf = KnockBuf::new(KNOCK.len());
    let ring = emu.sram_host_ptr(RING_BASE);

    // SAFETY: `knock_buf` is sized for KNOCK.len() entries; `ring` is the live
    // monitor ring; `payload` holds 2 writable u32s; the position args are null.
    let r = unsafe { emu.init_knock(&KNOCK, KNOCK_BITS, RING_DATA_SIZE, knock_buf.ptr()) };
    if r != OraResult::Ok {
        emu.clear_yield_hook();
        return Err(format!("init_knock returned {r:?}"));
    }

    let r = unsafe {
        emu.wait_for_knock(
            knock_buf.ptr(),
            ring,
            RING_ENTRIES_LOG2,
            WAIT_FLAG_DEBOUNCE_CS,
            payload.as_mut_ptr(),
            2,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    emu.clear_yield_hook();

    if r != OraResult::Ok {
        return Err(format!(
            "{}: wait_for_knock returned {r:?}",
            mode_label(mode)
        ));
    }

    // Verify the collected payload demangles to GROUP/CMD (NOP = 0x00/0x00),
    // in observed (bus) space — the space command bytes are signalled in.
    for (i, &want) in [0x00usize, 0x00usize].iter().enumerate() {
        let (r, observed) = emu.demangle_observed_addr(payload[i], false);
        if r != OraResult::Ok {
            return Err(format!(
                "{}: payload[{i}] demangle: {r:?}",
                mode_label(mode)
            ));
        }
        if (observed & 0xFF) as usize != want {
            return Err(format!(
                "{}: payload[{i}] = 0x{:02X} != 0x{:02X}",
                mode_label(mode),
                observed & 0xFF,
                want
            ));
        }
    }
    Ok(())
}
