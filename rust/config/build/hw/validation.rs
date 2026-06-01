// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#![allow(dead_code)]

use core::panic;
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};

include!("../../src/mcu.rs");

impl Port {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "0" => Some(Port::Zero),
            "A" => Some(Port::A),
            "B" => Some(Port::B),
            "C" => Some(Port::C),
            "D" => Some(Port::D),
            "NONE" => Some(Port::None),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Port::from_str(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid port: {}, must be None, A, B, C, or D", s))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McuFamily {
    Stm32f4,
    Rp2350,
    Rp2350B,
}

impl From<&McuFamily> for Family {
    fn from(family: &McuFamily) -> Self {
        match family {
            McuFamily::Stm32f4 => Family::Stm32f4,
            McuFamily::Rp2350 => Family::Rp2350,
            McuFamily::Rp2350B => Family::Rp2350,
        }
    }
}

impl McuFamily {
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "stm32f4" | "f4" => Some(McuFamily::Stm32f4),
            "rp2350" => Some(McuFamily::Rp2350),
            "rp2350b" => Some(McuFamily::Rp2350B),
            _ => None,
        }
    }

    pub fn max_valid_addr_pin(&self) -> u8 {
        match self {
            McuFamily::Stm32f4 => 13, // 15 - 2 (top two reserved for X1/X2)
            McuFamily::Rp2350 => 25,
            McuFamily::Rp2350B => 39,
        }
    }

    pub fn max_valid_addr_cs_pin(&self) -> u8 {
        match self {
            McuFamily::Stm32f4 => 15,
            McuFamily::Rp2350 => 25,
            McuFamily::Rp2350B => 39,
        }
    }

    pub fn max_valid_data_pin(&self) -> u8 {
        match self {
            McuFamily::Stm32f4 => 7,
            McuFamily::Rp2350 => 25,
            McuFamily::Rp2350B => 39,
        }
    }

    pub fn valid_pin_num(&self, pin: u8) -> bool {
        match self {
            McuFamily::Stm32f4 => pin <= 15,
            McuFamily::Rp2350 => pin <= 29,
            McuFamily::Rp2350B => pin <= 47,
        }
    }

    pub fn allowed_data_port(&self) -> Port {
        match self {
            McuFamily::Stm32f4 => Port::A,
            McuFamily::Rp2350 => Port::Zero,
            McuFamily::Rp2350B => Port::Zero,
        }
    }

    pub fn allowed_addr_port(&self) -> Port {
        match self {
            McuFamily::Stm32f4 => Port::C,
            McuFamily::Rp2350 => Port::Zero,
            McuFamily::Rp2350B => Port::Zero,
        }
    }

    pub fn allowed_cs_port(&self) -> Port {
        match self {
            McuFamily::Stm32f4 => Port::C,
            McuFamily::Rp2350 => Port::Zero,
            McuFamily::Rp2350B => Port::Zero,
        }
    }

    pub fn allowed_sel_port(&self) -> Port {
        match self {
            McuFamily::Stm32f4 => Port::B,
            McuFamily::Rp2350 => Port::Zero,
            McuFamily::Rp2350B => Port::Zero,
        }
    }

    pub fn valid_x1_pins(&self) -> Vec<u8> {
        match self {
            McuFamily::Stm32f4 => vec![14],
            McuFamily::Rp2350 => (0..26).collect(),
            McuFamily::Rp2350B => (0..40).collect(),
        }
    }

    pub fn valid_x2_pins(&self) -> Vec<u8> {
        match self {
            McuFamily::Stm32f4 => vec![15],
            McuFamily::Rp2350 => self.valid_x1_pins(),
            McuFamily::Rp2350B => self.valid_x1_pins(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct McuPorts {
    pub data_port: Port,
    pub addr_port: Port,
    pub cs_port: Port,
    pub sel_port: Port,
    pub status_port: Port,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChipPins {
    pub quantity: u8,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Copy)]
pub enum BitMode {
    #[serde(rename = "8")]
    Bit8,
    #[serde(rename = "16")]
    Bit16,
}

impl From<BitMode> for usize {
    fn from(mode: BitMode) -> usize {
        match mode {
            BitMode::Bit8 => 8,
            BitMode::Bit16 => 16,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Chip {
    pub pins: ChipPins,
    pub bit_modes: Vec<BitMode>,
    pub extra_types: Option<Vec<String>>,
}

impl Chip {
    pub const MAX_ADDR_PINS: usize = 19;

    pub fn max_addr_pins(&self) -> u8 {
        match self.pins.quantity {
            24 => 16, // Includes CS and X pins
            28 => 18, // Includes CS lines (to allow for 231024 which uses /OE as address line)
            32 => 19, // Addr pins, 512KB max
            40 => 19, // Just addr pins, 512KB max
            _ => panic!(
                "Unsupported ROM type {}, expected 24, 28, or 40-pin ROM",
                self.pins.quantity
            ),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct McuPins {
    pub data: Vec<u8>,
    pub addr: Vec<u8>,
    #[serde(default, deserialize_with = "deserialize_chip_map")]
    pub cs1: HashMap<String, u8>,
    #[serde(default, deserialize_with = "deserialize_chip_map")]
    pub cs2: HashMap<String, u8>,
    #[serde(default, deserialize_with = "deserialize_chip_map")]
    pub cs3: HashMap<String, u8>,
    pub x1: Option<u8>,
    pub x2: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_chip_map")]
    pub ce: HashMap<String, u8>,
    #[serde(default, deserialize_with = "deserialize_chip_map")]
    pub oe: HashMap<String, u8>,
    pub x_jumper_pull: u8,
    pub sel: Vec<u8>,
    pub sel_jumper_pull: Vec<u8>,
    /// If a sel pin is connected to SWCLK, specify it here
    #[serde(default = "invalid_pin")]
    pub swclk_sel: u8,
    /// If a sel pin is connected to SWDIO, specify it here
    #[serde(default = "invalid_pin")]
    pub swdio_sel: u8,
    pub status: u8,
    pub byte: Option<u8>,
    pub alt: Option<HashMap<String, HashMap<String, u8>>>,
}

#[derive(Debug, Deserialize, Clone, Default, PartialEq, Eq)]
pub enum ServeMode {
    #[default]
    #[serde(rename = "cpu")]
    Cpu,
    #[serde(rename = "pio")]
    Pio,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Mcu {
    #[serde(deserialize_with = "deserialize_mcu_family")]
    pub family: McuFamily,
    pub ports: McuPorts,
    pub pins: McuPins,
    #[serde(default)]
    pub usb: Option<McuUsb>,
    #[serde(default)]
    pub serve_mode: ServeMode,
}

#[derive(Debug, Deserialize, Clone)]
pub struct McuUsb {
    pub present: bool,
    pub pins: Option<McuUsbPins>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct McuUsbPins {
    pub vbus: u8,
    pub port: Port,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HwConfigJson {
    pub description: String,
    #[serde(default)]
    pub alt: Vec<String>,
    pub chip: Chip,
    pub mcu: Mcu,
}

fn deserialize_mcu_family<'de, D>(deserializer: D) -> Result<McuFamily, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    McuFamily::try_from_str(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("Invalid MCU family: {}", s)))
}

fn deserialize_chip_map<'de, D>(deserializer: D) -> Result<HashMap<String, u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    HashMap::deserialize(deserializer)
}

fn invalid_pin() -> u8 {
    255
}

pub fn validate_config(name: &str, config: &HwConfigJson) {
    // Check data pins
    let has8 = config.chip.bit_modes.contains(&BitMode::Bit8);
    let has16 = config.chip.bit_modes.contains(&BitMode::Bit16);
    if !has8 && !has16 {
        panic!("{name}: ROM bit modes must include at least one of 8 or 16")
    } else if has8 && !has16 && config.mcu.pins.data.len() != 8 {
        panic!(
            "{name}: data pins must be exactly 8 for 8-bit only ROM, found {}",
            config.mcu.pins.data.len()
        );
    } else if has16 && !has8 && config.mcu.pins.data.len() != 16 {
        panic!(
            "{name}: data pins must be exactly 16 for 16-bit only ROM, found {}",
            config.mcu.pins.data.len()
        );
    } else if has8 && has16 && config.mcu.pins.data.len() != 16 {
        panic!(
            "{name}: data pins must be exactly 16 for mixed 8/16-bit ROM, found {}",
            config.mcu.pins.data.len()
        );
    }
    for bit_mode in &config.chip.bit_modes {
        // Check we didn't add a mode
        if !matches![bit_mode, BitMode::Bit8 | BitMode::Bit16] {
            panic!(
                "{}: unsupported bit mode {:?}, must be 8 or 16",
                name, bit_mode
            );
        }
    }

    // Validate pins consistent within pin arrays
    let max_data_pins = if config.chip.bit_modes.contains(&BitMode::Bit16) {
        16
    } else {
        8
    };
    validate_pin_array(
        &config.mcu,
        &config.mcu.pins.data,
        "data",
        name,
        max_data_pins,
    );
    let max_addr_pins = config.chip.max_addr_pins();
    validate_pin_array(
        &config.mcu,
        &config.mcu.pins.addr,
        "addr",
        name,
        max_addr_pins,
    );
    validate_pin_array(&config.mcu, &config.mcu.pins.sel, "sel", name, 7);

    // Validate values in pin arrays are within valid ranges
    validate_pin_values(
        &config.mcu.pins.data,
        "data",
        name,
        8,
        config.mcu.family.max_valid_data_pin(),
    );

    match config.chip.pins.quantity {
        24 => validate_pin_values(
            &config.mcu.pins.addr,
            "addr",
            name,
            13,
            config.mcu.family.max_valid_addr_pin(),
        ),
        28 => validate_pin_values(
            &config.mcu.pins.addr,
            "addr",
            name,
            14,
            config.mcu.family.max_valid_addr_cs_pin(),
        ),
        32 => {
            validate_pin_values(
                &config.mcu.pins.addr,
                "addr",
                name,
                19,
                config.mcu.family.max_valid_addr_cs_pin(),
            );
        }
        40 => {
            validate_pin_values(
                &config.mcu.pins.addr,
                "addr",
                name,
                16,
                config.mcu.family.max_valid_addr_cs_pin(),
            );

            // A0 must be the _first_ address pin for 40-pin ROMs, so that the
            // LSB (as read in from the PIOs, the lowest GPIO), is A0.  This
            // allows, 16-bit mode, sticking a 0 bit as the LSB bit to get two
            // consecutive addresses for even addresses.
            let min_addr_pin = *config.mcu.pins.addr.iter().min().unwrap();
            let a0_index = config.mcu.pins.addr[0];
            if a0_index != min_addr_pin {
                //panic!(
                //    "{}: for 40-pin ROMs, A0 must be the lowest address pin and at index 0, found at index {}",
                //    name, a0_index
                //);
            }
        }
        _ => panic!(
            "{}: unsupported ROM type {}, expected 24, 28, 32 or 40-pin ROM",
            name, config.chip.pins.quantity
        ),
    }

    // Validate data pins are contiguous within 8-bit window on 8-boundary
    {
        let min_data_pin = *config.mcu.pins.data.iter().min().unwrap();
        let max_data_pin = *config.mcu.pins.data.iter().max().unwrap();

        if !min_data_pin.is_multiple_of(8) {
            panic!(
                "{}: data pins must start on 8-byte boundary, got min pin {}",
                name, min_data_pin
            );
        }

        let data_pins_windows = if has16 { 16 } else { 8 };
        if max_data_pin >= min_data_pin + data_pins_windows {
            panic!(
                "{}: data pins must be within {}-bit window, got range {}-{}",
                name, data_pins_windows, min_data_pin, max_data_pin
            );
        }
    }

    // Validate address pins are contiguous within 16-bit window on 8-boundary
    {
        let min_addr_pin = *config.mcu.pins.addr.iter().min().unwrap();
        let max_addr_pin = *config.mcu.pins.addr.iter().max().unwrap();

        // For CPU mode, address pins must start on 8-byte boundary, as when
        // ubfx is used, the shift is hard-coded to 8.  In PIO mode (only)
        // there is no such restriction.
        if config.mcu.serve_mode == ServeMode::Cpu && !min_addr_pin.is_multiple_of(8) {
            panic!(
                "{}: address pins must start on 8-byte boundary, got min pin {}",
                name, min_addr_pin
            );
        }

        let mut num_addr_pins = config.mcu.pins.addr.len() as u8;
        if num_addr_pins < 16 {
            num_addr_pins = 16;
        }
        if max_addr_pin >= min_addr_pin + num_addr_pins {
            panic!(
                "{}: address pins must be within {}-bit window, got range {}-{}",
                name, num_addr_pins, min_addr_pin, max_addr_pin
            );
        }
    }

    // Validate ports
    if config.mcu.ports.data_port != config.mcu.family.allowed_data_port() {
        panic!(
            "{}: data port must be {:?}, found {:?}",
            name,
            config.mcu.family.allowed_data_port(),
            config.mcu.ports.data_port
        );
    }
    if config.mcu.ports.addr_port != config.mcu.family.allowed_addr_port() {
        panic!(
            "{}: address port must be {:?}, found {:?}",
            name,
            config.mcu.family.allowed_addr_port(),
            config.mcu.ports.addr_port
        );
    }
    if config.mcu.ports.cs_port != config.mcu.family.allowed_cs_port() {
        panic!(
            "{}: CS port must be {:?}, found {:?}",
            name,
            config.mcu.family.allowed_cs_port(),
            config.mcu.ports.cs_port
        );
    }
    if config.mcu.ports.sel_port != config.mcu.family.allowed_sel_port() {
        panic!(
            "{}: SEL port must be {:?}, found {:?}",
            name,
            config.mcu.family.allowed_sel_port(),
            config.mcu.ports.sel_port
        );
    }

    // Validate optional pins
    if let Some(pin) = config.mcu.pins.x1 {
        validate_pin_number(&config.mcu, pin, "x1", name);
    }
    if let Some(pin) = config.mcu.pins.x2 {
        validate_pin_number(&config.mcu, pin, "x2", name);
    }

    // Validate X1/X2 pins
    if let Some(x1_pin) = config.mcu.pins.x1 {
        let valid_pins = config.mcu.family.valid_x1_pins();
        if !valid_pins.contains(&x1_pin) {
            panic!(
                "{}: X1 pin must be within {:?}, found {}",
                name, valid_pins, x1_pin
            );
        }
    }
    if let Some(x2_pin) = config.mcu.pins.x2 {
        let valid_pins = config.mcu.family.valid_x2_pins();
        if !valid_pins.contains(&x2_pin) {
            panic!(
                "{}: X2 pin must be within {:?}, found {}",
                name, valid_pins, x2_pin
            );
        }
    }

    // Both X1 and X2 must be provided together
    if config.mcu.pins.x1.is_some() != config.mcu.pins.x2.is_some() {
        panic!(
            "{}: X1 and X2 pins must both be provided or both omitted",
            name
        );
    }

    // Validate sel_jumper_pull
    if config.mcu.pins.sel_jumper_pull.len() != config.mcu.pins.sel.len() {
        panic!(
            "{}: sel_jumper_pull length {} does not match sel length {}",
            name,
            config.mcu.pins.sel_jumper_pull.len(),
            config.mcu.pins.sel.len()
        );
    }
    for &pull in &config.mcu.pins.sel_jumper_pull {
        if pull > 1 {
            panic!(
                "{}: sel_jumper_pull values must be 0 (pull down) or 1 (pull up), found {}",
                name, pull
            );
        }
    }

    // Validate SWCLK_SEL/SWDIO_SEL sel pins if provided
    // - Must be a valid pin number
    // - Must match a sel pin
    if config.mcu.pins.swclk_sel != 255 {
        validate_pin_number(&config.mcu, config.mcu.pins.swclk_sel, "swclk_sel", name);
        if !config.mcu.pins.sel.contains(&config.mcu.pins.swclk_sel) {
            panic!(
                "{}: swclk_sel pin {} not found in sel pins {:?}",
                name, config.mcu.pins.swclk_sel, config.mcu.pins.sel
            );
        }
    }
    if config.mcu.pins.swdio_sel != 255 {
        validate_pin_number(&config.mcu, config.mcu.pins.swdio_sel, "swdio_sel", name);
        if !config.mcu.pins.sel.contains(&config.mcu.pins.swdio_sel) {
            panic!(
                "{}: swdio_sel pin {} not found in sel pins {:?}",
                name, config.mcu.pins.swdio_sel, config.mcu.pins.sel
            );
        }
    }

    // Group pins by port for conflict checking
    let mut port_pins: HashMap<Port, Vec<(&str, u8)>> = HashMap::new();

    // Add data pins
    for &pin in &config.mcu.pins.data {
        port_pins
            .entry(config.mcu.ports.data_port)
            .or_default()
            .push(("data", pin));
    }

    // Add address pins
    for &pin in &config.mcu.pins.addr {
        port_pins
            .entry(config.mcu.ports.addr_port)
            .or_default()
            .push(("addr", pin));
    }

    // Add sel pins
    for &pin in &config.mcu.pins.sel {
        port_pins
            .entry(config.mcu.ports.sel_port)
            .or_default()
            .push(("sel", pin));
    }

    // Add CS pins
    for &pin in config.mcu.pins.cs1.values() {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("cs1", pin));
    }
    for &pin in config.mcu.pins.cs2.values() {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("cs2", pin));
    }
    for &pin in config.mcu.pins.cs3.values() {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("cs3", pin));
    }

    // Add optional pins
    if let Some(pin) = config.mcu.pins.x1 {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("x1", pin));
    }
    if let Some(pin) = config.mcu.pins.x2 {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("x2", pin));
    }

    for &pin in config.mcu.pins.ce.values() {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("ce", pin));
    }
    for &pin in config.mcu.pins.oe.values() {
        port_pins
            .entry(config.mcu.ports.cs_port)
            .or_default()
            .push(("oe", pin));
    }

    let pin = config.mcu.pins.status;
    port_pins
        .entry(config.mcu.ports.status_port)
        .or_default()
        .push(("status", pin));

    // Add USB pins
    #[allow(clippy::collapsible_if)]
    if let Some(usb) = &config.mcu.usb {
        if usb.present {
            if let Some(usb_pins) = &usb.pins {
                port_pins
                    .entry(usb_pins.port)
                    .or_default()
                    .push(("usb_vbus", usb_pins.vbus));
            }
        }
    }

    // Check for conflicts within each port
    for (port, pins) in port_pins {
        let mut used_pins: HashMap<u8, Vec<&str>> = HashMap::new();

        for (pin_type, pin_num) in pins {
            used_pins.entry(pin_num).or_default().push(pin_type);
        }

        for (pin_num, pin_types) in used_pins {
            if pin_types.len() > 1 {
                let cs_types: HashSet<&str> =
                    ["cs1", "cs2", "cs3", "ce", "oe"].into_iter().collect();
                let has_cs = pin_types.iter().any(|t| cs_types.contains(t));
                let all_cs_or_addr = pin_types
                    .iter()
                    .all(|t| cs_types.contains(t) || *t == "addr");

                if !(has_cs && all_cs_or_addr) {
                    panic!(
                        "{}: pin {} on port {:?} used by multiple incompatible functions: {:?}",
                        name, pin_num, port, pin_types
                    );
                }
            }
        }
    }

    // Validate serve_mode
    match config.mcu.serve_mode {
        ServeMode::Cpu => (),
        ServeMode::Pio => {
            // Only supported for RP2350
            if !matches!(config.mcu.family, McuFamily::Rp2350 | McuFamily::Rp2350B) {
                panic!(
                    "{}: serve_mode Pio is only supported for RP2350A/B family",
                    name
                );
            }
        }
    }
}

fn validate_pin_number(mcu: &Mcu, pin: u8, pin_name: &str, config_name: &str) {
    if !mcu.family.valid_pin_num(pin) && pin != 255 {
        panic!(
            "{}: invalid pin number {} for {}, must be valid or 255 if pin not exposed",
            config_name, pin, pin_name,
        );
    }
}

fn validate_pin_array(mcu: &Mcu, pins: &[u8], pin_type: &str, config_name: &str, max_pins: u8) {
    let mut seen = HashSet::new();
    let mut num_pins = 0;
    for &pin in pins {
        validate_pin_number(mcu, pin, pin_type, config_name);
        if !seen.insert(pin) {
            panic!(
                "{}: duplicate pin {} in {} array",
                config_name, pin, pin_type
            );
        }
        num_pins += 1;
    }
    if num_pins > max_pins as usize {
        panic!(
            "{}: too many pins in {} array, maximum is {}",
            config_name, pin_type, max_pins
        );
    }
}

fn validate_pin_values(
    pins: &[u8],
    pin_type: &str,
    config_name: &str,
    min_valid: usize,
    valid_value: u8,
) {
    for (ii, &pin) in pins.iter().enumerate() {
        if ii >= min_valid {
            break;
        }
        if pin > valid_value {
            panic!(
                "{}: invalid pin value {} in {} array, must be 0-{}",
                config_name, pin, pin_type, valid_value
            );
        }
    }
}
