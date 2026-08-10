// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom self` - the CLI's own release channel.

use crate::args::CommandTrait;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct SelfArgs {
    #[command(subcommand)]
    pub command: SelfCommands,
}

impl CommandTrait for SelfArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum SelfCommands {
    /// Check whether a newer One ROM CLI has been released.
    ///
    /// Compares this build against the newest release published for this
    /// platform, and says where to get it. Exits non-zero only if the check
    /// itself fails - finding an update is not an error.
    ///
    /// Example:
    ///
    ///   onerom self check
    Check(SelfCheckArgs),

    /// Download a One ROM CLI release.
    ///
    /// Downloads the published artifact for this platform - a .deb on Linux, a
    /// zip on Windows and macOS - and verifies it against the SHA-256 published
    /// alongside it. The file is saved, not installed; the install step is
    /// printed once it has been downloaded.
    ///
    /// Examples:
    ///
    ///   onerom self download
    ///
    ///   onerom self download --version 0.3.0 --path ~/Downloads
    ///
    ///   onerom self download --target aarch64-unknown-linux-gnu
    ///
    ///   onerom self download --target all --path ./dist
    Download(SelfDownloadArgs),
}

#[derive(Debug, Args)]
pub struct SelfCheckArgs {}

impl CommandTrait for SelfCheckArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct SelfDownloadArgs {
    /// CLI version to download (e.g. 0.3.0). Defaults to the latest release.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Platform to download for, as a target triple. Defaults to this machine's.
    ///
    /// macOS builds are universal binaries covering both architectures, so
    /// their target is 'universal-apple-darwin' rather than a real triple.
    ///
    /// Use 'all' to download every platform's artifact for the version, which
    /// requires --path rather than --output.
    #[arg(long, value_name = "TARGET")]
    pub target: Option<String>,

    /// Output file path. Defaults to the published filename, in this directory.
    #[arg(
        long,
        short,
        visible_alias = "out",
        value_name = "FILE",
        conflicts_with = "path"
    )]
    pub output: Option<String>,

    /// Output directory. Uses the published filename within the given directory.
    #[arg(long, value_name = "DIR", conflicts_with = "output")]
    pub path: Option<String>,

    /// Overwrite an existing file.
    #[arg(long, short)]
    pub force: bool,
}

impl CommandTrait for SelfDownloadArgs {
    fn requires_device(&self) -> bool {
        false
    }
}
