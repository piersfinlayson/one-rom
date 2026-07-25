// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom image`.

use crate::args::CommandTrait;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct ImageArgs {
    #[command(subcommand)]
    pub command: ImageCommands,
}

impl CommandTrait for ImageArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum ImageCommands {
    /// Swap adjacent byte pairs in a ROM image file.
    ///
    /// Reverses the byte order within each 16-bit word throughout the image.
    /// Required for 16-bit wide ROM types (e.g. 27C400) when the source image
    /// has bytes in the opposite order to that expected by One ROM.
    ///
    /// The input file must have an even number of bytes.
    ///
    /// Example:
    ///
    ///   onerom image swap-bytes --input kick.bin --output kick-swapped.bin
    SwapBytes(ImageSwapBytesArgs),

    /// Convert a ROM image between formats.
    ///
    /// Reads --input in the --from format and writes --output in the --to
    /// format. Formats: `binary` (raw) and `ihex` (Intel HEX). Extensible to
    /// further formats in future.
    ///
    /// --load-address applies only when one side is Intel HEX: it is the
    /// absolute Intel HEX address that maps to byte 0 of the ROM (subtracted
    /// when reading ihex, used as the base when writing ihex). Accepts a
    /// decimal or `0x`/`$`-prefixed hex value; defaults to 0.
    ///
    /// Examples:
    ///
    ///   onerom image convert --from ihex --to binary --input rom.hex --output rom.bin
    ///
    ///   onerom image convert --from binary --to ihex --input rom.bin --output rom.hex --load-address $E000
    Convert(ImageConvertArgs),
}

#[derive(Debug, Args)]
pub struct ImageSwapBytesArgs {
    /// Input ROM image file.
    #[arg(long, short, visible_alias = "in", value_name = "FILE")]
    pub input: String,

    /// Output file path.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: String,
}

impl CommandTrait for ImageSwapBytesArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct ImageConvertArgs {
    /// Input format: `binary` (aliases `bin`, `raw`) or `ihex`.
    #[arg(long, value_name = "FORMAT")]
    pub from: String,

    /// Output format: `binary` (aliases `bin`, `raw`) or `ihex`.
    #[arg(long, value_name = "FORMAT")]
    pub to: String,

    /// Input ROM image file.
    #[arg(long, visible_alias = "in", value_name = "FILE")]
    pub input: String,

    /// Output file path.
    #[arg(long, visible_alias = "out", value_name = "FILE")]
    pub output: String,

    /// Intel HEX load address (decimal, or `0x`/`$`-prefixed hex). Only valid
    /// when converting to or from ihex. Defaults to 0.
    #[arg(long, value_name = "ADDR")]
    pub load_address: Option<String>,
}

impl CommandTrait for ImageConvertArgs {
    fn requires_device(&self) -> bool {
        false
    }
}
