// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom update`.

use crate::args::CommandTrait;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[command(subcommand)]
    pub command: UpdateCommands,
}

impl CommandTrait for UpdateArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum UpdateCommands {
    /// Write a ROM image to a slot (ROM set) on the device (not yet supported).
    ///
    /// Writes the specified ROM image to the given flash slot. This
    /// persists across power cycles. The ROM type and chip-select
    /// configuration must match the slot's existing configuration, or
    /// the slot must be empty.
    ///
    /// Example:
    ///
    ///   onerom update flash --slot 2 --image kernal.bin
    Slot(UpdateSlotArgs),

    /// Commit a Live ROM image to flash (not yet supported).
    ///
    /// Persists the currently active RAM image to its corresponding
    /// flash slot so it survives power cycles.
    ///
    /// Example:
    ///
    ///   onerom update commit
    ///
    ///   onerom update commit --slot 2
    Commit(UpdateCommitArgs),

    /// Read or write One-Time Programmable (OTP) memory (not yet supported).
    ///
    /// Manages RP2350 OTP memory, including One ROM-specific USB
    /// configuration and other device identity data.
    ///
    /// This is an advanced operation. Incorrect OTP writes are
    /// irreversible.
    #[command(hide = true)]
    Otp(UpdateOtpArgs),
}

#[derive(Debug, Args)]
pub struct UpdateSlotArgs {
    /// Flash slot index to write (0-15).
    #[arg(long, value_name = "INDEX")]
    pub slot: u8,

    /// ROM image file to write to the slot.
    #[arg(long, value_name = "FILE")]
    pub image: String,
}

impl CommandTrait for UpdateSlotArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct UpdateCommitArgs {
    /// Slot index to commit. Commits the currently active slot if omitted.
    #[arg(long, value_name = "INDEX")]
    pub slot: Option<u8>,
}

impl CommandTrait for UpdateCommitArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct UpdateOtpArgs {
    /// Read OTP memory and display its contents.
    #[arg(long, conflicts_with = "write")]
    pub read: bool,

    /// Write a value to an OTP row. Format: <row>=<value>
    /// WARNING: OTP writes are irreversible.
    #[arg(long, value_name = "ROW=VALUE", conflicts_with = "read")]
    pub write: Option<String>,
}

impl CommandTrait for UpdateOtpArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
