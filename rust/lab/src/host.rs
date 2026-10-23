// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - reading with a host's bus timing
//!
//! Runs `onerom-lab-bus`'s state machines.  DMA feeds A and S a burst at a
//! time, and nothing else in Lab uses PIO or DMA.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{Ordering, compiler_fence};

use embassy_rp::pac;
use embassy_rp::pac::dma::vals::{DataSize, TreqSel};
use embassy_rp::pac::io::vals::Gpio0ctrlFuncsel;
use embassy_rp::pac::pads::vals::Drive;
use embassy_time::{Duration, Instant};
use onerom_config::hw::Board;
use onerom_lab_bus::{
    BurstState, C64, HOLD_CYCLES, HostProfile, MAX_SEGMENTS, MIN_CYCLES, MachineSetup, Machines,
    Profile, ProfileError, SYS_CLOCK_HZ, Segment, burst, cycles, machines,
};
use pio::{InstructionOperands, JmpCondition, SetDestination};

use crate::error::Error;

/// `c64`, or `profile`'s fields as `H`'s arguments.
pub fn name(profile: &HostProfile) -> String {
    if *profile == C64 {
        String::from("c64")
    } else {
        format!(
            "{}:{}:{}:{}:{}:{}",
            profile.cycle_ns,
            profile.extra,
            profile.glitch_ns,
            profile.cs_ns,
            profile.glitch_pos_ns,
            profile.glitch_every
        )
    }
}

/// The timing Lab drives for `profile`.
pub fn driven(profile: &HostProfile) -> String {
    let mut text = format!(
        "cycle {}, {} extra, CS {}",
        Ns(cycles(profile.cycle_ns)),
        profile.extra,
        Ns(cycles(profile.cs_ns))
    );
    if profile.glitch_ns == 0 {
        text.push_str(", no glitch");
    } else {
        text.push_str(&format!(
            ", glitch {} at {}, 1 in {} cycles",
            Ns(cycles(profile.glitch_ns)),
            Ns(cycles(profile.glitch_pos_ns)),
            profile.glitch_every
        ));
    }
    text
}

/// A time in PIO cycles, displayed in ns to one decimal place.
pub struct Ns(pub u32);

impl fmt::Display for Ns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hz = u64::from(SYS_CLOCK_HZ);
        let tenths = (u64::from(self.0) * 10_000_000_000 + hz / 2) / hz;
        match tenths % 10 {
            0 => write!(f, "{}ns", tenths / 10),
            t => write!(f, "{}.{t}ns", tenths / 10),
        }
    }
}

/// The message for `e`.
pub fn profile_error(e: ProfileError) -> String {
    let min = Ns(MIN_CYCLES);
    match e {
        ProfileError::CycleTooLong => format!("a cycle must be at most {}", Ns(HOLD_CYCLES)),
        ProfileError::GlitchTooShort => format!("a glitch must be 0 or at least {min}"),
        ProfileError::GlitchTooLong => {
            format!("a glitch must be at least {min} shorter than the cycle")
        }
        ProfileError::GlitchOutsideCycle => String::from("a glitch must end within its cycle"),
        ProfileError::CsTooShort => format!("chip select must be active for at least {min}"),
        ProfileError::CsTooLong => String::from("chip select must be active for at most the cycle"),
        ProfileError::EdgesTooClose => {
            format!("edges in a cycle must be together or at least {min} apart")
        }
        ProfileError::PulsesTooClose => {
            format!("chip select must be inactive for at least {min} between a glitch and the read")
        }
    }
}

/// A chip's lines for one bit mode.
pub struct Lines<'a> {
    /// Socket pin of each address bit, from bit 0.
    pub addr: &'a [u8],
    /// Socket pin of each select line, and whether it is active high.
    pub selects: &'a [(u8, bool)],
    /// GPIO of each data line, from D0.
    pub data: &'a [u8],
    /// GPIOs Lab drives from SIO for the address and select lines.
    pub sio: &'a [u8],
}

/// How a chip's lines map to the machines.
struct Plan {
    machines: Machines,
    addr_bits: u8,
    /// Each data line's bit in R's word, from D0.
    data: Vec<u8>,
}

impl Plan {
    fn new(board: Board, lines: &Lines) -> Result<Plan, Error> {
        // `machines` checks there are 1 to 32 address lines.
        let machines =
            machines(board, lines.addr, lines.selects, lines.data).ok_or(Error::HostPins)?;
        let data_base = machines.r.gpio_base + machines.r.in_base;
        Ok(Plan {
            machines,
            addr_bits: lines.addr.len() as u8,
            data: lines.data.iter().map(|&gpio| gpio - data_base).collect(),
        })
    }
}

