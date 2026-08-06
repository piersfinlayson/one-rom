// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom image` subcommands.

use crate::args::image::{ImageConvertArgs, ImageDeinterleaveArgs, ImageSwapBytesArgs};
use onerom_cli::{Error, Options};
use onerom_gen::{FileFormat, SizeHandling, Transform, decode_ihex, encode_ihex};

/// Apply a single transform to a standalone image file.
///
/// The transform itself comes from `onerom-gen`, so these subcommands and the
/// `transform=` slot key are the same operation and cannot drift apart.
/// [`SizeHandling::None`] is passed because a standalone image has no chip size
/// to reconcile against, which makes a length the transform cannot handle an
/// error rather than something to pad or truncate away.
fn transform_file(
    options: &Options,
    input: &str,
    output: &str,
    transform: &Transform,
    what: &str,
) -> Result<(), Error> {
    if options.verbose {
        println!("Reading ROM image from {input} ...");
    }
    let data = std::fs::read(input).map_err(|e| Error::io(input, e))?;

    let transformed = transform
        .apply(&data, &SizeHandling::None, 0)
        .map_err(|e| Error::ImageTransform(input.to_string(), e.to_string()))?
        .data;

    std::fs::write(output, &transformed).map_err(|e| Error::io(output, e))?;

    if options.verbose {
        println!("Wrote {} bytes to {output} {what}", transformed.len());
    } else {
        println!("Written to {output}");
    }

    Ok(())
}

pub async fn cmd_swap_bytes(options: &Options, args: &ImageSwapBytesArgs) -> Result<(), Error> {
    // Checked up front so the error can name the file, rather than reporting
    // the generic odd-length message the shared transform would produce.
    let len = std::fs::metadata(&args.input)
        .map_err(|e| Error::io(&args.input, e))?
        .len() as usize;
    if !len.is_multiple_of(2) {
        return Err(Error::OddLengthImage(args.input.clone(), len));
    }

    transform_file(
        options,
        &args.input,
        &args.output,
        &Transform::SwapBytes,
        "with byte pairs swapped",
    )
}

pub async fn cmd_deinterleave(
    options: &Options,
    args: &ImageDeinterleaveArgs,
) -> Result<(), Error> {
    let transform = Transform::Deinterleave {
        offset: args.offset,
        stride: args.stride,
        bytes: args.bytes,
    };

    // Report bad parameters before reading the file, so a typo in --stride
    // does not depend on the image being readable.
    transform.validate().map_err(|e| {
        Error::InvalidArgument("--stride/--offset/--bytes".to_string(), e.to_string())
    })?;

    let what = format!(
        "keeping lane {} of {} ({} byte{} per lane)",
        args.offset,
        args.stride,
        args.bytes,
        if args.bytes == 1 { "" } else { "s" }
    );
    transform_file(options, &args.input, &args.output, &transform, &what)
}

pub async fn cmd_convert(options: &Options, args: &ImageConvertArgs) -> Result<(), Error> {
    // --from/--to are validated by clap against onerom-gen's format list, and
    // --load-address by the same parser the config file uses, so both arrive
    // already parsed.
    let (from, to) = (args.from, args.to);
    let load_address = args.load_address.unwrap_or_default();

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
