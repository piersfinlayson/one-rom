// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM RBCP Tester
//!
//! Runs the host-control plugin's own C source natively against the firmware
//! emulator, driving it over emulated ROM bus cycles as a real host would.
//! The plugin is neither reimplemented nor stubbed: this is the same source
//! that is cross-compiled for the device, linked against the same plugin API.
//!
//! # Suites
//!
//! Scenarios are grouped into suites, which answer different questions:
//!
//! - `conformance` — does the device obey the RBCP specification?  One group
//!   of scenarios per specification section, each asserting what the spec
//!   requires rather than what this implementation happens to do.
//! - `integration` — does a realistic application work end to end?  Whole
//!   flows, modelled on the specification's worked example and the 6502
//!   reference host.
//!
//! Further suites are a table entry.
//!
//! # Environment variables
//!
//! | Variable     | Required | Description                                     |
//! |--------------|----------|-------------------------------------------------|
//! | `BOARD`      | yes      | Board name, e.g. `fire-24-a`                    |
//! | `CONFIG`     | yes      | Path to the firmware config JSON file           |
//! | `BASE_DIR`   | no       | Project root for resolving relative paths       |
//! | `ONEROM_LOG` | no       | Set to `1` to enable firmware logging to stdout |
//! | `RUST_LOG`   | no       | Tester log level (default: `warn`)              |
//!
//! # Arguments
//!
//! `--suite <name>` runs one suite; `--scenario <substr>` runs only scenarios
//! whose name contains the substring.  Both default to everything.
//!
//! A scenario that does not apply to the device under test — the RAM slot
//! count and slot size vary with the ROM type and the board — reports `SKIP`
//! with its reason rather than passing vacuously.
//!
//! Exits 0 if every scenario that ran passed, 1 otherwise.

use std::path::{Path, PathBuf};
use std::process;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, OraResult};
use onerom_fw_tester::pin_cache::PinCache;
use onerom_fw_tester::timing;
use onerom_gen::Config;
use onerom_plugin_tester::{ffi, harness::Plugin};

mod driver;
mod suites;

use driver::Bus;

/// Where the plugin's capture ring is placed in emulated SRAM: above the
/// served region, aligned to the ring's own size.
const RING_BASE: u32 = 0x2008_1000;

/// Everything a scenario needs to know about the device it is talking to,
/// beyond the bus itself.
pub struct Ctx {
    pub config: Config,
    pub base_dir: PathBuf,
    pub board: Board,
    pub set_idx: usize,
    pub chip_type: ChipType,
    /// Low address lines the device does not observe.
    pub unobserved: u8,
    /// Size of a RAM slot, in bytes.
    pub ram_slot_size: u32,
    /// Number of RAM slots the device has.
    pub ram_slot_count: u8,
    /// The RAM slot being served when the scenario starts.
    pub active_ram_slot: u8,
    /// Size of the device's dedicated NV storage, in bytes.
    pub nv_size: u32,
}

impl Ctx {
    /// A command page clear of the back-channel, mirroring the reference
    /// host's layout: page 0 for signalling, the back-channel above it.
    pub fn command_page(&self) -> u16 {
        0
    }

    /// Back-channel start: the first byte address above the command page, so
    /// that back-channel polling never looks like command signalling.  Always
    /// 4-byte aligned, as the specification requires.
    pub fn bch_start(&self) -> u32 {
        0x100u32 << self.unobserved
    }

    pub fn bch_size(&self) -> u16 {
        512
    }

    /// The byte a command-mode scenario arms and then reads its verdict from.
    ///
    /// Sited immediately above the back-channel: clear of the command page, so
    /// reading it is never mistaken for command signalling, and clear of the
    /// region itself, so a probe and a command-response session can coexist.
    /// See [`Bus::arm_probe`].
    pub fn probe_addr(&self) -> u32 {
        self.bch_start() + u32::from(self.bch_size())
    }

