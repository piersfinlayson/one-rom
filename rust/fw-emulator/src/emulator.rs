// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Safe Rust wrapper around the C firmware emulation layer.
//!
//! # Lifecycle
//!
//! ```text
//!   Emulator::set_logging(enabled)  — optional, before boot, or after()
//!   Emulator::set_rp_variant(variant)  — before boot
//!   Emulator::set_sel_image(n)      - before boot
//!   Emulator::boot()                — calls firmware_main(), populates global state
//!        │
//!        ▼
//!   emu.limp_mode()                 — available immediately after boot
//!   emu.pios_enabled()
//!        │
//!   emu.setup_epio(word_size)       — creates epio_t, wires up SRAM + DMA chain
//!        │
//!        ▼
//!   emu.step_cycles(n)
//!   emu.drive_gpios(gpios, level)
//!   emu.read_pin_states()
//! ```
//!
//! # Thread safety
//!
//! `firmware_main` writes global C state.  Run tests with
//! `RUST_TEST_THREADS=1` (or `-- --test-threads=1`) to avoid races.

use crate::ffi;
use onerom_config::mcu::RpVariant;

use std::cell::RefCell;
use std::sync::OnceLock;

thread_local! {
    /// The closure installed via [`Emulator::set_yield_hook`], invoked by the
    /// C `onerom_test_yield` hook through [`yield_trampoline`].
    static YIELD_HOOK: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// C-ABI trampoline registered with the firmware's yield hook.  Dispatches to
/// the thread-local closure installed by [`Emulator::set_yield_hook`].
unsafe extern "C" fn yield_trampoline() {
    YIELD_HOOK.with(|h| {
        if let Ok(mut guard) = h.try_borrow_mut()
            && let Some(f) = guard.as_mut()
        {
            f();
        }
    });
}

/// Pristine image of `onerom_runtime_info`, captured on the first boot before
/// firmware_main() runs.  Restored on every subsequent in-process boot to
/// reproduce the cold-boot RAM state that Reset_Handler establishes on
/// hardware but which is skipped when firmware_main() is invoked directly via
/// FFI.  onerom_runtime_info is the only firmware RAM global, so this one
/// snapshot is the whole job.
static PRISTINE_RUNTIME: OnceLock<Vec<u8>> = OnceLock::new();

// ── Plugin API result type ────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum OraResult {
    Ok,
    Error,
    InvalidSize,
    InvalidArg,
    InternalError,
    ControlPinActive,
    InsufficientFreeMem,
    SlotActive,
    InvalidSlot,
    NoSlotActive,
    NotSupported,
    TypeMismatch,
    GpioInUse,
    LogChannelInUse,
    LogFull,
    /// A code this binding does not know. Carries the C type's own width,
    /// which `-fshort-enums` makes one byte.
    Unknown(ffi::ora_result_t),
}

impl From<ffi::ora_result_t> for OraResult {
    fn from(r: ffi::ora_result_t) -> Self {
        match r {
            ffi::ora_result_t_ORA_RESULT_OK => Self::Ok,
            ffi::ora_result_t_ORA_RESULT_ERROR => Self::Error,
            ffi::ora_result_t_ORA_RESULT_INVALID_SIZE => Self::InvalidSize,
            ffi::ora_result_t_ORA_RESULT_INVALID_ARG => Self::InvalidArg,
            ffi::ora_result_t_ORA_RESULT_INTERNAL_ERROR => Self::InternalError,
            ffi::ora_result_t_ORA_RESULT_CONTROL_PIN_ACTIVE => Self::ControlPinActive,
            ffi::ora_result_t_ORA_RESULT_INSUFFICIENT_FREE_MEM => Self::InsufficientFreeMem,
            ffi::ora_result_t_ORA_RESULT_SLOT_ACTIVE => Self::SlotActive,
            ffi::ora_result_t_ORA_RESULT_INVALID_SLOT => Self::InvalidSlot,
            ffi::ora_result_t_ORA_RESULT_NO_SLOT_ACTIVE => Self::NoSlotActive,
            ffi::ora_result_t_ORA_RESULT_NOT_SUPPORTED => Self::NotSupported,
            ffi::ora_result_t_ORA_RESULT_TYPE_MISMATCH => Self::TypeMismatch,
            ffi::ora_result_t_ORA_RESULT_GPIO_IN_USE => Self::GpioInUse,
            ffi::ora_result_t_ORA_RESULT_LOG_CHANNEL_IN_USE => Self::LogChannelInUse,
            ffi::ora_result_t_ORA_RESULT_LOG_FULL => Self::LogFull,
            other => Self::Unknown(other),
        }
    }
}

impl OraResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// The serving algorithms and address-sampling window of the running ROM slot.
///
/// See [`Emulator::serving_alg`].  The window matters because the address state
/// machine samples its pins free-running, ungated by chip select: a control line
/// inside `addr_window` is part of the SRAM index, so asserting it forces a
/// refetch, while one outside it only gates the data output drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingAlg {
    pub addr_alg: ffi::onerom_alg_addr_t,
    pub cs_alg: ffi::onerom_alg_cs_t,
    pub data_alg: ffi::onerom_alg_data_t,

    /// First GPIO the address state machine samples.
    pub addr_window_base: u8,

    /// How many consecutive GPIOs it samples from `addr_window_base`.
    pub addr_window_pins: u8,
}

impl ServingAlg {
    /// Whether `gpio` is inside the address state machine's sampled window,
    /// and so forms part of the SRAM index the DMA reads.
    pub fn samples_gpio(&self, gpio: u8) -> bool {
        gpio >= self.addr_window_base && (gpio - self.addr_window_base) < self.addr_window_pins
    }
}

// ── Plugin API helper types ───────────────────────────────────────────────────

pub struct RamSlotInfo {
    pub addr: u32,
    pub size: u32,
    pub rom_type: u32,
}

pub struct FlashSlotInfo {
    /// Points directly into firmware memory; valid for the lifetime of the emulator.
    pub name: Option<&'static std::ffi::CStr>,
    pub rom_type: u32,
    pub rom_count: u8,
}

/// One GPIO's state as reported by `ORA_ID_GPIO_QUERY`.
///
/// `gpio_use` is an [`ffi::ora_gpio_use_t`] value; it is left raw rather than
/// mapped to a Rust enum so a test can report an unexpected value verbatim
/// instead of collapsing it into a catch-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioInfo {
    /// Bytes the firmware reported writing.
    pub size: u8,
    pub gpio_use: u8,
    pub level: u8,
    pub is_output: u8,
}

