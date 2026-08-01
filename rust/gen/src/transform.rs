// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Byte-level transformations applied to a supplied ROM image.
//!
//! A ROM image as distributed does not always have its bytes arranged the way
//! the target chip needs them.  The two cases One ROM meets in practice are
//! 16-bit wide parts whose image was produced with the opposite byte order
//! (e.g. a 27C400 image assembled big-endian), and wide images that interleave
//! several narrower devices — a 32-bit ROM set distributed as one file, from
//! which a single 8- or 16-bit lane has to be extracted.
//!
//! [`Transform`] describes one such operation.  A chip config carries an
//! ordered list of them (`transform` in a config file, `transform=` in a CLI
//! `--slot`), applied left to right.
//!
//! ## Where transforms run
//!
//! Transforms run **after** any [`Location`](crate::Location) slice and
//! **before** the image is reconciled against the chip size by
//! [`SizeHandling`].  That ordering is deliberate:
//!
//! - After the location slice, so `start`/`length` stay expressed against the
//!   file as the user sees it rather than against a rearranged image.
//! - Before size handling, so padding and duplication operate on the final
//!   byte order rather than the transform operating on filler bytes.
//!
//! For an Intel HEX source the file is decoded to a flat binary first, so
//! transforms behave identically whichever format the image arrived in.
//!
//! ## Order is significant
//!
//! The list is applied in order, and the order changes the result.  Extracting
//! the upper half of each 32-bit word and *then* swapping its byte pairs is a
//! different image from swapping first and then extracting.  The common 68k
//! case is `deinterleave` followed by `swap_bytes`.
//!
//! ## Text encoding
//!
//! The same textual form is used by the CLI `--slot transform=` key, the
//! standalone `onerom image` subcommands, and the provenance string recorded
//! in firmware metadata.  Members of a list are joined with `+`; parameters
//! follow a `:` and are separated by `/`.
//!
//! | Text                              | Meaning                                        |
//! |-----------------------------------|------------------------------------------------|
//! | `swap_bytes`                      | Reverse the byte pairs of each 16-bit word.     |
//! | `deinterleave:<offset>/<stride>`  | Keep lane `offset` of `stride` byte-wide lanes. |
//! | `deinterleave:<offset>/<stride>/<bytes>` | As above, with lanes `bytes` wide.       |
//! | `deinterleave:1/2/2+swap_bytes`   | Upper 16 bits of each 32-bit word, byte-swapped. |
//!
//! Each name has aliases, accepted identically by the CLI and by a config
//! file: `swap_bytes` also takes `swap-bytes`/`swapbytes`, and `deinterleave`
//! also takes `de_interleave`/`de-interleave`/`deint`.  Whichever spelling is
//! written, the canonical one is what [`Transform`]'s
//! [`Display`](core::fmt::Display) writes back out, so the string recorded in
//! metadata is stable.
//!
//! In a config file the structured JSON form is used instead, and a
//! parameterless transform is a bare string:
//!
//! ```json
//! "transform": ["swap_bytes"]
//! "transform": [{ "deinterleave": { "offset": 1, "stride": 2, "bytes": 2 } }, "swap_bytes"]
//! ```
//!
//! ## Why the deinterleave parameters are positional
//!
//! `offset` selects *which lane*, not a named half: there is deliberately no
//! `high`/`low` spelling.  Which half of a 32-bit word `offset = 0` yields
//! depends on the byte order of the source image, which the generator cannot
//! know.  Byte order is [`Transform::SwapBytes`]'s job, and keeping the two
//! separate is what lets them compose.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::SizeHandling;

/// Default lane width: a single byte.
const DEFAULT_BYTES: usize = 1;

fn default_bytes() -> usize {
    DEFAULT_BYTES
}

/// Accepted spellings of [`Transform::SwapBytes`], canonical first.
///
/// The serde aliases on the variant itself must match this list, so that a
/// config file accepts exactly what the CLI does.
const SWAP_BYTES_NAMES: &[&str] = &["swap_bytes", "swap-bytes", "swapbytes"];