    /// The byte the fence writes to.
    ///
    /// Must be distinct from [`Ctx::probe_addr`], or the fence would overwrite
    /// the very value the verdict discriminates on.
    ///
    /// Deliberately the *adjacent* byte, so that on a word-organised ROM the
    /// probe and the fence fall in the same word, one in each half.  A pair
    /// four bytes apart would leave both on even offsets, and every command
    /// mode scenario would then pass without ever reading an odd byte —
    /// exactly the gap that hid a broken 8-bit read path.  It also asserts the
    /// device writes the slot a byte at a time, rather than disturbing the
    /// neighbouring half of the word.
    pub fn fence_addr(&self) -> u32 {
        self.probe_addr() + 1
    }

    /// A valid page that is *not* the command page.
    ///
    /// Used to send command bytes the device must ignore while it is filtering
    /// on the command page.  Page 1 is where the back-channel already lives,
    /// so it is known to be within range of every ROM the tester runs against.
    pub fn other_page(&self) -> u16 {
        1
    }

    /// A byte a scenario may write to without disturbing the probe or fence.
    ///
    /// Sited past both, so a scenario needing a second observable write — the
    /// Command Framing scenarios need two pokes in flight — has somewhere to
    /// put it.
    pub fn scratch_addr(&self) -> u32 {
        self.fence_addr() + 3
    }

    /// A RAM slot that is not the one being served, if the device has one.
    ///
    /// Setup, not an assertion: a scenario needs *some* slot it may name, and
    /// the next index round is one.  It is deliberately the lowest such index
    /// rather than anything derived from what the device advertises — a plugin
    /// may offer a host fewer slots than the firmware has, and predicting how
    /// many would be mirroring the plugin rather than testing it.  Which slots
    /// a host may actually name is asserted over the bus, in the Read group.
    pub fn inactive_ram_slot(&self) -> Option<u8> {
        (self.ram_slot_count > 1).then(|| (self.active_ram_slot + 1) % self.ram_slot_count)
    }

    pub fn session(&self) -> driver::Session {
        driver::Session {
            command_page: self.command_page(),
            bch_start: self.bch_start(),
            bch_size: self.bch_size(),
            complete: driver::DEFAULT_COMPLETE,
            status_ok: driver::DEFAULT_STATUS_OK,
        }
    }
}

/// How a scenario ended.
///
/// A scenario that cannot run against the device in front of it is neither a
/// pass nor a failure.  Which scenarios those are is a property of the
/// configuration under test — RAM slot count and slot size vary with the ROM
/// type and the board — so the judgement belongs to the scenario, at the point
/// it discovers it, rather than to a table written in advance.
pub enum Outcome {
    Pass,
    /// Not applicable to this device, for the stated reason.
    Skip(String),
}

pub type ScenarioFn = fn(&mut Bus, &Ctx) -> Result<Outcome, String>;

pub struct Scenario {
    /// Dotted name, filtered on by `--scenario`.
    pub name: &'static str,
    /// The specification section this asserts, or a one-line description of
    /// the flow for an integration scenario.
    pub spec_ref: &'static str,
    pub run: ScenarioFn,
}

