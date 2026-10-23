// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! An epio instance apart from the firmware's, for PIO programs a test sets up
//! itself.

use crate::ffi;

/// Words a state machine's TX or RX FIFO holds.  epio doesn't model joining
/// the two.
pub const FIFO_DEPTH: u8 = ffi::MAX_FIFO_DEPTH as u8;

/// A state machine's configuration registers, as the RP2350 lays them out.
pub type SmReg = ffi::epio_sm_reg_t;

/// An epio instance.  It starts with every state machine disabled
/// and every GPIO an input.
pub struct Epio {
    epio: *mut ffi::epio_t,
}

impl Default for Epio {
    fn default() -> Self {
        Self::new()
    }
}

impl Epio {
    /// # Panics
    ///
    /// If epio can't allocate the instance.
    pub fn new() -> Self {
        // SAFETY: no arguments, and the result is checked.
        let epio = unsafe { ffi::epio_init() };
        assert!(!epio.is_null(), "epio_init returned null");
        Self { epio }
    }

    /// Sets a PIO block's GPIOBASE, 0 or 16.
    pub fn set_gpiobase(&self, block: u8, base: u8) {
        unsafe { ffi::epio_set_gpiobase(self.epio, block, u32::from(base)) };
    }

    /// Writes `code` to a block's instruction memory from `offset`.
    pub fn load(&self, block: u8, offset: u8, code: &[u16]) {
        for (i, &instr) in code.iter().enumerate() {
            unsafe { ffi::epio_set_instr(self.epio, block, offset + i as u8, instr) };
        }
    }

    /// Sets a state machine's configuration registers.
    pub fn set_sm_reg(&self, block: u8, sm: u8, mut reg: SmReg) {
        unsafe { ffi::epio_set_sm_reg(self.epio, block, sm, &mut reg) };
    }

    /// Runs `instr` on a state machine at once, as a CPU write to its
    /// SMx_INSTR register does.
    pub fn exec(&self, block: u8, sm: u8, instr: u16) {
        unsafe { ffi::epio_force_sm_instr(self.epio, block, sm, instr) };
    }

    pub fn enable_sm(&self, block: u8, sm: u8) {
        unsafe { ffi::epio_enable_sm(self.epio, block, sm) };
    }

    pub fn disable_sm(&self, block: u8, sm: u8) {
        unsafe { ffi::epio_disable_sm(self.epio, block, sm) };
    }

    /// The program counter of a state machine.
    pub fn pc(&self, block: u8, sm: u8) -> u8 {
        unsafe { ffi::epio_peek_sm_pc(self.epio, block, sm) }
    }

    /// Words in a state machine's TX FIFO.
    pub fn tx_depth(&self, block: u8, sm: u8) -> u8 {
        unsafe { ffi::epio_tx_fifo_depth(self.epio, block, sm) }
    }

    /// # Panics
    ///
    /// If the TX FIFO is full.
    pub fn push_tx(&self, block: u8, sm: u8, word: u32) {
        assert!(self.tx_depth(block, sm) < FIFO_DEPTH, "TX FIFO full");
        unsafe { ffi::epio_push_tx_fifo(self.epio, block, sm, word) };
    }

    /// The oldest word in a state machine's RX FIFO, or `None` where it's
    /// empty.
    pub fn pop_rx(&self, block: u8, sm: u8) -> Option<u32> {
        let depth = unsafe { ffi::epio_rx_fifo_depth(self.epio, block, sm) };
        (depth != 0).then(|| unsafe { ffi::epio_pop_rx_fifo(self.epio, block, sm) })
    }

    /// Switches `gpio`'s output to `block`, driving `level`.
    pub fn set_output(&self, block: u8, gpio: u8, level: bool) {
        unsafe {
            ffi::epio_set_gpio_output_control(self.epio, gpio, block);
            ffi::epio_set_gpio_output(self.epio, gpio);
            ffi::epio_set_gpio_output_level(self.epio, gpio, u8::from(level));
        }
    }

    /// Drives the GPIOs in `gpios` from outside at `level`.  Every other GPIO
    /// goes back to its pull.
    pub fn drive_gpios(&self, gpios: u64, level: u64) {
        unsafe { ffi::epio_drive_gpios_ext(self.epio, gpios, level) };
    }

    /// The level of every GPIO, bit n for GPIO n.
    pub fn read_pin_states(&self) -> u64 {
        unsafe { ffi::epio_read_pin_states(self.epio) }
    }

    /// Advances every enabled state machine by `cycles`.
    pub fn step_cycles(&self, cycles: u32) {
        unsafe { ffi::epio_step_cycles(self.epio, cycles) };
    }
}

impl Drop for Epio {
    fn drop(&mut self) {
        // SAFETY: `epio` came from epio_init and isn't used again.
        unsafe { ffi::epio_free(self.epio) };
    }
}