/// Accepted spellings of [`Transform::Deinterleave`], canonical first.
const DEINTERLEAVE_NAMES: &[&str] = &["deinterleave", "de_interleave", "de-interleave", "deint"];

/// Separator between members of a transform list in its text encoding.
///
/// `+` rather than `,` because a transform list has to survive inside the
/// CLI's comma-separated `--slot` specification.
pub const TRANSFORM_LIST_SEPARATOR: &str = "+";

/// The result of applying one or more [`Transform`]s to an image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Transformed {
    /// The transformed image.
    pub data: Vec<u8>,

    /// Whether the chip's [`SizeHandling`] was needed to make an odd-length
    /// image even for [`Transform::SwapBytes`].
    ///
    /// The caller needs this to tell apart two situations that otherwise look
    /// identical once the transform has run: a size handling the user set
    /// purely so the transform could resolve an odd byte, and one that is
    /// genuinely redundant because the image was already the right size.  Only
    /// the latter should be reported back as an unnecessary setting.
    pub used_size_handling: bool,
}

/// A single byte-level transformation of a supplied ROM image.
///
/// See the [module documentation](self) for where transforms run in the image
/// pipeline, why order matters, and the text encoding used by the CLI and by
/// firmware metadata.
///
/// This enum is `#[non_exhaustive]`: it is expected to grow (bit-order
/// reversal and inversion are the obvious candidates), and adding a variant
/// should not be a breaking change for downstream crates.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Transform {
    /// Reverse the byte order within each 16-bit word throughout the image.
    ///
    /// Required for 16-bit wide parts (e.g. 27C400) when the source image was
    /// produced with the opposite byte order to the one One ROM serves.
    ///
    /// The image must have an even length.  If it does not, the chip's
    /// [`SizeHandling`] decides: `pad` appends one blank byte, `truncate`
    /// drops the trailing byte, and `none`/`duplicate` report
    /// [`TransformError::OddLength`].
    #[serde(alias = "swap-bytes", alias = "swapbytes")]
    SwapBytes,

    /// Extract one lane from an interleaved image.
    ///
    /// The image contains `stride` interleaved lanes of `bytes` bytes each;
    /// lane `offset` is kept and the rest discarded.  The result is `1/stride`
    /// of the input length.
    ///
    /// | Source             | Wanted            | `offset` | `stride` | `bytes` |
    /// |--------------------|-------------------|----------|----------|---------|
    /// | 16-bit interleaved | even / odd bytes  | 0 / 1    | 2        | 1       |
    /// | 32-bit interleaved | byte *n* of 4     | 0–3      | 4        | 1       |
    /// | 32-bit interleaved | one 16-bit half   | 0 / 1    | 2        | 2       |
    ///
    /// The image length must be a multiple of `bytes * stride` (one full set
    /// of lanes); a ragged tail means the image is not the interleaved set it
    /// was taken for, so it is reported rather than silently dropped.
    #[serde(alias = "de_interleave", alias = "de-interleave", alias = "deint")]
    Deinterleave {
        /// Which lane to keep.  Must be less than `stride`.
        offset: usize,

        /// How many lanes the image interleaves.  Must be at least 2.
        stride: usize,

        /// Width of one lane, in bytes.  Defaults to 1.
        ///
        /// This is the width of the device the lane feeds: 1 for an 8-bit
        /// part, 2 to keep 16-bit words intact.
        #[serde(default = "default_bytes", alias = "unit")]
        bytes: usize,
    },
}

