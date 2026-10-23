// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The PIO block and state machine each program runs on, and their settings.

use onerom_config::hw::Board;
use pio::{InstructionOperands, Program};

use crate::pack::{WINDOW, Wiring, wiring};
use crate::program::{DRIVE_SHIFT, ShiftConfig, drive_program, sample_program, sample_shift};

/// One state machine, set up to run its program.
#[derive(Debug)]
pub struct MachineSetup {
    /// PIO block.
    pub block: u8,
    /// State machine in the block.
    pub sm: u8,
    /// The block's GPIOBASE.
    pub gpio_base: u8,
    /// Address in the block's instruction memory the program is loaded at.
    pub offset: u8,
    pub program: Program<32>,
    pub shift: ShiftConfig,
    /// First GPIO `out pins` drives, counted from `gpio_base`.
    pub out_base: u8,
    pub out_count: u8,
    /// First GPIO `in pins` samples, counted from `gpio_base`.
    pub in_base: u8,
}

impl MachineSetup {
    /// The program's instructions as loaded at `offset`.
    pub fn code(&self) -> impl Iterator<Item = u16> + '_ {
        self.program.code.iter().map(|&code| {
            // A JMP's address is absolute, in its low 5 bits.
            match InstructionOperands::decode(code) {
                Some(InstructionOperands::JMP { .. }) => code + u16::from(self.offset),
                _ => code,
            }
        })
    }

    /// The instruction the program wraps from, as loaded.
    pub fn wrap_top(&self) -> u8 {
        self.program.wrap.source + self.offset
    }

    /// The instruction the program wraps to, as loaded, which is also its
    /// entry point.
    pub fn wrap_bottom(&self) -> u8 {
        self.program.wrap.target + self.offset
    }
}

/// Machines A, S and R for one chip on one board.
///
/// S and R are in one block and A is in the next, so one write to S's
/// block's CTRL starts all three together.
#[derive(Debug)]
pub struct Machines {
    pub wiring: Wiring,
    pub a: MachineSetup,
    pub s: MachineSetup,
    pub r: MachineSetup,
}

/// Each machine's PIO block and state machine.
const A: (u8, u8) = (1, 0);
const S: (u8, u8) = (0, 0);
const R: (u8, u8) = (0, 1);

/// Sets up the machines for a chip on `board`.
///
/// `addr` and `selects` are as for [`wiring`], and `data` is the GPIO of
/// each data line.
///
/// Returns `None` where [`wiring`] does, or where R's block doesn't reach
/// every data line.
pub fn machines(
    board: Board,
    addr: &[u8],
    selects: &[(u8, bool)],
    data: &[u8],
) -> Option<Machines> {
    let wiring = wiring(board, addr, selects)?;
    let data_base = *data.iter().min()?;
    let data_top = *data.iter().max()?;
    // R's block is at GPIOBASE 0.
    if data_top >= WINDOW {
        return None;
    }
    let data_bits = data_top - data_base + 1;

    let drive_s = drive_program(wiring.s.out_count());
    let a = MachineSetup {
        block: A.0,
        sm: A.1,
        gpio_base: wiring.a_gpio_base,
        offset: 0,
        program: drive_program(wiring.a.out_count()),
        shift: DRIVE_SHIFT,
        out_base: wiring.a.out_base() - wiring.a_gpio_base,
        out_count: wiring.a.out_count(),
        in_base: 0,
    };
    let r = MachineSetup {
        block: R.0,
        sm: R.1,
        gpio_base: 0,
        offset: drive_s.code.len() as u8,
        program: sample_program(data_bits),
        shift: sample_shift(data_bits),
        out_base: 0,
        out_count: 0,
        in_base: data_base,
    };
    let s = MachineSetup {
        block: S.0,
        sm: S.1,
        gpio_base: 0,
        offset: 0,
        program: drive_s,
        shift: DRIVE_SHIFT,
        out_base: wiring.s.out_base(),
        out_count: wiring.s.out_count(),
        in_base: 0,
    };
    Some(Machines { wiring, a, s, r })
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::chip::ChipType;
    use std::vec::Vec;

    /// A 2364's address and select pins and data GPIOs on fire-24-e.
    fn lines() -> (Vec<u8>, Vec<(u8, bool)>, Vec<u8>) {
        let board = Board::Fire24E;
        let chip = ChipType::Chip2364;
        let selects = chip
            .control_lines()
            .iter()
            .filter(|c| c.name == "cs1")
            .map(|c| (c.pin, false))
            .collect();
        let data = chip
            .data_pins()
            .iter()
            .map(|&p| board.gpios_for_socket_pin(p)[0])
            .collect();
        (chip.address_pins().to_vec(), selects, data)
    }

    fn fire_24_e() -> Machines {
        let (addr, selects, data) = lines();
        machines(Board::Fire24E, &addr, &selects, &data).unwrap()
    }

    /// One CTRL write in S's block starts all three machines.
    #[test]
    fn one_write_starts_all() {
        let m = fire_24_e();
        assert_eq!(m.r.block, m.s.block);
        assert_eq!(m.a.block, m.s.block + 1);
        assert_ne!(m.r.sm, m.s.sm);
        assert_eq!(m.s.gpio_base, m.r.gpio_base);
    }

    /// R's program follows S's in their block, with its JMPs moved there.
    #[test]
    fn r_loads_after_s() {
        let m = fire_24_e();
        let s_len = m.s.program.code.len() as u8;
        assert_eq!(m.r.offset, s_len);
        assert_eq!(m.r.wrap_bottom(), s_len);
        assert_eq!(m.r.wrap_top(), s_len + 2);
        let jmp = m.r.code().nth(1).unwrap();
        assert!(matches!(
            InstructionOperands::decode(jmp),
            Some(InstructionOperands::JMP { address, .. }) if address == s_len + 1
        ));
    }

    #[test]
    fn data_outside_r_window() {
        let (addr, selects, _) = lines();
        assert!(machines(Board::Fire24E, &addr, &selects, &[WINDOW]).is_none());
        assert!(machines(Board::Fire24E, &addr, &selects, &[]).is_none());
    }
}
