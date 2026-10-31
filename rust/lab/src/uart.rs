// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - serial traffic on a hardware UART
//!
//! embassy-rp configures the pins and the UART.  DMA then moves the traffic,
//! so it keeps running while another command keeps the CPU busy, and a task
//! checks what was received every [`CHECK_INTERVAL`].

use core::cell::{RefCell, UnsafeCell};
use core::fmt;
use core::ops::RangeInclusive;

use critical_section::Mutex;
use embassy_rp::pac;
use embassy_rp::pac::dma::vals::{DataSize, TransCountMode, TreqSel};
use embassy_rp::pac::pads::regs::GpioCtrl as PadCtrl;
use embassy_rp::peripherals::*;
use embassy_rp::uart::{Blocking, Config, DataBits, Parity, StopBits, UartRx, UartTx};
use onerom_config::pin::{HeaderPin, Pin, ResolvedPin};

use embassy_time::{Duration, Instant, Timer};

use crate::error::Error;

pub const DEFAULT_TX: Pin = Pin::Header(HeaderPin::Select(0));
pub const DEFAULT_RX: Pin = Pin::Header(HeaderPin::Select(1));
pub const DEFAULT_BAUD: u32 = 38400;

/// What Lab sends.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum Mode {
    /// Bytes 0x00 to 0xFF repeating, at full line rate.
    #[default]
    Pattern,
    /// Each byte received.
    Echo,
}

impl Mode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pattern" => Some(Self::Pattern),
            "echo" => Some(Self::Echo),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Echo => "echo",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum UartId {
    Uart0,
    Uart1,
}

impl fmt::Display for UartId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uart0 => write!(f, "UART0"),
            Self::Uart1 => write!(f, "UART1"),
        }
    }
}

impl UartId {
    fn regs(self) -> pac::uart::Uart {
        match self {
            Self::Uart0 => pac::UART0,
            Self::Uart1 => pac::UART1,
        }
    }

    fn dreqs(self) -> (TreqSel, TreqSel) {
        match self {
            Self::Uart0 => (TreqSel::UART0_TX, TreqSel::UART0_RX),
            Self::Uart1 => (TreqSel::UART1_TX, TreqSel::UART1_RX),
        }
    }

    /// Returns the UART to its state at boot, with empty FIFOs.
    fn reset(self) {
        let set = |w: &mut pac::resets::regs::Peripherals, on| match self {
            Self::Uart0 => w.set_uart0(on),
            Self::Uart1 => w.set_uart1(on),
        };
        pac::RESETS.reset().modify(|w| set(w, true));
        pac::RESETS.reset().modify(|w| set(w, false));
        while !match self {
            Self::Uart0 => pac::RESETS.reset_done().read().uart0(),
            Self::Uart1 => pac::RESETS.reset_done().read().uart1(),
        } {}
    }
}

/// The baud rates the UART can generate from the peripheral clock.
pub fn baud_range() -> RangeInclusive<u32> {
    let clk = embassy_rp::clocks::clk_peri_freq();
    clk / (16 * 65535) + 1..=clk / 16
}

macro_rules! uart_id {
    (UART0) => {
        UartId::Uart0
    };
    (UART1) => {
        UartId::Uart1
    };
}