impl Transform {
    /// The text forms accepted by [`Transform::try_from_str`], for use in
    /// error messages listing what is supported.
    pub fn supported_forms() -> &'static [&'static str] {
        &["swap_bytes", "deinterleave:<offset>/<stride>[/<bytes>]"]
    }

    /// Validates this transform's parameters.
    ///
    /// Called before any image data is touched, so a misconfigured transform
    /// is reported without depending on the image that would have been fed to
    /// it.
    pub fn validate(&self) -> Result<(), TransformError> {
        match self {
            Transform::SwapBytes => Ok(()),
            Transform::Deinterleave {
                offset,
                stride,
                bytes,
            } => {
                if *bytes == 0 {
                    return Err(TransformError::InvalidBytes { bytes: *bytes });
                }
                if *stride < 2 {
                    return Err(TransformError::InvalidStride { stride: *stride });
                }
                if offset >= stride {
                    return Err(TransformError::InvalidOffset {
                        offset: *offset,
                        stride: *stride,
                    });
                }
                bytes
                    .checked_mul(*stride)
                    .ok_or(TransformError::GroupOverflow {
                        bytes: *bytes,
                        stride: *stride,
                    })?;
                Ok(())
            }
        }
    }

    /// Applies this transform to `data`, returning the transformed image.
    ///
    /// `size_handling` and `blank_byte` are consulted only to resolve an
    /// odd-length image ahead of [`Transform::SwapBytes`]; see that variant's
    /// documentation.  A caller with no chip size to reconcile against — the
    /// standalone `onerom image` subcommands — passes [`SizeHandling::None`],
    /// which makes an odd-length image an error.
    pub fn apply(
        &self,
        data: &[u8],
        size_handling: &SizeHandling,
        blank_byte: u8,
    ) -> Result<Transformed, TransformError> {
        self.validate()?;
        match self {
            Transform::SwapBytes => {
                let (data, used_size_handling) = swap_bytes(data, size_handling, blank_byte)?;
                Ok(Transformed {
                    data,
                    used_size_handling,
                })
            }
            Transform::Deinterleave {
                offset,
                stride,
                bytes,
            } => Ok(Transformed {
                data: deinterleave(data, *offset, *stride, *bytes)?,
                used_size_handling: false,
            }),
        }
    }

    /// Parses a single transform from its text encoding.
    ///
    /// See the [module documentation](self) for the accepted forms.
    pub fn try_from_str(s: &str) -> Result<Self, TransformError> {
        let (name, params) = match s.split_once(':') {
            Some((name, params)) => (name.trim(), Some(params.trim())),
            None => (s.trim(), None),
        };

        match name {
            s if SWAP_BYTES_NAMES.contains(&s) => match params {
                None => Ok(Transform::SwapBytes),
                Some(_) => Err(TransformError::UnexpectedParameters {
                    name: name.to_owned(),
                }),
            },
            s if DEINTERLEAVE_NAMES.contains(&s) => match params {
                Some(params) => parse_deinterleave(params),
                None => Err(TransformError::MissingParameters {
                    name: name.to_owned(),
                }),
            },
            _ => Err(TransformError::UnknownTransform {
                name: name.to_owned(),
            }),
        }
    }
}

impl core::fmt::Display for Transform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Transform::SwapBytes => write!(f, "swap_bytes"),
            Transform::Deinterleave {
                offset,
                stride,
                bytes,
            } => {
                // The bytes is omitted when it is the default, so the common
                // byte-wise forms stay short and round-trip unchanged.
                if *bytes == DEFAULT_BYTES {
                    write!(f, "deinterleave:{offset}/{stride}")
                } else {
                    write!(f, "deinterleave:{offset}/{stride}/{bytes}")
                }
            }
        }
    }
}

/// Parses a `+`-separated list of transforms from its text encoding.
///
/// An empty (or all-whitespace) string yields an empty list.  Order is
/// preserved: the transforms are applied left to right.
pub fn parse_transform_list(s: &str) -> Result<Vec<Transform>, TransformError> {
    if s.trim().is_empty() {
        return Ok(Vec::new());
    }
    s.split(TRANSFORM_LIST_SEPARATOR)
        .map(Transform::try_from_str)
        .collect()
}

/// Formats a list of transforms in the text encoding accepted by
/// [`parse_transform_list`].
///
/// Used for the CLI and for the provenance string recorded in firmware
/// metadata, so a built image says how its ROM data was derived.
pub fn format_transform_list(transforms: &[Transform]) -> String {
    transforms
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(TRANSFORM_LIST_SEPARATOR)
}

