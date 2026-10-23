// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lab's host bus, wired to the emulated One ROM.
//!
//! [`LabHost`] runs `onerom-lab-bus`'s programs, and the words Lab feeds them,
//! in a second epio instance set up as `rust/lab/src/host.rs` sets up the
//! RP2350.  It steps that instance one cycle at a time beside the firmware's.
//!
//! Lab and One ROM are on the same board, and each socket pin connects Lab's
//! GPIO to the same GPIO on One ROM.  Each cycle Lab's lines reach One ROM's
//! inputs.  One ROM's data lines reach R [`SYNC_CYCLES`] later, standing in
//! for Lab's input synchroniser, which epio doesn't model.  One ROM's own
//! synchroniser isn't modelled, as in the other testers.
//!
//! Lab's start sequence drives the RP2350's registers and DMA, so
//! [`LabHost::read`] repeats it.

use std::collections::VecDeque;
use std::iter::Peekable;
use std::vec;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_fw_emulator::epio::{Epio, FIFO_DEPTH, SmReg};
use onerom_fw_tester::pin_cache::PinCache;
use onerom_gen::socket_pin_offset;
use onerom_lab_bus::{
    BurstState, MAX_SEGMENTS, MachineSetup, Machines, Profile, SYNC_CYCLES, SYS_CLOCK_HZ, Segment,
    Wiring, burst, machines,
};
use pio::{InstructionOperands, JmpCondition};

/// Lab's system clock in MHz.
pub const LAB_MHZ: u16 = (SYS_CLOCK_HZ / 1_000_000) as u16;

/// Cycles from enabling a machine to its first `out`, which stalls while
/// autopull fills the empty OSR (RP2350 datasheet, 11.5.4.2).
pub const START_CYCLES: u32 = 1;

/// A state machine and its entry point.
#[derive(Clone, Copy)]
struct Machine {
    block: u8,
    sm: u8,
    entry: u8,
}

impl Machine {
    fn new(setup: &MachineSetup) -> Machine {
        Machine {
            block: setup.block,
            sm: setup.sm,
            entry: setup.wrap_bottom(),
        }
    }
}

/// One read through the host bus.
pub struct Read {
    /// The data lines, D0 in bit 0.
    pub data: u16,
    /// The burst's segments, the hold last.
    pub segments: Vec<Segment>,
    /// [`Burst::sample_at`](onerom_lab_bus::Burst::sample_at).
    pub sample_at: u32,
}

/// Lab's machines and their wiring for one chip on one board, in one bit
/// mode.
pub struct LabHost {
    epio: Epio,
    profile: Profile,
    wiring: Wiring,
    state: BurstState,
    /// A, S and R.
    machines: [Machine; 3],
    /// Each line Lab drives: its GPIO, and every GPIO of its socket pin.
    lines: Vec<(u8, u64)>,
    /// GPIO of each data line, from D0.
    data: Vec<u8>,
    data_base: u8,
}

