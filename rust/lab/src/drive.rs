// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - data line drive
//!
//! Drives chosen data lines as a square wave with every control line
//! deasserted so the chip in the socket doesn't drive them.  Each toggle is
//! one GPIO write so the chosen lines change together.

use embassy_rp::pac;

use crate::error::Error;

pub const DEFAULT_DELAY_CYCLES: u32 = 16;

pub struct Driver {
    mask: u32,
    delay_cycles: u32,
    pub lines: u16,
    pub toggles: u64,
}

impl Driver {
    /// `gpios` lists the data line GPIOs, D0 first.  `lines` has bit n set to
    /// drive Dn, and `None` drives them all.
    pub fn new(gpios: &[u8], lines: Option<u16>, delay_cycles: u32) -> Result<Self, Error> {
        if gpios.len() < 8 || gpios.len() > 16 || gpios.iter().any(|&gpio| gpio >= 32) {
            return Err(Error::DrivePins);
        }
        let all = ((1u32 << gpios.len()) - 1) as u16;
        let lines = lines.unwrap_or(all);
        if lines & !all != 0 {
            return Err(Error::DriveLines(gpios.len()));
        }
        let mask = (0..gpios.len())
            .filter(|line| lines & (1 << line) != 0)
            .fold(0u32, |mask, line| mask | (1 << gpios[line]));
        Ok(Self {
            mask,
            delay_cycles,
            lines,
            toggles: 0,
        })
    }

    #[inline(never)]
    pub fn toggle(&mut self, count: u32) {
        let out = pac::SIO.gpio_out(0);
        for _ in 0..count {
            out.value_xor().write_value(self.mask);
            cortex_m::asm::delay(self.delay_cycles);
        }
        self.toggles += count as u64;
    }
}
