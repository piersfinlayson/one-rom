// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `console`.

use onerom_cli::Error;

use super::{help, written_failure};

const DEVICE: &str = "One ROM Fire 32 C - Firmware: v0.8.1 State: Running Serial: DE3F9C232F655B6B";

#[test]
fn helps() {
    help(&["onerom", "console", "--help"]);
}

#[test]
fn without_serial_port_1() {
    println!("$ onerom console");
    written_failure(&Error::SerialPortNotFound(DEVICE.to_string()));
}

#[test]
fn without_serial_port_2() {
    println!("$ onerom console --serial-port 2");
    written_failure(&Error::SerialPort2NotFound(DEVICE.to_string()));
}

#[test]
fn rejects_serial_port_3() {
    help(&["onerom", "console", "--serial-port", "3"]);
}