pub struct Suite {
    pub name: &'static str,
    pub blurb: &'static str,
    pub scenarios: &'static [Scenario],
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut suite_filter: Option<String> = None;
    let mut scenario_filter: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--suite" => suite_filter = args.next(),
            "--scenario" => scenario_filter = args.next(),
            other => {
                eprintln!("unknown argument '{other}' (expected --suite or --scenario)");
                process::exit(2);
            }
        }
    }

    let board_str = std::env::var("BOARD").expect("BOARD env var must be set (e.g. fire-24-a)");
    let board =
        Board::try_from_str(&board_str).unwrap_or_else(|| panic!("Unknown board '{board_str}'"));
    let log_enabled = std::env::var("ONEROM_LOG")
        .map(|v| v == "1")
        .unwrap_or(false);
    let config_path = std::env::var("CONFIG").expect("CONFIG env var must be set");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{base_dir_str}': {e}"));
    let config_json = std::fs::read_to_string(base_dir.join(&config_path))
        .unwrap_or_else(|e| panic!("Failed to read config '{config_path}': {e}"));
    let config: Config = serde_json::from_str(&config_json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{config_path}': {e}"));

    let modes = modes_to_run(&config);

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

    for suite in suites::SUITES {
        if let Some(f) = &suite_filter
            && suite.name != f
        {
            continue;
        }
        let selected: Vec<&Scenario> = suite
            .scenarios
            .iter()
            .filter(|s| match &scenario_filter {
                Some(f) => s.name.contains(f.as_str()),
                None => true,
            })
            .collect();
        if selected.is_empty() {
            continue;
        }

        println!("\n== {} — {}", suite.name, suite.blurb);
        for sc in selected {
            // A chip the host can read either as bytes or as words has to obey
            // the protocol both ways, and which applies is the host's wiring
            // rather than anything the device chooses — so every scenario runs
            // once per mode the chip supports, as the PIO tester does.
            for &mode in modes {
                let label = format!("{} [{mode}-bit]", sc.name);
                match run_scenario(sc, board, &config, &base_dir, log_enabled, mode) {
                    Ok(Outcome::Pass) => {
                        println!("PASS  {label}");
                        passed += 1;
                    }
                    Ok(Outcome::Skip(why)) => {
                        println!("SKIP  {label}\n        {why}");
                        skipped += 1;
                    }
                    Err(e) => {
                        println!("FAIL  {label}\n        [{}]\n        {e}", sc.spec_ref);
                        failed += 1;
                    }
                }
            }
        }
    }

    // Skips are always reported, count included when zero: a suite that
    // quietly dropped scenarios would otherwise read as full coverage.
    println!(
        "\nrbcp: {passed} passed, {failed} failed, {skipped} skipped  \
         [{board_str} / {config_path}]"
    );
    process::exit(if failed == 0 { 0 } else { 1 });
}

/// The bit modes every scenario is run in.
///
/// A chip the host can read either as bytes or as words has to obey the
/// protocol both ways, and which applies is the host's wiring — it drives
/// `BYTE#` — rather than anything the device chooses.  So each supported mode
/// gets its own pass, as the PIO tester does.
///
/// The exception is a set built with the `force_16_bit` firmware override: the
/// firmware then serves with a data algorithm that ignores `BYTE#` altogether,
/// so there is no 8-bit behaviour to test and an 8-bit pass would be asserting
/// against something the device was never asked to do.
fn modes_to_run(config: &Config) -> &'static [u8] {
    let Some(set) = config.chip_sets.first() else {
        return &[8];
    };
    let force_16_bit = set
        .firmware_overrides
        .as_ref()
        .and_then(|fw| fw.fire.as_ref())
        .map(|f| f.force_16_bit)
        .unwrap_or(false);
    if force_16_bit {
        return &[16];
    }
    set.chips
        .first()
        .map(|c| c.chip_type.resolved().bit_modes())
        .unwrap_or(&[8])
}

/// The width the firmware serves this set at, independent of the mode the host
/// is currently driving.
///
/// This is what [`Emulator::setup_epio`] wants, and it is *not* the bit mode
/// under test: a chip that can be read either way is always served by the
/// wider data path, and `BYTE#` selects a half of it at run time.  Passing the
/// mode here instead configures the DMA chain for narrow words and discards
/// the `BYTE#`/A-1 handling entirely — the device then serves the low half of
/// every word whatever the host asks for.
fn native_word_size(config: &Config) -> u8 {
    config
        .chip_sets
        .first()
        .and_then(|s| s.chips.first())
        .and_then(|c| c.chip_type.resolved().bit_modes().iter().max().copied())
        .unwrap_or(8)
}

/// Why the RBCP driver cannot exercise a chip type, if it cannot.
///
/// A hand-maintained list keyed on the chip type, following the address
/// monitor tester's `monitor_skip_reason`: never inferred from a firmware or
/// plugin return value, so coverage is not silently dropped by a bug in the
/// thing under test.  Each entry is removed when the driver gains support, at
/// which point the cases start being exercised again.
fn rbcp_skip_reason(chip_type: ChipType) -> Option<&'static str> {
    let _ = chip_type;
    // Currently empty: the driver serves byte- and word-organised ROMs alike,
    // and runs every scenario in each bit mode the chip supports.  Kept so a
    // future limitation is recorded as a deliberate, reviewed skip rather than
    // a silent gap.
    None
}