/// One LED's state as reported by `ORA_ID_LED_GET`.
///
/// `mode` is an [`ffi::ora_led_mode_t`] value, left raw for the reason
/// [`GpioInfo`]'s `gpio_use` is: a test reports an unexpected one verbatim
/// rather than collapsing it into a catch-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedState {
    /// Bytes the firmware reported writing.
    pub size: u8,
    pub led: u8,
    pub present: u8,
    pub mode: u8,
    pub brightness: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub gpio: u8,
    pub reserved: u8,
    pub period_ms: u16,
}

/// Per-ROM detail for one ROM within a flash slot (via
/// `ORA_ID_GET_FLASH_SLOT_EXT_INFO`).
pub struct FlashSlotExtInfo {
    /// The ROM type string exactly as the user specified it (e.g. `27LC512`,
    /// not the canonical `27512`). `None` only if the firmware returned NULL,
    /// which the API forbids on success. Points directly into firmware memory.
    pub rom_type: Option<&'static std::ffi::CStr>,
    /// The ROM's filename string, or `None` if none. Points directly into
    /// firmware memory.
    pub filename: Option<&'static std::ffi::CStr>,
    pub chip_size: u32,
    pub rbcp_rom_type: u32,
}

// ── Internal macro ────────────────────────────────────────────────────────────

/// Call through `ora_fn_lookup` to a named plugin API function.
///
/// Panics if lookup returns NULL (indicates unimplemented or deprecated ID).
/// Transmute from `*mut c_void` to `Option<fn>` is sound: both are pointer-
/// sized and Rust's null-pointer optimisation means a non-null pointer
/// transmutes to `Some(fn)`.
macro_rules! plugin_call {
    ($id:expr, $fn_t:ty $(, $arg:expr)*) => {{
        let ptr = unsafe { ffi::ora_fn_lookup($id) };
        assert!(!ptr.is_null(), "ora_fn_lookup returned NULL for id {}", $id);
        let f: $fn_t = unsafe { std::mem::transmute(ptr) };
        unsafe { f.unwrap()($($arg),*) }
    }};
}

pub const ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS: u32 = 0x00000001;
pub const ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS: u32 = 0x00000002;

/// A handle to a running One ROM firmware emulator instance.
pub struct Emulator {
    /// Non-null after [`Self::setup_epio`] has been called.
    epio: *mut ffi::epio_t,
}

impl Emulator {
    /// Initialise the firmware by calling `firmware_main`.
    ///
    /// Firmware global state (limp mode flag, PIO enable state, etc.) is
    /// valid immediately after this returns.  Call [`Self::setup_epio`]
    /// before using any cycle-stepping or GPIO methods.
    pub fn boot() -> Self {
        // firmware_main() is called directly through FFI, bypassing
        // Reset_Handler (compiled out in the test build).  On hardware
        // Reset_Handler re-establishes the firmware's RAM state from its flash
        // image on every reset; here it never runs, so an in-process reboot
        // would otherwise inherit the previous run's mutated runtime info.
        // Put back the firmware's other process-global state.  In a test build
        // its statics are ordinary host objects, so nothing restores them
        // between the many boots one process runs — a channel a plugin claimed,
        // a pin it drove.  See onerom_test_reset() in firmware/test.
        unsafe { ffi::onerom_test_reset() };

        unsafe {
            let ptr = ffi::ffi_runtime_info_ptr() as *mut u8;
            let size = ffi::ffi_runtime_info_size() as usize;
            match PRISTINE_RUNTIME.get() {
                None => {
                    // First boot: snapshot the static-initialised image before
                    // firmware_main() mutates it.
                    let mut snapshot = vec![0u8; size];
                    core::ptr::copy_nonoverlapping(ptr, snapshot.as_mut_ptr(), size);
                    let _ = PRISTINE_RUNTIME.set(snapshot);
                }
                Some(snapshot) => {
                    // Subsequent boot: restore the cold-boot image.
                    core::ptr::copy_nonoverlapping(snapshot.as_ptr(), ptr, size);
                }
            }

            // s_host_sram_ptr is a !REAL_HARDWARE static whose cold-boot value
            // is NULL; setup_epio re-establishes it each boot.  Reset it here
            // so that, before this boot's setup_epio runs, sram_to_host falls
            // back to the firmware's real SRAM rather than the previous boot's
            // freed epio buffer.
            ffi::set_host_sram_ptr(core::ptr::null_mut());

            // SAFETY: firmware_main initialises the remaining global state and
            // returns (stubs prevent it from spinning or touching hardware).
            ffi::firmware_main();
        }
        Self {
            epio: core::ptr::null_mut(),
        }
    }

    /// Enable or disable logging from the firmware (goes to stdout if
    /// enabled).
    pub fn set_logging(enabled: bool) {
        unsafe { ffi::ffi_set_logging(enabled as u8) };
    }

    /// Set the RP variant (affects GPIO pinout).
    pub fn set_rp_variant(variant: Option<RpVariant>) {
        let is_b = matches!(variant, Some(RpVariant::Rp235xB));
        unsafe { ffi::stub_set_rp_variant(is_b as u8) };
    }

    /// Create and configure the emulated PIO handle.
    ///
    /// `word_size` is passed to `ffi_epio_setup_dma_chain`.
    ///
    /// After creating the epio instance and copying the current firmware SRAM
    /// content into it, the firmware's `sram_to_host()` is redirected to
    /// write directly into epio's buffer via `set_host_sram_ptr()`.  From
    /// this point all firmware SRAM writes (reprogram, copy-flash, etc.) land
    /// directly in epio without any explicit sync step.
    ///
    /// # Panics
    ///
    /// Panics if called twice, or if `epio_from_apio` returns null.
    ///
    pub fn setup_epio(&mut self, word_size: u8) {
        assert!(self.epio.is_null(), "setup_epio called twice");

        // SAFETY: firmware_main has populated global state that epio_from_apio reads.
        let epio = unsafe { ffi::epio_from_apio() };
        assert!(!epio.is_null(), "epio_from_apio returned null");

        // Copy the current firmware SRAM content (ROM images, slot tables)
        // into epio's buffer so the simulation starts with the correct data.
        // SAFETY: epio is non-null and freshly allocated.
        unsafe { ffi::ffi_epio_setup_sram(epio) };

        // Redirect firmware's sram_to_host() to write into epio's buffer.
        // From this point, firmware SRAM writes are immediately visible to
        // the running epio simulation without any explicit sync.
        // SAFETY: epio is non-null; epio_get_sram_ptr returns its internal buffer.
        let sram_ptr = unsafe { ffi::epio_get_sram_ptr(epio) };
        assert!(!sram_ptr.is_null(), "epio_get_sram_ptr returned null");
        unsafe { ffi::set_host_sram_ptr(sram_ptr) };

        unsafe { ffi::ffi_epio_setup_dma_chain(epio, word_size) };

        self.epio = epio;
    }