/// Generates, from one table of GPIOs, a function returning the UART a GPIO
/// is a pin of and a function configuring it as that pin.  embassy-rp's pin
/// traits check each entry at compile time.
macro_rules! uart_pins {
    ($uart_of:ident, $new:ident, $driver:ident,
     $($(#[$attr:meta])* $gpio:literal => $pin:ident, $uart:ident;)*) => {
        fn $uart_of(gpio: u8) -> Option<UartId> {
            match gpio {
                $($(#[$attr])* $gpio => Some(uart_id!($uart)),)*
                _ => None,
            }
        }

        fn $new(gpio: u8, config: Config) -> $driver<'static, Blocking> {
            match gpio {
                $($(#[$attr])* $gpio => $driver::new_blocking(
                    unsafe { $uart::steal() },
                    unsafe { $pin::steal() },
                    config,
                ),)*
                n => panic!("GPIO {} isn't a UART pin", n),
            }
        }
    };
}

// From the RP2350 datasheet's GPIO function table.  Function F2 for GPIOs
// 0 and 1 of each group of 4, and F11 for GPIOs 2 and 3.
uart_pins! { tx_uart, new_tx, UartTx,
    0 => PIN_0, UART0;
    2 => PIN_2, UART0;
    4 => PIN_4, UART1;
    6 => PIN_6, UART1;
    8 => PIN_8, UART1;
    10 => PIN_10, UART1;
    12 => PIN_12, UART0;
    14 => PIN_14, UART0;
    16 => PIN_16, UART0;
    18 => PIN_18, UART0;
    20 => PIN_20, UART1;
    22 => PIN_22, UART1;
    24 => PIN_24, UART1;
    26 => PIN_26, UART1;
    28 => PIN_28, UART0;
    #[cfg(feature = "rp235xb")]
    30 => PIN_30, UART0;
    #[cfg(feature = "rp235xb")]
    32 => PIN_32, UART0;
    #[cfg(feature = "rp235xb")]
    34 => PIN_34, UART0;
    #[cfg(feature = "rp235xb")]
    36 => PIN_36, UART1;
    #[cfg(feature = "rp235xb")]
    38 => PIN_38, UART1;
    #[cfg(feature = "rp235xb")]
    40 => PIN_40, UART1;
    #[cfg(feature = "rp235xb")]
    42 => PIN_42, UART1;
    #[cfg(feature = "rp235xb")]
    44 => PIN_44, UART0;
    #[cfg(feature = "rp235xb")]
    46 => PIN_46, UART0;
}

uart_pins! { rx_uart, new_rx, UartRx,
    1 => PIN_1, UART0;
    3 => PIN_3, UART0;
    5 => PIN_5, UART1;
    7 => PIN_7, UART1;
    9 => PIN_9, UART1;
    11 => PIN_11, UART1;
    13 => PIN_13, UART0;
    15 => PIN_15, UART0;
    17 => PIN_17, UART0;
    19 => PIN_19, UART0;
    21 => PIN_21, UART1;
    23 => PIN_23, UART1;
    25 => PIN_25, UART1;
    27 => PIN_27, UART1;
    29 => PIN_29, UART0;
    #[cfg(feature = "rp235xb")]
    31 => PIN_31, UART0;
    #[cfg(feature = "rp235xb")]
    33 => PIN_33, UART0;
    #[cfg(feature = "rp235xb")]
    35 => PIN_35, UART0;
    #[cfg(feature = "rp235xb")]
    37 => PIN_37, UART1;
    #[cfg(feature = "rp235xb")]
    39 => PIN_39, UART1;
    #[cfg(feature = "rp235xb")]
    41 => PIN_41, UART1;
    #[cfg(feature = "rp235xb")]
    43 => PIN_43, UART1;
    #[cfg(feature = "rp235xb")]
    45 => PIN_45, UART0;
    #[cfg(feature = "rp235xb")]
    47 => PIN_47, UART0;
}

/// The UART that has `tx` as its TX pin and `rx` as its RX pin.
pub fn uart_for(tx: ResolvedPin, rx: ResolvedPin) -> Result<UartId, Error> {
    let tx_id = tx_uart(tx.gpio()).ok_or(Error::UartTxPin(tx))?;
    let rx_id = rx_uart(rx.gpio()).ok_or(Error::UartRxPin(rx))?;
    if tx_id == rx_id {
        Ok(tx_id)
    } else {
        Err(Error::UartPins(tx, tx_id, rx, rx_id))
    }
}

/// DMA channels 0 and 1 are host.rs's `DMA_A` and `DMA_S`.
const DMA_TX: usize = 2;
const DMA_RX: usize = 3;

/// A channel restarts itself after this many transfers, so the count of
/// transfers made is known modulo it.  A multiple of the ring's length, so the
/// count modulo the ring's length is the position in the ring.
const DMA_COUNT: u32 = 1 << 27;
const DMA_COUNT_MASK: u32 = DMA_COUNT - 1;

/// A DMA channel's CTRL EN bit.
const CTRL_EN: u32 = 1;

pub const CHECK_INTERVAL: Duration = Duration::from_millis(50);

/// What pattern mode sends.  Aligned to its size so a DMA read ring wraps
/// round it.
#[repr(C, align(256))]
struct Pattern([u8; 256]);

static PATTERN: Pattern = Pattern({
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = i as u8;
        i += 1;
    }
    table
});

/// Received characters, each with its error flags in bits 8 to 11 as the
/// UART's data register holds them.  4096 characters is about a second at
/// 38400 baud, longer than any other command keeps the CPU without the check
/// task running.  Aligned to its size so a DMA write ring wraps round it.
const RING_ENTRIES: u32 = 4096;
const RING_BITS: u8 = 13;

#[repr(C, align(8192))]
struct Ring(UnsafeCell<[u16; RING_ENTRIES as usize]>);

// Only the DMA channel writes the ring, and only the check task, under the
// critical section, reads it.
unsafe impl Sync for Ring {}

static RING: Ring = Ring(UnsafeCell::new([0; RING_ENTRIES as usize]));

const DR_FE: u16 = 1 << 8;
const DR_PE: u16 = 1 << 9;
const DR_BE: u16 = 1 << 10;
const DR_OE: u16 = 1 << 11;

#[derive(Debug, Copy, Clone, Default)]
pub struct Counts {
    pub sent: u64,
    pub received: u64,
    /// Received bytes out of the pattern's sequence
    pub gaps: u64,
    pub framing: u64,
    pub parity: u64,
    pub breaks: u64,
    pub overruns: u64,
    /// Received characters overwritten in the ring before they were checked
    pub lost: u64,
}

/// The transfers `ch` has made, modulo [`DMA_COUNT`].
fn transfers(ch: usize) -> u32 {
    (DMA_COUNT - pac::DMA.ch(ch).trans_count().read().count()) & DMA_COUNT_MASK
}

fn start_dma(ch: usize, read: u32, write: u32, f: impl FnOnce(&mut pac::dma::regs::CtrlTrig)) {
    let c = pac::DMA.ch(ch);
    c.read_addr().write_value(read);
    c.write_addr().write_value(write);
    c.trans_count().write(|t| {
        t.set_mode(TransCountMode::TRIGGER_SELF);
        t.set_count(DMA_COUNT);
    });
    c.ctrl_trig().write(|t| {
        t.set_irq_quiet(true);
        t.set_chain_to(ch as u8);
        f(t);
        t.set_en(true);
    });
}

fn stop_dma(chs: &[usize]) {
    // RP2350-E5: clear EN before an abort, through an alias that doesn't
    // start the channel.
    let mut mask = 0;
    for &ch in chs {
        pac::DMA.ch(ch).al1_ctrl().modify(|c| *c &= !CTRL_EN);
        mask |= 1 << ch;
    }
    pac::DMA.chan_abort().write(|c| c.set_chan_abort(mask));
    while pac::DMA.chan_abort().read().chan_abort() & mask != 0 {}
    for &ch in chs {
        while pac::DMA.ch(ch).ctrl_trig().read().busy() {}
    }
}

/// The UART drivers, kept so their pins stay connected.
struct Drivers {
    _tx: UartTx<'static, Blocking>,
    _rx: UartRx<'static, Blocking>,
}

// Lab runs on one core, and the drivers are only dropped under the critical
// section.
unsafe impl Send for Drivers {}

/// A UART sending and receiving by DMA.
struct Traffic {
    id: UartId,
    mode: Mode,
    started: Instant,
    /// Each pin's GPIO and its IO and pad control registers before the UART
    /// took it.
    saved: [(u8, u32, u32); 2],
    counts: Counts,
    /// Transfers already counted, modulo [`DMA_COUNT`]
    tx_seen: u32,
    rx_seen: u32,
    /// The byte the pattern sends next, once one has been received
    next: Option<u8>,
    /// [`Traffic::stop_sending`] has run
    tx_stopped: bool,
    _drivers: Drivers,
}

static TRAFFIC: Mutex<RefCell<Option<Traffic>>> = Mutex::new(RefCell::new(None));

impl Traffic {
    fn new(id: UartId, tx: u8, rx: u8, baud: u32, mode: Mode) -> Self {
        let saved = [tx, rx].map(|gpio| {
            (
                gpio,
                pac::IO_BANK0.gpio(usize::from(gpio)).ctrl().read().0,
                pac::PADS_BANK0.gpio(usize::from(gpio)).read().0,
            )
        });

        id.reset();
        let mut config = Config::default();
        config.baudrate = baud;
        config.data_bits = DataBits::DataBits8;
        config.stop_bits = StopBits::STOP1;
        config.parity = Parity::ParityNone;
        let drivers = Drivers {
            _tx: new_tx(tx, config),
            _rx: new_rx(rx, config),
        };

        // embassy-rp leaves the pad's pull-down on.  An RX line nothing drives
        // would then read as a break.
        pac::PADS_BANK0.gpio(usize::from(rx)).modify(|w| {
            w.set_pde(false);
            w.set_pue(true);
        });

        // The line read low until the pull-up took over, so what the UART
        // received before then is discarded.  Two characters' time covers a
        // break the UART was part way through.
        embassy_time::block_for(Duration::from_micros(
            u64::from(2 * 10 * 1_000_000 / baud) + 1,
        ));
        let regs = id.regs();
        while !regs.uartfr().read().rxfe() {
            let _ = regs.uartdr().read();
        }
        regs.uartrsr().write(|w| w.0 = 0);

        let dr = regs.uartdr().as_ptr() as u32;
        let (tx_dreq, rx_dreq) = id.dreqs();
        match mode {
            Mode::Pattern => {
                start_dma(DMA_TX, PATTERN.0.as_ptr() as u32, dr, |t| {
                    t.set_treq_sel(tx_dreq);
                    t.set_data_size(DataSize::SIZE_BYTE);
                    t.set_incr_read(true);
                    t.set_ring_size(8);
                    t.set_ring_sel(false);
                });
                start_dma(DMA_RX, dr, RING.0.get() as u32, |t| {
                    t.set_treq_sel(rx_dreq);
                    t.set_data_size(DataSize::SIZE_HALFWORD);
                    t.set_incr_write(true);
                    t.set_ring_size(RING_BITS);
                    t.set_ring_sel(true);
                });
            }
            // Each received byte is written straight back to the data
            // register.
            Mode::Echo => start_dma(DMA_TX, dr, dr, |t| {
                t.set_treq_sel(rx_dreq);
                t.set_data_size(DataSize::SIZE_BYTE);
            }),
        }
        regs.uartdmacr().write(|w| {
            w.set_txdmae(true);
            w.set_rxdmae(true);
        });

        Self {
            id,
            mode,
            started: Instant::now(),
            saved,
            counts: Counts::default(),
            tx_seen: 0,
            rx_seen: 0,
            next: None,
            tx_stopped: false,
            _drivers: drivers,
        }
    }

    /// Counts what the DMA channels have moved since the last call, and in
    /// pattern mode checks what was received.
    fn check(&mut self) {
        // An aborted channel's count no longer says what it moved.
        if !self.tx_stopped {
            let sent = transfers(DMA_TX);
            let delta = u64::from(sent.wrapping_sub(self.tx_seen) & DMA_COUNT_MASK);
            self.tx_seen = sent;
            self.counts.sent += delta;
            if self.mode == Mode::Echo {
                self.counts.received += delta;
            }
        }
        if self.mode == Mode::Echo {
            return;
        }

        let received = transfers(DMA_RX);
        let mut unread = received.wrapping_sub(self.rx_seen) & DMA_COUNT_MASK;
        if unread > RING_ENTRIES {
            self.counts.lost += u64::from(unread - RING_ENTRIES);
            self.rx_seen = (self.rx_seen + unread - RING_ENTRIES) & DMA_COUNT_MASK;
            self.next = None;
            unread = RING_ENTRIES;
        }
        let ring = RING.0.get() as *const u16;
        for _ in 0..unread {
            // SAFETY: the index is within the ring.  An entry the DMA channel
            // overwrites during the check reads as its newer character, which
            // takes a check running about a second late.
            let c = unsafe {
                core::ptr::read_volatile(ring.add((self.rx_seen & (RING_ENTRIES - 1)) as usize))
            };
            self.rx_seen = (self.rx_seen + 1) & DMA_COUNT_MASK;
            self.receive(c);
        }
    }

    /// A character with a framing, parity or break error isn't counted as
    /// received.  A break also has a framing error, which isn't counted.  An
    /// overrun flags the first character after those lost, and that character
    /// is good.
    fn receive(&mut self, c: u16) {
        let counts = &mut self.counts;
        if c & DR_OE != 0 {
            counts.overruns += 1;
        }
        if c & DR_BE != 0 {
            counts.breaks += 1;
            return;
        }
        if c & (DR_FE | DR_PE) != 0 {
            counts.framing += u64::from(c & DR_FE != 0);
            counts.parity += u64::from(c & DR_PE != 0);
            return;
        }
        let byte = c as u8;
        counts.received += 1;
        if self.next.is_some_and(|next| next != byte) {
            counts.gaps += 1;
        }
        self.next = Some(byte.wrapping_add(1));
    }

    /// Stops the DMA channels feeding the UART.  The TX FIFO still holds up
    /// to 32 bytes.
    fn stop_sending(&mut self) {
        // With its request off the channel moves nothing more, so the count
        // taken before the abort is final.
        let regs = self.id.regs();
        regs.uartdmacr().modify(|w| w.set_txdmae(false));
        cortex_m::asm::dsb();
        self.check();
        stop_dma(&[DMA_TX]);
        self.tx_stopped = true;
    }

    fn sending(&self) -> bool {
        self.id.regs().uartfr().read().busy()
    }

    /// Puts the pins back as they were and resets the UART.
    fn finish(mut self) -> (Counts, Duration) {
        self.check();
        stop_dma(&[DMA_TX, DMA_RX]);
        for &(gpio, io, pad) in &self.saved {
            pac::IO_BANK0
                .gpio(usize::from(gpio))
                .ctrl()
                .write_value(pac::io::regs::GpioCtrl(io));
            pac::PADS_BANK0
                .gpio(usize::from(gpio))
                .write_value(PadCtrl(pad));
        }
        self.id.reset();
        (self.counts, self.started.elapsed())
    }
}

/// Starts the UART that [`uart_for`] returned for `tx` and `rx`, at 8N1, in
/// the background.
pub fn start(id: UartId, tx: u8, rx: u8, baud: u32, mode: Mode) -> Result<(), Error> {
    critical_section::with(|cs| {
        let mut traffic = TRAFFIC.borrow_ref_mut(cs);
        if traffic.is_some() {
            return Err(Error::UartRunning);
        }
        *traffic = Some(Traffic::new(id, tx, rx, baud, mode));
        Ok(())
    })
}

/// Stops the traffic [`start`] started, once the UART has sent what it
/// holds, and returns its counts and how long it ran.
pub async fn stop() -> Result<(Counts, Duration), Error> {
    critical_section::with(|cs| {
        TRAFFIC
            .borrow_ref_mut(cs)
            .as_mut()
            .map(Traffic::stop_sending)
            .ok_or(Error::UartNotRunning)
    })?;
    while critical_section::with(|cs| {
        TRAFFIC
            .borrow_ref(cs)
            .as_ref()
            .is_some_and(Traffic::sending)
    }) {
        embassy_futures::yield_now().await;
    }
    critical_section::with(|cs| TRAFFIC.borrow_ref_mut(cs).take())
        .map(Traffic::finish)
        .ok_or(Error::UartNotRunning)
}

/// Checks the received characters every [`CHECK_INTERVAL`] while [`start`]'s
/// traffic runs.
#[embassy_executor::task]
pub async fn check_task() -> ! {
    loop {
        Timer::after(CHECK_INTERVAL).await;
        critical_section::with(|cs| {
            if let Some(traffic) = TRAFFIC.borrow_ref_mut(cs).as_mut() {
                traffic.check();
            }
        });
    }
}