/// Checks a host bus can read `lines` on `board`.
pub fn check(board: Board, lines: &Lines) -> Result<(), Error> {
    Plan::new(board, lines).map(|_| ())
}

/// A state machine and the start of its program.
#[derive(Clone, Copy)]
struct Machine {
    pio: pac::pio::Pio,
    sm: usize,
    entry: u8,
    /// The IO function connecting a GPIO to the machine's block.
    funcsel: Gpio0ctrlFuncsel,
    /// The DMA request of the machine's TX FIFO.
    treq: TreqSel,
}

impl Machine {
    fn new(setup: &MachineSetup) -> Machine {
        let (pio, funcsel, tx0) = [
            (pac::PIO0, Gpio0ctrlFuncsel::PIO0_0, TreqSel::PIO0_TX0),
            (pac::PIO1, Gpio0ctrlFuncsel::PIO1_0, TreqSel::PIO1_TX0),
        ][usize::from(setup.block)];
        Machine {
            pio,
            sm: usize::from(setup.sm),
            entry: setup.wrap_bottom(),
            funcsel,
            treq: TreqSel::from_bits(tx0.to_bits() + setup.sm),
        }
    }

    fn regs(&self) -> pac::pio::StateMachine {
        self.pio.sm(self.sm)
    }

    fn exec(&self, operands: InstructionOperands) {
        self.regs()
            .instr()
            .write(|w| w.set_instr(operands.encode()));
    }

    /// Empties the FIFOs, resets the machine and puts it at its entry point.
    /// The machine must be stopped.
    fn reset(&self) {
        // Changing a FIFO join empties the FIFOs.
        for _ in 0..2 {
            self.regs()
                .shiftctrl()
                .modify(|w| w.set_fjoin_rx(!w.fjoin_rx()));
        }
        self.pio.ctrl().modify(|w| w.set_sm_restart(1 << self.sm));
        self.exec(InstructionOperands::JMP {
            condition: JmpCondition::Always,
            address: self.entry,
        });
        self.pio.fdebug().write(|w| w.set_txstall(1 << self.sm));
    }

    fn tx_stalled(&self) -> bool {
        self.pio.fdebug().read().txstall() & (1 << self.sm) != 0
    }

    /// The TX FIFO is full, or `dma` has written every word.
    fn fed(&self, dma: usize) -> bool {
        self.pio.fstat().read().txfull() & (1 << self.sm) != 0
            || !pac::DMA.ch(dma).ctrl_trig().read().busy()
    }
}

/// DMA channels feeding A and S.
const DMA_A: usize = 0;
const DMA_S: usize = 1;

/// A DMA channel's CTRL EN bit.
const CTRL_EN: u32 = 1;

/// Added to twice a burst's length for the time a read waits for the DMA and
/// for R's sample.
const TIMEOUT_MARGIN_US: u64 = 100;

/// A burst's segments and words.  Static rather than on the heap
/// (`HEAP_SIZE`).
struct Buffers {
    segments: [Segment; MAX_SEGMENTS],
    a: [u32; MAX_SEGMENTS],
    s: [u32; MAX_SEGMENTS],
}

struct BuffersCell(UnsafeCell<Buffers>);

// Only the CLI task reads through the host bus, one read at a time.
unsafe impl Sync for BuffersCell {}

const NO_SEGMENT: Segment = Segment {
    addr: 0,
    selected: false,
    cycles: 0,
};

static BUFFERS: BuffersCell = BuffersCell(UnsafeCell::new(Buffers {
    segments: [NO_SEGMENT; MAX_SEGMENTS],
    a: [0; MAX_SEGMENTS],
    s: [0; MAX_SEGMENTS],
}));

fn buffers() -> &'static mut Buffers {
    unsafe { &mut *BUFFERS.0.get() }
}

/// The machines, set up to read one chip in one bit mode.  Dropping it stops
/// them and returns the GPIOs to SIO.
pub struct HostBus {
    profile: Profile,
    plan: Plan,
    /// A, S and R.
    machines: [Machine; 3],
    /// Bit 0 of a burst's address is this bit of a read's address.
    addr_shift: u32,
    /// Each GPIO switched to PIO, with its IO and pad control as they were.
    switched: Vec<(u8, u32, u32)>,
    /// SIO output enables cleared, for GPIOs 0-31 and 32-47.
    released: [u32; 2],
}

