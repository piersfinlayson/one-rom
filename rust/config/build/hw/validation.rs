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
    #[serde(default)]
    pub non_signal: Option<Vec<u8>>,
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
pub struct ExternalFlash {
    pub cs_pin: u8,
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
    pub neo: Option<u8>,
    /// Maps socket pin number to the GPIO(s) connected to it
    pub socket_pin_to_gpio: Option<HashMap<u8, Vec<u8>>>,
    pub x_pin_to_gpio: Option<HashMap<u8, Vec<u8>>>,
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
    pub external_flash: Option<ExternalFlash>,
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
    /// Optional physical jumper-header descriptor; absent for boards not yet
    /// characterised (a consumer then falls back to a generic description).
    #[serde(default)]
    pub jumper_header: Option<JsonJumperHeader>,
}

/// Raw JSON form of a board's jumper header: columns keyed by 1-based column
/// number, each a map of row number ("1"/"2"/"3") to a list of role tokens.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonJumperHeader {
    pub columns: HashMap<String, HashMap<String, Vec<String>>>,
}

/// A parsed, validated header role (build-time owned mirror of the runtime
/// `crate::hw::header::HeaderRole`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderRoleP {
    Power5V,
    Gnd,
    Run,
    Bootsel,
    Select(u8),
    Swclk,
    Swdio,
    X1,
    X2,
    Addr(u8),
}

/// A parsed pad state (build-time owned mirror of `HeaderSlot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderSlotP {
    NotPopulated,
    NotConnected,
    Roles(Vec<HeaderRoleP>),
}

