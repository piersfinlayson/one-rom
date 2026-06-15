// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "pio-test-gen",
    about = "Generate host C metadata for PIO testing",
    disable_version_flag = true
)]
pub struct Args {
    /// ROM config JSON file path
    #[arg(short = 'c', long)]
    pub config: String,

    /// Firmware version (e.g. "0.7.0")
    #[arg(long)]
    pub version: String,

    /// Board (e.g. "fire-32-a")
    #[arg(short = 'b', long)]
    pub board: String,

    /// Output C file path
    #[arg(short = 'o', long)]
    pub output: String,

    /// Enable boot logging
    #[arg(long)]
    pub boot_logging: bool,

    /// Enable verbose/debug logging
    #[arg(short = 'v', long)]
    pub verbose: bool,
}
