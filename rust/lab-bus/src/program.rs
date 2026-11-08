// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The PIO programs and their shift register settings.

use pio::{Assembler, InSource, JmpCondition, OutDestination, Program};

/// The most lines one drive program drives. One word then encodes a segment
/// of up to [`HOLD_CYCLES`](crate::HOLD_CYCLES).
pub const MAX_PINS: u8 = 21;

/// The program machines A and S run, for `pins` lines.
///
/// Each word is one segment. `out pins` drives the image, `out x` loads the
/// length and `jmp x--` holds the image for the rest of the segment.
///
/// # Panics
///
/// If `pins` is 0 or over [`MAX_PINS`].
pub fn drive_program(pins: u8) -> Program<32> {
    assert!((1..=MAX_PINS).contains(&pins));

    let mut a = Assembler::<32>::new();
    let mut wrap_target = a.label();
    let mut wrap_source = a.label();
    let mut hold = a.label();

    a.bind(&mut wrap_target);
    a.out(OutDestination::PINS, pins);
    a.out(OutDestination::X, 32 - pins);
    a.bind(&mut hold);
    a.jmp(JmpCondition::XDecNonZero, &mut hold);
    a.bind(&mut wrap_source);

    a.assemble_with_wrap(wrap_source, wrap_target)
}

/// The program machine R runs, sampling `bits` data lines.
///
/// Its one word per burst is the delay to the sample,
/// [`Burst::sample_at`](crate::Burst::sample_at).
///
/// # Panics
///
/// If `bits` is 0 or over 32.
pub fn sample_program(bits: u8) -> Program<32> {
    let mut a = Assembler::<32>::new();
    let mut wrap_target = a.label();
    let mut wrap_source = a.label();
    let mut delay = a.label();

    a.bind(&mut wrap_target);
    a.out(OutDestination::X, 32);
    a.bind(&mut delay);
    a.jmp(JmpCondition::XDecNonZero, &mut delay);
    a.r#in(InSource::PINS, bits);
    a.bind(&mut wrap_source);

    a.assemble_with_wrap(wrap_source, wrap_target)
}

/// Settings for one of a machine's shift registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shift {
    /// Shifts right rather than left.
    pub right: bool,
    /// Pulls or pushes automatically at `threshold` bits.
    pub auto: bool,
    /// From 1 to 32.
    pub threshold: u8,
}

/// A machine's shift register settings, as its SHIFTCTRL register holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShiftConfig {
    /// The output shift register, which pulls.
    pub osr: Shift,
    /// The input shift register, which pushes.
    pub isr: Shift,
    /// The RX FIFO joins the TX FIFO.
    pub join_tx: bool,
}

/// The hardware's reset settings for a shift register.
const RESET_SHIFT: Shift = Shift {
    right: true,
    auto: false,
    threshold: 32,
};

const AUTOPULL_32: Shift = Shift {
    right: true,
    auto: true,
    threshold: 32,
};

/// Shift settings for [`drive_program`].
pub const DRIVE_SHIFT: ShiftConfig = ShiftConfig {
    osr: AUTOPULL_32,
    isr: RESET_SHIFT,
    join_tx: true,
};

/// Shift settings for [`sample_program`] sampling `bits` data lines.
///
/// The pushed word has the first sampled GPIO in bit 0.
pub const fn sample_shift(bits: u8) -> ShiftConfig {
    ShiftConfig {
        osr: AUTOPULL_32,
        isr: Shift {
            right: false,
            auto: true,
            threshold: bits,
        },
        join_tx: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pio::{InstructionOperands as Op, Wrap};

    fn op(code: u16) -> Op {
        Op::decode(code).unwrap()
    }

    #[test]
    fn drive_program_instructions() {
        for pins in [1, 8, 12, MAX_PINS] {
            let p = drive_program(pins);
            assert_eq!(p.code.len(), 3);
            assert!(matches!(
                op(p.code[0]),
                Op::OUT { destination: OutDestination::PINS, bit_count } if bit_count == pins
            ));
            assert!(matches!(
                op(p.code[1]),
                Op::OUT { destination: OutDestination::X, bit_count } if bit_count == 32 - pins
            ));
            assert!(matches!(
                op(p.code[2]),
                Op::JMP {
                    condition: JmpCondition::XDecNonZero,
                    address: 2
                }
            ));
            assert_eq!(
                p.wrap,
                Wrap {
                    source: 2,
                    target: 0
                }
            );
            assert_eq!(p.side_set.bits(), 0);
            assert_eq!(p.origin, None);
        }
    }

    #[test]
    fn sample_program_instructions() {
        for bits in [8, 16] {
            let p = sample_program(bits);
            assert_eq!(p.code.len(), 3);
            assert!(matches!(
                op(p.code[0]),
                Op::OUT {
                    destination: OutDestination::X,
                    bit_count: 32
                }
            ));
            assert!(matches!(
                op(p.code[1]),
                Op::JMP {
                    condition: JmpCondition::XDecNonZero,
                    address: 1
                }
            ));
            assert!(matches!(
                op(p.code[2]),
                Op::IN { source: InSource::PINS, bit_count } if bit_count == bits
            ));
            assert_eq!(
                p.wrap,
                Wrap {
                    source: 2,
                    target: 0
                }
            );
            assert_eq!(p.side_set.bits(), 0);
        }
    }

    #[test]
    #[should_panic]
    fn drive_program_no_pins() {
        drive_program(0);
    }

    #[test]
    #[should_panic]
    fn drive_program_too_many_pins() {
        drive_program(MAX_PINS + 1);
    }
}
