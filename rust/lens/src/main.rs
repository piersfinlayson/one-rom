// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lens — a browser tool that runs the One ROM PIO/DMA algorithms via
//! [`onerom_fw_emulator`] and visualises the resulting waveforms.
//!
//! This crate compiles to WebAssembly (`wasm32-unknown-emscripten`) and exposes
//! a small C ABI that the browser front-end (`logic-analyzer.js`) drives through
//! Emscripten's `ccall`/`cwrap`.  The heavy lifting — booting the firmware,
//! stepping cycles, driving and reading GPIOs — lives in the emulator; Lens is a
//! thin, browser-facing layer over it.
//!
//! The pin geometry (address/data GPIO lists, control lines, word size) is
//! computed for `CONFIG`/`BOARD` at build time and embedded as consts by
//! `build.rs`, so each `.wasm` targets one hardware variant + ROM image and no
//! generator runs in the browser.
//!
//! This is a binary (not a cdylib) purely so Emscripten emits its JS loader
//! (`onerom-lens.js`) alongside the `.wasm`; `main` is a no-op and the runtime
//! is kept alive (`-sEXIT_RUNTIME=0`) so the browser can `ccall` the exported
//! `onerom_*` functions after load.

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_char};

/// No-op entry point — the module is driven entirely through the exported
/// `onerom_*` functions after Emscripten instantiates it.
fn main() {}

use onerom_fw_emulator::Emulator;
use onerom_fw_emulator::driver::{self, ControlLine};

/// Build-time embedded pin geometry (`ADDR_GPIOS`, `DATA_GPIOS`, `CONTROL_LINES`,
/// `BYTE_N_GPIO`, `NUM_ADDR_BITS`, `NUM_DATA_BITS`, `WORD_SIZE`).
mod geometry {
    include!(concat!(env!("OUT_DIR"), "/geometry.rs"));
}

/// Live emulator plus the runtime (owned) form of the embedded geometry.
struct Lens {
    emu: Emulator,
    addr_gpios: Vec<Vec<u8>>,
    data_gpios: Vec<u8>,
    control_lines: Vec<ControlLine>,
    byte_n_gpio: Option<u8>,
    /// Cycles stepped since the last reset — the emulator itself is stateless
    /// about wall-clock, so the waveform's time axis is tracked here.
    cycles: Cell<u64>,
}

impl Lens {
    fn from_geometry(emu: Emulator) -> Self {
        Self {
            emu,
            addr_gpios: geometry::ADDR_GPIOS.iter().map(|s| s.to_vec()).collect(),
            data_gpios: geometry::DATA_GPIOS.to_vec(),
            control_lines: geometry::CONTROL_LINES
                .iter()
                .map(|c| ControlLine {
                    name: c.name,
                    gpios: c.gpios.to_vec(),
                    assert_high: c.assert_high,
                    commoned: false,
                })
                .collect(),
            byte_n_gpio: geometry::BYTE_N_GPIO,
            cycles: Cell::new(0),
        }
    }

    /// GPIO of the (first) control line named `name`, or `0xFF` if this ROM has
    /// no such line.
    fn control_pin(&self, name: &str) -> u32 {
        self.control_lines
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.gpios.first())
            .map_or(0xFF, |&g| g as u32)
    }
}

thread_local! {
    // Wasm/Emscripten is single-threaded and JS calls are serialised, so a
    // thread-local holds the one live instance.  `None` until `onerom_init`.
    static LENS: RefCell<Option<Lens>> = const { RefCell::new(None) };

    // Backing store for the string `onerom_get_pio_disassembly` hands back to
    // JS: it must outlive the call so `UTF8ToString` can read it, and is
    // replaced on the next call.
    static DISASM: RefCell<CString> = RefCell::new(CString::default());
}

/// Boot the emulator, wire up epio, and build the lens state.
///
/// Returns `1` if the PIO state machines came up, `0` if not, `-1` if the
/// firmware entered limp mode.  Safe to call once; a second call re-boots.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_init() -> i32 {
    let mut emu = Emulator::boot();
    if emu.limp_mode() {
        return -1;
    }
    emu.setup_epio(geometry::WORD_SIZE);
    let ok = emu.pios_enabled();
    LENS.with_borrow_mut(|slot| *slot = Some(Lens::from_geometry(emu)));
    if ok { 1 } else { 0 }
}