impl LabHost {
    /// Sets up the machines for the served chip in `mode`, with `profile`.
    ///
    /// Fails where Lab's host bus can't read the chip on this board.
    pub fn new(
        board: Board,
        chip_type: ChipType,
        cache: &PinCache,
        mode: u8,
        profile: Profile,
    ) -> Result<LabHost, String> {
        let offset = socket_pin_offset(chip_type.chip_pins(), board.chip_pins())
            .and_then(|offset| u8::try_from(offset).ok())
            .ok_or("the chip doesn't fit the board's socket")?;

        // Lab's 16-bit reads don't drive A-1 and read D15.
        let (addr_pins, data_lines) = if mode == 16 {
            (&chip_type.address_pins()[1..], 16)
        } else {
            (chip_type.address_pins(), 8)
        };
        let addr: Vec<u8> = addr_pins.iter().map(|&pin| pin + offset).collect();

        // The select lines Lab drives, at the config's levels.
        let mut selects = Vec::new();
        for spec in chip_type.control_lines() {
            if !matches!(spec.name, "ce" | "oe" | "cs1" | "cs2" | "cs3") {
                continue;
            }
            if let Some(line) = cache.control_lines.iter().find(|l| l.name == spec.name) {
                selects.push((spec.pin + offset, line.assert_high));
            }
        }

        let data = cache.data_gpios[..data_lines.min(cache.data_gpios.len())].to_vec();
        let Machines { wiring, a, s, r } = machines(board, &addr, &selects, &data)
            .ok_or("Lab's host bus can't read this chip on this board")?;

        let driven = wiring.a.gpios() | wiring.s.gpios();
        let lines = addr
            .iter()
            .chain(selects.iter().map(|(pin, _)| pin))
            .map(|&pin| {
                let gpios = board.gpios_for_socket_pin(pin);
                let lab = *gpios
                    .iter()
                    .find(|&&gpio| driven >> gpio & 1 == 1)
                    .expect("wiring() drives one GPIO of each pin");
                (lab, gpios.iter().fold(0, |mask, &gpio| mask | 1 << gpio))
            })
            .collect();

        let epio = Epio::new();
        for setup in [&a, &s, &r] {
            epio.set_gpiobase(setup.block, setup.gpio_base);
            load(&epio, setup);
        }
        // As Lab's `HostBus::switch_pins`.
        for (setup, packer) in [(&a, &wiring.a), (&s, &wiring.s)] {
            let idle = packer.image(0, false);
            for gpio in (0..64).filter(|&gpio| packer.gpios() >> gpio & 1 == 1) {
                let level = idle >> (gpio - packer.out_base()) & 1 == 1;
                epio.set_output(setup.block, gpio, level);
            }
        }

        Ok(LabHost {
            epio,
            profile,
            state: BurstState::new(addr.len() as u8),
            machines: [Machine::new(&a), Machine::new(&s), Machine::new(&r)],
            lines,
            data_base: r.gpio_base + r.in_base,
            wiring,
            data,
        })
    }

    /// The levels of Lab's lines for a segment at `addr`, bit n for GPIO n.
    /// Only the GPIOs Lab drives are set.
    pub fn levels(&self, addr: u32, selected: bool) -> u64 {
        let w = &self.wiring;
        [&w.a, &w.s].iter().fold(0, |levels, packer| {
            levels | (u64::from(packer.image(addr, selected)) << packer.out_base()) & packer.gpios()
        })
    }

    /// The GPIOs Lab drives.
    pub fn driven(&self) -> u64 {
        self.wiring.a.gpios() | self.wiring.s.gpios()
    }

    /// The GPIO of each data line R samples, from D0.
    pub fn data_gpios(&self) -> &[u8] {
        &self.data
    }

    /// The segments of the next burst for `addr`, without running it.
    pub fn next_burst(&self, addr: u32) -> Vec<Segment> {
        let mut segments = [Segment::default(); MAX_SEGMENTS];
        let mut state = self.state;
        let b = burst(&self.profile, &mut state, addr, &mut segments);
        segments[..b.segments].to_vec()
    }