/// A parsed header column (build-time owned mirror of `HeaderColumn`).
#[derive(Debug, Clone)]
pub struct HeaderColumnP {
    pub col: u8,
    pub row1: HeaderSlotP,
    pub row2: HeaderSlotP,
    pub row3: Option<HeaderSlotP>,
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
        if !matches!(bit_mode, BitMode::Bit8 | BitMode::Bit16) {
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

    if let Some(pin) = config.mcu.pins.neo {
        validate_pin_number(&config.mcu, pin, "neo", name);
    }
    if let Some(ef) = &config.mcu.external_flash {
        validate_pin_number(&config.mcu, ef.cs_pin, "external_flash.cs_pin", name);
    }

    validate_socket_and_x_pins(config, name);

    if let Some(header) = &config.jumper_header {
        let cols = parse_jumper_header(name, header);
        validate_jumper_header(name, &cols, &config.mcu.pins);
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

fn validate_socket_and_x_pins(config: &HwConfigJson, name: &str) {
    let socket_map = &config.mcu.pins.socket_pin_to_gpio;
    let x_map = &config.mcu.pins.x_pin_to_gpio;

    if x_map.is_some() && socket_map.is_none() {
        panic!(
            "{}: x_pin_to_gpio requires socket_pin_to_gpio to also be present",
            name
        );
    }
    if socket_map.is_none() && x_map.is_none() {
        return;
    }

    let non_signal = config.chip.pins.non_signal.as_ref().unwrap_or_else(|| {
        panic!(
            "{}: chip.pins.non_signal must be specified when socket_pin_to_gpio or x_pin_to_gpio is present",
            name
        )
    });

    for &pin in non_signal {
        if pin == 0 || pin > config.chip.pins.quantity {
            panic!(
                "{}: non_signal pin {} out of range 1-{}",
                name, pin, config.chip.pins.quantity
            );
        }
    }

    // GPIOs that socket/X GPIOs must not clash with
    let mut other_gpios: Vec<(&str, u8)> = vec![];
    for &pin in &config.mcu.pins.sel {
        other_gpios.push(("sel", pin));
    }
    #[allow(clippy::collapsible_if)]
    if let Some(usb) = &config.mcu.usb {
        if let Some(usb_pins) = &usb.pins {
            other_gpios.push(("usb_vbus", usb_pins.vbus));
        }
    }
    other_gpios.push(("status", config.mcu.pins.status));
    if let Some(neo) = config.mcu.pins.neo {
        other_gpios.push(("neo", neo));
    }

    let mut seen_gpios: HashMap<u8, (&str, u8)> = HashMap::new();

    if let Some(socket_map) = socket_map {
        for &pin in socket_map.keys() {
            if pin == 0 || pin > config.chip.pins.quantity {
                panic!(
                    "{}: socket_pin_to_gpio pin {} out of range 1-{}",
                    name, pin, config.chip.pins.quantity
                );
            }
            if non_signal.contains(&pin) {
                panic!(
                    "{}: socket pin {} appears in both socket_pin_to_gpio and non_signal",
                    name, pin
                );
            }
        }
        for pin in 1..=config.chip.pins.quantity {
            if !socket_map.contains_key(&pin) && !non_signal.contains(&pin) {
                panic!(
                    "{}: socket pin {} is not covered by socket_pin_to_gpio or non_signal",
                    name, pin
                );
            }
        }

        for (&pin, gpios) in socket_map {
            for &gpio in gpios {
                validate_pin_number(&config.mcu, gpio, "socket_pin_to_gpio", name);
                if let Some((prev_map, prev_pin)) =
                    seen_gpios.insert(gpio, ("socket_pin_to_gpio", pin))
                {
                    panic!(
                        "{}: GPIO {} used by both {} pin {} and socket_pin_to_gpio pin {}",
                        name, gpio, prev_map, prev_pin, pin
                    );
                }
            }
        }
    }

    if let Some(x_map) = x_map {
        if x_map.len() > 2 {
            panic!(
                "{}: x_pin_to_gpio has {} entries, maximum is 2",
                name,
                x_map.len()
            );
        }
        for (&pin, gpios) in x_map {
            if pin == 0 || pin > 2 {
                panic!("{}: x_pin_to_gpio pin {} out of range 1-2", name, pin);
            }
            for &gpio in gpios {
                validate_pin_number(&config.mcu, gpio, "x_pin_to_gpio", name);
                if let Some((prev_map, prev_pin)) = seen_gpios.insert(gpio, ("x_pin_to_gpio", pin))
                {
                    panic!(
                        "{}: GPIO {} used by both {} pin {} and x_pin_to_gpio pin {}",
                        name, gpio, prev_map, prev_pin, pin
                    );
                }
            }
        }
    }

    for (gpio, (map_name, pin)) in &seen_gpios {
        for (other_label, other_gpio) in &other_gpios {
            if gpio == other_gpio {
                panic!(
                    "{}: GPIO {} used by both {} pin {} and {}",
                    name, gpio, map_name, pin, other_label
                );
            }
        }
    }
}

// ---- Jumper-header parsing, validation and code generation ----

fn parse_role_token(name: &str, token: &str) -> HeaderRoleP {
    match token {
        "5v" => HeaderRoleP::Power5V,
        "gnd" => HeaderRoleP::Gnd,
        "run" => HeaderRoleP::Run,
        "bootsel" => HeaderRoleP::Bootsel,
        "swclk" => HeaderRoleP::Swclk,
        "swdio" => HeaderRoleP::Swdio,
        "x1" => HeaderRoleP::X1,
        "x2" => HeaderRoleP::X2,
        _ => {
            if let Some(letter) = token.strip_prefix("sel_") {
                let bytes = letter.as_bytes();
                if bytes.len() == 1 && bytes[0].is_ascii_lowercase() {
                    return HeaderRoleP::Select(bytes[0] - b'a');
                }
            }
            // Address line broken out on the header, e.g. "a17" = A17.
            if let Some(Ok(n)) = token.strip_prefix('a').map(str::parse::<u8>) {
                return HeaderRoleP::Addr(n);
            }
            panic!("{name}: invalid jumper_header role token '{token}'");
        }
    }
}

fn parse_slot(name: &str, tokens: &[String]) -> HeaderSlotP {
    if tokens.is_empty() {
        panic!("{name}: jumper_header slot has no role tokens (use \"nc\" or \"np\")");
    }
    if tokens.len() == 1 {
        match tokens[0].as_str() {
            "np" => return HeaderSlotP::NotPopulated,
            "nc" => return HeaderSlotP::NotConnected,
            _ => {}
        }
    }
    for t in tokens {
        if t == "nc" || t == "np" {
            panic!("{name}: jumper_header slot mixes '{t}' with other roles");
        }
    }
    if tokens.len() > 2 {
        panic!(
            "{name}: jumper_header slot has {} roles, at most 2 are allowed",
            tokens.len()
        );
    }
    HeaderSlotP::Roles(tokens.iter().map(|t| parse_role_token(name, t)).collect())
}

/// Parse a board's raw JSON jumper header into an ordered, structurally-valid
/// list of columns. Panics (failing the build) on any malformed entry.
pub fn parse_jumper_header(name: &str, header: &JsonJumperHeader) -> Vec<HeaderColumnP> {
    let mut cols: Vec<HeaderColumnP> = Vec::new();

    for (col_key, rows) in &header.columns {
        let col: u8 = col_key.parse().unwrap_or_else(|_| {
            panic!("{name}: jumper_header column key '{col_key}' is not a number")
        });
        if col == 0 {
            panic!("{name}: jumper_header column numbers are 1-based, found 0");
        }

        for row_key in rows.keys() {
            if !matches!(row_key.as_str(), "1" | "2" | "3") {
                panic!(
                    "{name}: jumper_header column {col} has invalid row '{row_key}' (expected 1, 2 or 3)"
                );
            }
        }

        let row1_tokens = rows
            .get("1")
            .unwrap_or_else(|| panic!("{name}: jumper_header column {col} is missing row 1"));
        let row2_tokens = rows
            .get("2")
            .unwrap_or_else(|| panic!("{name}: jumper_header column {col} is missing row 2"));

        cols.push(HeaderColumnP {
            col,
            row1: parse_slot(name, row1_tokens),
            row2: parse_slot(name, row2_tokens),
            row3: rows.get("3").map(|t| parse_slot(name, t)),
        });
    }

    cols.sort_by_key(|c| c.col);

    for pair in cols.windows(2) {
        if pair[0].col == pair[1].col {
            panic!("{name}: jumper_header has duplicate column {}", pair[0].col);
        }
    }

    cols
}

#[allow(clippy::wildcard_enum_match_arm)]
fn slot_roles(slot: &HeaderSlotP) -> &[HeaderRoleP] {
    match slot {
        HeaderSlotP::Roles(roles) => roles,
        _ => &[],
    }
}

/// Cross-check a parsed jumper header against the board's electrical pin
/// assignments, so the physical descriptor cannot drift from the `sel` /
/// `swclk_sel` / `swdio_sel` / `x1` / `x2` data. Panics (failing the build) on
/// any inconsistency.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn validate_jumper_header(name: &str, cols: &[HeaderColumnP], pins: &McuPins) {
    // Flatten to (col, row, slot).
    let mut slots: Vec<(u8, u8, &HeaderSlotP)> = Vec::new();
    for c in cols {
        slots.push((c.col, 1, &c.row1));
        slots.push((c.col, 2, &c.row2));
        if let Some(r3) = &c.row3 {
            slots.push((c.col, 3, r3));
        }
    }

    let mut select_bits: Vec<u8> = Vec::new();
    let mut swclk_locs: Vec<(u8, u8)> = Vec::new();
    let mut swdio_locs: Vec<(u8, u8)> = Vec::new();
    let mut x1_locs: Vec<(u8, u8)> = Vec::new();
    let mut x2_locs: Vec<(u8, u8)> = Vec::new();
    let mut addr_lines: Vec<u8> = Vec::new();

    for (col, row, slot) in &slots {
        for role in slot_roles(slot) {
            // The extra (row 3) pad carries only "extra config" roles - an X pin
            // or a high address line broken out on the header.
            if *row == 3
                && !matches!(
                    role,
                    HeaderRoleP::X1 | HeaderRoleP::X2 | HeaderRoleP::Addr(_)
                )
            {
                panic!(
                    "{name}: jumper_header row 3 (col {col}) may only carry X pins or address lines"
                );
            }
            match role {
                HeaderRoleP::Select(b) => select_bits.push(*b),
                HeaderRoleP::Swclk => swclk_locs.push((*col, *row)),
                HeaderRoleP::Swdio => swdio_locs.push((*col, *row)),
                HeaderRoleP::X1 => {
                    x1_locs.push((*col, *row));
                    if *row != 3 {
                        panic!("{name}: jumper_header X1 must be on row 3, found row {row}");
                    }
                }
                HeaderRoleP::X2 => {
                    x2_locs.push((*col, *row));
                    if *row != 3 {
                        panic!("{name}: jumper_header X2 must be on row 3, found row {row}");
                    }
                }
                HeaderRoleP::Addr(n) => addr_lines.push(*n),
                _ => {}
            }
        }
    }

    // Address lines broken out on the header must exist on the board.
    for &n in &addr_lines {
        if n as usize >= pins.addr.len() {
            panic!(
                "{name}: jumper_header address line A{n} out of range (board has {} address lines, A0..A{})",
                pins.addr.len(),
                pins.addr.len().saturating_sub(1)
            );
        }
    }

    // Image-select bits must be exactly 0..sel.len(), each present once.
    select_bits.sort_unstable();
    let expected: Vec<u8> = (0..pins.sel.len() as u8).collect();
    if select_bits != expected {
        panic!(
            "{name}: jumper_header select bits {:?} do not match the {} sel pin(s) (expected {:?})",
            select_bits,
            pins.sel.len(),
            expected
        );
    }

    // SWD multiplex consistency.
    check_swd(name, "swclk", &swclk_locs, pins.swclk_sel, &slots, pins);
    check_swd(name, "swdio", &swdio_locs, pins.swdio_sel, &slots, pins);

    // X-pin presence must match the board's x1/x2 GPIO definitions.
    if pins.x1.is_some() != !x1_locs.is_empty() {
        panic!("{name}: jumper_header X1 presence does not match the board's x1 pin");
    }
    if pins.x2.is_some() != !x2_locs.is_empty() {
        panic!("{name}: jumper_header X2 presence does not match the board's x2 pin");
    }
    if x1_locs.len() > 1 || x2_locs.len() > 1 {
        panic!("{name}: jumper_header defines X1 or X2 more than once");
    }
}

#[allow(clippy::wildcard_enum_match_arm)]
fn check_swd(
    name: &str,
    which: &str,
    locs: &[(u8, u8)],
    sel_gpio: u8,
    slots: &[(u8, u8, &HeaderSlotP)],
    pins: &McuPins,
) {
    // At most one pad may carry a given SWD signal.
    if locs.len() > 1 {
        panic!("{name}: jumper_header tags {which} more than once");
    }

    // The select bit (if any) sharing the SWD pad.
    let muxed_bit = |col: u8, row: u8| -> Option<u8> {
        let slot = slots
            .iter()
            .find(|(c, r, _)| *c == col && *r == row)
            .map(|(_, _, s)| *s)?;
        slot_roles(slot).iter().find_map(|role| match role {
            HeaderRoleP::Select(b) => Some(*b),
            _ => None,
        })
    };

    if sel_gpio == 255 {
        // This board does not route SWD onto an image-select pin. A standalone
        // SWD pad is fine (e.g. a 2-select board whose SWD pins share the header
        // but are not image selects); it just must not sit on a select pad,
        // which would imply a multiplexing the board does not declare.
        if let Some(&(col, row)) = locs.first() {
            assert!(
                muxed_bit(col, row).is_none(),
                "{name}: jumper_header {which} shares an image-select pad (col {col}) but no {which}_sel pin is set"
            );
        }
        return;
    }

    // SWD is multiplexed onto an image-select pin: the header must tag it, on the
    // select pad whose GPIO is {which}_sel.
    if locs.len() != 1 {
        panic!("{name}: jumper_header must tag {which} exactly once ({which}_sel is set)");
    }
    let (col, row) = locs[0];
    let bit = muxed_bit(col, row).unwrap_or_else(|| {
        panic!("{name}: jumper_header {which} pad (col {col}) is not also an image-select pad")
    });
    let gpio = pins.sel.get(bit as usize).copied().unwrap_or_else(|| {
        panic!("{name}: jumper_header {which} select bit {bit} is out of range")
    });
    if gpio != sel_gpio {
        panic!(
            "{name}: jumper_header {which} is on select bit {bit} (GPIO {gpio}) but {which}_sel is GPIO {sel_gpio}"
        );
    }
}

fn format_role(role: &HeaderRoleP) -> String {
    match role {
        HeaderRoleP::Power5V => "HeaderRole::Power5V".to_string(),
        HeaderRoleP::Gnd => "HeaderRole::Gnd".to_string(),
        HeaderRoleP::Run => "HeaderRole::Run".to_string(),
        HeaderRoleP::Bootsel => "HeaderRole::Bootsel".to_string(),
        HeaderRoleP::Select(b) => format!("HeaderRole::Select({b})"),
        HeaderRoleP::Swclk => "HeaderRole::Swclk".to_string(),
        HeaderRoleP::Swdio => "HeaderRole::Swdio".to_string(),
        HeaderRoleP::X1 => "HeaderRole::X1".to_string(),
        HeaderRoleP::X2 => "HeaderRole::X2".to_string(),
        HeaderRoleP::Addr(n) => format!("HeaderRole::Addr({n})"),
    }
}

fn format_slot(slot: &HeaderSlotP) -> String {
    match slot {
        HeaderSlotP::NotPopulated => "HeaderSlot::NotPopulated".to_string(),
        HeaderSlotP::NotConnected => "HeaderSlot::NotConnected".to_string(),
        HeaderSlotP::Roles(roles) => {
            let inner = roles.iter().map(format_role).collect::<Vec<_>>().join(", ");
            format!("HeaderSlot::Roles(&[{inner}])")
        }
    }
}

/// Emit the Rust literal (a `JumperHeader { .. }` expression) for a parsed
/// header, for embedding in the generated board accessor.
pub fn format_jumper_header(cols: &[HeaderColumnP]) -> String {
    let col_strs: Vec<String> = cols
        .iter()
        .map(|c| {
            let row3 = match &c.row3 {
                Some(s) => format!("Some({})", format_slot(s)),
                None => "None".to_string(),
            };
            format!(
                "HeaderColumn {{ col: {}, row1: {}, row2: {}, row3: {} }}",
                c.col,
                format_slot(&c.row1),
                format_slot(&c.row2),
                row3
            )
        })
        .collect();
    format!("JumperHeader {{ columns: &[{}] }}", col_strs.join(", "))
}