/// Drive the address bus and control lines for one read.
///
/// * `addr`     — logical address to present.
/// * `cs_active` — non-zero asserts the chip's control lines; zero deasserts.
/// * `bit_mode`  — 8 or 16; only meaningful on chips with a BYTE# pin (27C400).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_drive_addr(addr: u32, cs_active: u32, bit_mode: u32) {
    LENS.with_borrow(|slot| {
        let Some(lens) = slot.as_ref() else { return };

        // 27C400-family word mode: addr_gpios[0] is A-1, which is also D15 — a
        // data output in word mode.  Driving it as an address bit would fight
        // the data bus, so skip it and drive the word index onto A0-A17.  In
        // byte mode the full set is used, with A-1 as the byte-select LSB.
        let word_mode = bit_mode == 16 && lens.byte_n_gpio.is_some();
        let addr_gpios: &[Vec<u8>] = if word_mode {
            &lens.addr_gpios[1..]
        } else {
            &lens.addr_gpios
        };

        let mut m = driver::addr_mask(addr as usize, addr_gpios);
        m = driver::merge(m, driver::ctrl_mask(&lens.control_lines, cs_active != 0));
        // BYTE# pin (27C400 only): low = 8-bit (byte) mode, high = 16-bit (word).
        if let Some(g) = lens.byte_n_gpio {
            m = driver::merge(m, driver::byte_n_mask(g, bit_mode as u8));
        }
        lens.emu.drive_gpios(m.0, m.1);
    });
}

/// Stop driving every GPIO (release the bus).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_release_pins() {
    LENS.with_borrow(|slot| {
        if let Some(lens) = slot.as_ref() {
            lens.emu.drive_gpios(0, 0);
        }
    });
}

/// Advance the emulation by `cycles` PIO/DMA cycles.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_step(cycles: u32) {
    LENS.with_borrow(|slot| {
        if let Some(lens) = slot.as_ref() {
            lens.emu.step_cycles(cycles);
            lens.cycles.set(lens.cycles.get() + cycles as u64);
        }
    });
}

/// Raw GPIO pin-state bitmask (bit N = GPIO N high).  Returned as `f64` so it
/// crosses the JS boundary exactly (GPIO indices are well under 2^53).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_read_pin_states() -> f64 {
    LENS.with_borrow(|slot| slot.as_ref().map_or(0, |l| l.emu.read_pin_states()) as f64)
}

/// Bitmask of GPIOs currently actively driven (the rest are High-Z); used to
/// render tristate/High-Z regions.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_read_driven_pins() -> f64 {
    LENS.with_borrow(|slot| slot.as_ref().map_or(0, |l| l.emu.read_driven_pins()) as f64)
}

/// Cycles stepped since the last [`onerom_reset_cycle_count`].
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_cycle_count() -> f64 {
    LENS.with_borrow(|slot| slot.as_ref().map_or(0, |l| l.cycles.get()) as f64)
}

/// Reset the cycle counter (does not reset the emulation).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_reset_cycle_count() {
    LENS.with_borrow(|slot| {
        if let Some(lens) = slot.as_ref() {
            lens.cycles.set(0);
        }
    });
}

/// Current SYSCLK frequency in MHz, as reported by the running firmware.  A Lens
/// cycle is a PIO cycle clocked from SYSCLK, so a duration in nanoseconds is
/// `cycles * 1000 / sysclk_mhz`.  Returns 0 before the emulator is initialised.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_sysclk_mhz() -> u32 {
    LENS.with_borrow(|slot| slot.as_ref().map_or(0, |l| l.emu.sysclk_mhz()))
}

/// Read the current value on the data bus (`data_bits` least-significant bits).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_read_data(data_bits: u32) -> u32 {
    LENS.with_borrow(|slot| {
        let Some(lens) = slot.as_ref() else { return 0 };
        let states = lens.emu.read_pin_states();
        let n = (data_bits as usize).min(lens.data_gpios.len());
        let mut value = 0u32;
        for (bit, &gpio) in lens.data_gpios.iter().take(n).enumerate() {
            if (states >> gpio) & 1 == 1 {
                value |= 1 << bit;
            }
        }
        value
    })
}

// ── Geometry getters (read the embedded consts; no init required) ─────────────

