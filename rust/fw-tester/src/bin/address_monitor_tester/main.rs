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
//!
//! Env: `CONFIG` (config JSON), `BOARD` (e.g. `fire-24-a`), optional
//! `BASE_DIR`, `ONEROM_LOG=1`.  Exits 0 if all cases pass, 1 otherwise.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::driver;
use onerom_fw_emulator::{ffi, Emulator, OraResult};
use onerom_gen::{ChipConfig, Config};

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
const KNOCK: [u32; 6] = [b'!' as u32, b'R' as u32, b'B' as u32, b'C' as u32, b'P' as u32, b'!' as u32];
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
    let log_enabled = std::env::var("ONEROM_LOG").map(|v| v == "1").unwrap_or(false);
    let config_path = std::env::var("CONFIG").expect("CONFIG env var must be set");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{}': {}", base_dir_str, e));
    let config_json = std::fs::read_to_string(base_dir.join(&config_path))
        .unwrap_or_else(|e| panic!("Failed to read config '{}': {}", config_path, e));
    let config: Config = serde_json::from_str(&config_json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{}': {}", config_path, e));

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
        let label = format!("set {} ({})", idx, chip_type.name());

        // Knock detection is an 8-bit-ROM mechanism (the knock is matched
        // against A0-A7 of single-byte accesses); 16-bit parts are out of
        // scope for the address monitor.
        if matches!(chip_type, ChipType::Chip27C400 | ChipType::Chip27C200) {
            println!("SKIP  {label}: 16-bit ROM (knock detection is 8-bit only)");
            continue;
        }

        match run_case(board, chip_type, chip.clone(), idx as u8, log_enabled) {
            Ok(()) => {
                println!("PASS  {label}");
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

/// Run one case on a detached worker thread, with a watchdog on the main
/// thread.  The whole case (boot, setup, both layers) runs on the worker
/// because the [`Emulator`] is not `Send` and the only step that can block —
/// `wait_for_knock` — must be interruptible.  Layer 1 runs first and returns
/// deterministically, so broken firmware fails fast without reaching the
/// blocking path; the timeout only bites if capture works for a single access
/// but knock detection never completes.  On timeout the worker is signalled to
/// park at its next yield and abandoned (it holds only leaked emulator state).
fn run_case(
    board: Board,
    chip_type: ChipType,
    chip: ChipConfig,
    sel: u8,
    log_enabled: bool,
) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let r = run_case_inner(board, chip_type, &chip, sel, log_enabled, &stop_worker);
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

fn run_case_inner(
    board: Board,
    chip_type: ChipType,
    chip: &ChipConfig,
    sel: u8,
    log_enabled: bool,
    stop: &Arc<AtomicBool>,
) -> Result<(), String> {
    let word_size: u8 = if matches!(chip_type, ChipType::Chip27C400 | ChipType::Chip27C200) {
        16
    } else {
        8
    };

    let emu = boot(board, sel, word_size, log_enabled)?;
    let cache = PinCache::build(chip_type, chip, board);

    setup_monitor(&emu)?;

    // Layer 1: one CS-active access must produce exactly one ring entry that
    // demangles to the driven address.
    layer1_capture(&emu, &cache)?;

    // Layer 2: the real wait_for_knock must detect "!RBCP!" and collect the
    // trailing GROUP/CMD payload, driven through the yield hook.
    layer2_knock(&emu, &cache, stop)?;

    Ok(())
}

fn boot(board: Board, sel: u8, word_size: u8, log_enabled: bool) -> Result<Emulator, String> {
    Emulator::set_logging(log_enabled);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(sel);
    let mut emu = Emulator::boot();
    if emu.limp_mode() {
        return Err("firmware entered limp mode".to_string());
    }
    emu.setup_epio(word_size);
    Ok(emu)
}

/// Arm the seam, configure and start the address monitor, and init the knock.
fn setup_monitor(emu: &Emulator) -> Result<(), String> {
    emu.arm_monitor();
    let ring = emu.sram_host_ptr(RING_BASE);
    let r = emu.setup_address_monitor(
        ring,
        RING_ENTRIES_LOG2,
        ffi::ora_monitor_mode_t_ORA_MONITOR_MODE_CONTROL,
        RING_DATA_SIZE,
    );
    if r != OraResult::Ok {
        return Err(format!("setup_address_monitor returned {r:?}"));
    }
    emu.update_from_apio();
    emu.start_address_monitor();
    emu.update_from_apio();
    Ok(())
}

/// Drive one CS-active read of logical address `addr`: settle the address with
/// CS deasserted, assert CS, then deassert.  Mirrors a real ROM access cycle.
fn drive_access(emu: &Emulator, cache: &PinCache, addr: usize) {
    let a = driver::addr_mask(addr, &cache.addr_gpios);
    let cs_on = driver::ctrl_mask(&cache.control_lines, true);
    let cs_off = driver::ctrl_mask(&cache.control_lines, false);

    let settle = driver::merge(a, cs_off);
    emu.drive_gpios(settle.0, settle.1);
    emu.step_cycles(8);

    let active = driver::merge(a, cs_on);
    emu.drive_gpios(active.0, active.1);
    emu.step_cycles(16);

    emu.drive_gpios(settle.0, settle.1);
    emu.step_cycles(8);
}

fn layer1_capture(emu: &Emulator, cache: &PinCache) -> Result<(), String> {
    let slot = emu.get_address_monitor_ring_write_pos();
    if slot.is_null() {
        return Err("get_address_monitor_ring_write_pos returned NULL".to_string());
    }
    let before = unsafe { *slot };

    let addr = KNOCK[0] as usize; // '!'
    drive_access(emu, cache, addr);

    let after = unsafe { *slot };
    if after == before {
        return Err(
            "capture pipeline produced no ring entry (write pointer did not advance) — \
             address-monitor SMs are not feeding the capture DMA"
                .to_string(),
        );
    }

    // The entry that was written sits at `before`; demangle and check it.
    let phys = unsafe { *before };
    let (r, logical) = emu.demangle_addr(phys, true);
    if r != OraResult::Ok {
        return Err(format!("captured entry failed to demangle: {r:?}"));
    }
    if (logical & 0xFF) as usize != addr {
        return Err(format!(
            "captured address 0x{:02X} != driven 0x{:02X}",
            logical & 0xFF,
            addr
        ));
    }
    Ok(())
}

fn layer2_knock(emu: &Emulator, cache: &PinCache, stop: &Arc<AtomicBool>) -> Result<(), String> {
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
            drive_access(emu, cache, schedule[cursor]);
            cursor += 1;
        } else {
            emu.step_cycles(8);
        }
    });

    let mut payload = [0u32; 2];
    let mut knock_buf = KnockBuf::new(KNOCK.len());
    let ring = emu.sram_host_ptr(RING_BASE);

    let r = emu.init_knock(&KNOCK, KNOCK_BITS, RING_DATA_SIZE, knock_buf.ptr());
    if r != OraResult::Ok {
        emu.clear_yield_hook();
        return Err(format!("init_knock returned {r:?}"));
    }

    let r = emu.wait_for_knock(
        knock_buf.ptr(),
        ring,
        RING_ENTRIES_LOG2,
        WAIT_FLAG_DEBOUNCE_CS,
        payload.as_mut_ptr(),
        2,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    emu.clear_yield_hook();

    if r != OraResult::Ok {
        return Err(format!("wait_for_knock returned {r:?}"));
    }

    // Verify the collected payload demangles to GROUP/CMD (NOP = 0x00/0x00).
    for (i, &want) in [0x00usize, 0x00usize].iter().enumerate() {
        let (r, logical) = emu.demangle_addr(payload[i], false);
        if r != OraResult::Ok {
            return Err(format!("payload[{i}] demangle: {r:?}"));
        }
        if (logical & 0xFF) as usize != want {
            return Err(format!(
                "payload[{i}] = 0x{:02X} != 0x{:02X}",
                logical & 0xFF,
                want
            ));
        }
    }
    Ok(())
}
