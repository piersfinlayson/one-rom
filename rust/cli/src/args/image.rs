// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom image`.

use crate::args::CommandTrait;
use clap::builder::{PossibleValue, TypedValueParser};
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;
use onerom_gen::{FileFormat, LoadAddress};

/// Value parser for `--from`/`--to`, driven by `onerom-gen`'s format list.
///
/// A format added to [`FileFormat`] is accepted here and appears in `--help`
/// with no CLI change, and a typo now fails at parse time rather than part-way
/// through the conversion. `onerom-gen` deliberately carries no clap
/// dependency, which is why this is a hand-written parser rather than a
/// `ValueEnum` derived on `FileFormat` itself.
///
/// Parsing goes through [`FileFormat::try_from_str`] so every alias it accepts
/// (`bin`, `raw`, `hex`, `intel-hex`, …) keeps working, while only the
/// canonical spellings are advertised - a plain list of possible values would
/// have accepted the canonical names alone.
#[derive(Clone)]
struct ImageFormatParser;

impl TypedValueParser for ImageFormatParser {
    type Value = FileFormat;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let invalid = || {
            let arg = arg
                .map(|a| a.to_string())
                .unwrap_or_else(|| "--from/--to".into());
            cmd.clone().error(
                clap::error::ErrorKind::InvalidValue,
                format!(
                    "invalid format '{}' for '{arg}'\n  Supported values: {}",
                    value.to_string_lossy(),
                    FileFormat::supported_values()
                        .iter()
                        .map(|f| f.name())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            )
        };
        let text = value.to_str().ok_or_else(invalid)?;
        FileFormat::try_from_str(text).ok_or_else(invalid)
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            FileFormat::supported_values()
                .iter()
                .map(|f| PossibleValue::new(f.name())),
        ))
    }
}

/// Parse an Intel HEX load address, sharing the config file's spellings.
///
/// [`LoadAddress::parse_str`] is what the config and `--slot load-address=`
/// both use, so all three accept a decimal, `0x`- or `$`-prefixed value
/// identically.
fn parse_load_address(s: &str) -> Result<LoadAddress, String> {
    LoadAddress::parse_str(s).map_err(|e| e.to_string())
}

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

    /// Extract one lane from an interleaved ROM image.
    ///
    /// The image contains --stride interleaved lanes of --bytes bytes each;
    /// lane --offset is kept and the rest discarded.  Used to split a wide ROM
    /// image, distributed as a single interleaved file, into the narrower
    /// images each device needs.
    ///
    /// The input length must be a multiple of --bytes x --stride.
    ///
    /// Odd bytes of a 16-bit interleaved image:
    ///
    ///   onerom image deinterleave --input rom16.bin --output odd.bin --offset 1 --stride 2
    ///
    /// Byte 2 of a 32-bit interleaved image:
    ///
    ///   onerom image deinterleave --input rom32.bin --output b2.bin --offset 2 --stride 4
    ///
    /// The upper 16-bit half of each 32-bit word:
    ///
    ///   onerom image deinterleave --input rom32.bin --output hi.bin --offset 1 --stride 2 --bytes 2
    Deinterleave(ImageDeinterleaveArgs),

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
pub struct ImageDeinterleaveArgs {
    /// Input ROM image file.
    #[arg(long, short, visible_alias = "in", value_name = "FILE")]
    pub input: String,

    /// Output file path.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: String,

    /// Which lane to keep.  Must be less than --stride.
    #[arg(long, value_name = "N")]
    pub offset: usize,

    /// How many lanes the image interleaves.  Must be at least 2.
    #[arg(long, value_name = "N")]
    pub stride: usize,

    /// Width of one lane, in bytes.  Use 2 to keep 16-bit words together.
    #[arg(long, visible_alias = "unit", value_name = "N", default_value_t = 1)]
    pub bytes: usize,
}

impl CommandTrait for ImageDeinterleaveArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct ImageConvertArgs {
    /// Input format. `binary` also accepts `bin` and `raw`; `ihex` also
    /// accepts `intel-hex` and `hex`.
    #[arg(long, value_name = "FORMAT", value_parser = ImageFormatParser)]
    pub from: FileFormat,

    /// Output format. `binary` also accepts `bin` and `raw`; `ihex` also
    /// accepts `intel-hex` and `hex`.
    #[arg(long, value_name = "FORMAT", value_parser = ImageFormatParser)]
    pub to: FileFormat,

    /// Input ROM image file.
    #[arg(long, short, visible_alias = "in", value_name = "FILE")]
    pub input: String,

    /// Output file path.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: String,

    /// Intel HEX load address (decimal, or `0x`/`$`-prefixed hex). Only valid
    /// when converting to or from ihex. Defaults to 0.
    #[arg(long, value_name = "ADDR", value_parser = parse_load_address)]
    pub load_address: Option<LoadAddress>,
}

impl CommandTrait for ImageConvertArgs {
    fn requires_device(&self) -> bool {
        false
    }
}
