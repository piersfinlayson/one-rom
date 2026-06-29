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

use std::sync::OnceLock;

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
    Unknown(u32),
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
            other => Self::Unknown(other),
        }
    }
}

impl OraResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
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

    // ── Firmware state queries (valid after boot()) ──────────────────────────

    /// Returns `true` if the firmware is operating in limp mode.
    pub fn limp_mode(&self) -> bool {
        unsafe { ffi::ffi_limp_mode() as i32 != 0 }
    }

    /// Returns `true` if the PIO state machines are enabled.
    pub fn pios_enabled(&self) -> bool {
        unsafe { ffi::ffi_pios_enabled() as i32 != 0 }
    }

    // ── ROM image selection ──────────────────────────────────────────────────

    /// Tell the stub which ROM image to present.
    pub fn set_sel_image(image: u8) {
        unsafe { ffi::stub_set_sel_image(image as _) };
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

    pub fn get_chip_size_from_type(&self, chip_type: u32) -> u32 {
        plugin_call!(
            ffi::api_id_t_ORA_ID_GET_CHIP_SIZE_FROM_TYPE,
            ffi::ora_get_chip_size_from_type_fn_t,
            chip_type
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
