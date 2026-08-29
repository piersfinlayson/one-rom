// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What can be worked out about a ROM image from its own bytes.
//!
//! Three nouns. An [`Evidence`] is something observed in the image. A [`Claim`]
//! is what one piece of evidence says about one question. A [`Conclusion`] is
//! what this module is prepared to assert, having weighed the claims.
//!
//! [`Identity`] holds one conclusion per question. Today the only question is
//! byte order.

pub mod byte_order;

use alloc::vec::Vec;

pub use byte_order::ByteOrder;

/// Something observed in a ROM image.
///
/// It becomes evidence only once attached to a [`Claim`], because what it says
/// depends on the question being asked of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Evidence {
    /// The header word Amiga ROMs start with, shared by Kickstart and DiagROM.
    /// It also opens the high half of a 32-bit Amiga ROM pair.
    AmigaRomHeader,
    /// The header word an Atari ST TOS image starts with.
    AtariTosHeader,
    /// A 68000 `JMP.L` opcode as the image's first word. This opens the low
    /// half of an Amiga 32-bit ROM pair, but it is a bare opcode and another
    /// 68000 image could legitimately start the same way.
    M68kJumpOpcode,
}

impl core::fmt::Display for Evidence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::AmigaRomHeader => "an Amiga ROM header",
            Self::AtariTosHeader => "an Atari ST TOS header",
            Self::M68kJumpOpcode => "a 68000 JMP.L instruction",
        })
    }
}

/// What one piece of evidence says about one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim<T> {
    /// What this evidence says the answer is.
    pub value: T,
    /// What was observed.
    pub evidence: Evidence,
}

/// What this module asserts about one question, having weighed the claims.
///
/// Only the enum is `#[non_exhaustive]`, not its variants. Marking the variants
/// too would stop a caller building a meaningless state - an `Agreed` with no
/// claims, or one whose claims contradict its value - at the price of a `..` in
/// every pattern every caller ever writes, forever, against a construction
/// nobody attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Conclusion<T> {
    /// Nothing bearing on the question was recognised.
    Unknown,
    /// Every claim agreed.
    Agreed {
        /// The answer every claim gave.
        value: T,
        /// The claims that gave it, never empty.
        claims: Vec<Claim<T>>,
    },
    /// Claims were found and they disagreed, so nothing is asserted.
    Disputed {
        /// The claims, in the order they were made.
        claims: Vec<Claim<T>>,
    },
}

impl<T: PartialEq> Conclusion<T> {
    /// Weigh a set of claims into a conclusion.
    fn weigh(claims: Vec<Claim<T>>) -> Self
    where
        T: Copy,
    {
        let Some(first) = claims.first().map(|c| c.value) else {
            return Self::Unknown;
        };
        if claims.iter().all(|c| c.value == first) {
            Self::Agreed {
                value: first,
                claims,
            }
        } else {
            Self::Disputed { claims }
        }
    }
}

impl<T> Conclusion<T> {
    /// The answer, where the claims agreed on one.
    ///
    /// The common question a caller has. `None` covers both nothing recognised
    /// and claims that disagreed - use [`claims`](Self::claims) to tell them
    /// apart, or match the variants.
    #[must_use]
    pub fn agreed(&self) -> Option<&T> {
        match self {
            Self::Agreed { value, .. } => Some(value),
            Self::Unknown | Self::Disputed { .. } => None,
        }
    }

    /// Everything found bearing on the question, whether or not it agreed.
    #[must_use]
    pub fn claims(&self) -> &[Claim<T>] {
        match self {
            Self::Unknown => &[],
            Self::Agreed { claims, .. } | Self::Disputed { claims } => claims,
        }
    }
}

/// What could be worked out about a ROM image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Identity {
    /// Which byte order a 16-bit image is stored in.
    pub byte_order: Conclusion<ByteOrder>,
}

