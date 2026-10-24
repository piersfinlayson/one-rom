// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Packing segments into words, and assigning a board's lines to machines.

use onerom_config::hw::Board;

use crate::burst::{MIN_CYCLES, Segment};
use crate::program::MAX_PINS;

/// GPIOs one PIO block reaches from its GPIOBASE.
pub(crate) const WINDOW: u8 = 32;

/// Packs segments into the words one drive machine consumes.
///
/// A word contains the GPIO image in its low `out_count` bits and the
/// segment length less [`MIN_CYCLES`] above them. GPIOs in the machine's
/// range that it doesn't drive are 0 in the image.
#[derive(Clone, Debug)]
pub struct Packer {
    /// Address bit and its GPIO's bit in the image.
    addr: [(u8, u8); 32],
    addr_count: u8,
    selected: u32,
    unselected: u32,
    gpios: u32,
    out_base: u8,
    out_count: u8,
}

impl Packer {
    /// - `addr` pairs an address bit with the GPIO driving it.
    /// - `selects` pairs a select line's GPIO with whether it is active high.
    /// - `out_base` and `out_count` are the first GPIO of the machine's range
    ///   and its number of GPIOs.
    ///
    /// GPIOs are numbered from 0, not from a PIO block's GPIOBASE.
    ///
    /// # Panics
    ///
    /// If `out_count` is 0 or over [`MAX_PINS`], a GPIO is outside the
    /// range, or `addr` has an address bit over 31 or more than 32 entries.
    pub fn new(addr: &[(u8, u8)], selects: &[(u8, bool)], out_base: u8, out_count: u8) -> Packer {
        assert!((1..=MAX_PINS).contains(&out_count));
        assert!(addr.len() <= 32);
        let bit = |gpio: u8| {
            assert!(gpio >= out_base && gpio - out_base < out_count);
            gpio - out_base
        };

        let mut packer = Packer {
            addr: [(0, 0); 32],
            addr_count: addr.len() as u8,
            selected: 0,
            unselected: 0,
            gpios: 0,
            out_base,
            out_count,
        };
        for (i, &(addr_bit, gpio)) in addr.iter().enumerate() {
            assert!(addr_bit < 32);
            packer.addr[i] = (addr_bit, bit(gpio));
            packer.gpios |= 1 << bit(gpio);
        }
        for &(gpio, active_high) in selects {
            let mask = 1 << bit(gpio);
            if active_high {
                packer.selected |= mask;
            } else {
                packer.unselected |= mask;
            }
            packer.gpios |= mask;
        }
        packer
    }

    /// The first GPIO of the machine's range.
    pub fn out_base(&self) -> u8 {
        self.out_base
    }

    /// The pin count for [`drive_program`](crate::drive_program).
    pub fn out_count(&self) -> u8 {
        self.out_count
    }

    /// The GPIOs the machine drives, bit n for GPIO n.
    pub fn gpios(&self) -> u64 {
        u64::from(self.gpios) << self.out_base
    }

    /// The image for `addr` with the select lines active or inactive. Bit 0
    /// is GPIO `out_base`.
    pub fn image(&self, addr: u32, selected: bool) -> u32 {
        let mut image = if selected {
            self.selected
        } else {
            self.unselected
        };
        for &(addr_bit, bit) in &self.addr[..usize::from(self.addr_count)] {
            image |= ((addr >> addr_bit) & 1) << bit;
        }
        image
    }

    /// The longest segment one word encodes.
    fn max_cycles(&self) -> u32 {
        (1 << (32 - u32::from(self.out_count))) - 1 + MIN_CYCLES
    }

    fn word(&self, image: u32, cycles: u32) -> u32 {
        image | ((cycles - MIN_CYCLES) << self.out_count)
    }

    /// Packs `segments` into `out` and returns the number of words written.
    ///
    /// Adjacent segments with the same image merge. A run longer than one
    /// word encodes is split into several words.
    ///
    /// # Panics
    ///
    /// If a run is under [`MIN_CYCLES`] or `out` is too short. One word per
    /// segment is enough while no segment is over
    /// [`HOLD_CYCLES`](crate::HOLD_CYCLES).
    pub fn pack(&self, segments: &[Segment], out: &mut [u32]) -> usize {
        let mut n = 0;
        let mut run: Option<(u32, u64)> = None;
        for segment in segments {
            let image = self.image(segment.addr, segment.selected);
            match &mut run {
                Some((run_image, cycles)) if *run_image == image => {
                    *cycles += u64::from(segment.cycles);
                }
                _ => {
                    if let Some((run_image, cycles)) = run {
                        n = self.split(run_image, cycles, out, n);
                    }
                    run = Some((image, u64::from(segment.cycles)));
                }
            }
        }
        if let Some((run_image, cycles)) = run {
            n = self.split(run_image, cycles, out, n);
        }
        n
    }

