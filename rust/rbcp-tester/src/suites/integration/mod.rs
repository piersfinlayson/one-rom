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

//! # Flows still to come
//!
//! - **NV-backed auto-boot.** The worked example's optional extra: "the
//!   bootloader could also store the last-selected slot index in NV storage
//!   using NV_POKE_COMMIT_BYTE, and read it back on boot to auto-boot the last
//!   selection without presenting the menu."  Waits on the NV group.
//! - **Recovery from a host reset mid-command.** A bootloader interrupted part
//!   way through a command frame, whose next run must re-establish framing and
//!   complete the job.

use crate::Scenario;

pub mod bootloader;

pub static SCENARIOS: &[Scenario] = &[Scenario {
    name: "integration.bootloader.kernal_bootloader",
    spec_ref: "Example — C64 Kernal Bootloader; Group 0x02 — SLOT_POKE (the safe pattern)",
    run: bootloader::kernal_bootloader,
}];