/// GPIO number carrying address bit `bit` (first GPIO if a pin drives several);
/// `0xFF` if out of range.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_addr_pin(bit: u32) -> u32 {
    geometry::ADDR_GPIOS
        .get(bit as usize)
        .and_then(|g| g.first())
        .map_or(0xFF, |&g| g as u32)
}

/// GPIO number carrying data bit `bit`; `0xFF` if out of range.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_data_pin(bit: u32) -> u32 {
    geometry::DATA_GPIOS
        .get(bit as usize)
        .map_or(0xFF, |&g| g as u32)
}

/// Number of control lines (CE/OE/CS + any oversized-ROM half-selects).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_lens_get_num_control_lines() -> u32 {
    geometry::CONTROL_LINES.len() as u32
}

/// GPIO number for control line `idx`; `0xFF` if out of range.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_control_pin(idx: u32) -> u32 {
    geometry::CONTROL_LINES
        .get(idx as usize)
        .and_then(|c| c.gpios.first())
        .map_or(0xFF, |&g| g as u32)
}

/// `1` if control line `idx` asserts by driving HIGH, else `0`.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_control_assert_high(idx: u32) -> u32 {
    geometry::CONTROL_LINES
        .get(idx as usize)
        .map_or(0, |c| c.assert_high as u32)
}

/// Number of address bits driven for this ROM.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_lens_get_num_addr_bits() -> u32 {
    geometry::NUM_ADDR_BITS as u32
}

/// Number of data bits for this ROM.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_lens_get_num_data_bits() -> u32 {
    geometry::NUM_DATA_BITS as u32
}

/// Addressable size in bytes (the address space lens drives).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_lens_get_rom_size() -> u32 {
    1u32 << geometry::NUM_ADDR_BITS
}

// Per-name control-pin getters.  Each returns the GPIO of that named control
// line for this ROM, or `0xFF` if the ROM has no such line (e.g. a 2364 has only
// cs1).  These preserve the browser front-end's fixed CS1/CS2/CS3/CE/OE/BYTE
// signal rows while the underlying set is metadata-driven.

macro_rules! control_pin_getter {
    ($fn_name:ident, $line:literal) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $fn_name() -> u32 {
            LENS.with_borrow(|slot| slot.as_ref().map_or(0xFF, |l| l.control_pin($line)))
        }
    };
}

control_pin_getter!(onerom_get_cs1_pin, "cs1");
control_pin_getter!(onerom_get_cs2_pin, "cs2");
control_pin_getter!(onerom_get_cs3_pin, "cs3");
control_pin_getter!(onerom_get_ce_pin, "ce");
control_pin_getter!(onerom_get_oe_pin, "oe");
control_pin_getter!(onerom_get_x1_pin, "x1");
control_pin_getter!(onerom_get_x2_pin, "x2");

/// GPIO of the BYTE# pin (27C400), or `0xFF` if absent.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_byte_pin() -> u32 {
    geometry::BYTE_N_GPIO.map_or(0xFF, |g| g as u32)
}

/// Whether GPIO `gpio` is read inverted by the firmware's pin routing (1/0).
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_gpio_input_inverted(gpio: u32) -> u32 {
    LENS.with_borrow(|slot| {
        slot.as_ref()
            .map_or(0, |l| l.emu.gpio_input_inverted(gpio as u8) as u32)
    })
}

/// Disassemble the PIO programs to a NUL-terminated string and return a pointer
/// into wasm memory (read it from JS with `UTF8ToString`).  The pointer stays
/// valid until the next call.
#[unsafe(no_mangle)]
pub extern "C" fn onerom_get_pio_disassembly() -> *const c_char {
    let text = LENS.with_borrow(|slot| {
        let Some(lens) = slot.as_ref() else {
            return String::new();
        };
        // RP2350 has three PIO blocks of four state machines; include every SM
        // that actually holds a program.
        let mut out = String::new();
        for block in 0..3u8 {
            for sm in 0..4u8 {
                if let Some(d) = lens.emu.disassemble_sm(block, sm)
                    && d.contains(".program")
                {
                    out.push_str(&d);
                    out.push_str("\n\n");
                }
            }
        }
        out
    });
    DISASM.with_borrow_mut(|c| {
        *c = CString::new(text).unwrap_or_default();
        c.as_ptr()
    })
}
