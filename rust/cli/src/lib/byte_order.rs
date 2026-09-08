// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reporting what `onerom-app` made of a 16-bit image's byte order.
//!
//! `onerom-app` states facts. This says what they mean for the user, which is
//! the split that crate draws for its own errors.
//!
//! Nothing is printed outside `--verbose` unless the order was identified *and*
//! it is wrong for what the caller is about to do. An unidentified image is a
//! verbose line, because a user who cannot act on it does not need it.

use onerom_app::identity::{ByteOrder, identify};
use onerom_config::chip::ChipType;
use onerom_gen::{Builder, Transform};

/// What the caller is about to do with the image, which decides the wording.
pub enum Intent {
    /// Write out a byte-swapped copy (`onerom image swap-bytes`).
    Swapping,
    /// Serve the image as it stands, with no `swap_bytes` applied.
    ServeAsIs,
    /// Serve it having applied `swap_bytes`.
    ServeSwapped,
}

/// The file's name as the user gave it, without its directory or URL path.
///
/// These lines print in a list of slots, where a full path drowns the sentence.
fn basename(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Report what the image's byte order means for what is about to happen to it.
///
/// Call only for a 16-bit-wide chip type.
/// One line of output, and which stream it belongs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The text, without a trailing newline.
    pub text: String,
    /// Whether this is a warning, and so goes to stderr.
    pub warning: bool,
}

/// What to say about an image's byte order, or `None` to say nothing.
///
/// Separated from the printing so the decision can be tested. The image is
/// read as supplied - where the slot carries a `swap_bytes`, what reaches the
/// machine is the other order, and that is what the wording is chosen against.
#[must_use]
pub fn message(image: &[u8], name: &str, intent: &Intent, verbose: bool) -> Option<Message> {
    let name = basename(name);
    let conclusion = identify(image).byte_order;

    let warn = |text: String| {
        Some(Message {
            text,
            warning: true,
        })
    };
    let say = |text: String| {
        verbose.then_some(Message {
            text,
            warning: false,
        })
    };

    let Some(order) = conclusion.agreed() else {
        return match intent {
            Intent::Swapping => say(format!(
                "Unable to tell which way around the byte pairs are in {name}.  Swapping\n  \
                 as requested."
            )),
            Intent::ServeAsIs | Intent::ServeSwapped => say(format!(
                "Unable to tell which way around the byte pairs are in {name}.  If the slot\n  \
                 does not work, try transform=swap_bytes."
            )),
        };
    };

    // Every claim carries the header that identified the image, so they are all
    // the same phrase and the first is the whole story.
    let saw = conclusion.claims()[0].evidence;

    let served = match intent {
        Intent::Swapping | Intent::ServeAsIs => *order,
        Intent::ServeSwapped => match *order {
            ByteOrder::HighByteFirst => ByteOrder::LowByteFirst,
            ByteOrder::LowByteFirst => ByteOrder::HighByteFirst,
        },
    };

    match (intent, served == ByteOrder::ONE_ROM) {
        // J1
        (Intent::Swapping, true) => warn(format!(
            "Warning: {name} starts with {saw}, low byte of each pair first.\n  \
             It is already the way One ROM needs it, and swapping the bytes will stop it\n  \
             working with One ROM."
        )),
        // J2
        (Intent::Swapping, false) => say(format!(
            "{name} starts with {saw}, high byte of each pair first.\n  \
             Swapping the bytes makes it correct for One ROM."
        )),
        // J4
        (Intent::ServeAsIs, false) => warn(format!(
            "Warning: {name} starts with {saw}, high byte of each pair first.\n  \
             One ROM needs the low byte of each pair first.  Add transform=swap_bytes to\n  \
             this slot."
        )),
        // J5
        (Intent::ServeSwapped, false) => warn(format!(
            "Warning: {name} starts with {saw} and was already the way One\n  \
             ROM needs it.  transform=swap_bytes has swapped it the wrong way round.\n  \
             Remove transform=swap_bytes from this slot."
        )),
        // J6
        (Intent::ServeAsIs | Intent::ServeSwapped, true) => say(format!(
            "{name} starts with {saw}, low byte of each pair first.\n  \
             This is correct for One ROM."
        )),
    }
}

/// Print what [`message`] decided, if anything.
pub fn report(image: &[u8], name: &str, intent: &Intent, verbose: bool) {
    if let Some(m) = message(image, name, intent, verbose) {
        if m.warning {
            eprintln!("{}", m.text);
        } else {
            println!("{}", m.text);
        }
    }
}

/// Whether a slot's byte order can be checked, and what is about to happen to
/// its image.
///
/// `None` where the chip is 8 bits wide, since its image holds no 16-bit words,
/// or where the slot carries any transform besides a lone `swap_bytes`, since
/// the bytes at the start of the file are then not the bytes served.
#[must_use]
pub fn slot_intent(chip_type: ChipType, transforms: &[Transform]) -> Option<Intent> {
    if !chip_type.supports_bit_mode(16) {
        return None;
    }
    match transforms {
        [] => Some(Intent::ServeAsIs),
        [Transform::SwapBytes] => Some(Intent::ServeSwapped),
        _ => None,
    }
}

