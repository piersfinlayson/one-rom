// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Turning a CLI word into a word on screen.
//!
//! The CLI writes `control`, `rgb`, `swap-bytes` and `colour`.  A screen shows
//! `Control`, `RGB`, `Swap Bytes` and `Colour`.  The rule is here rather than
//! in a screen because every consumer of this crate needs the same answer —
//! two consumers capitalising One ROM's own words differently is the kind of
//! difference nobody notices until both are on screen at once.
//!
//! Two cases, and which one applies is about where the words sit rather than
//! what they say:
//!
//! - [`heading`] for a title or a tab, in title case — `Swap Bytes`.
//! - [`label`] for the name of one option, in sentence case — `Swap bytes`.
//!
//! [`ACRONYMS`] overrides both, in either position, and takes a plural — a
//! `gpios` reads `GPIOs` and not `GPIOS`.

use alloc::string::String;

/// One ROM's words that are read out letter by letter, and stay upper case
/// wherever they appear.
///
/// This is product vocabulary, not an English rule, so it is a list and not an
/// algorithm.  A word is matched without regard to case and with a trailing
/// `s` allowed, so `rgb`, `RGB` and `gpios` all find their entry.
pub const ACRONYMS: &[&str] = &[
    "RGB", "GPIO", "LED", "ROM", "USB", "CLI", "MSD", "SWD", "OTP", "ID", "VID", "PID", "JSON",
    "CS",
];

/// A title or a tab, in title case — `Swap Bytes`.
pub fn heading(words: &str) -> String {
    join(words, |word, _| capitalise(word))
}

/// The name of one option, in sentence case — `Swap bytes`.
pub fn label(words: &str) -> String {
    join(
        words,
        |word, first| {
            if first { capitalise(word) } else { lower(word) }
        },
    )
}

/// Splits on the separators the CLI uses, applies `each`, and rejoins with
/// spaces.  An acronym answers for itself and never reaches `each`.
fn join(words: &str, each: impl Fn(&str, bool) -> String) -> String {
    let mut out = String::new();

    for (index, word) in words
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .enumerate()
    {
        if index > 0 {
            out.push(' ');
        }
        match acronym(word) {
            Some(upper) => out.push_str(&upper),
            None => out.push_str(&each(word, index == 0)),
        }
    }

    out
}

/// The upper-case form of `word` where it is in [`ACRONYMS`], keeping a
/// trailing plural lower — `GPIOs`.
///
/// The whole word is tried before the plural stem, because `CS` is an acronym
/// that ends in `s` and stripping first turns it into a search for `C`.
fn acronym(word: &str) -> Option<String> {
    let known = |candidate: &str| {
        ACRONYMS
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(candidate))
    };

    let plural = !known(word) && word.len() > 1 && (word.ends_with('s') || word.ends_with('S'));
    let stem = if plural {
        &word[..word.len() - 1]
    } else {
        word
    };

    if !known(stem) {
        return None;
    }

    let mut out = String::new();
    for c in stem.chars() {
        out.extend(c.to_uppercase());
    }
    if plural {
        out.push('s');
    }
    Some(out)
}

/// `bytes` becomes `Bytes`.
fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::new();
            out.extend(first.to_uppercase());
            for c in chars {
                out.extend(c.to_lowercase());
            }
            out
        }
        None => String::new(),
    }
}

/// `Bytes` becomes `bytes`.
fn lower(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        out.extend(c.to_lowercase());
    }
    out
}
