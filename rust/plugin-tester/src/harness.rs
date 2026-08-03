// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Runs the plugin alongside the test driver, in strict alternation.
//!
//! # Why two threads
//!
//! On a device the plugin runs on its own core, concurrently with serving, and
//! blocks in busy-waits until the bus gives it something.  Here the emulated
//! hardware only advances when something advances it, so the plugin's entry
//! point — which never returns — cannot simply be called from the test.
//!
//! So the plugin gets a thread of its own, and the two hand control back and
//! forth:
//!
//! ```text
//!   driver thread                     plugin thread
//!   ─────────────                     ─────────────
//!   start()  ────────── spawn ──────▶ install hooks
//!        (blocked)                    enter plugin
//!                                     ...
//!                                     ORA_TEST_YIELD  ─┐
//!        ◀───────────── yielded ──────────────────────┘
//!   drive the bus,
//!   read the results
//!   resume() ────────── go ─────────▶ (returns from the hook)
//!        (blocked)                    ...
//! ```
//!
//! Only one thread is ever runnable, so despite [`Emulator`] not being `Send`
//! there is no concurrent access: the driver touches the emulator only while
//! the plugin is parked inside its yield hook, and vice versa.  This mirrors
//! what the device does closely enough for protocol testing, while leaving the
//! test in charge of when the plugin makes progress — which is what lets a
//! test deliberately *withhold* progress and observe the plugin missing bus
//! activity, as it would on hardware when the ring wraps.
//!
//! # One plugin per process
//!
//! The firmware's state, the plugin's statics and the yield hook are all
//! process-global.  Run one plugin at a time; a second scenario re-boots the
//! firmware and re-enters the plugin, which re-initialises its own state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use onerom_fw_emulator::Emulator;

use crate::ffi;

/// How long the driver waits for the plugin to reach its next yield.
///
/// Generous: it only bites when the plugin is not going to yield at all —
/// stuck in a loop with no seam, or waiting on bus activity that the driver
/// has not produced.  A tight bound would turn a slow host into a flaky test.
pub const YIELD_TIMEOUT: Duration = Duration::from_secs(10);

/// What the driver tells the parked plugin to do next.
enum Command {
    /// Return from the yield hook and carry on.
    Go,
    /// Park forever.  Used to abandon the plugin thread at the end of a
    /// scenario, or after the driver has given up on it.
    Stop,
}

thread_local! {
    /// The plugin thread's end of the handoff.  Installed by [`run_plugin`]
    /// before the plugin is entered, and used only by the yield hook.
    static PLUGIN_SIDE: OnceLock<(Sender<()>, Receiver<Command>)> = const { OnceLock::new() };
}

/// Hand control to the driver and block until it says to carry on.
///
/// Called from the plugin thread, from inside whatever busy-wait the plugin is
/// sitting in — either its own `ORA_TEST_YIELD`, or the firmware's
/// `onerom_test_yield` when the plugin is blocked inside an API call such as
/// `wait_for_knock`.  Both route here, because from the driver's point of view
/// they are the same event: the plugin can make no further progress until the
/// emulation moves.
fn yield_to_driver() {
    PLUGIN_SIDE.with(|side| {
        let Some((to_driver, from_driver)) = side.get() else {
            panic!(
                "the plugin yielded on a thread with no handoff installed — \
                 the hooks must be installed on the thread that runs the plugin, \
                 not on the driver thread"
            );
        };
        if to_driver.send(()).is_err() {
            // The driver is gone; nothing will ever resume us.
            park_forever();
        }
        match from_driver.recv() {
            Ok(Command::Go) => (),
            Ok(Command::Stop) | Err(_) => park_forever(),
        }
    });
}

/// C-ABI entry for `ORA_TEST_YIELD`.
unsafe extern "C" fn c_yield_hook() {
    yield_to_driver();
}

/// Park the current thread permanently.  The plugin thread holds no resource
/// the driver needs, so abandoning it is cheaper than unwinding out of C.
fn park_forever() -> ! {
    loop {
        std::thread::park();
    }
}

