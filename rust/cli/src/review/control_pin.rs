// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `--pin`.

use super::help;

#[test]
fn image_select_pads_past_sel_e() {
    help(&["onerom", "inspect", "gpio", "--pin", "sel_f"]);
    println!();
    help(&["onerom", "inspect", "gpio", "--pin", "sel_g"]);
}

#[test]
fn names_that_arent_pins() {
    help(&["onerom", "inspect", "gpio", "--pin", "sel_h"]);
    println!();
    help(&["onerom", "inspect", "gpio", "--pin", "a17"]);
}

#[test]
fn helps() {
    for command in [
        &["control", "pin"][..],
        &["control", "reset"],
        &["inspect", "gpio"],
    ] {
        let mut words = vec!["onerom"];
        words.extend(command);
        words.push("--help");
        help(&words);
        println!();
    }
}