    /// Writes one run as words from `out[n]`, none under [`MIN_CYCLES`], and
    /// returns the next free index.
    fn split(&self, image: u32, mut cycles: u64, out: &mut [u32], mut n: usize) -> usize {
        let min = u64::from(MIN_CYCLES);
        let max = u64::from(self.max_cycles());
        assert!(cycles >= min);
        while cycles > max {
            let take = if cycles - max < min {
                cycles - min
            } else {
                max
            };
            out[n] = self.word(image, take as u32);
            n += 1;
            cycles -= take;
        }
        out[n] = self.word(image, cycles as u32);
        n + 1
    }
}

/// The packers for machines A and S for one chip on one board.
#[derive(Clone, Debug)]
pub struct Wiring {
    /// Machine A, which drives the address lines.
    pub a: Packer,
    /// GPIOBASE of A's PIO block.
    pub a_gpio_base: u8,
    /// Machine S, which drives the select lines and any address line outside
    /// A's window. Its PIO block's GPIOBASE is 0.
    pub s: Packer,
}

/// Assigns each line of a chip on `board` to machine A or S.
///
/// - `addr` is the socket pin of each address bit, from bit 0.
/// - `selects` pairs a select line's socket pin with whether it is active
///   high.
///
/// A's window starts at GPIO 16 on 32- and 40-pin boards and at GPIO 0 on
/// the others. An address line is driven by A where one of its GPIOs is in
/// A's window and otherwise by S. A pin wired to two GPIOs is driven on the
/// first one in its machine's window.
///
/// Returns `None` where a line has no GPIO in its machine's window, a
/// machine's GPIOs span more than [`MAX_PINS`], or `addr` is longer than 32.
pub fn wiring(board: Board, addr: &[u8], selects: &[(u8, bool)]) -> Option<Wiring> {
    if addr.len() > 32 || selects.len() > usize::from(WINDOW) {
        return None;
    }
    let a_gpio_base = if board.chip_pins() > 28 { 16 } else { 0 };
    let in_window = |pin: u8, base: u8| {
        board
            .gpios_for_socket_pin(pin)
            .iter()
            .copied()
            .find(|&gpio| gpio >= base && gpio - base < WINDOW)
    };

    let mut a_addr = Lines::new();
    let mut s_addr = Lines::new();
    for (bit, &pin) in addr.iter().enumerate() {
        if let Some(gpio) = in_window(pin, a_gpio_base) {
            a_addr.push((bit as u8, gpio));
        } else {
            s_addr.push((bit as u8, in_window(pin, 0)?));
        }
    }
    let mut s_selects = [(0, false); WINDOW as usize];
    for (i, &(pin, active_high)) in selects.iter().enumerate() {
        s_selects[i] = (in_window(pin, 0)?, active_high);
    }
    let s_selects = &s_selects[..selects.len()];

    let a_gpios = a_addr.as_slice().iter().map(|&(_, gpio)| gpio);
    let (a_base, a_count) = span(a_gpios)?;
    let s_gpios = s_addr.as_slice().iter().map(|&(_, gpio)| gpio);
    let (s_base, s_count) = span(s_gpios.chain(s_selects.iter().map(|&(gpio, _)| gpio)))?;

    Some(Wiring {
        a: Packer::new(a_addr.as_slice(), &[], a_base, a_count),
        a_gpio_base,
        s: Packer::new(s_addr.as_slice(), s_selects, s_base, s_count),
    })
}

/// The first GPIO and the number of GPIOs from it to the last, where that is
/// from 1 to [`MAX_PINS`].
fn span(gpios: impl Iterator<Item = u8> + Clone) -> Option<(u8, u8)> {
    let first = gpios.clone().min()?;
    let count = gpios.max()? - first + 1;
    (count <= MAX_PINS).then_some((first, count))
}

/// Address bits and their GPIOs.
struct Lines {
    lines: [(u8, u8); 32],
    count: usize,
}

impl Lines {
    fn new() -> Lines {
        Lines {
            lines: [(0, 0); 32],
            count: 0,
        }
    }

    fn push(&mut self, line: (u8, u8)) {
        self.lines[self.count] = line;
        self.count += 1;
    }

