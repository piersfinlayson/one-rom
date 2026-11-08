// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom-lab-bus` - One ROM Lab's PIO host bus, which reads a ROM with a
//! host's bus timing.
//!
//! PIO state machines run the bus:
//! - A drives the address lines.
//! - S drives the select lines.
//! - R samples the data lines.
//!
//! A [`Profile`] sets the timing, and a [`HostProfile`] sets it in ns.
//! [`burst`] turns one read into the [`Segment`]s of a burst. A [`Packer`]
//! turns those into the words A or S consumes, and [`wiring`] builds both
//! packers for a chip on a board. [`machines`] sets up each machine for that
//! chip.
//!
//! Times outside [`HostProfile`] are in PIO cycles at clock divider 1.

#![no_std]

#[cfg(test)]
extern crate std;

mod burst;
mod machine;
mod pack;
mod program;

pub use burst::{
    Burst, BurstState, C64, HOLD_CYCLES, HostProfile, MAX_SEGMENTS, MIN_CYCLES, Profile,
    ProfileError, SYNC_CYCLES, SYS_CLOCK_HZ, Segment, burst, cycles,
};
pub use machine::{MachineSetup, Machines, machines};
pub use pack::{Packer, Wiring, wiring};
pub use program::{
    DRIVE_SHIFT, MAX_PINS, Shift, ShiftConfig, drive_program, sample_program, sample_shift,
};