    /// Install a yield hook invoked whenever the firmware would busy-wait on
    /// hardware the emulator drives (see `onerom_test_yield` in the firmware).
    ///
    /// The closure typically advances the simulation — drive the next stimulus
    /// and step cycles — so a blocking firmware poll (e.g. `wait_for_knock`)
    /// makes progress in this single-threaded harness.  Replaces any previously
    /// installed hook.
    pub fn set_yield_hook(&self, f: impl FnMut() + 'static) {
        YIELD_HOOK.with(|h| *h.borrow_mut() = Some(Box::new(f)));
        unsafe { ffi::set_onerom_test_yield_hook(Some(yield_trampoline)) };
    }

    /// Remove any installed yield hook.
    pub fn clear_yield_hook(&self) {
        unsafe { ffi::set_onerom_test_yield_hook(None) };
        YIELD_HOOK.with(|h| *h.borrow_mut() = None);
    }

    /// Arm the address-monitor emulation seam.
    ///
    /// Installs the hook the firmware's `pio_setup_address_monitor_dma` calls
    /// under emulation, so a later `setup_address_monitor` (via the plugin API)
    /// wires up epio's capture channel from the block/SM the firmware chose.
    /// Call after [`Self::setup_epio`] and before configuring the monitor.
    pub fn arm_monitor(&self) {
        unsafe { ffi::ffi_epio_arm_monitor(self.epio_or_panic()) };
    }

    /// Apply accumulated apio state (new/enabled SMs, GPIO config) to the live
    /// epio instance.  Call after any firmware step that extends the PIO
    /// configuration via `APIO_ASM_CONTINUE` — e.g. `setup_address_monitor`
    /// and `start_address_monitor`.
    pub fn update_from_apio(&self) {
        unsafe { ffi::epio_update_from_apio(self.epio_or_panic()) };
    }

    // ── Address-monitor plugin API (via ora_fn_lookup) ───────────────────────

    /// A raw host pointer into epio's SRAM buffer at RP2350 address `addr`.
    /// The firmware is handed this as its `ring_buf`, so its native pointer
    /// arithmetic and dereferences hit the same buffer epio's capture DMA
    /// writes into.
    pub fn sram_host_ptr(&self, addr: u32) -> *mut u32 {
        let base = unsafe { ffi::epio_get_sram_ptr(self.epio_or_panic()) };
        // SRAM_BASE is 0x20000000 in both the firmware and epio.
        unsafe { base.add((addr - 0x2000_0000) as usize) as *mut u32 }
    }

