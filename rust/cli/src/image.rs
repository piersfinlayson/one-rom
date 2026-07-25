// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom image` subcommands.

use crate::args::image::{ImageConvertArgs, ImageSwapBytesArgs};
use onerom_cli::{Error, Options};
use onerom_gen::{FileFormat, LoadAddress, decode_ihex, encode_ihex};

pub async fn cmd_swap_bytes(options: &Options, args: &ImageSwapBytesArgs) -> Result<(), Error> {
    if options.verbose {
        println!("Reading ROM image from {} ...", args.input);
    }
    let data = std::fs::read(&args.input).map_err(|e| Error::io(&args.input, e))?;

    if data.len() % 2 != 0 {
        return Err(Error::OddLengthImage(args.input.clone(), data.len()));
    }

    let swapped: Vec<u8> = data.chunks_exact(2).flat_map(|w| [w[1], w[0]]).collect();

    std::fs::write(&args.output, &swapped).map_err(|e| Error::io(&args.output, e))?;

    if options.verbose {
        println!(
            "Wrote {} bytes to {} with byte pairs swapped",
            swapped.len(),
            args.output
        );
    } else {
        println!("Written to {}", args.output);
    }

    Ok(())
}

fn parse_format(flag: &str, value: &str) -> Result<FileFormat, Error> {
    FileFormat::try_from_str(value).ok_or_else(|| {
        Error::InvalidArgument(
            flag.to_string(),
            format!("Invalid format '{value}': expected 'binary' or 'ihex'"),
        )
    })
}

pub async fn cmd_convert(options: &Options, args: &ImageConvertArgs) -> Result<(), Error> {
    let from = parse_format("--from", &args.from)?;
    let to = parse_format("--to", &args.to)?;

    let load_address = match &args.load_address {
        Some(s) => LoadAddress::parse_str(s)
            .map_err(|e| Error::InvalidArgument("--load-address".to_string(), e.to_string()))?,
        None => LoadAddress::default(),
    };

    // A load address only means anything when Intel HEX is on one side.
    if from == FileFormat::Binary && to == FileFormat::Binary && !load_address.is_zero() {
        return Err(Error::InvalidArgument(
            "--load-address".to_string(),
            "load address is only valid when converting to or from ihex".to_string(),
        ));
    }

    if options.verbose {
        println!("Reading {from} image from {} ...", args.input);
    }
    let data = std::fs::read(&args.input).map_err(|e| Error::io(&args.input, e))?;

    // Decode to a flat binary, then re-encode into the requested format.
    let binary = match from {
        FileFormat::Binary => data,
        FileFormat::IntelHex => decode_ihex(&data, load_address.0)
            .map_err(|e| Error::IhexDecode(args.input.clone(), e.to_string()))?,
    };
    let output_bytes = match to {
        FileFormat::Binary => binary,
        FileFormat::IntelHex => encode_ihex(&binary, load_address.0).into_bytes(),
    };

    std::fs::write(&args.output, &output_bytes).map_err(|e| Error::io(&args.output, e))?;

    if options.verbose {
        println!(
            "Wrote {} bytes to {} ({from} -> {to})",
            output_bytes.len(),
            args.output
        );
    } else {
        println!("Written to {}", args.output);
    }

    Ok(())
}