impl HostBus {
    /// Sets up the machines and switches the address and select lines to
    /// them.
    pub fn start(
        board: Board,
        profile: Profile,
        lines: &Lines,
        addr_shift: u32,
    ) -> Result<HostBus, Error> {
        let plan = Plan::new(board, lines)?;
        let m = &plan.machines;
        let mut bus = HostBus {
            profile,
            machines: [Machine::new(&m.a), Machine::new(&m.s), Machine::new(&m.r)],
            plan,
            addr_shift,
            switched: Vec::new(),
            released: [0; 2],
        };
        bus.stop();
        bus.configure();
        bus.switch_pins(lines.sio);
        Ok(bus)
    }

    fn configure(&mut self) {
        let m = &self.plan.machines;
        for (machine, setup) in self.machines.iter().zip([&m.a, &m.s, &m.r]) {
            machine
                .pio
                .gpiobase()
                .write(|g| g.set_gpiobase(setup.gpio_base == 16));
            load(machine, setup);
        }
    }

    /// Drives each machine's lines inactive, then switches them from SIO to
    /// PIO.  Lab's SIO GPIOs that PIO doesn't drive become inputs: the other
    /// GPIO of a pin wired to two, and A-1 in 16-bit mode.
    fn switch_pins(&mut self, sio: &[u8]) {
        let setup = &self.plan.machines;
        let w = &setup.wiring;
        let driven = w.a.gpios() | w.s.gpios();

        for &gpio in sio {
            let (bank, bit) = (usize::from(gpio / 32), 1 << (gpio % 32));
            if driven >> gpio & 1 == 0 && pac::SIO.gpio_oe(bank).value().read() & bit != 0 {
                self.released[bank] |= bit;
            }
        }
        for (bank, &bits) in self.released.iter().enumerate() {
            pac::SIO.gpio_oe(bank).value_clr().write_value(bits);
        }

        let [a, s, _] = self.machines;
        let machines = [(a, &w.a, setup.a.gpio_base), (s, &w.s, setup.s.gpio_base)];
        for (m, packer, gpio_base) in machines {
            let idle = packer.image(0, false);
            let run = m.regs().pinctrl().read();
            for gpio in (0..48).filter(|&gpio| packer.gpios() >> gpio & 1 == 1) {
                let level = (idle >> (gpio - packer.out_base())) as u8 & 1;
                m.regs().pinctrl().write(|p| {
                    p.set_set_base(gpio - gpio_base);
                    p.set_set_count(1);
                });
                m.exec(InstructionOperands::SET {
                    destination: SetDestination::PINS,
                    data: level,
                });
                m.exec(InstructionOperands::SET {
                    destination: SetDestination::PINDIRS,
                    data: 1,
                });

                let io = pac::IO_BANK0.gpio(usize::from(gpio)).ctrl();
                let pad = pac::PADS_BANK0.gpio(usize::from(gpio));
                self.switched.push((gpio, io.read().0, pad.read().0));
                io.write(|c| c.set_funcsel(m.funcsel as u8));
                pad.modify(|c| {
                    c.set_iso(false);
                    c.set_drive(Drive::_8M_A);
                    c.set_slewfast(true);
                });
            }
            m.regs().pinctrl().write_value(run);
        }
    }

    /// Stops the machines and the DMA channels.  The lines keep their last
    /// levels.
    fn stop(&self) {
        let [a, s, _] = self.machines;
        s.pio.ctrl().write(|c| {
            c.set_sm_enable(0);
            c.set_next_pio_mask(1 << a.sm);
            c.set_nextprev_sm_disable(true);
        });

        // RP2350-E5: clear EN before an abort, through an alias that doesn't
        // start the channel.
        for ch in [DMA_A, DMA_S] {
            pac::DMA.ch(ch).al1_ctrl().modify(|c| *c &= !CTRL_EN);
        }
        let mask = (1 << DMA_A) | (1 << DMA_S);
        pac::DMA.chan_abort().write(|c| c.set_chan_abort(mask));
        while pac::DMA.chan_abort().read().chan_abort() & mask != 0 {}
        for ch in [DMA_A, DMA_S] {
            while pac::DMA.ch(ch).ctrl_trig().read().busy() {}
        }
    }

    /// A [`BurstState`] for this bus.
    pub fn burst_state(&self) -> BurstState {
        BurstState::new(self.plan.addr_bits)
    }