    /// One ROM, wired to these lines.  `background` is held throughout.
    pub fn device<'a>(&self, emu: &'a Emulator, background: (u64, u64)) -> Device<'a> {
        Device {
            emu,
            background,
            lines: self.lines.clone(),
        }
    }

    /// Reads `addr` with one burst.
    ///
    /// Each cycle `wire` is passed Lab's GPIO levels and returns One ROM's,
    /// bit n for GPIO n.
    ///
    /// Lab stops the machines once R has sampled, which leaves the lines at
    /// the hold's levels until the next burst.  Here the burst runs to the end
    /// of its hold instead.
    pub fn read(&mut self, addr: u32, mut wire: impl FnMut(u64) -> u64) -> Result<Read, String> {
        let mut segments = [Segment::default(); MAX_SEGMENTS];
        let b = burst(&self.profile, &mut self.state, addr, &mut segments);
        let segments = segments[..b.segments].to_vec();
        let mut a_words = vec![0; MAX_SEGMENTS];
        let mut s_words = vec![0; MAX_SEGMENTS];
        let a_count = self.wiring.a.pack(&segments, &mut a_words);
        let s_count = self.wiring.s.pack(&segments, &mut s_words);
        a_words.truncate(a_count);
        s_words.truncate(s_count);
        let mut a_words = a_words.into_iter().peekable();
        let mut s_words = s_words.into_iter().peekable();

        // Lab puts each machine at its entry point and writes R's word, and
        // its DMA fills A's and S's TX FIFOs before they start and keeps them
        // filled.
        let [a, s, r] = self.machines;
        for m in self.machines {
            self.epio.exec(m.block, m.sm, jmp(m.entry));
        }
        self.epio.push_tx(r.block, r.sm, b.sample_at);
        top_up(&self.epio, a, &mut a_words);
        top_up(&self.epio, s, &mut s_words);
        for m in self.machines {
            self.epio.enable_sm(m.block, m.sm);
        }

        let data_mask = self.data.iter().fold(0, |mask, &gpio| mask | 1 << gpio);
        let mut synced: VecDeque<u64> = VecDeque::new();
        let cycles = START_CYCLES + segments.iter().map(|s| s.cycles).sum::<u32>();
        for _ in 0..cycles {
            top_up(&self.epio, a, &mut a_words);
            top_up(&self.epio, s, &mut s_words);
            let pins = wire(self.epio.read_pin_states());
            if synced.is_empty() {
                synced.extend([pins; SYNC_CYCLES as usize]);
            }
            synced.push_back(pins);
            let seen = synced.pop_front().expect("filled above");
            self.epio.drive_gpios(data_mask, seen);
            self.epio.step_cycles(1);
        }
        for m in self.machines {
            self.epio.disable_sm(m.block, m.sm);
        }

        for (name, m, words_left) in [
            ("A", a, a_words.peek().is_some()),
            ("S", s, s_words.peek().is_some()),
        ] {
            if words_left
                || self.epio.tx_depth(m.block, m.sm) != 0
                || self.epio.pc(m.block, m.sm) != m.entry
            {
                return Err(format!(
                    "machine {name} hadn't run its words to the end of the burst's {cycles} \
                     cycles"
                ));
            }
        }
        let sampled = self
            .epio
            .pop_rx(r.block, r.sm)
            .ok_or("machine R didn't sample the data lines")?;
        let data = self
            .data
            .iter()
            .enumerate()
            .fold(0, |value, (line, &gpio)| {
                value | (((sampled >> (gpio - self.data_base)) & 1) as u16) << line
            });
        Ok(Read {
            data,
            segments,
            sample_at: b.sample_at,
        })
    }
}

/// One ROM, stepped one cycle at a time with Lab's lines on its inputs.
pub struct Device<'a> {
    emu: &'a Emulator,
    /// Lines held throughout: `/BYTE` where the chip has one.
    background: (u64, u64),
    /// Each line Lab drives: its GPIO, and every GPIO of its socket pin.
    lines: Vec<(u8, u64)>,
}

impl Device<'_> {
    /// One ROM's GPIOs Lab drives, and their levels, for Lab's levels `lab`.
    fn inputs(&self, lab: u64) -> (u64, u64) {
        self.lines
            .iter()
            .fold(self.background, |(mask, level), &(gpio, gpios)| {
                let high = if lab >> gpio & 1 == 1 { gpios } else { 0 };
                (mask | gpios, level | high)
            })
    }

    /// Whether One ROM reads its select lines as active with Lab's levels
    /// `lab`.
    pub fn selected(&self, cache: &PinCache, lab: u64) -> bool {
        let (_, level) = self.inputs(lab);
        cache
            .control_lines
            .iter()
            .all(|line| (level >> line.gpios[0] & 1 == 1) == line.assert_high)
    }

    /// Drives Lab's levels `lab` onto One ROM, returns One ROM's GPIO levels
    /// and steps it a cycle.
    pub fn cycle(&self, lab: u64) -> u64 {
        let (mask, level) = self.inputs(lab);
        self.emu.drive_gpios(mask, level);
        let pins = self.emu.read_pin_states();
        self.emu.step_cycles(1);
        pins
    }
}

