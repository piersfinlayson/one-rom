// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Which byte order a 16-bit ROM image is stored in.
//!
//! One ROM serves a 16-bit part low byte of each word first. A 68000 image as
//! normally distributed stores the high byte first, so it needs a `swap_bytes`
//! transform before it will serve. Getting that wrong produces a machine that
//! comes up dead with nothing to say why.
//!
//! # Two independent checks
//!
//! **The header word.** An image's first 16-bit word is looked up against a
//! table of known headers, held as a 68000 stores them. A file whose first word
//! equals an entry is high byte first, and one whose first word is that entry
//! reversed is low byte first. The Amiga's `0x1111` reads the same both ways,
//! so it recognises the image without saying which way round it is.
//!
//! **The address table.** The longwords after such a header are ROM addresses
//! in the `0x00F0_0000` range, so the top byte of each is zero. High byte first
//! puts that zero at an even offset, low byte first at an odd one.
//!
//! The second runs only where the first recognised something. Ungated it
//! answers for images that have no byte order at all - an Atari ST's ROM pair
//! is two 8-bit-wide parts holding alternate bytes, and one of those files
//! satisfies the even-offset pattern while containing no 16-bit word.
//!
//! Where both speak they each make a [`Claim`], and the caller of
//! [`identify`](super::identify) gets `Agreed` or `Disputed` accordingly.

use alloc::vec::Vec;

use super::{Claim, Evidence};

/// Which end of each 16-bit word a ROM image stores first.
///
/// There are two orders and there will only ever be two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// High byte of each 16-bit word first, as a 68000 image is normally
    /// distributed.
    HighByteFirst,
    /// Low byte of each 16-bit word first, which is what One ROM serves.
    LowByteFirst,
}

impl ByteOrder {
    /// The order One ROM reads a 16-bit image in.
    ///
    /// An image in the other order is served intact, and the machine receives
    /// each pair of bytes the wrong way round.
    pub const ONE_ROM: Self = Self::LowByteFirst;
}

/// One recognised header word, held as a 68000 stores it.
struct Header {
    magic: u16,
    evidence: Evidence,
}

/// The recognised headers, each checked against a real image in both orders.
///
/// `0x4EF9` is a bare 68000 `JMP.L` rather than anything Amiga specific. It
/// lands at offset 0 in the low half of an Amiga 32-bit ROM pair, whose first
/// word is the header's second word, and that half has nothing more
/// distinctive in it.
const HEADERS: &[Header] = &[
    Header {
        magic: 0x1114,
        evidence: Evidence::AmigaRomHeader,
    },
    Header {
        magic: 0x1111,
        evidence: Evidence::AmigaRomHeader,
    },
    Header {
        // Checked against one image, an Atari ST 520 STFM TOS dump.
        magic: 0x602E,
        evidence: Evidence::AtariTosHeader,
    },
    Header {
        magic: 0x4EF9,
        evidence: Evidence::M68kJumpOpcode,
    },
];

/// Offsets the address table check reads, and so the shortest image it judges.
const ADDRESS_TABLE_LEN: usize = 8;

/// The header entry whose magic matches, either way round.
fn header(image: &[u8]) -> Option<&'static Header> {
    let first = u16::from_be_bytes([*image.first()?, *image.get(1)?]);
    HEADERS
        .iter()
        .find(|h| first == h.magic || first == h.magic.swap_bytes())
}

/// What the header word says, where it is not the same read backwards.
fn header_claim(image: &[u8]) -> Option<Claim<ByteOrder>> {
    let header = header(image)?;
    if header.magic == header.magic.swap_bytes() {
        return None;
    }
    let first = u16::from_be_bytes([image[0], image[1]]);
    Some(Claim {
        value: if first == header.magic {
            ByteOrder::HighByteFirst
        } else {
            ByteOrder::LowByteFirst
        },
        evidence: header.evidence,
    })
}

/// What the ROM addresses after the header say.
///
/// Gated on a recognised header - see the module documentation for what it
/// answers without one.
fn address_table_claim(image: &[u8]) -> Option<Claim<ByteOrder>> {
    let header = header(image)?;
    if image.len() < ADDRESS_TABLE_LEN {
        return None;
    }
    let high_first = image[4] == 0 && image[6] == 0;
    let low_first = image[5] == 0 && image[7] == 0;
    let value = match (high_first, low_first) {
        (true, false) => ByteOrder::HighByteFirst,
        (false, true) => ByteOrder::LowByteFirst,
        (true, true) | (false, false) => return None,
    };
    // The header's evidence, not the address check's own: what a caller
    // needs to name is the image it recognised, and the addresses are only
    // readable as addresses because that header gated them.
    Some(Claim {
        value,
        evidence: header.evidence,
    })
}

/// Everything recognised in an image that bears on its byte order.
pub(super) fn claims(image: &[u8]) -> Vec<Claim<ByteOrder>> {
    [header_claim(image), address_table_claim(image)]
        .into_iter()
        .flatten()
        .collect()
}