    /// Reads `addr` with one burst and returns the data lines, D0 in bit 0.
    pub fn read(&mut self, addr: usize, state: &mut BurstState) -> Result<u16, Error> {
        let b = buffers();
        let addr = (addr >> self.addr_shift) as u32;
        let burst = burst(&self.profile, state, addr, &mut b.segments);
        let segments = &b.segments[..burst.segments];
        let w = &self.plan.machines.wiring;
        let a_words = w.a.pack(segments, &mut b.a);
        let s_words = w.s.pack(segments, &mut b.s);

        let [a, s, r] = self.machines;
        for m in &self.machines {
            m.reset();
        }
        r.pio.txf(r.sm).write_value(burst.sample_at);
        feed(DMA_A, &b.a[..a_words], a);
        feed(DMA_S, &b.s[..s_words], s);

        let timeout = Duration::from_micros(
            u64::from(burst.sample_at) * 2 / u64::from(SYS_CLOCK_HZ / 1_000_000)
                + TIMEOUT_MARGIN_US,
        );
        let start = Instant::now();
        while !(a.fed(DMA_A) && s.fed(DMA_S)) {
            if start.elapsed() > timeout {
                self.stop();
                return Err(Error::HostTimeout);
            }
        }

        let sampled = critical_section::with(|_| {
            // One write starts S and R, and A in the next block.
            s.pio.ctrl().write(|c| {
                c.set_sm_enable((1 << s.sm) | (1 << r.sm));
                c.set_next_pio_mask(1 << a.sm);
                c.set_nextprev_sm_enable(true);
            });
            let start = Instant::now();
            while pac::PIO0.fstat().read().rxempty() & (1 << r.sm) != 0 {
                if start.elapsed() > timeout {
                    self.stop();
                    return Err(Error::HostTimeout);
                }
            }
            let raw = r.pio.rxf(r.sm).read();
            let stalled = a.tx_stalled() || s.tx_stalled();
            self.stop();
            if stalled {
                Err(Error::HostStall)
            } else {
                Ok(raw)
            }
        })?;

        Ok(self
            .plan
            .data
            .iter()
            .enumerate()
            .fold(0, |value, (line, &bit)| {
                value | (((sampled >> bit) & 1) as u16) << line
            }))
    }
}

impl Drop for HostBus {
    fn drop(&mut self) {
        self.stop();
        for &(gpio, io, pad) in self.switched.iter().rev() {
            pac::IO_BANK0
                .gpio(usize::from(gpio))
                .ctrl()
                .write_value(pac::io::regs::GpioCtrl(io));
            pac::PADS_BANK0
                .gpio(usize::from(gpio))
                .write_value(pac::pads::regs::GpioCtrl(pad));
        }
        for (bank, &bits) in self.released.iter().enumerate() {
            pac::SIO.gpio_oe(bank).value_set().write_value(bits);
        }
    }
}

/// Loads `setup`'s program into `m`'s block and sets up `m` to run it.
fn load(m: &Machine, setup: &MachineSetup) {
    for (i, code) in setup.code().enumerate() {
        m.pio
            .instr_mem(usize::from(setup.offset) + i)
            .write(|w| w.set_instr_mem(code));
    }

    let sm = m.regs();
    let shift = setup.shift;
    sm.clkdiv().write(|c| c.set_int(1));
    sm.execctrl().write(|e| {
        e.set_wrap_bottom(setup.wrap_bottom());
        e.set_wrap_top(setup.wrap_top());
    });
    sm.shiftctrl().write(|c| {
        // A threshold of 32 is written as 0.
        c.set_out_shiftdir(shift.osr.right);
        c.set_autopull(shift.osr.auto);
        c.set_pull_thresh(shift.osr.threshold % 32);
        c.set_in_shiftdir(shift.isr.right);
        c.set_autopush(shift.isr.auto);
        c.set_push_thresh(shift.isr.threshold % 32);
        c.set_fjoin_tx(shift.join_tx);
    });
    sm.pinctrl().write(|p| {
        p.set_out_base(setup.out_base);
        p.set_out_count(setup.out_count);
        p.set_in_base(setup.in_base);
    });
}

/// Starts `ch` writing `words` to `m`'s TX FIFO.
fn feed(ch: usize, words: &[u32], m: Machine) {
    let c = pac::DMA.ch(ch);
    c.read_addr().write_value(words.as_ptr() as u32);
    c.write_addr().write_value(m.pio.txf(m.sm).as_ptr() as u32);
    c.trans_count().write(|t| {
        t.set_mode(0.into());
        t.set_count(words.len() as u32);
    });
    compiler_fence(Ordering::SeqCst);
    c.ctrl_trig().write(|t| {
        t.set_treq_sel(m.treq);
        t.set_data_size(DataSize::SIZE_WORD);
        t.set_incr_read(true);
        t.set_incr_write(false);
        t.set_chain_to(ch as u8);
        t.set_en(true);
    });
}
