// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect peek memory`.

#[test]
fn help() {
    super::help(&["onerom", "inspect", "peek", "memory", "--help"]);
}