    /// `ORA_ID_SETUP_ADDRESS_MONITOR`.  `ring_buf` must be a host pointer into
    /// epio SRAM (see [`Self::sram_host_ptr`]).
    ///
    /// # Safety
    /// `ring_buf` must point at a valid, correctly aligned ring buffer of
    /// `2^ring_entries_log2` entries within epio SRAM, live for the monitor's
    /// lifetime.
    pub unsafe fn setup_address_monitor(
        &self,
        ring_buf: *mut u32,
        ring_entries_log2: u8,
        mode: ffi::ora_monitor_mode_t,
        data_size: u8,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_SETUP_ADDRESS_MONITOR,
            ffi::ora_setup_address_monitor_fn_t,
            ring_buf,
            ring_entries_log2,
            mode,
            data_size,
            core::ptr::null_mut()
        ))
    }

    /// `ORA_ID_INIT_KNOCK`.  Fills the caller-allocated `knock` structure.
    ///
    /// # Safety
    /// `knock` must point at a writable `ora_knock_t` sized for `knock_seq.len()`
    /// entries (see `ORA_KNOCK_SIZE`).
    pub unsafe fn init_knock(
        &self,
        knock_seq: &[u32],
        knock_bits: u8,
        data_size: u8,
        knock: *mut ffi::ora_knock_t,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_INIT_KNOCK,
            ffi::ora_init_knock_fn_t,
            knock_seq.as_ptr(),
            knock_seq.len() as u8,
            knock_bits,
            data_size,
            knock
        ))
    }

    /// `ORA_ID_START_ADDRESS_MONITOR`.
    pub fn start_address_monitor(&self) {
        plugin_call!(
            ffi::api_id_t_ORA_ID_START_ADDRESS_MONITOR,
            ffi::ora_start_address_monitor_fn_t
        );
    }

    /// `ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS`.  Returns the slot whose
    /// pointed-to value is the current ring write pointer.
    pub fn get_address_monitor_ring_write_pos(&self) -> *mut *mut u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS,
            ffi::ora_get_address_monitor_ring_write_pos_fn_t
        )
    }

    /// `ORA_ID_WAIT_FOR_KNOCK`.  Blocking: drive the simulation forward via a
    /// yield hook (see [`Self::set_yield_hook`]) so this can make progress.
    ///
    /// # Safety
    /// `knock` and `ring_buf` must be valid for the monitor; `payload_out` must
    /// point at `payload_len` writable `u32`s; `start_pos`/`next_read_out` are
    /// each either null or valid.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn wait_for_knock(
        &self,
        knock: *const ffi::ora_knock_t,
        ring_buf: *mut u32,
        ring_entries_log2: u8,
        flags: u32,
        payload_out: *mut u32,
        payload_len: u8,
        start_pos: *mut u32,
        next_read_out: *mut *mut u32,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_WAIT_FOR_KNOCK,
            ffi::ora_wait_for_knock_fn_t,
            knock,
            ring_buf,
            ring_entries_log2,
            flags,
            payload_out,
            payload_len,
            start_pos,
            next_read_out
        ))
    }

    // ── Firmware state queries (valid after boot()) ──────────────────────────

    /// Returns `true` if the firmware is operating in limp mode.
    pub fn limp_mode(&self) -> bool {
        unsafe { ffi::ffi_limp_mode() as i32 != 0 }
    }

    /// Returns `true` if the PIO state machines are enabled.
    pub fn pios_enabled(&self) -> bool {
        unsafe { ffi::ffi_pios_enabled() as i32 != 0 }
    }

    /// The serving algorithms and address window the current ROM slot runs.
    ///
    /// `None` before a slot is being served.  Read from the live slot config,
    /// so it is what the firmware is actually running rather than a host-side
    /// re-derivation that could drift from it.
    pub fn serving_alg(&self) -> Option<ServingAlg> {
        let mut raw = ffi::ffi_serving_alg_t {
            addr_alg: 0,
            cs_alg: 0,
            data_alg: 0,
            addr_window_base: 0,
            addr_window_pins: 0,
        };
        if unsafe { ffi::ffi_serving_alg(&mut raw) } == 0 {
            return None;
        }
        Some(ServingAlg {
            addr_alg: raw.addr_alg as ffi::onerom_alg_addr_t,
            cs_alg: raw.cs_alg as ffi::onerom_alg_cs_t,
            data_alg: raw.data_alg as ffi::onerom_alg_data_t,
            addr_window_base: raw.addr_window_base,
            addr_window_pins: raw.addr_window_pins,
        })
    }

    // ── ROM image selection ──────────────────────────────────────────────────

    /// Tell the stub which ROM image to present.
    pub fn set_sel_image(image: u8) {
        unsafe { ffi::stub_set_sel_image(image as _) };
    }

    /// The image-select value the firmware read from the sel pins on this boot.
    ///
    /// This is the firmware's own reading, not the stub's view of what it
    /// drove, so it covers the whole request -> pins -> firmware path.  It
    /// equals the value passed to [`Self::set_sel_image`], except that a value
    /// the board's sel pins cannot express wraps, exactly as on hardware.
    ///
    /// Check it after [`Self::boot`] rather than assuming the request took
    /// effect: a case that silently runs against the wrong image reports
    /// whatever that image does under the intended case's label.
    pub fn sel_image(&self) -> u8 {
        unsafe { ffi::ffi_image_sel() }
    }

    // ── GPIO / cycle operations (require setup_epio()) ───────────────────────

    /// Drive external GPIO states into the emulator.
    ///
    /// `gpios` is a bitmask of pins to affect; `level` is the level for each.
    pub fn drive_gpios(&self, gpios: u64, level: u64) {
        unsafe { ffi::epio_drive_gpios_ext(self.epio_or_panic(), gpios, level) };
    }

    /// Read the current emulated GPIO pin states.
    pub fn read_pin_states(&self) -> u64 {
        unsafe { ffi::epio_read_pin_states(self.epio_or_panic()) }
    }

    /// Advance the emulation by `cycles` clock cycles.
    pub fn step_cycles(&self, cycles: u32) {
        unsafe { ffi::epio_step_cycles(self.epio_or_panic(), cycles) };
    }

    /// Return a bitmask of all GPIO pins currently driven by the PIO or
    /// externally.
    pub fn read_driven_pins(&self) -> u64 {
        unsafe { ffi::epio_read_driven_pins(self.epio_or_panic()) }
    }

    /// Return a bitmask of all GPIO pins that have a pull-up configured.
    pub fn read_pull_up_pins(&self) -> u64 {
        unsafe { ffi::epio_read_pull_up_pins(self.epio_or_panic()) }
    }

    /// Return a bitmask of all GPIO pins that have a pull-down configured.
    pub fn read_pull_down_pins(&self) -> u64 {
        unsafe { ffi::epio_read_pull_down_pins(self.epio_or_panic()) }
    }

    /// Whether GPIO `pin` is read inverted by the firmware's pin routing.
    pub fn gpio_input_inverted(&self, pin: u8) -> bool {
        unsafe { ffi::epio_get_gpio_input_inverted(self.epio_or_panic(), pin) != 0 }
    }

    /// Disassemble one PIO state machine (`block`, `sm`) to text, or `None` if
    /// it has no program / is unavailable.
    pub fn disassemble_sm(&self, block: u8, sm: u8) -> Option<String> {
        let mut buf = [0u8; 4096];
        let n = unsafe {
            ffi::epio_disassemble_sm(
                self.epio_or_panic(),
                block,
                sm,
                buf.as_mut_ptr() as *mut core::ffi::c_char,
                buf.len(),
            )
        };
        if n <= 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn epio_or_panic(&self) -> *mut ffi::epio_t {
        assert!(
            !self.epio.is_null(),
            "call setup_epio() before using GPIO/cycle methods"
        );
        self.epio
    }

    // ── Plugin API ────────────────────────────────────────────────────────────

    /// Returns true if the given API ID resolves to a non-NULL function pointer.
    /// Use this to verify lookup table coverage.
    pub fn plugin_lookup_valid(&self, id: ffi::api_id_t) -> bool {
        !unsafe { ffi::ora_fn_lookup(id) }.is_null()
    }

    pub fn map_addr_to_phys(&self, logical_addr: u32) -> u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_MAP_ADDR_TO_PHYS,
            ffi::ora_map_addr_to_phys_fn_t,
            logical_addr
        )
    }

    pub fn demangle_addr(&self, physical_addr: u32, check_control_pins: bool) -> (OraResult, u32) {
        let mut logical: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_DEMANGLE_ADDR,
            ffi::ora_demangle_addr_fn_t,
            physical_addr,
            &mut logical as *mut u32,
            check_control_pins as u8
        );
        (OraResult::from(r), logical)
    }

    /// Demangle a ring capture to the address the device observes on its
    /// address lines, rather than the logical byte address.
    ///
    /// The two differ on the 40-pin variant, whose monitor does not observe the
    /// ROM's least-significant address line; this is the space host-to-device
    /// command signalling travels in.  See `ora_demangle_observed_addr_fn_t`.
    pub fn demangle_observed_addr(
        &self,
        physical_addr: u32,
        check_control_pins: bool,
    ) -> (OraResult, u32) {
        let mut observed: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_DEMANGLE_OBSERVED_ADDR,
            ffi::ora_demangle_observed_addr_fn_t,
            physical_addr,
            &mut observed as *mut u32,
            check_control_pins as u8
        );
        (OraResult::from(r), observed)
    }

    /// Number of least-significant address bits the device does not observe for
    /// the current ROM: 0 on the 24/28/32-pin variants, 1 on the 40-pin.
    pub fn get_unobserved_addr_bits(&self) -> (OraResult, u8) {
        let mut bits: u8 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_UNOBSERVED_ADDR_BITS,
            ffi::ora_get_unobserved_addr_bits_fn_t,
            &mut bits as *mut u8
        );
        (OraResult::from(r), bits)
    }

    pub fn map_data_to_phys(&self, logical_data: u8) -> u8 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_MAP_DATA_TO_PHYS,
            ffi::ora_map_data_to_phys_fn_t,
            logical_data
        )
    }

    pub fn demangle_data(&self, physical_data: u8) -> (OraResult, u8) {
        let mut logical: u8 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_DEMANGLE_DATA,
            ffi::ora_demangle_data_fn_t,
            physical_data,
            &mut logical as *mut u8
        );
        (OraResult::from(r), logical)
    }

    pub fn get_ram_slot_count(&self) -> u8 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_RAM_SLOT_COUNT,
            ffi::ora_get_ram_slot_count_fn_t
        )
    }

    pub fn get_ram_slot_info(&self, ram_slot: u8) -> (OraResult, Option<RamSlotInfo>) {
        let mut addr: u32 = 0;
        let mut size: u32 = 0;
        let mut rom_type: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_RAM_SLOT_INFO,
            ffi::ora_get_ram_slot_info_fn_t,
            ram_slot,
            &mut addr as *mut u32,
            &mut size as *mut u32,
            &mut rom_type as *mut u32
        );
        let r = OraResult::from(r);
        let info = r.is_ok().then_some(RamSlotInfo {
            addr,
            size,
            rom_type,
        });
        (r, info)
    }

    pub fn get_active_ram_slot(&self) -> (OraResult, Option<u8>) {
        let mut slot: u8 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_ACTIVE_RAM_SLOT,
            ffi::ora_get_active_ram_slot_fn_t,
            &mut slot as *mut u8
        );
        let r = OraResult::from(r);
        let slot = r.is_ok().then_some(slot);
        (r, slot)
    }

    pub fn set_active_ram_slot(&self, ram_slot: u8) -> OraResult {
        let result = OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_SET_ACTIVE_RAM_SLOT,
            ffi::ora_set_active_ram_slot_fn_t,
            ram_slot
        ));
        // set_active_ram_slot calls pio_switch_rom_region, which uses
        // APIO_ASM_CONTINUE to accumulate pre-instructions that update the
        // address SM's X register with the new SRAM region base.  Apply
        // those to the live epio instance now so the simulation serves from
        // the correct slot.
        if result.is_ok() {
            unsafe { ffi::epio_update_from_apio(self.epio_or_panic()) };
        }
        result
    }

    pub fn get_flash_slot_count(&self, flags: u32) -> u8 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_COUNT,
            ffi::ora_get_flash_slot_count_fn_t,
            flags
        )
    }

    pub fn get_flash_slot_info(
        &self,
        flash_slot: u8,
        flags: u32,
    ) -> (OraResult, Option<FlashSlotInfo>) {
        let mut name_ptr: *const std::os::raw::c_char = std::ptr::null();
        let mut rom_type: u32 = 0;
        let mut rom_count: u8 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_INFO,
            ffi::ora_get_flash_slot_info_fn_t,
            flash_slot,
            flags,
            &mut name_ptr as *mut *const std::os::raw::c_char,
            &mut rom_type as *mut u32,
            &mut rom_count as *mut u8
        );
        let r = OraResult::from(r);
        let info = r.is_ok().then(|| {
            let name = (!name_ptr.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(name_ptr) });
            FlashSlotInfo {
                name,
                rom_type,
                rom_count,
            }
        });
        (r, info)
    }

    pub fn get_flash_slot_ext_info(
        &self,
        flash_slot: u8,
        rom_index: u8,
        flags: u32,
    ) -> (OraResult, Option<FlashSlotExtInfo>) {
        let mut rom_type_ptr: *const std::os::raw::c_char = std::ptr::null();
        let mut filename_ptr: *const std::os::raw::c_char = std::ptr::null();
        let mut chip_size: u32 = 0;
        let mut rbcp_rom_type: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_EXT_INFO,
            ffi::ora_get_flash_slot_ext_info_fn_t,
            flash_slot,
            rom_index,
            flags,
            &mut rom_type_ptr as *mut *const std::os::raw::c_char,
            &mut filename_ptr as *mut *const std::os::raw::c_char,
            &mut chip_size as *mut u32,
            &mut rbcp_rom_type as *mut u32
        );
        let r = OraResult::from(r);
        let info = r.is_ok().then(|| {
            let rom_type = (!rom_type_ptr.is_null())
                .then(|| unsafe { std::ffi::CStr::from_ptr(rom_type_ptr) });
            let filename = (!filename_ptr.is_null())
                .then(|| unsafe { std::ffi::CStr::from_ptr(filename_ptr) });
            FlashSlotExtInfo {
                rom_type,
                filename,
                chip_size,
                rbcp_rom_type,
            }
        });
        (r, info)
    }

    pub fn reprogram_ram_rom_slot(
        &self,
        slot: u8,
        offset: u32,
        buf: &[u8],
        allow_active: bool,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_REPROGRAM_RAM_ROM_SLOT,
            ffi::ora_reprogram_ram_rom_slot_fn_t,
            slot,
            offset,
            buf.as_ptr(),
            buf.len() as u32,
            allow_active as u8
        ))
    }

    pub fn read_ram_rom_slot(&self, slot: u8, offset: u32, buf: &mut [u8]) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_READ_RAM_ROM_SLOT,
            ffi::ora_read_ram_rom_slot_fn_t,
            slot,
            offset,
            buf.as_mut_ptr(),
            buf.len() as u32
        ))
    }

    pub fn copy_flash_slot_to_ram_slot(
        &self,
        flash_slot: u8,
        flags: u32,
        ram_slot: u8,
        copy_flags: u32,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT,
            ffi::ora_copy_flash_slot_to_ram_slot_fn_t,
            flash_slot,
            flags,
            ram_slot,
            copy_flags
        ))
    }

    pub fn get_device_version(&self, max_len: u32) -> (OraResult, Option<String>) {
        let mut buf = vec![0u8; max_len as usize];
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_DEVICE_VERSION,
            ffi::ora_get_device_version_fn_t,
            buf.as_mut_ptr(),
            max_len
        );
        let r = OraResult::from(r);
        let s = r.is_ok().then(|| {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..end]).into_owned()
        });
        (r, s)
    }

    pub fn get_metadata_str(&self, key: ffi::ora_metadata_key_t) -> (OraResult, Option<String>) {
        let mut ptr: *const std::os::raw::c_char = std::ptr::null();
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_METADATA_STR,
            ffi::ora_get_metadata_str_fn_t,
            key,
            &mut ptr as *mut *const std::os::raw::c_char
        );
        let r = OraResult::from(r);
        // An unset optional field is OK with a NULL pointer (None), distinct
        // from a non-OK result (e.g. NotSupported for an unknown key).
        let s = (r.is_ok() && !ptr.is_null()).then(|| {
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        });
        (r, s)
    }

    pub fn get_metadata_uint(&self, key: ffi::ora_metadata_key_t) -> (OraResult, Option<u32>) {
        let mut val: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_METADATA_UINT,
            ffi::ora_get_metadata_uint_fn_t,
            key,
            &mut val as *mut u32
        );
        let r = OraResult::from(r);
        let v = r.is_ok().then_some(val);
        (r, v)
    }

    /// `ORA_ID_GET_METADATA_UINT_AT`, reading element `index` of an
    /// array-valued key.
    pub fn get_metadata_uint_at(
        &self,
        key: ffi::ora_metadata_key_t,
        index: u32,
    ) -> (OraResult, Option<u32>) {
        let mut val: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_METADATA_UINT_AT,
            ffi::ora_get_metadata_uint_at_fn_t,
            key,
            index,
            &mut val as *mut u32
        );
        let r = OraResult::from(r);
        let v = r.is_ok().then_some(val);
        (r, v)
    }

    /// See [`Self::get_compile_option_uint_null_out`].
    pub fn get_metadata_uint_at_null_out(
        &self,
        key: ffi::ora_metadata_key_t,
        index: u32,
    ) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_GET_METADATA_UINT_AT,
            ffi::ora_get_metadata_uint_at_fn_t,
            key,
            index,
            std::ptr::null_mut::<u32>()
        ))
    }

    /// `ORA_ID_GET_PLUGIN_UPTIME_MS`.
    pub fn get_plugin_uptime_ms(&self) -> u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_PLUGIN_UPTIME_MS,
            ffi::ora_get_plugin_uptime_ms_fn_t
        )
    }

    /// Place the microsecond counter behind `ORA_ID_GET_PLUGIN_UPTIME_MS` at `us`.
    ///
    /// The count is the harness's, not a device's - there is no TIMER0 in this
    /// process - so a test can put it anywhere, including either side of the
    /// 49.7 day wrap, without waiting for wall time.
    pub fn set_timer_us(&self, us: u64) {
        unsafe { ffi::stub_set_timer_us(us) };
    }

    /// Move the microsecond counter behind `ORA_ID_GET_PLUGIN_UPTIME_MS` on by
    /// `delta_us`.
    pub fn advance_timer_us(&self, delta_us: u64) {
        unsafe { ffi::stub_advance_timer_us(delta_us) };
    }

    /// Run the LED engine's frame, the way TIMER0 alarm 1 does on a device.
    ///
    /// There is no alarm in this process, so a harness stands where the
    /// interrupt does: move the clock to [`Emulator::led_next_deadline_ms`] and
    /// call this. It advances whatever is animating and ends a hold that has
    /// expired.
    pub fn led_frame(&self) {
        unsafe { ffi::ffi_led_frame() };
    }

    /// When the LED engine next wants a frame, in the milliseconds a plugin
    /// sees, or `None` when nothing is animating and no hold is running.
    pub fn led_next_deadline_ms(&self) -> Option<u32> {
        let mut ms: u32 = 0;
        let have = unsafe { ffi::ffi_led_next_deadline(&raw mut ms) };

        (have != 0).then_some(ms)
    }

    /// Forget what both LEDs are doing, so a test starts from a known state
    /// rather than from what the test before it left.
    ///
    /// Nothing is driven from here - a channel's mode is forgotten, not turned
    /// off - so a test that cares about the pin sets a mode after it.
    pub fn led_reset(&self) {
        unsafe { ffi::ffi_led_reset() };
    }

    /// The last colour the LED engine sent to the RGB LED, and how many it has
    /// sent.
    ///
    /// The value is what the chip reads - green, red then blue, with brightness
    /// and any fade already applied - so this is the LED's actual output rather
    /// than what was asked for.
    pub fn led_last_pixel(&self) -> (u32, u32) {
        let mut count: u32 = 0;
        let pixel = unsafe { ffi::ffi_led_last_pixel(&raw mut count) };

        (pixel, count)
    }

    /// Script the counter's value across successive reads of its halves, one
    /// entry consumed per half-read, holding at the last entry once spent.
    ///
    /// The firmware reads the high half, the low half, then the high half
    /// again, retrying while the high half moved. Scripting a change part way
    /// through that sequence is the only way to exercise the retry - on a
    /// device the high half moves once every 71 minutes.
    pub fn set_timer_raw_script(&self, values: &[u64]) {
        unsafe { ffi::stub_set_timer_raw_script(values.as_ptr(), values.len() as u32) };
    }

    /// `ORA_ID_GPIO_QUERY`, telling the firmware the caller's structure is
    /// `caller_size` bytes.
    ///
    /// Fields beyond what the firmware writes are returned as the sentinel
    /// `0xFF` this function pre-fills them with, so a caller exercising the
    /// forward-compatibility contract can tell "not written" from "written as
    /// zero".
    pub fn gpio_query_sized(&self, gpio: u8, caller_size: u8) -> (OraResult, GpioInfo) {
        let mut info = ffi::ora_gpio_info_t {
            size: caller_size,
            use_: 0xFF,
            level: 0xFF,
            is_output: 0xFF,
        };
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GPIO_QUERY,
            ffi::ora_gpio_query_fn_t,
            gpio,
            &mut info as *mut ffi::ora_gpio_info_t
        );
        (
            OraResult::from(r),
            GpioInfo {
                size: info.size,
                gpio_use: info.use_,
                level: info.level,
                is_output: info.is_output,
            },
        )
    }

    /// `ORA_ID_GPIO_QUERY` with the full structure this build knows about.
    pub fn gpio_query(&self, gpio: u8) -> (OraResult, GpioInfo) {
        self.gpio_query_sized(gpio, size_of::<ffi::ora_gpio_info_t>() as u8)
    }

    /// A zeroed `ORA_ID_LED_SET` request, with its size field set as a caller
    /// built against this header sets it.
    ///
    /// Zero in a field means the firmware chooses, so a caller fills in only
    /// what it means to ask for.
    pub fn led_request() -> ffi::ora_led_request_t {
        ffi::ora_led_request_t {
            size: size_of::<ffi::ora_led_request_t>() as u8,
            led: 0,
            mode: 0,
            brightness: 0,
            red: 0,
            green: 0,
            blue: 0,
            reserved0: 0,
            period_ms: 0,
            reserved1: [0; 2],
            hold_ms: 0,
        }
    }

    /// `ORA_ID_LED_SET`.
    ///
    /// The request is passed exactly as given, size field included, so a caller
    /// exercising the size contract states its own.
    pub fn led_set(&self, req: &ffi::ora_led_request_t) -> OraResult {
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LED_SET,
            ffi::ora_led_set_fn_t,
            req as *const ffi::ora_led_request_t
        );
        OraResult::from(r)
    }

    /// `ORA_ID_LED_SET` with a NULL request.
    pub fn led_set_null(&self) -> OraResult {
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LED_SET,
            ffi::ora_led_set_fn_t,
            std::ptr::null()
        );
        OraResult::from(r)
    }

    /// `ORA_ID_LED_GET`, telling the firmware the caller's structure is
    /// `caller_size` bytes.
    ///
    /// Fields beyond what the firmware writes are returned as the sentinel
    /// `0xFF` this function pre-fills them with, so a caller can tell "not
    /// written" from "written as zero".
    pub fn led_get_sized(&self, led: u8, caller_size: u8) -> (OraResult, LedState) {
        let mut state = ffi::ora_led_state_t {
            size: caller_size,
            led: 0xFF,
            present: 0xFF,
            mode: 0xFF,
            brightness: 0xFF,
            red: 0xFF,
            green: 0xFF,
            blue: 0xFF,
            gpio: 0xFF,
            reserved: 0xFF,
            period_ms: 0xFFFF,
        };
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LED_GET,
            ffi::ora_led_get_fn_t,
            led,
            &mut state as *mut ffi::ora_led_state_t
        );
        (
            OraResult::from(r),
            LedState {
                size: state.size,
                led: state.led,
                present: state.present,
                mode: state.mode,
                brightness: state.brightness,
                red: state.red,
                green: state.green,
                blue: state.blue,
                gpio: state.gpio,
                reserved: state.reserved,
                period_ms: state.period_ms,
            },
        )
    }

    /// `ORA_ID_LED_GET` with the full structure this build knows about.
    pub fn led_get(&self, led: u8) -> (OraResult, LedState) {
        self.led_get_sized(led, size_of::<ffi::ora_led_state_t>() as u8)
    }

    /// `ORA_ID_LED_GET` with a NULL state pointer.
    pub fn led_get_null(&self, led: u8) -> OraResult {
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LED_GET,
            ffi::ora_led_get_fn_t,
            led,
            std::ptr::null_mut()
        );
        OraResult::from(r)
    }

    /// `ORA_ID_SET_STATUS_LED`, driving the status LED as a plugin does.
    ///
    /// The LED is shared - every plugin reaches it through this one call, and
    /// reads the result back through `ORA_METADATA_KEY_STATUS_LED_STATE`.  So a
    /// harness calling it is standing in for the other plugin, which is what
    /// lets a test check that the plugin under test leaves the LED alone.
    pub fn set_status_led(&self, on: bool) {
        plugin_call!(
            ffi::api_id_t_ORA_ID_SET_STATUS_LED,
            ffi::ora_set_status_led_fn_t,
            u8::from(on)
        )
    }

    // ── Logging plugin API ───────────────────────────────────────────────────

    /// Act as `plugin` for subsequent logging API calls.
    ///
    /// On a device the calling core identifies the plugin. There is no core to
    /// read here, so the harness says — which is what lets a test claim a
    /// channel as one plugin and check the other is kept out.
    pub fn set_calling_plugin(&self, plugin: ffi::ora_plugin_type_t) {
        unsafe { ffi::set_host_calling_plugin(plugin) };
    }

    /// `ORA_ID_LOG_OPEN_WRITE`.
    ///
    /// `name` must outlive the claim, exactly as it must on a device: the
    /// firmware stores the pointer rather than copying the string, so callers
    /// pass a `&'static CStr`.
    pub fn log_open_write(&self, channel: u32, name: &'static std::ffi::CStr) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_OPEN_WRITE,
            ffi::ora_log_open_write_fn_t,
            channel,
            name.as_ptr()
        ))
    }

    /// `ORA_ID_LOG_WRITE`.
    pub fn log_write(&self, channel: u32, buf: &[u8]) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_WRITE,
            ffi::ora_log_write_fn_t,
            channel,
            buf.as_ptr() as *const core::ffi::c_void,
            buf.len() as u32
        ))
    }

    /// `ORA_ID_LOG_CLOSE_WRITE`.
    pub fn log_close_write(&self, channel: u32) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_CLOSE_WRITE,
            ffi::ora_log_close_write_fn_t,
            channel
        ))
    }

    /// `ORA_ID_LOG_OPEN_READ`.
    pub fn log_open_read(&self, channel: u32) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_OPEN_READ,
            ffi::ora_log_open_read_fn_t,
            channel
        ))
    }

    /// `ORA_ID_LOG_READ`, returning the result and the bytes actually copied.
    ///
    /// The returned `Vec` is truncated to what the firmware reported copying,
    /// so a test comparing it against what was written also checks the count.
    pub fn log_read(&self, channel: u32, max_len: u32) -> (OraResult, Vec<u8>) {
        let mut buf = vec![0u8; max_len as usize];
        let mut copied: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_READ,
            ffi::ora_log_read_fn_t,
            channel,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            max_len,
            &mut copied as *mut u32
        );
        buf.truncate(copied as usize);
        (OraResult::from(r), buf)
    }

    /// `ORA_ID_LOG_CLOSE_READ`.
    pub fn log_close_read(&self, channel: u32) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_CLOSE_READ,
            ffi::ora_log_close_read_fn_t,
            channel
        ))
    }

    /// `ORA_ID_SET_PLUGIN_CONTEXT`.
    ///
    /// `context` is an opaque value the firmware only stores, so a test can
    /// pass any distinguishable number rather than a real pointer.
    pub fn set_plugin_context(&self, plugin: ffi::ora_plugin_type_t, context: usize) {
        plugin_call!(
            ffi::api_id_t_ORA_ID_SET_PLUGIN_CONTEXT,
            ffi::ora_set_plugin_context_fn_t,
            plugin,
            context as *mut core::ffi::c_void
        )
    }

    /// `ORA_ID_GET_PLUGIN_CONTEXT`.
    pub fn get_plugin_context(&self, plugin: ffi::ora_plugin_type_t) -> usize {
        let p = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_PLUGIN_CONTEXT,
            ffi::ora_get_plugin_context_fn_t,
            plugin
        );
        p as usize
    }

    /// The two plugin context slots read straight out of the runtime info
    /// block, as `(system, user)`.
    ///
    /// `api.h` publishes the addresses of these two fields as
    /// `ORA_GET_PLUGIN_CONTEXT_SYSTEM` and `ORA_GET_PLUGIN_CONTEXT_USER`, so
    /// reading them directly is what an interrupt handler on a device does.
    /// A test comparing these against what the API stored therefore checks the
    /// API and the macros agree, rather than only that the API is
    /// self-consistent.
    pub fn plugin_context_addrs(&self) -> (usize, usize) {
        unsafe {
            (
                ffi::ffi_system_plugin_context() as usize,
                ffi::ffi_user_plugin_context() as usize,
            )
        }
    }

    /// `ORA_ID_LOG_QUERY`, returning the result and (size, free, pending).
    pub fn log_query(&self, channel: u32) -> (OraResult, u32, u32, u32) {
        let (mut size, mut free, mut pending) = (0u32, 0u32, 0u32);
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_QUERY,
            ffi::ora_log_query_fn_t,
            channel,
            &mut size as *mut u32,
            &mut free as *mut u32,
            &mut pending as *mut u32
        );
        (OraResult::from(r), size, free, pending)
    }

    // ── Compile options and log categories ───────────────────────────────────

    /// `ORA_ID_GET_COMPILE_OPTION_UINT`.
    pub fn get_compile_option_uint(
        &self,
        option: ffi::ora_compile_option_t,
    ) -> (OraResult, Option<u32>) {
        let mut val: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_UINT,
            ffi::ora_get_compile_option_uint_fn_t,
            option,
            &mut val as *mut u32
        );
        let r = OraResult::from(r);
        let v = r.is_ok().then_some(val);
        (r, v)
    }

    /// `ORA_ID_GET_COMPILE_OPTION_UINT` with a NULL out pointer.
    ///
    /// The NULL guard is what stops a plugin's mistake becoming a fault on a
    /// device, so it is worth a test of its own, and a test cannot reach it
    /// through a wrapper that always supplies somewhere to write.
    pub fn get_compile_option_uint_null_out(&self, option: ffi::ora_compile_option_t) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_UINT,
            ffi::ora_get_compile_option_uint_fn_t,
            option,
            std::ptr::null_mut::<u32>()
        ))
    }

    /// `ORA_ID_GET_COMPILE_OPTION_STR`.
    pub fn get_compile_option_str(
        &self,
        option: ffi::ora_compile_option_t,
    ) -> (OraResult, Option<String>) {
        let mut ptr: *const std::os::raw::c_char = std::ptr::null();
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_STR,
            ffi::ora_get_compile_option_str_fn_t,
            option,
            &mut ptr as *mut *const std::os::raw::c_char
        );
        let r = OraResult::from(r);
        let s = (r.is_ok() && !ptr.is_null()).then(|| {
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        });
        (r, s)
    }

    /// `ORA_ID_GET_COMPILE_OPTION_STR` with a NULL out pointer.
    ///
    /// See [`Self::get_compile_option_uint_null_out`].
    pub fn get_compile_option_str_null_out(&self, option: ffi::ora_compile_option_t) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_STR,
            ffi::ora_get_compile_option_str_fn_t,
            option,
            std::ptr::null_mut::<*const std::os::raw::c_char>()
        ))
    }

    /// `ORA_ID_LOG_CATEGORY_ENABLED`.
    pub fn log_category_enabled(
        &self,
        category: ffi::ora_log_category_t,
    ) -> (OraResult, Option<u32>) {
        let mut enabled: u32 = 0;
        let r = plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_CATEGORY_ENABLED,
            ffi::ora_log_category_enabled_fn_t,
            category,
            &mut enabled as *mut u32
        );
        let r = OraResult::from(r);
        let v = r.is_ok().then_some(enabled);
        (r, v)
    }

    /// `ORA_ID_LOG_CATEGORY_ENABLED` with a NULL out pointer.
    ///
    /// See [`Self::get_compile_option_uint_null_out`].
    pub fn log_category_enabled_null_out(&self, category: ffi::ora_log_category_t) -> OraResult {
        OraResult::from(plugin_call!(
            ffi::api_id_t_ORA_ID_LOG_CATEGORY_ENABLED,
            ffi::ora_log_category_enabled_fn_t,
            category,
            std::ptr::null_mut::<u32>()
        ))
    }

    pub fn get_chip_size_from_type(&self, chip_type: u32) -> u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_CHIP_SIZE_FROM_TYPE,
            ffi::ora_get_chip_size_from_type_fn_t,
            chip_type
        )
    }

    /// Current SYSCLK frequency in MHz, as reported by the running firmware
    /// (`ORA_ID_GET_SYSCLK_MHZ`).  The PIO — and hence a Lens cycle — is clocked
    /// from SYSCLK, so this is the divisor for converting cycles to real time.
    pub fn sysclk_mhz(&self) -> u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_SYSCLK_MHZ,
            ffi::ora_get_sysclk_mhz_fn_t
        )
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        if !self.epio.is_null() {
            // SAFETY: epio was allocated by epio_from_apio and has not been freed.
            unsafe { ffi::epio_free(self.epio) };
            self.epio = core::ptr::null_mut();
        }
    }
}

// SAFETY: epio_t is heap-allocated C state with no thread-local components.
// We take responsibility for correct single-threaded usage in tests.
unsafe impl Send for Emulator {}
