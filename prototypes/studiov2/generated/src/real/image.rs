// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The three image commands that run for real: a format conversion and two
//! transforms.
//!
//! Every byte of work is `onerom-gen`'s, and every failure is
//! `onerom-cli`'s [`Error`], so what reaches the pane is what the CLI itself
//! would print for the same input.  The only thing written here is the reading
//! and writing of the files, which `onerom-gen` deliberately leaves to its
//! caller — its codecs and transforms take a buffer.

use onerom_cli::Error;
use onerom_gen::{
    FileFormat, LoadAddress, SizeHandling, Transform, decode_ihex, decode_srec, encode_ihex,
    encode_srec,
};

use super::{Inputs, Need, Runner, Takes};
use crate::stub::Output;

/// Convert an image between the formats `onerom-gen` knows.
pub const CONVERT: Runner = Runner {
    needs: &[
        Need {
            long: "from",
            takes: Takes::OneOf,
        },
        Need {
            long: "to",
            takes: Takes::OneOf,
        },
        Need {
            long: "input",
            takes: Takes::Path,
        },
        Need {
            long: "output",
            takes: Takes::Path,
        },
        Need {
            long: "load-address",
            takes: Takes::Words,
        },
    ],
    does: "the format conversion in onerom-gen",
    work: convert,
};

/// Swap the bytes of every pair, for a 16-bit image stored the other way round.
pub const SWAP: Runner = Runner {
    needs: &[
        Need {
            long: "input",
            takes: Takes::Path,
        },
        Need {
            long: "output",
            takes: Takes::Path,
        },
    ],
    does: "the byte-swap transform in onerom-gen",
    work: swap,
};

/// Keep one lane of an interleaved image.
pub const DEINTERLEAVE: Runner = Runner {
    needs: &[
        Need {
            long: "input",
            takes: Takes::Path,
        },
        Need {
            long: "output",
            takes: Takes::Path,
        },
        Need {
            long: "offset",
            takes: Takes::Count,
        },
        Need {
            long: "stride",
            takes: Takes::Count,
        },
        Need {
            long: "bytes",
            takes: Takes::Count,
        },
    ],
    does: "the deinterleave transform in onerom-gen",
    work: deinterleave,
};

/// Decode to a flat binary and re-encode, the way the CLI does it.
fn convert(inputs: &Inputs) -> Result<Output, Error> {
    let input = inputs.text("input");
    let output = inputs.text("output");
    let from = format(inputs.text("from"), "--from")?;
    let to = format(inputs.text("to"), "--to")?;
    let load = address(inputs.text("load-address"))?;

    // A load address only means anything when a record-oriented format, whose
    // records carry addresses, is on one side.
    if from.is_binary() && to.is_binary() && !load.is_zero() {
        return Err(Error::InvalidArgument(
            "--load-address".to_owned(),
            "load address is only valid when converting to or from ihex or srec".to_owned(),
        ));
    }

    let data = std::fs::read(input).map_err(|error| Error::io(input, error))?;

    let decoded = |message: String| Error::ImageDecode {
        path: input.to_owned(),
        format: from,
        message,
    };
    let binary = match from {
        FileFormat::Binary => data,
        FileFormat::IntelHex => {
            decode_ihex(&data, load.0).map_err(|error| decoded(error.to_string()))?
        }
        FileFormat::Srec => {
            decode_srec(&data, load.0).map_err(|error| decoded(error.to_string()))?
        }
        _ => return Err(unsupported("--from", from)),
    };

    let bytes = match to {
        FileFormat::Binary => binary,
        FileFormat::IntelHex => encode_ihex(&binary, load.0).into_bytes(),
        FileFormat::Srec => encode_srec(&binary, load.0).into_bytes(),
        _ => return Err(unsupported("--to", to)),
    };

    write(output, &bytes)?;
    Ok(Output::Line(format!(
        "Wrote {} bytes to {output} ({from} -> {to})",
        bytes.len()
    )))
}

/// Swap every byte pair.
///
/// The odd length is caught before the transform so the message can name the
/// file, which is what the CLI does and why it reads the length first.
///
/// The CLI also says what the image looks like before it writes anything, so a
/// user swapping one that is already the right way round hears about it.  That
/// goes to stdout and has nowhere to go on a pane, so it is not done here.
fn swap(inputs: &Inputs) -> Result<Output, Error> {
    let input = inputs.text("input");
    let output = inputs.text("output");

    let length = std::fs::metadata(input)
        .map_err(|error| Error::io(input, error))?
        .len() as usize;
    if !length.is_multiple_of(2) {
        return Err(Error::OddLengthImage(input.to_owned(), length));
    }

    let bytes = transform(input, &Transform::SwapBytes)?;
    write(output, &bytes)?;
    Ok(Output::Line(format!(
        "Wrote {} bytes to {output} with byte pairs swapped",
        bytes.len()
    )))
}

/// Keep one lane out of every stride.
fn deinterleave(inputs: &Inputs) -> Result<Output, Error> {
    let input = inputs.text("input");
    let output = inputs.text("output");
    let offset = inputs.count("offset").unwrap_or_default();
    let stride = inputs.count("stride").unwrap_or_default();
    let lane = inputs.count("bytes").unwrap_or_default();

    let wanted = Transform::Deinterleave {
        offset,
        stride,
        bytes: lane,
    };

    // Bad parameters before the file is read, so a typo in --stride does not
    // depend on the image being readable.
    wanted.validate().map_err(|error| {
        Error::InvalidArgument("--stride/--offset/--bytes".to_owned(), error.to_string())
    })?;

    let bytes = transform(input, &wanted)?;
    write(output, &bytes)?;
    Ok(Output::Line(format!(
        "Wrote {} bytes to {output} keeping lane {offset} of {stride} ({lane} byte{} per lane)",
        bytes.len(),
        if lane == 1 { "" } else { "s" }
    )))
}

/// Reads an image and applies one transform to it.
///
/// [`SizeHandling::None`] is what makes a length the transform cannot handle an
/// error rather than something padded or truncated away, which is the whole
/// point of running these for real.
fn transform(input: &str, transform: &Transform) -> Result<Vec<u8>, Error> {
    let data = std::fs::read(input).map_err(|error| Error::io(input, error))?;
    Ok(transform
        .apply(&data, &SizeHandling::None, 0)
        .map_err(|error| Error::ImageTransform(input.to_owned(), error.to_string()))?
        .data)
}

/// Writes the answer out.
fn write(output: &str, bytes: &[u8]) -> Result<(), Error> {
    std::fs::write(output, bytes).map_err(|error| Error::io(output, error))
}

/// A format name as the description offered it.
///
/// The names come from `onerom-gen`'s own list, so one it does not know means
/// the two lists have moved apart rather than that a user typed something odd.
fn format(value: &str, arg: &str) -> Result<FileFormat, Error> {
    FileFormat::try_from_str(value).ok_or_else(|| {
        Error::InvalidArgument(arg.to_owned(), format!("'{value}' is not a known format"))
    })
}

/// The load address, which is zero where nothing was given.
fn address(value: &str) -> Result<LoadAddress, Error> {
    if value.is_empty() {
        return Ok(LoadAddress::default());
    }
    LoadAddress::parse_str(value)
        .map_err(|error| Error::InvalidArgument("--load-address".to_owned(), error.to_string()))
}

/// A format `onerom-gen` knows and this conversion has no arm for.
fn unsupported(arg: &str, format: FileFormat) -> Error {
    Error::InvalidArgument(
        arg.to_owned(),
        format!(
            "conversion is not implemented for {} images",
            format.display_name()
        ),
    )
}