/// A running plugin, parked at a yield point.
///
/// Dropping this parks the plugin thread permanently and abandons it.
pub struct Plugin {
    to_plugin: Sender<Command>,
    from_plugin: Receiver<()>,
    started: Arc<AtomicBool>,
}

impl Plugin {
    /// Start the plugin and run it up to its first yield.
    ///
    /// The caller must have booted the firmware, called
    /// [`Emulator::setup_epio`] and [`Emulator::arm_monitor`], and pointed the
    /// plugin's ring buffer at emulated SRAM, before calling this.
    ///
    /// # Safety
    ///
    /// `emu` must outlive the returned `Plugin`.  The plugin thread holds a
    /// raw pointer to it, which is sound only because the two threads never
    /// run at the same time.
    pub unsafe fn start(emu: &Emulator) -> Result<Self, String> {
        let (to_driver, from_plugin) = channel();
        let (to_plugin, from_driver) = channel();
        let started = Arc::new(AtomicBool::new(false));

        let emu_addr = emu as *const Emulator as usize;
        let started_thread = Arc::clone(&started);

        std::thread::Builder::new()
            .name("onerom-plugin".to_string())
            .spawn(move || {
                PLUGIN_SIDE.with(|side| {
                    let _ = side.set((to_driver, from_driver));
                });

                // Both seams must be installed from this thread: the
                // emulator's hook dispatches through a thread-local, so one
                // installed on the driver thread would silently do nothing
                // here and the plugin would spin forever.
                //
                // SAFETY: the pointer is valid for the plugin's lifetime, and
                // only one of the two threads runs at a time.
                let emu = unsafe { &*(emu_addr as *const Emulator) };
                emu.set_yield_hook(yield_to_driver);
                // SAFETY: installing a plain function pointer.
                unsafe { ffi::ora_host_test_set_yield_hook(Some(c_yield_hook)) };

                started_thread.store(true, Ordering::SeqCst);

                // SAFETY: the firmware is booted and the ring is placed; this
                // is the plugin's own entry point and does not return.
                unsafe { ffi::ora_host_test_run_plugin() };
            })
            .map_err(|e| format!("could not spawn the plugin thread: {e}"))?;

        let plugin = Plugin {
            to_plugin,
            from_plugin,
            started,
        };
        plugin.await_yield("plugin start")?;

        // The plugin configures and starts the address monitor back to back
        // during its own setup, with no yield in between, so the driver cannot
        // interleave these.  By the first yield both have happened, so one
        // call here applies the whole of the PIO configuration the plugin
        // asked for.
        emu.update_from_apio();

        Ok(plugin)
    }

    /// Let the plugin run until its next yield.
    pub fn resume(&self, what: &str) -> Result<(), String> {
        self.to_plugin
            .send(Command::Go)
            .map_err(|_| format!("{what}: the plugin thread has gone away"))?;
        self.await_yield(what)
    }

    /// Wait for the plugin to reach a yield point.
    fn await_yield(&self, what: &str) -> Result<(), String> {
        match self.from_plugin.recv_timeout(YIELD_TIMEOUT) {
            Ok(()) => Ok(()),
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "{what}: the plugin did not yield within {:?} — it is either in a loop \
                 with no ORA_TEST_YIELD, or waiting for bus activity the driver has not \
                 produced",
                YIELD_TIMEOUT
            )),
            Err(RecvTimeoutError::Disconnected) => {
                if self.started.load(Ordering::SeqCst) {
                    Err(format!("{what}: the plugin returned or panicked"))
                } else {
                    Err(format!("{what}: the plugin thread died before starting"))
                }
            }
        }
    }

    /// The plugin header's version, as `(major, minor, patch, build)`.
    pub fn version(&self) -> (u8, u8, u8, u8) {
        // SAFETY: reads a constant in the plugin's own image.
        let v = unsafe { ffi::ora_host_test_plugin_version() };
        ((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        // Park the plugin rather than trying to unwind it: it is inside C, in
        // a call that never returns.  It holds nothing the next scenario
        // needs — the firmware and plugin state are re-initialised by the next
        // boot and entry.
        let _ = self.to_plugin.send(Command::Stop);
    }
}