/// Work out what can be worked out about a ROM image.
#[must_use]
pub fn identify(image: &[u8]) -> Identity {
    Identity {
        byte_order: Conclusion::weigh(byte_order::claims(image)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ByteOrder::{HighByteFirst, LowByteFirst};
    use alloc::format;
    use alloc::vec;

    // First eight bytes of real images, in the order the file holds them.
    const KS13: [u8; 8] = [0x11, 0x11, 0xf9, 0x4e, 0xfc, 0x00, 0xd2, 0x00];
    const DIAGROM: [u8; 8] = [0x11, 0x14, 0x44, 0x47, 0x00, 0xf8, 0x00, 0xd6];
    const DIAGROM_16BIT: [u8; 8] = [0x14, 0x11, 0x47, 0x44, 0xf8, 0x00, 0xd6, 0x00];
    const DIAGROM_32BIT_HI: [u8; 8] = [0x14, 0x11, 0xf8, 0x00, 0xf8, 0x00, 0xf8, 0x00];
    const DIAGROM_32BIT_LO: [u8; 8] = [0x47, 0x44, 0xd6, 0x00, 0xb4, 0x1a, 0xb4, 0x1a];
    const KS_32BIT_HI: [u8; 8] = [0x14, 0x11, 0xf8, 0x00, 0x00, 0x00, 0x2f, 0x00];
    const KS_32BIT_LO: [u8; 8] = [0xf9, 0x4e, 0xd2, 0x00, 0xff, 0xff, 0x60, 0x00];
    const TOS: [u8; 8] = [0x60, 0x2e, 0x01, 0x02, 0x00, 0xfc, 0x00, 0x30];
    /// One 8-bit-wide half of the ST pair, holding alternate bytes. It has no
    /// byte order, and satisfies the address table pattern by accident.
    const TOS_HALF: [u8; 8] = [0x60, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x89];

    fn swapped(data: &[u8; 8]) -> [u8; 8] {
        let mut out = *data;
        for i in (0..out.len()).step_by(2) {
            out.swap(i, i + 1);
        }
        out
    }

    #[track_caller]
    fn assert_agreed(image: &[u8], order: ByteOrder, evidence: &[Evidence]) {
        let c = identify(image).byte_order;
        assert_eq!(c.agreed(), Some(&order), "{c:?}");
        let found: Vec<Evidence> = c.claims().iter().map(|c| c.evidence).collect();
        assert_eq!(found, evidence, "{c:?}");
    }

    #[test]
    fn both_checks_agree_where_both_speak() {
        use Evidence::{AmigaRomHeader, AtariTosHeader};
        for (image, order) in [
            (&DIAGROM, HighByteFirst),
            (&DIAGROM_16BIT, LowByteFirst),
            (&DIAGROM_32BIT_HI, LowByteFirst),
            (&KS_32BIT_HI, LowByteFirst),
        ] {
            assert_agreed(image, order, &[AmigaRomHeader, AmigaRomHeader]);
        }
        assert_agreed(&TOS, HighByteFirst, &[AtariTosHeader, AtariTosHeader]);
        assert_agreed(
            &swapped(&TOS),
            LowByteFirst,
            &[AtariTosHeader, AtariTosHeader],
        );
    }

    #[test]
    fn a_palindromic_header_recognises_but_the_addresses_decide() {
        assert_agreed(&KS13, LowByteFirst, &[Evidence::AmigaRomHeader]);
        assert_agreed(&swapped(&KS13), HighByteFirst, &[Evidence::AmigaRomHeader]);
    }

    #[test]
    fn the_low_half_of_a_pair_gets_one_claim_or_none() {
        // A Kickstart low half opens with a JMP, and the addresses after it
        // are the low halves of longwords, so they say nothing.
        assert_agreed(&KS_32BIT_LO, LowByteFirst, &[Evidence::M68kJumpOpcode]);
        assert_agreed(
            &swapped(&KS_32BIT_LO),
            HighByteFirst,
            &[Evidence::M68kJumpOpcode],
        );
        // DiagROM's low half opens with the second word of its own header,
        // which is nothing we know.
        for image in [DIAGROM_32BIT_LO, swapped(&DIAGROM_32BIT_LO)] {
            assert_eq!(identify(&image).byte_order, Conclusion::Unknown);
        }
    }

    #[test]
    fn an_8_bit_wide_half_is_not_answered_for() {
        // The address table pattern holds, so the header gate is what keeps
        // this silent.
        for image in [TOS_HALF, swapped(&TOS_HALF)] {
            assert_eq!(identify(&image).byte_order, Conclusion::Unknown);
        }
    }

    #[test]
    fn nothing_recognised_is_unknown() {
        for image in [&[][..], &[0x11], &[0x11, 0x11], &[0xde, 0xad, 0xbe, 0xef]] {
            assert_eq!(identify(image).byte_order, Conclusion::Unknown);
        }
        // A recognised header whose addresses hold both patterns, and one that
        // holds neither.
        assert_eq!(
            identify(&[0x11, 0x11, 0xf9, 0x4e, 0, 0, 0, 0]).byte_order,
            Conclusion::Unknown
        );
        assert_eq!(
            identify(&[0x11, 0x11, 0xf9, 0x4e, 1, 2, 3, 4]).byte_order,
            Conclusion::Unknown
        );
    }

    #[test]
    fn disagreeing_claims_assert_nothing_and_keep_both() {
        let claims = vec![
            Claim {
                value: HighByteFirst,
                evidence: Evidence::AmigaRomHeader,
            },
            Claim {
                value: LowByteFirst,
                evidence: Evidence::AmigaRomHeader,
            },
        ];
        let c = Conclusion::weigh(claims.clone());
        assert_eq!(c.agreed(), None);
        assert_eq!(c.claims(), &claims[..]);
        assert!(matches!(c, Conclusion::Disputed { .. }));
    }

    #[test]
    fn one_rom_reads_the_low_byte_of_each_pair_first() {
        assert_eq!(ByteOrder::ONE_ROM, LowByteFirst);
    }

    #[test]
    fn evidence_renders_as_a_noun_phrase() {
        assert_eq!(
            format!("{}", Evidence::AmigaRomHeader),
            "an Amiga ROM header"
        );
        assert_eq!(
            format!("{}", Evidence::AtariTosHeader),
            "an Atari ST TOS header"
        );
        assert_eq!(
            format!("{}", Evidence::M68kJumpOpcode),
            "a 68000 JMP.L instruction"
        );
    }
}
