// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration scenarios: do realistic application flows work end to end?
//!
//! Where the conformance suite asks whether each rule is obeyed, these ask
//! whether a real application built on the protocol works — and check the
//! outcome the way the application would see it, by reading the ROM, rather
//! than by asking the device about itself.
//!
//! Modelled on the specification's "Example — C64 Kernal Bootloader" and the
//! 6502 reference host's worked example.

use crate::Scenario;

pub static SCENARIOS: &[Scenario] = &[];