    fn as_slice(&self) -> &[(u8, u8)] {
        &self.lines[..self.count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::burst::{BurstState, C64, HOLD_CYCLES, MAX_SEGMENTS, burst};
    use onerom_config::chip::{ChipType, chip_type_names_for_pins};
    use onerom_config::hw::BOARDS;
    use onerom_config::mcu::Family;
    use std::vec;
    use std::vec::Vec;

    fn c64_burst(addr_bits: u8) -> Vec<Segment> {
        let p = C64.to_bus().unwrap();
        let mut state = BurstState::new(addr_bits);
        let mut out = [Segment::default(); MAX_SEGMENTS];
        let b = burst(&p, &mut state, 0x0123, &mut out);
        out[..b.segments].to_vec()
    }

    fn pack(packer: &Packer, segments: &[Segment]) -> Vec<u32> {
        let mut out = vec![0; segments.len()];
        let n = packer.pack(segments, &mut out);
        out.truncate(n);
        out
    }

    /// The image in each cycle the words drive.
    fn unpack(packer: &Packer, words: &[u32]) -> Vec<u32> {
        let mask = (1 << packer.out_count) - 1;
        words
            .iter()
            .flat_map(|&w| {
                let cycles = (w >> packer.out_count) + MIN_CYCLES;
                vec![w & mask; cycles as usize]
            })
            .collect()
    }

    fn images(packer: &Packer, segments: &[Segment]) -> Vec<u32> {
        segments
            .iter()
            .flat_map(|s| vec![packer.image(s.addr, s.selected); s.cycles as usize])
            .collect()
    }

    /// 16 address lines from GPIO 8, in reverse order.
    fn addr_packer() -> Packer {
        let addr: Vec<(u8, u8)> = (0..16).map(|bit| (bit, 23 - bit)).collect();
        Packer::new(&addr, &[], 8, 16)
    }

    /// An active-low and an active-high select line, with a gap between.
    fn select_packer() -> Packer {
        Packer::new(&[], &[(10, false), (12, true)], 10, 3)
    }

    #[test]
    fn image_maps_lines() {
        let a = addr_packer();
        assert_eq!(a.image(0x0001, false), 1 << 15);
        assert_eq!(a.image(0x8000, true), 1);
        assert_eq!(a.gpios(), 0xffff << 8);

        let s = select_packer();
        assert_eq!(s.image(0xffff, false), 0b001);
        assert_eq!(s.image(0, true), 0b100);
        assert_eq!(s.gpios(), 0b101 << 10);
    }

    #[test]
    fn merges() {
        let segments = c64_burst(13);
        let a = addr_packer();
        let s = select_packer();
        let a_words = pack(&a, &segments);
        let s_words = pack(&s, &segments);
        assert_eq!(a_words.len(), 4);
        assert_eq!(s_words.len(), 9);
        assert_eq!(unpack(&a, &a_words), images(&a, &segments));
        assert_eq!(unpack(&s, &s_words), images(&s, &segments));
    }

    #[test]
    fn splits() {
        let addr: Vec<(u8, u8)> = (0..21).map(|bit| (bit, bit)).collect();
        let a = Packer::new(&addr, &[], 0, MAX_PINS);
        assert_eq!(a.max_cycles(), HOLD_CYCLES);

        // A packer without select lines merges the first two segments.
        for extra in [
            1,
            2,
            3,
            HOLD_CYCLES - 1,
            HOLD_CYCLES,
            HOLD_CYCLES + 1,
            5 * HOLD_CYCLES + 2,
        ] {
            let segments = [
                Segment {
                    addr: 1,
                    selected: false,
                    cycles: HOLD_CYCLES,
                },
                Segment {
                    addr: 1,
                    selected: true,
                    cycles: extra,
                },
                Segment {
                    addr: 2,
                    selected: false,
                    cycles: 3,
                },
            ];
            let mut out = [0; 16];
            let n = a.pack(&segments, &mut out);
            if extra <= HOLD_CYCLES {
                assert!(n <= segments.len());
            }
            assert_eq!(
                unpack(&a, &out[..n]),
                images(&a, &segments),
                "extra {extra}"
            );
        }
    }

    #[test]
    #[should_panic]
    fn gpio_outside_range() {
        Packer::new(&[(0, 7)], &[], 8, 16);
    }

    fn fire_boards() -> impl Iterator<Item = Board> {
        BOARDS
            .into_iter()
            .filter(|b| matches!(b.mcu_family(), Family::Rp2350))
    }

    /// The chip types Lab lists for a board.
    fn chip_types(board: Board) -> Vec<ChipType> {
        let names = chip_type_names_for_pins(board.chip_pins()).unwrap();
        let mut chips: Vec<ChipType> = names
            .iter()
            .map(|n| ChipType::try_from_str(n).unwrap())
            .collect();
        chips.extend(board.extra_chip_types());
        chips
    }

    /// Lab's offset from a chip pin to a socket pin.
    fn offset(board: Board, chip: ChipType) -> u8 {
        if chip.chip_pins() == 24 && board.chip_pins() == 28 {
            2
        } else {
            0
        }
    }

    /// Socket pins of a chip's address lines and select lines.
    fn socket_pins(board: Board, chip: ChipType) -> (Vec<u8>, Vec<(u8, bool)>) {
        let offset = offset(board, chip);
        let addr = chip.address_pins().iter().map(|p| p + offset).collect();
        let selects = chip
            .control_lines()
            .iter()
            .filter(|c| ["ce", "oe", "cs1", "cs2", "cs3", "cs4"].contains(&c.name))
            .map(|c| {
                (
                    c.pin + offset,
                    c.line_type.fixed_active_level().unwrap_or(false),
                )
            })
            .collect();
        (addr, selects)
    }

    /// For every Fire board and every chip type it holds, each line is
    /// driven on one of its pin's GPIOs, in its machine's window, and packs
    /// to the right level.
    #[test]
    fn every_board_and_chip() {
        for board in fire_boards() {
            for chip in chip_types(board) {
                let (addr, selects) = socket_pins(board, chip);
                let w = wiring(board, &addr, &selects)
                    .unwrap_or_else(|| panic!("{} {}", board.name(), chip.name()));
                let driven = w.a.gpios() | w.s.gpios();
                assert_eq!(w.a.gpios() & w.s.gpios(), 0);
                assert_eq!(w.a.gpios() >> w.a_gpio_base >> WINDOW, 0);
                assert_eq!(w.s.gpios() >> WINDOW, 0);

                let one_gpio = |pin: u8| {
                    let gpios: Vec<u8> = board
                        .gpios_for_socket_pin(pin)
                        .iter()
                        .copied()
                        .filter(|&g| driven >> g & 1 == 1)
                        .collect();
                    assert_eq!(gpios.len(), 1, "{} {} pin {pin}", board.name(), chip.name());
                    gpios[0]
                };
                let level = |gpio: u8, addr: u32, selected: bool| {
                    let p = if w.a.gpios() >> gpio & 1 == 1 {
                        &w.a
                    } else {
                        &w.s
                    };
                    p.image(addr, selected) >> (gpio - p.out_base()) & 1 == 1
                };

                for addr_value in [0, u32::MAX, 0x5555_5555, 0xaaaa_aaaa] {
                    for (bit, &pin) in addr.iter().enumerate() {
                        let gpio = one_gpio(pin);
                        assert_eq!(level(gpio, addr_value, false), addr_value >> bit & 1 == 1);
                    }
                }
                for &(pin, active_high) in &selects {
                    let gpio = one_gpio(pin);
                    assert_eq!(level(gpio, 0, true), active_high);
                    assert_eq!(level(gpio, 0, false), !active_high);
                }

                // R samples the data lines from GPIOBASE 0 with one `in pins`.
                let data: Vec<u8> = chip
                    .data_pins()
                    .iter()
                    .map(|&p| {
                        let gpios = board.gpios_for_socket_pin(p + offset(board, chip));
                        *gpios.iter().find(|&&g| g < WINDOW).unwrap()
                    })
                    .collect();
                let first = *data.iter().min().unwrap();
                let last = *data.iter().max().unwrap();
                assert_eq!(usize::from(last - first) + 1, data.len());
            }
        }
    }

    /// fire-32-a's GPIO 13 is the top address line of the 27C080, and only
    /// S reaches it.
    #[test]
    fn fire_32_a_gpio_13() {
        let board = Board::Fire32A;
        let chip = ChipType::Chip27C080;
        let (addr, selects) = socket_pins(board, chip);
        let w = wiring(board, &addr, &selects).unwrap();
        assert_eq!(w.a_gpio_base, 16);
        assert_eq!(w.s.gpios() >> 13 & 1, 1);
        let top = 1 << (addr.len() - 1);
        assert_eq!(w.s.image(top, false) >> (13 - w.s.out_base()) & 1, 1);
        assert_eq!(w.a.image(top, false), 0);
    }
}
