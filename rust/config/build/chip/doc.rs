// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use super::validation::{ChipFunction, ChipType, ChipTypesConfig, ControlLineType};
use std::collections::BTreeMap;

/// Generate complete ROM types markdown documentation
pub fn generate_chip_types_markdown(config: &ChipTypesConfig) -> String {
    let mut doc = String::new();

    // Header
    doc.push_str(r#"# Chip Type Specifications

This document provides detailed specifications for the different Chip types One ROM supports, and aims to support in future, including pinouts, control lines, and programming requirements.

The document is auto-generated from the [json/rom-types.json](/rust/config/json/rom-types.json) configuration file.  That file was created by researching datasheets for the various Chip types.

Some of the pin names have been modified from the datasheet values for consistency beween Chip types:

- /OE on 2704/2408 is called Program, but serves as /OE when in read mode.  Other 27xx ROMs use /OE for that pin, hence the /OE name is used here. 
- Similarly /CE on 2704/2708 ROMs is called /CS, but is called /CE for consistency with other ROM types.
- 23256/23512 chip select lines are often called CE/OE on datasheets, but are mask programmable to be active high or low, hence these are referred to within this doc as CS lines, like the other 23xx ROMs.

There are also some other inconsistencies between types:

- 2332's CS2 is pin 21 and pin 18 is A11.  On the 2316, CS2 is pin 18, and CS3 pin 21.
- The 2332's A11 is pin18, but the 2732's A11 is pin 21.

## Contents

- [24-pin Mask ROM Family (23xx)](#24-pin-mask-rom-family-23xx)
- [28-pin Mask ROM Family (23xxx)](#28-pin-mask-rom-family-23xx)
- [32-pin Mask ROM Family (23xxx)](#32-pin-mask-rom-family-23xx)
- [24-pin EPROM Family (27xx)](#24-pin-eprom-family-27xx)
- [28-pin EPROM Family (2764 and 27xxx)](#28-pin-eprom-family-27xx)
- [32-pin EPROM Family (27xxx)](#32-pin-eprom-family-27xx)
- [40-pin EPROM Family (27xxx)](#40-pin-eprom-family-27xx)
- [24-pin EEPROM Family (28Cxx)](#24-pin-eeprom-family-28cxx)
- [28-pin EEPROM Family (28Cxx)](#28-pin-eeprom-family-28cxx)
- [32-pin EEPROM Family (28Cxx)](#32-pin-eeprom-family-28cxx)
- [RAM Chips](#ram-chips)
- [Pin Function Comparison](#pin-function-comparison)
- [Detailed Pinouts](#detailed-pinouts)

"#);

    // Group ROMs by family
    let families = group_by_family(config);

    // Family comparison tables
    if let Some(roms) = families.get("mask_24pin") {
        doc.push_str(&generate_family_comparison_table(
            "24-pin Mask ROM Family (23xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("mask_28pin") {
        doc.push_str(&generate_family_comparison_table(
            "28-pin Mask ROM Family (23xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("mask_32pin") {
        doc.push_str(&generate_family_comparison_table(
            "32-pin Mask ROM Family (23xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eprom_24pin") {
        doc.push_str(&generate_family_comparison_table(
            "24-pin EPROM Family (27xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eprom_28pin") {
        doc.push_str(&generate_family_comparison_table(
            "28-pin EPROM Family (27xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eprom_32pin") {
        doc.push_str(&generate_family_comparison_table(
            "32-pin EPROM Family (27xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eprom_40pin") {
        doc.push_str(&generate_family_comparison_table(
            "40-pin EPROM Family (27xx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eeprom_24pin") {
        doc.push_str(&generate_family_comparison_table(
            "24-pin EEPROM Family (28Cxx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eeprom_28pin") {
        doc.push_str(&generate_family_comparison_table(
            "28-pin EEPROM Family (28Cxx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(roms) = families.get("eeprom_32pin") {
        doc.push_str(&generate_family_comparison_table(
            "32-pin EEPROM Family (28Cxx)",
            roms,
            config,
        ));
        doc.push('\n');
    }

    if let Some(rams) = families.get("ram_chips") {
        doc.push_str(&generate_family_comparison_table("RAM Chips", rams, config));
        doc.push('\n');
    }

    // Pin comparison tables
    doc.push_str("## Pin Function Comparison\n\n");
    doc.push_str(&generate_pin_comparison_table(config, 24));
    doc.push('\n');
    doc.push_str(&generate_pin_comparison_table(config, 28));
    doc.push('\n');
    doc.push_str(&generate_pin_comparison_table(config, 32));
    doc.push('\n');
    doc.push_str(&generate_pin_comparison_table(config, 40));
    doc.push('\n');

    // Detailed pinout tables
    doc.push_str("## Detailed Pinouts\n\n");
    let sorted_roms = get_sorted_chip_types(config);
    for (type_name, chip_type) in sorted_roms {
        if chip_type.function.is_plugin() {
            continue; // Skip plugins for now - they don't fit into the standard categories
        }
        doc.push_str(&generate_detailed_pinout(type_name, chip_type));
        doc.push('\n');
    }

    doc
}

/// Group ROM types by family (mask/eprom and pin count)
fn group_by_family(config: &ChipTypesConfig) -> BTreeMap<&'static str, Vec<(&String, &ChipType)>> {
    let mut families: BTreeMap<&'static str, Vec<(&String, &ChipType)>> = BTreeMap::new();

    for (type_name, chip_type) in &config.chip_types {
        let key = if type_name.starts_with("23") && chip_type.function == ChipFunction::Rom {
            if chip_type.pins == 24 {
                "mask_24pin"
            } else if chip_type.pins == 28 {
                "mask_28pin"
            } else if chip_type.pins == 32 {
                "mask_32pin"
            } else {
                panic!("Unexpected pin count for 23xx ROM: {}", chip_type.pins);
            }
        } else if type_name.starts_with("27") && chip_type.function == ChipFunction::Rom {
            if chip_type.pins == 24 {
                "eprom_24pin"
            } else if chip_type.pins == 28 {
                "eprom_28pin"
            } else if chip_type.pins == 32 {
                "eprom_32pin"
            } else if chip_type.pins == 40 {
                "eprom_40pin"
            } else {
                panic!("Unexpected pin count for 27xx ROM: {}", chip_type.pins);
            }
        } else if type_name.starts_with("28") && chip_type.function == ChipFunction::Rom {
            if chip_type.pins == 24 {
                "eeprom_24pin"
            } else if chip_type.pins == 28 {
                "eeprom_28pin"
            } else if chip_type.pins == 32 {
                "eeprom_32pin"
            } else {
                panic!("Unexpected pin count for 28Cxx EEPROM: {}", chip_type.pins);
            }
        } else if chip_type.function == ChipFunction::Ram {
            "ram_chips"
        } else {
            continue;
        };

        families
            .entry(key)
            .or_default()
            .push((type_name, chip_type));
    }

    // Sort each family by size
    for roms in families.values_mut() {
        roms.sort_by_key(|(_, rom)| rom.size);
    }

    families
}

/// Generate comparison table for a ROM family
fn generate_family_comparison_table(
    title: &str,
    roms: &[(&String, &ChipType)],
    _config: &ChipTypesConfig,
) -> String {
    let mut table = String::new();

    table.push_str(&format!("## {}\n\n", title));
    table.push_str("| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |\n");
    table.push_str("|-----------|---------|------|---------------|---------------|-------------|-----------|\n");

    for (type_name, chip_type) in roms {
        let size_str = format_size(chip_type.size);
        let addr_lines = format!(
            "{} (A0-A{})",
            chip_type.address.len(),
            chip_type.address.len() - 1
        );

        let control_str = format_control_lines(chip_type);
        let prog_str = format_programming_pins(chip_type);

        let aliases_str = chip_type
            .aliases
            .as_ref()
            .map(|a| a.join(", "))
            .unwrap_or_default();
        let supported_str = if chip_type.supported.is_some() {
            "✓"
        } else {
            "✗"
        };
        table.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            type_name, aliases_str, size_str, addr_lines, control_str, prog_str, supported_str
        ));
    }

    table
}

/// Generate pin comparison table for a given package size
fn generate_pin_comparison_table(config: &ChipTypesConfig, pin_count: u8) -> String {
    let mut table = String::new();

    // Get all ROMs with the specified pin count sorted by size
    let mut roms: Vec<_> = config
        .chip_types
        .iter()
        .filter(|(_, rom)| rom.pins == pin_count)
        .collect();
    roms.sort_by_key(|(name, rom)| {
        let family = if name.starts_with("23") { 0 } else { 1 };
        (family, rom.size, *name)
    });

    if roms.is_empty() {
        return table;
    }

    table.push_str(&format!("### {pin_count}-pin Package\n\n"));
    table.push_str("| Pin |");
    for (type_name, _) in &roms {
        table.push_str(&format!(" {} |", type_name));
    }
    table.push_str("\n|-----|");
    for _ in &roms {
        table.push_str("------|");
    }
    table.push('\n');

    // Generate row for each pin
    for pin in 1..=pin_count {
        table.push_str(&format!("| {} |", pin));
        for (_, chip_type) in &roms {
            let function = get_pin_function(pin, chip_type);
            table.push_str(&format!(" {} |", function));
        }
        table.push('\n');
    }

    table
}

/// Generate detailed pinout for a single ROM type
fn generate_detailed_pinout(type_name: &str, chip_type: &ChipType) -> String {
    let mut doc = String::new();

    doc.push_str(&format!(
        "### {} - {}\n\n",
        type_name, chip_type.description
    ));
    doc.push_str(&format!("**Package:** {}-pin DIP  \n", chip_type.pins));
    doc.push_str(&format!("**Capacity:** {} bytes  \n", chip_type.size));

    // Control line summary
    let control_summary = format_control_lines_detailed(chip_type);
    doc.push_str(&format!("**Control:** {}  \n\n", control_summary));

    // Pin table
    doc.push_str("| Function | Pins | Notes |\n");
    doc.push_str("|----------|------|-------|\n");

    // Address lines
    let addr_pins: Vec<String> = chip_type.address.iter().map(|p| p.to_string()).collect();
    doc.push_str(&format!(
        "| Address (A0-A{}) | {} | {} address lines |\n",
        chip_type.address.len() - 1,
        addr_pins.join(","),
        chip_type.address.len()
    ));

    // Data lines
    let data_pins: Vec<String> = chip_type.data.iter().map(|p| p.to_string()).collect();
    doc.push_str(&format!(
        "| Data (D0-D7) | {} | 8 data lines |\n",
        data_pins.join(",")
    ));

    // Control lines
    let mut control_lines: Vec<_> = chip_type.control.iter().collect();
    control_lines.sort_by_key(|(name, _)| *name);

    for (name, control) in control_lines {
        let polarity = match control.line_type {
            ControlLineType::Configurable => "Configurable polarity",
            ControlLineType::FixedActiveLow => "Active low",
        };
        doc.push_str(&format!(
            "| {} | {} | {} |\n",
            name.to_uppercase(),
            control.pin,
            polarity
        ));
    }

    // Programming pins
    if let Some(ref prog) = chip_type.programming {
        if let Some(ref vpp) = prog.vpp {
            doc.push_str(&format!(
                "| VPP | {} | {} during read |\n",
                vpp.pin,
                format_read_state(&vpp.read_state)
            ));
        }
        if let Some(ref pgm) = prog.pgm {
            doc.push_str(&format!(
                "| /PGM | {} | {} during read |\n",
                pgm.pin,
                format_read_state(&pgm.read_state)
            ));
        }
        if let Some(ref pe) = prog.pe {
            doc.push_str(&format!(
                "| PE | {} | {} during read |\n",
                pe.pin,
                format_read_state(&pe.read_state)
            ));
        }
    }

    // Power pins
    if let Some(ref power_pins) = chip_type.power {
        for power_pin in power_pins {
            doc.push_str(&format!(
                "| {} | {} | {} |\n",
                power_pin.name, power_pin.pin, power_pin.voltage
            ));
        }
    }

    doc
}

// Helper functions

fn get_sorted_chip_types(config: &ChipTypesConfig) -> Vec<(&String, &ChipType)> {
    let mut types: Vec<_> = config.chip_types.iter().collect();
    types.sort_by_key(|(name, chip_type)| {
        let family = if name.starts_with("23") { 0 } else { 1 };
        (family, chip_type.size, *name)
    });
    types
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

fn format_control_lines(chip_type: &ChipType) -> String {
    let mut lines = Vec::new();
    let mut control_vec: Vec<_> = chip_type.control.iter().collect();
    control_vec.sort_by_key(|(name, _)| *name);

    for (name, control) in control_vec {
        let polarity = match control.line_type {
            ControlLineType::Configurable => "",
            ControlLineType::FixedActiveLow => "/",
        };
        lines.push(format!(
            "{}{} (pin {})",
            polarity,
            name.to_uppercase(),
            control.pin
        ));
    }

    if lines.is_empty() {
        "None".to_string()
    } else {
        lines.join(", ")
    }
}

fn format_control_lines_detailed(chip_type: &ChipType) -> String {
    let count = chip_type.control.len();
    if count == 0 {
        return "None".to_string();
    }

    let has_configurable = chip_type
        .control
        .values()
        .any(|c| c.line_type == ControlLineType::Configurable);

    if has_configurable {
        format!(
            "{} configurable CS line{}",
            count,
            if count > 1 { "s" } else { "" }
        )
    } else {
        let names: Vec<_> = chip_type
            .control
            .keys()
            .map(|n| format!("/{}", n.to_uppercase()))
            .collect();
        names.join(", ")
    }
}

fn format_programming_pins(chip_type: &ChipType) -> String {
    if let Some(ref prog) = chip_type.programming {
        let mut parts = Vec::new();

        if let Some(ref vpp) = prog.vpp {
            parts.push(format!(
                "VPP: pin {} ({})",
                vpp.pin,
                format_read_state(&vpp.read_state)
            ));
        }

        if let Some(ref pgm) = prog.pgm {
            parts.push(format!(
                "/PGM: pin {} ({})",
                pgm.pin,
                format_read_state(&pgm.read_state)
            ));
        }

        if parts.is_empty() {
            "None".to_string()
        } else {
            parts.join("; ")
        }
    } else {
        "None".to_string()
    }
}

fn format_read_state(state: &str) -> String {
    match state {
        "vcc" => "VCC during read".to_string(),
        "high" => "High during read".to_string(),
        "low" => "Low during read".to_string(),
        "chip_select" => "Acts as /OE".to_string(),
        _ => state.to_string(),
    }
}

fn get_pin_function(pin: u8, chip_type: &ChipType) -> String {
    let mut functions = Vec::new();

    // Check address lines
    if let Some(pos) = chip_type.address.iter().position(|&p| p == pin) {
        return format!("A{}", pos);
    }

    // Check data lines
    if let Some(pos) = chip_type.data.iter().position(|&p| p == pin) {
        return format!("D{}", pos);
    }

    // Check control lines
    for (name, control) in &chip_type.control {
        if control.pin == pin {
            let prefix = match control.line_type {
                ControlLineType::FixedActiveLow => "/",
                ControlLineType::Configurable => "",
            };
            functions.push(format!("{}{}", prefix, name.to_uppercase()));
        }
    }

    // Check programming pins
    #[allow(clippy::collapsible_if)]
    if let Some(ref prog) = chip_type.programming {
        if let Some(ref vpp) = prog.vpp {
            if vpp.pin == pin {
                functions.push("VPP".to_string());
            }
        }
        if let Some(ref pgm) = prog.pgm {
            if pgm.pin == pin {
                functions.push("/PGM".to_string());
            }
        }
        if let Some(ref pe) = prog.pe {
            if pe.pin == pin {
                functions.push("PE".to_string());
            }
        }
    }

    if !functions.is_empty() {
        return functions.join("+");
    }

    // Check power pins
    if let Some(ref power_pins) = chip_type.power {
        for power_pin in power_pins {
            if power_pin.pin == pin {
                return power_pin.name.clone();
            }
        }
    }

    "NC".to_string()
}