/// Boot the firmware, start the plugin, and run one scenario against it.
///
/// Every scenario gets a fresh boot and a fresh entry into the plugin, so no
/// scenario can be influenced by another's leftover state: the firmware's RAM
/// is restored to its cold-boot image, and re-entering the plugin re-runs its
/// own initialisation.
fn run_scenario(
    sc: &Scenario,
    board: Board,
    config: &Config,
    base_dir: &Path,
    log_enabled: bool,
    word_size: u8,
) -> Result<Outcome, String> {
    let set_idx = 0usize;
    let chip = config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .ok_or("config has no chip sets")?;
    let chip_type = chip.chip_type.resolved();

    if let Some(reason) = rbcp_skip_reason(chip_type) {
        return Ok(Outcome::Skip(format!("{} — {reason}", chip_type.name())));
    }

    Emulator::set_logging(log_enabled);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(set_idx as u8);
    let mut emu = Emulator::boot();

    if emu.sel_image() != set_idx as u8 {
        return Err(format!(
            "firmware selected image {}, not {set_idx}",
            emu.sel_image()
        ));
    }
    if emu.limp_mode() {
        return Err("firmware entered limp mode".to_string());
    }

    emu.setup_epio(native_word_size(config));
    emu.arm_monitor();

    let ring = emu.sram_host_ptr(RING_BASE);
    // SAFETY: `ring` is within epio's SRAM and outlives the plugin.
    unsafe { ffi::ora_host_test_set_ring_buf(ring) };

    // SAFETY: `emu` outlives `plugin`, which is dropped at the end of this fn.
    let plugin = unsafe { Plugin::start(&emu)? };

    let (r, unobserved) = emu.get_unobserved_addr_bits();
    if r != OraResult::Ok {
        return Err(format!("get_unobserved_addr_bits returned {r:?}"));
    }

    let (r, slot_info) = emu.get_ram_slot_info(0);
    let ram_slot_size = match (r, slot_info) {
        (OraResult::Ok, Some(info)) => info.size,
        _ => return Err(format!("get_ram_slot_info returned {r:?}")),
    };

    let (r, active_ram_slot) = emu.get_active_ram_slot();
    let active_ram_slot = match (r, active_ram_slot) {
        (OraResult::Ok, Some(slot)) => slot,
        _ => return Err(format!("get_active_ram_slot returned {r:?}")),
    };

    // Erase the device's NV storage.
    //
    // On a device this is a reserved flash sector, and the specification says
    // that "before having been written by any host, the entire NV storage on
    // any device is initialized to 0xFF".  In this process it is an ordinary
    // object in the shim, which zero-initialises and — unlike the firmware's
    // RAM, which the emulator restores from its cold-boot image — survives a
    // reboot, so what one scenario committed would still be there for the
    // next.  Erasing it here gives every scenario the same starting point a
    // device gives a host that has never written to it.
    //
    // SAFETY: the plugin is parked at a yield, so nothing else is reading it.
    let nv_size = unsafe { ffi::ora_host_test_nv_storage_size() };
    // SAFETY: the shim's region is `nv_size` bytes and outlives the scenario.
    unsafe { std::ptr::write_bytes(ffi::ora_host_test_nv_storage(), 0xFF, nv_size as usize) };
    // The shim's record of what the plugin asked the flash hardware to do is
    // process-global for the same reason, so it starts each scenario empty.
    // SAFETY: as above.
    unsafe { ffi::ora_host_test_reset_flash_log() };

    let ctx = Ctx {
        config: config.clone(),
        base_dir: base_dir.to_path_buf(),
        board,
        set_idx,
        chip_type,
        unobserved,
        ram_slot_size,
        ram_slot_count: emu.get_ram_slot_count(),
        active_ram_slot,
        nv_size,
    };

    let cache = PinCache::build(chip_type, chip, board);
    let mut bus = Bus::new(&emu, &plugin, &cache, chip_type, unobserved, word_size);

    // Settle before the first access, as the other testers do.
    emu.step_cycles(timing::CYCLES_BEFORE_START);

    (sc.run)(&mut bus, &ctx)
}
