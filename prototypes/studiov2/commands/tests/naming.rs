// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What a CLI word looks like on screen.

use studiov2_commands::name::{heading, label};

#[test]
fn a_heading_is_title_case() {
    assert_eq!(heading("swap-bytes"), "Swap Bytes");
    assert_eq!(heading("control"), "Control");
    assert_eq!(heading("load-address"), "Load Address");
}

#[test]
fn an_option_label_is_sentence_case() {
    assert_eq!(label("swap-bytes"), "Swap bytes");
    assert_eq!(label("colour"), "Colour");
    assert_eq!(label("base-firmware"), "Base firmware");
    assert_eq!(label("log-level"), "Log level");
}

#[test]
fn an_acronym_stays_upper_in_either_position() {
    assert_eq!(heading("rgb"), "RGB");
    assert_eq!(label("msd"), "MSD");
    assert_eq!(label("vid-pid"), "VID PID");
    assert_eq!(heading("inspect-gpio"), "Inspect GPIO");
    assert_eq!(label("save-config-json"), "Save config JSON");
}

#[test]
fn an_acronym_pluralises_with_a_lower_s() {
    assert_eq!(heading("gpios"), "GPIOs");
    assert_eq!(label("leds"), "LEDs");
}

#[test]
fn a_word_ending_in_s_that_is_not_an_acronym_is_left_alone() {
    assert_eq!(heading("chips"), "Chips");
    assert_eq!(label("slots"), "Slots");
    assert_eq!(heading("releases"), "Releases");
}

#[test]
fn every_word_of_every_command_and_option_survives_both() {
    for command in studiov2_commands::COMMANDS {
        for word in command.path {
            assert!(!heading(word).is_empty(), "heading ate {word}");
        }
        for opt in command.opts {
            assert!(!label(opt.long).is_empty(), "label ate {}", opt.long);
        }
    }
}

#[test]
fn an_acronym_that_ends_in_s_is_not_mistaken_for_a_plural() {
    assert_eq!(heading("cs"), "CS");
    assert_eq!(label("cs"), "CS");
}