/// Applies a list of transforms in order, returning the transformed image.
///
/// Each transform sees the output of the one before it, so validation (an even
/// length for [`Transform::SwapBytes`], a whole number of groups for
/// [`Transform::Deinterleave`]) applies to the intermediate image at that point
/// in the chain, not to the original.
pub fn apply_transforms(
    data: &[u8],
    transforms: &[Transform],
    size_handling: &SizeHandling,
    blank_byte: u8,
) -> Result<Transformed, TransformError> {
    let mut current = Transformed {
        data: data.to_vec(),
        used_size_handling: false,
    };
    for transform in transforms {
        let next = transform.apply(&current.data, size_handling, blank_byte)?;
        current = Transformed {
            data: next.data,
            used_size_handling: current.used_size_handling || next.used_size_handling,
        };
    }
    Ok(current)
}

/// Reverses the byte order within each 16-bit word.
///
/// Returns the transformed image and whether the chip's [`SizeHandling`] was
/// needed to make the length even.
fn swap_bytes(
    data: &[u8],
    size_handling: &SizeHandling,
    blank_byte: u8,
) -> Result<(Vec<u8>, bool), TransformError> {
    // An odd-length image cannot be split into 16-bit words.  Where the chip
    // config says how to reconcile a mismatched size, honour it; otherwise
    // report it rather than guessing.
    let padded: Vec<u8>;
    let mut used_size_handling = false;
    let data: &[u8] = if data.len().is_multiple_of(2) {
        data
    } else {
        used_size_handling = true;
        match size_handling {
            SizeHandling::Pad => {
                let mut v = data.to_vec();
                v.push(blank_byte);
                padded = v;
                &padded
            }
            SizeHandling::Truncate => &data[..data.len() - 1],
            SizeHandling::None | SizeHandling::Duplicate => {
                return Err(TransformError::OddLength { len: data.len() });
            }
        }
    };

    Ok((
        data.chunks_exact(2).flat_map(|w| [w[1], w[0]]).collect(),
        used_size_handling,
    ))
}

/// Keeps lane `offset` of the `stride` lanes, each `bytes` wide.
fn deinterleave(
    data: &[u8],
    offset: usize,
    stride: usize,
    bytes: usize,
) -> Result<Vec<u8>, TransformError> {
    // Parameters are validated by the caller (Transform::apply), so the group
    // size cannot overflow and offset is within stride.
    let group = bytes * stride;

    if !data.len().is_multiple_of(group) {
        return Err(TransformError::Ragged {
            len: data.len(),
            group,
        });
    }

    let mut out = Vec::with_capacity(data.len() / stride);
    let mut pos = offset * bytes;
    while pos < data.len() {
        out.extend_from_slice(&data[pos..pos + bytes]);
        pos += group;
    }

    Ok(out)
}

/// Parses the parameters of a `deinterleave` transform: `<offset>/<stride>`
/// or `<offset>/<stride>/<bytes>`.
fn parse_deinterleave(params: &str) -> Result<Transform, TransformError> {
    let bad = |reason: &str| TransformError::BadParameters {
        params: params.to_owned(),
        reason: reason.to_string(),
    };

    let mut parts = params.split('/');
    let offset = parts.next().unwrap_or_default().trim();
    let stride = parts
        .next()
        .ok_or_else(|| bad("expected <offset>/<stride>"))?;
    let bytes = parts.next();
    if parts.next().is_some() {
        return Err(bad("too many values, expected <offset>/<stride>[/<bytes>]"));
    }

    let number = |value: &str, what: &str| -> Result<usize, TransformError> {
        value
            .trim()
            .parse::<usize>()
            .map_err(|_| bad(&format!("{what} '{}' is not a number", value.trim())))
    };

    let transform = Transform::Deinterleave {
        offset: number(offset, "offset")?,
        stride: number(stride, "stride")?,
        bytes: match bytes {
            Some(bytes) => number(bytes, "bytes")?,
            None => DEFAULT_BYTES,
        },
    };
    transform.validate()?;

    Ok(transform)
}