/// Check every 16-bit slot in a built config and report what its image's byte
/// order means for it.
///
/// Called once the ROM files are loaded and before the image is assembled, so a
/// warning arrives before the user waits for a build.
pub fn report_slots(builder: &Builder, verbose: bool) {
    for (index, chip) in builder
        .config()
        .chip_sets
        .iter()
        .flat_map(|set| &set.chips)
        .enumerate()
    {
        let Some(intent) = slot_intent(chip.chip_type.resolved(), &chip.transform) else {
            continue;
        };
        let Some(data) = builder.chip_data(index) else {
            continue;
        };
        report(data, &chip.file, &intent, verbose);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::chip::ChipType;

    // First eight bytes of real images.
    /// An Amiga ROM as One ROM reads it, low byte of each pair first.
    const LOW_FIRST: [u8; 8] = [0x11, 0x11, 0xf9, 0x4e, 0xfc, 0x00, 0xd2, 0x00];
    /// The same image as a 68000 holds it, high byte first.
    const HIGH_FIRST: [u8; 8] = [0x11, 0x11, 0x4e, 0xf9, 0x00, 0xfc, 0x00, 0xd2];
    /// Nothing recognisable.
    const UNKNOWN: [u8; 8] = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04];

    #[track_caller]
    fn warns(image: &[u8], intent: &Intent, opening: &str) {
        for verbose in [false, true] {
            let m = message(image, "/tmp/kick.rom", intent, verbose)
                .unwrap_or_else(|| panic!("expected a warning, verbose={verbose}"));
            assert!(m.warning, "should be a warning: {}", m.text);
            assert!(m.text.starts_with(opening), "got: {}", m.text);
            assert!(m.text.contains("kick.rom"), "got: {}", m.text);
            assert!(!m.text.contains("/tmp/"), "path not trimmed: {}", m.text);
        }
    }

    #[track_caller]
    fn says_only_when_verbose(image: &[u8], intent: &Intent, opening: &str) {
        assert_eq!(message(image, "kick.rom", intent, false), None);
        let m = message(image, "kick.rom", intent, true).expect("expected a line");
        assert!(!m.warning, "should not be a warning: {}", m.text);
        assert!(m.text.starts_with(opening), "got: {}", m.text);
    }

    #[test]
    fn j1_swapping_an_image_that_is_already_right() {
        warns(
            &LOW_FIRST,
            &Intent::Swapping,
            "Warning: kick.rom starts with an Amiga ROM header, low byte",
        );
    }

    #[test]
    fn j2_swapping_an_image_that_needs_it() {
        says_only_when_verbose(
            &HIGH_FIRST,
            &Intent::Swapping,
            "kick.rom starts with an Amiga ROM header, high byte",
        );
    }

    #[test]
    fn j4_serving_an_image_stored_the_other_way_round() {
        warns(
            &HIGH_FIRST,
            &Intent::ServeAsIs,
            "Warning: kick.rom starts with an Amiga ROM header, high byte",
        );
    }

    #[test]
    fn j5_swapping_a_slot_whose_image_did_not_need_it() {
        // The file is already the way One ROM reads it, so applying swap_bytes
        // serves the wrong order.  This is the case the wiring got wrong first
        // time: the check reads the file as supplied and must flip the answer.
        warns(
            &LOW_FIRST,
            &Intent::ServeSwapped,
            "Warning: kick.rom starts with an Amiga ROM header and was already",
        );
        // And the converse says nothing outside --verbose.
        says_only_when_verbose(
            &HIGH_FIRST,
            &Intent::ServeSwapped,
            "kick.rom starts with an Amiga ROM header, low byte",
        );
    }

    #[test]
    fn j6_serving_an_image_that_is_already_right() {
        says_only_when_verbose(
            &LOW_FIRST,
            &Intent::ServeAsIs,
            "kick.rom starts with an Amiga ROM header, low byte",
        );
    }

    #[test]
    fn j3_and_j7_an_unrecognised_image() {
        for (intent, tail) in [
            (Intent::Swapping, "Swapping\n  as requested."),
            (Intent::ServeAsIs, "try transform=swap_bytes."),
            (Intent::ServeSwapped, "try transform=swap_bytes."),
        ] {
            assert_eq!(message(&UNKNOWN, "mystery.bin", &intent, false), None);
            let m = message(&UNKNOWN, "mystery.bin", &intent, true).expect("expected a line");
            assert!(!m.warning);
            assert!(
                m.text.starts_with(
                    "Unable to tell which way around the byte pairs are in mystery.bin."
                ),
                "got: {}",
                m.text
            );
            assert!(m.text.ends_with(tail), "got: {}", m.text);
        }
    }

    #[test]
    fn a_slot_is_checked_only_for_a_16_bit_chip() {
        assert!(matches!(
            slot_intent(ChipType::Chip27C400, &[]),
            Some(Intent::ServeAsIs)
        ));
        assert!(matches!(
            slot_intent(ChipType::Chip27C200, &[]),
            Some(Intent::ServeAsIs)
        ));
        assert!(slot_intent(ChipType::Chip2364, &[]).is_none());
    }

    #[test]
    fn a_slot_is_checked_only_where_swap_bytes_is_the_whole_transform() {
        assert!(matches!(
            slot_intent(ChipType::Chip27C400, &[Transform::SwapBytes]),
            Some(Intent::ServeSwapped)
        ));
        // A deinterleave moves what sits at the start of the file, so the
        // bytes this reads are not the bytes served.
        let deint = Transform::Deinterleave {
            offset: 0,
            stride: 2,
            bytes: 1,
        };
        assert!(slot_intent(ChipType::Chip27C400, std::slice::from_ref(&deint)).is_none());
        assert!(slot_intent(ChipType::Chip27C400, &[deint, Transform::SwapBytes]).is_none());
        assert!(
            slot_intent(
                ChipType::Chip27C400,
                &[Transform::SwapBytes, Transform::SwapBytes]
            )
            .is_none()
        );
    }
}