/// Pushes words to machine `m`'s TX FIFO until it is full or `words` runs out.
fn top_up(epio: &Epio, m: Machine, words: &mut Peekable<vec::IntoIter<u32>>) {
    while epio.tx_depth(m.block, m.sm) < FIFO_DEPTH {
        let Some(word) = words.next() else {
            break;
        };
        epio.push_tx(m.block, m.sm, word);
    }
}

/// As Lab's `load`.
fn load(epio: &Epio, setup: &MachineSetup) {
    let code: Vec<u16> = setup.code().collect();
    epio.load(setup.block, setup.offset, &code);

    let shift = setup.shift;
    let thresh = |threshold: u8| u32::from(threshold % 32);
    let reg = SmReg {
        clkdiv: 1 << CLKDIV_INT_LSB,
        execctrl: u32::from(setup.wrap_top()) << EXECCTRL_WRAP_TOP_LSB
            | u32::from(setup.wrap_bottom()) << EXECCTRL_WRAP_BOTTOM_LSB,
        shiftctrl: u32::from(shift.osr.right) << SHIFTCTRL_OUT_SHIFTDIR
            | u32::from(shift.osr.auto) << SHIFTCTRL_AUTOPULL
            | thresh(shift.osr.threshold) << SHIFTCTRL_PULL_THRESH_LSB
            | u32::from(shift.isr.right) << SHIFTCTRL_IN_SHIFTDIR
            | u32::from(shift.isr.auto) << SHIFTCTRL_AUTOPUSH
            | thresh(shift.isr.threshold) << SHIFTCTRL_PUSH_THRESH_LSB
            | u32::from(shift.join_tx) << SHIFTCTRL_FJOIN_TX,
        pinctrl: u32::from(setup.out_base) << PINCTRL_OUT_BASE_LSB
            | u32::from(setup.out_count) << PINCTRL_OUT_COUNT_LSB
            | u32::from(setup.in_base) << PINCTRL_IN_BASE_LSB,
    };
    epio.set_sm_reg(setup.block, setup.sm, reg);
}

/// `jmp entry`, which Lab writes to SMx_INSTR to put a machine at its entry.
fn jmp(entry: u8) -> u16 {
    InstructionOperands::JMP {
        condition: JmpCondition::Always,
        address: entry,
    }
    .encode()
}

// State machine register fields (RP2350 datasheet, 11.7).
const CLKDIV_INT_LSB: u32 = 16;
const EXECCTRL_WRAP_TOP_LSB: u32 = 12;
const EXECCTRL_WRAP_BOTTOM_LSB: u32 = 7;
const SHIFTCTRL_AUTOPUSH: u32 = 16;
const SHIFTCTRL_AUTOPULL: u32 = 17;
const SHIFTCTRL_IN_SHIFTDIR: u32 = 18;
const SHIFTCTRL_OUT_SHIFTDIR: u32 = 19;
const SHIFTCTRL_PUSH_THRESH_LSB: u32 = 20;
const SHIFTCTRL_PULL_THRESH_LSB: u32 = 25;
const SHIFTCTRL_FJOIN_TX: u32 = 30;
const PINCTRL_OUT_BASE_LSB: u32 = 0;
const PINCTRL_IN_BASE_LSB: u32 = 15;
const PINCTRL_OUT_COUNT_LSB: u32 = 20;