/// Error returned when parsing or applying a [`Transform`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum TransformError {
    /// The transform name is not recognised.
    UnknownTransform { name: String },

    /// Parameters were supplied to a transform that takes none.
    UnexpectedParameters { name: String },

    /// A transform that requires parameters was given none.
    MissingParameters { name: String },

    /// A transform's parameters could not be parsed.
    BadParameters { params: String, reason: String },

    /// `stride` was less than 2, so the transform would keep everything (or
    /// nothing) rather than deinterleaving.
    InvalidStride { stride: usize },

    /// `offset` was not less than `stride`, so it selects no lane.
    InvalidOffset { offset: usize, stride: usize },

    /// The lane width was zero, so the transform would select nothing.
    InvalidBytes { bytes: usize },

    /// `bytes * stride` overflowed.
    GroupOverflow { bytes: usize, stride: usize },

    /// `swap_bytes` was applied to an odd-length image and the chip's size
    /// handling gave no way to resolve it.
    OddLength { len: usize },

    /// `deinterleave` was applied to an image whose length is not a whole
    /// number of groups.
    Ragged { len: usize, group: usize },
}

impl core::fmt::Display for TransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TransformError::UnknownTransform { name } => write!(
                f,
                "unknown transform '{name}'\n  Supported: {}",
                Transform::supported_forms().join(", ")
            ),
            TransformError::UnexpectedParameters { name } => {
                write!(f, "transform '{name}' does not take parameters")
            }
            TransformError::MissingParameters { name } => write!(
                f,
                "transform '{name}' requires parameters, e.g. 'deinterleave:1/2'"
            ),
            TransformError::BadParameters { params, reason } => {
                write!(f, "invalid transform parameters '{params}': {reason}")
            }
            TransformError::InvalidStride { stride } => {
                write!(f, "deinterleave stride must be at least 2, got {stride}")
            }
            TransformError::InvalidOffset { offset, stride } => write!(
                f,
                "deinterleave offset must be less than the stride, got offset {offset} with stride {stride}"
            ),
            TransformError::InvalidBytes { bytes } => {
                write!(
                    f,
                    "deinterleave lane width must be at least 1 byte, got {bytes}"
                )
            }
            TransformError::GroupOverflow { bytes, stride } => write!(
                f,
                "deinterleave lane width {bytes} multiplied by stride {stride} overflows"
            ),
            TransformError::OddLength { len } => write!(
                f,
                "swap_bytes requires an even-length image, got {len} bytes.\n  Use size_handling pad or truncate to resolve the odd byte."
            ),
            TransformError::Ragged { len, group } => write!(
                f,
                "deinterleave requires the image length to be a multiple of {group} bytes (lane width x stride), got {len} bytes"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Two 32-bit words, each byte distinct so any rearrangement is visible.
    const W32: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

    fn deint(data: &[u8], offset: usize, stride: usize, bytes: usize) -> Vec<u8> {
        Transform::Deinterleave {
            offset,
            stride,
            bytes,
        }
        .apply(data, &SizeHandling::None, PAD)
        .unwrap()
        .data
    }

    const PAD: u8 = 0xAA;

    fn swap(data: &[u8]) -> Vec<u8> {
        Transform::SwapBytes
            .apply(data, &SizeHandling::None, PAD)
            .unwrap()
            .data
    }

    //
    // swap_bytes
    //

    #[test]
    fn swap_bytes_reverses_each_word() {
        assert_eq!(
            swap(&[0x12, 0x34, 0x56, 0x78]),
            vec![0x34, 0x12, 0x78, 0x56]
        );
    }

    #[test]
    fn swap_bytes_is_its_own_inverse() {
        assert_eq!(swap(&swap(&W32)), W32.to_vec());
    }

    #[test]
    fn swap_bytes_accepts_an_empty_image() {
        assert_eq!(swap(&[]), Vec::<u8>::new());
    }

    #[test]
    fn swap_bytes_rejects_odd_length_without_size_handling() {
        for handling in [SizeHandling::None, SizeHandling::Duplicate] {
            assert_eq!(
                Transform::SwapBytes.apply(&[1, 2, 3], &handling, PAD),
                Err(TransformError::OddLength { len: 3 })
            );
        }
    }

    #[test]
    fn swap_bytes_pads_odd_length_with_the_blank_byte() {
        // Pad appends one blank byte, which then becomes the low byte of the
        // final word.  0xFF is used here to stand in for an ihex image.
        assert_eq!(
            Transform::SwapBytes
                .apply(&[1, 2, 3], &SizeHandling::Pad, 0xFF)
                .unwrap()
                .data,
            vec![2, 1, 0xFF, 3]
        );
        assert_eq!(
            Transform::SwapBytes
                .apply(&[1, 2, 3], &SizeHandling::Pad, PAD)
                .unwrap()
                .data,
            vec![2, 1, PAD, 3]
        );
    }

    #[test]
    fn swap_bytes_truncates_odd_length_when_asked() {
        assert_eq!(
            Transform::SwapBytes
                .apply(&[1, 2, 3], &SizeHandling::Truncate, PAD)
                .unwrap()
                .data,
            vec![2, 1]
        );
    }

    //
    // deinterleave
    //

    #[test]
    fn deinterleave_16_bit_source_to_bytes() {
        assert_eq!(deint(&W32, 0, 2, 1), vec![0, 2, 4, 6]);
        assert_eq!(deint(&W32, 1, 2, 1), vec![1, 3, 5, 7]);
    }

    #[test]
    fn deinterleave_32_bit_source_to_bytes() {
        assert_eq!(deint(&W32, 0, 4, 1), vec![0, 4]);
        assert_eq!(deint(&W32, 1, 4, 1), vec![1, 5]);
        assert_eq!(deint(&W32, 2, 4, 1), vec![2, 6]);
        assert_eq!(deint(&W32, 3, 4, 1), vec![3, 7]);
    }

    #[test]
    fn deinterleave_32_bit_source_to_16_bit_halves() {
        assert_eq!(deint(&W32, 0, 2, 2), vec![0, 1, 4, 5]);
        assert_eq!(deint(&W32, 1, 2, 2), vec![2, 3, 6, 7]);
    }

    #[test]
    fn deinterleave_unit_selects_whole_groups_of_bytes() {
        // A bytes larger than one byte is what removes any need for an
        // offset *set*: taking a 16-bit half of a 32-bit word keeps the byte
        // pair together, which repeated single-byte selections could not
        // express in one operation.
        let expected: Vec<u8> = W32
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 4 < 2)
            .map(|(_, b)| *b)
            .collect();
        assert_eq!(deint(&W32, 0, 2, 2), expected);
    }

    #[test]
    fn deinterleave_extracts_a_16_bit_lane_from_a_64_bit_image() {
        let data: Vec<u8> = (0..16).collect();
        assert_eq!(deint(&data, 2, 4, 2), vec![4, 5, 12, 13]);
    }

    #[test]
    fn deinterleave_rejects_a_ragged_image() {
        assert_eq!(
            Transform::Deinterleave {
                offset: 0,
                stride: 4,
                bytes: 1
            }
            .apply(&[0, 1, 2, 3, 4], &SizeHandling::None, PAD),
            Err(TransformError::Ragged { len: 5, group: 4 })
        );
    }

    #[test]
    fn deinterleave_rejects_invalid_parameters() {
        let cases = [
            (0, 1, 1, TransformError::InvalidStride { stride: 1 }),
            (0, 0, 1, TransformError::InvalidStride { stride: 0 }),
            (
                2,
                2,
                1,
                TransformError::InvalidOffset {
                    offset: 2,
                    stride: 2,
                },
            ),
            (0, 2, 0, TransformError::InvalidBytes { bytes: 0 }),
        ];
        for (offset, stride, bytes, expected) in cases {
            assert_eq!(
                Transform::Deinterleave {
                    offset,
                    stride,
                    bytes
                }
                .apply(&W32, &SizeHandling::None, PAD),
                Err(expected)
            );
        }
    }

    //
    // Ordering
    //

    fn chain(spec: &str) -> Vec<u8> {
        apply_transforms(&W32, &parsed(spec), &SizeHandling::None, PAD)
            .unwrap()
            .data
    }

    #[test]
    fn byte_wise_deinterleave_does_not_commute_with_swap_bytes() {
        // Taking every other byte and then swapping pairs is a different
        // image from swapping first and then taking every other byte: the
        // swap moves bytes across the boundary the deinterleave selects on.
        let deint_then_swap = chain("deinterleave:0/2+swap_bytes");
        let swap_then_deint = chain("swap_bytes+deinterleave:0/2");

        assert_eq!(deint_then_swap, vec![2, 0, 6, 4]);
        assert_eq!(swap_then_deint, vec![1, 3, 5, 7]);
        assert_ne!(deint_then_swap, swap_then_deint);
    }

    #[test]
    fn word_aligned_deinterleave_commutes_with_swap_bytes() {
        // A 16-bit bytes selects whole words, so it never splits a pair that
        // swap_bytes acts on and the two orders agree.  Worth pinning: it is
        // why the common 68k recipe survives being written either way round.
        assert_eq!(chain("deinterleave:1/2/2+swap_bytes"), vec![3, 2, 7, 6]);
        assert_eq!(chain("swap_bytes+deinterleave:1/2/2"), vec![3, 2, 7, 6]);
    }

    #[test]
    fn empty_transform_list_leaves_the_image_alone() {
        assert_eq!(
            apply_transforms(&W32, &[], &SizeHandling::None, PAD)
                .unwrap()
                .data,
            W32.to_vec()
        );
    }

    #[test]
    fn later_transforms_see_the_earlier_output() {
        // A deinterleave that leaves an odd number of bytes makes the
        // following swap_bytes fail, proving the chain is sequential.
        let data: Vec<u8> = (0..15).collect();
        assert_eq!(
            apply_transforms(
                &data,
                &parsed("deinterleave:0/3+swap_bytes"),
                &SizeHandling::None,
                PAD
            ),
            Err(TransformError::OddLength { len: 5 })
        );
    }

    //
    // Text encoding
    //

    fn parsed(s: &str) -> Vec<Transform> {
        parse_transform_list(s).unwrap()
    }

    #[test]
    fn parses_a_single_transform() {
        assert_eq!(parsed("swap_bytes"), vec![Transform::SwapBytes]);
        assert_eq!(
            parsed("deinterleave:1/2"),
            vec![Transform::Deinterleave {
                offset: 1,
                stride: 2,
                bytes: 1
            }]
        );
        assert_eq!(
            parsed("deinterleave:1/2/2"),
            vec![Transform::Deinterleave {
                offset: 1,
                stride: 2,
                bytes: 2
            }]
        );
    }

    #[test]
    fn parses_a_list_preserving_order() {
        assert_eq!(
            parsed("deinterleave:1/2/2+swap_bytes"),
            vec![
                Transform::Deinterleave {
                    offset: 1,
                    stride: 2,
                    bytes: 2
                },
                Transform::SwapBytes,
            ]
        );
    }

    #[test]
    fn parses_an_empty_list() {
        assert!(parsed("").is_empty());
        assert!(parsed("   ").is_empty());
    }

    #[test]
    fn text_encoding_round_trips() {
        for s in [
            "swap_bytes",
            "deinterleave:1/2",
            "deinterleave:3/4/2",
            "deinterleave:1/2/2+swap_bytes",
        ] {
            assert_eq!(format_transform_list(&parsed(s)), s);
        }
    }

    #[test]
    fn default_unit_is_omitted_when_formatting() {
        assert_eq!(
            Transform::Deinterleave {
                offset: 1,
                stride: 2,
                bytes: 1
            }
            .to_string(),
            "deinterleave:1/2"
        );
    }

    #[test]
    fn rejects_bad_text() {
        assert!(matches!(
            Transform::try_from_str("nonsense"),
            Err(TransformError::UnknownTransform { .. })
        ));
        assert!(matches!(
            Transform::try_from_str("swap_bytes:1/2"),
            Err(TransformError::UnexpectedParameters { .. })
        ));
        assert!(matches!(
            Transform::try_from_str("deinterleave"),
            Err(TransformError::MissingParameters { .. })
        ));
        assert!(matches!(
            Transform::try_from_str("deinterleave:1"),
            Err(TransformError::BadParameters { .. })
        ));
        assert!(matches!(
            Transform::try_from_str("deinterleave:1/2/3/4"),
            Err(TransformError::BadParameters { .. })
        ));
        assert!(matches!(
            Transform::try_from_str("deinterleave:x/2"),
            Err(TransformError::BadParameters { .. })
        ));
        // Parameter validation happens at parse time, not just at apply time.
        assert!(matches!(
            Transform::try_from_str("deinterleave:2/2"),
            Err(TransformError::InvalidOffset { .. })
        ));
    }

    /// Every accepted spelling of a transform name, in the text encoding.
    ///
    /// The CLI and a config file must agree on these, so the same list drives
    /// the serde test below.
    const SWAP_SPELLINGS: &[&str] = &["swap_bytes", "swap-bytes", "swapbytes"];
    const DEINTERLEAVE_SPELLINGS: &[&str] =
        &["deinterleave", "de_interleave", "de-interleave", "deint"];

    #[test]
    fn text_accepts_every_swap_bytes_spelling() {
        for spelling in SWAP_SPELLINGS {
            assert_eq!(
                Transform::try_from_str(spelling),
                Ok(Transform::SwapBytes),
                "text spelling {spelling:?} not accepted"
            );
        }
    }

    #[test]
    fn text_accepts_every_deinterleave_spelling() {
        for spelling in DEINTERLEAVE_SPELLINGS {
            assert_eq!(
                Transform::try_from_str(&alloc::format!("{spelling}:1/2")),
                Ok(Transform::Deinterleave {
                    offset: 1,
                    stride: 2,
                    bytes: 1
                }),
                "text spelling {spelling:?} not accepted"
            );
        }
    }

    #[test]
    fn spellings_normalise_when_formatted() {
        // Whichever spelling was written, one canonical form comes back out,
        // so the metadata provenance string is stable.
        for spelling in DEINTERLEAVE_SPELLINGS {
            let parsed = Transform::try_from_str(&alloc::format!("{spelling}:1/2")).unwrap();
            assert_eq!(parsed.to_string(), "deinterleave:1/2");
        }
        for spelling in SWAP_SPELLINGS {
            let parsed = Transform::try_from_str(spelling).unwrap();
            assert_eq!(parsed.to_string(), "swap_bytes");
        }
    }

    //
    // Serde
    //

    #[test]
    fn serde_accepts_every_swap_bytes_spelling() {
        for spelling in SWAP_SPELLINGS {
            let json = alloc::format!("\"{spelling}\"");
            assert_eq!(
                serde_json::from_str::<Transform>(&json).ok(),
                Some(Transform::SwapBytes),
                "config spelling {spelling:?} not accepted"
            );
        }
    }

    #[test]
    fn serde_accepts_every_deinterleave_spelling() {
        for spelling in DEINTERLEAVE_SPELLINGS {
            let json = alloc::format!("{{\"{spelling}\":{{\"offset\":1,\"stride\":2}}}}");
            assert_eq!(
                serde_json::from_str::<Transform>(&json).ok(),
                Some(Transform::Deinterleave {
                    offset: 1,
                    stride: 2,
                    bytes: 1
                }),
                "config spelling {spelling:?} not accepted"
            );
        }
    }

    #[test]
    fn parameterless_transform_serialises_as_a_bare_string() {
        let json = serde_json::to_string(&Transform::SwapBytes).unwrap();
        assert_eq!(json, "\"swap_bytes\"");
        assert_eq!(
            serde_json::from_str::<Transform>(&json).unwrap(),
            Transform::SwapBytes
        );
    }

    #[test]
    fn parameterised_transform_serialises_as_a_map() {
        let transform = Transform::Deinterleave {
            offset: 1,
            stride: 2,
            bytes: 2,
        };
        let json = serde_json::to_string(&transform).unwrap();
        assert_eq!(
            json,
            r#"{"deinterleave":{"offset":1,"stride":2,"bytes":2}}"#
        );
        assert_eq!(serde_json::from_str::<Transform>(&json).unwrap(), transform);
    }

    #[test]
    fn deserialised_unit_defaults_to_one() {
        let transform: Transform =
            serde_json::from_str(r#"{"deinterleave":{"offset":1,"stride":2}}"#).unwrap();
        assert_eq!(
            transform,
            Transform::Deinterleave {
                offset: 1,
                stride: 2,
                bytes: 1
            }
        );
    }
}
