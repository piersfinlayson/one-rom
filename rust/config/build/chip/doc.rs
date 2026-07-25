// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use super::validation::{ChipType, ChipTypesConfig, ControlLine, ControlLineType};
use super::{CHIP_FAMILIES, chip_family};
use std::collections::BTreeMap;

/// Package pin counts to emit pin function comparison tables for.
///
/// Mirrors `VALID_PIN_COUNTS` in validation.  A pin count with no chip types
/// produces no table.
const PIN_COUNTS: &[u8] = &[24, 28, 32, 40];

/// Generate complete ROM types markdown documentation
pub fn generate_chip_types_markdown(config: &ChipTypesConfig) -> String {
    let mut doc = String::new();

    // Group ROMs by family.  Done up front, so that the contents can list only
    // those families which actually have chip types in them.
    let families = group_by_family(config);

    // Header
    doc.push_str(r#"# Chip Type Specifications

This document provides detailed specifications for the different Chip types One ROM supports, and aims to support in future, including pinouts, control lines, and programming requirements.

The document is auto-generated from the [json/chip-types.json](/rust/config/json/chip-types.json) configuration file.  That file was created by researching datasheets for the various Chip types.

Some of the pin names have been modified from the datasheet values for consistency beween Chip types:

- /OE on 2704/2408 is called Program, but serves as /OE when in read mode.  Other 27xx ROMs use /OE for that pin, hence the /OE name is used here. 
- Similarly /CE on 2704/2708 ROMs is called /CS, but is called /CE for consistency with other ROM types.
- 23256/23512 chip select lines are often called CE/OE on datasheets, but are mask programmable to be active high or low, hence these are referred to within this doc as CS lines, like the other 23xx ROMs.
- Chips whose enables have a polarity fixed by the silicon use CE/OE where every enable is active low (the JEDEC convention followed by the 27xx and 28xx families).  Where the enables are not all active low, CS lines are used instead: the HM7641 has CS1 and CS2 fixed active low, and CS3 and CS4 fixed active high.  A leading `/` in the tables below indicates an active low line.

There are also some other inconsistencies between types:

- 2332's CS2 is pin 21 and pin 18 is A11.  On the 2316, CS2 is pin 18, and CS3 pin 21.
- The 2332's A11 is pin18, but the 2732's A11 is pin 21.

## Contents

"#);

    // Contents, generated from the families actually present so that it cannot
    // drift out of step with the sections below.
    for family in CHIP_FAMILIES {
        if families.contains_key(family.key) {
            doc.push_str(&format!(
                "- [{}](#{})\n",
                family.doc_heading,
                heading_anchor(family.doc_heading)
            ));
        }
    }
    doc.push_str("- [Pin Function Comparison](#pin-function-comparison)\n");
    doc.push_str("- [Detailed Pinouts](#detailed-pinouts)\n\n");

    // Family comparison tables
    for family in CHIP_FAMILIES {
        if let Some(chips) = families.get(family.key) {
            doc.push_str(&generate_family_comparison_table(
                family.doc_heading,
                chips,
                config,
            ));
            doc.push('\n');
        }
    }

    // Pin comparison tables
    doc.push_str("## Pin Function Comparison\n\n");
    for pin_count in PIN_COUNTS {
        doc.push_str(&generate_pin_comparison_table(config, *pin_count));
        doc.push('\n');
    }

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

/// Convert a markdown heading into the anchor GitHub will generate for it
///
/// Punctuation is dropped, the result is lowercased, and spaces become hyphens.
fn heading_anchor(heading: &str) -> String {
    heading
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .collect::<String>()
        .to_lowercase()
        .replace(' ', "-")
}

/// Group ROM types by family
///
/// Classification is shared with the crate documentation generator, so that the
/// two cannot disagree about which family a chip type belongs to.
fn group_by_family(config: &ChipTypesConfig) -> BTreeMap<&'static str, Vec<(&String, &ChipType)>> {
    let mut families: BTreeMap<&'static str, Vec<(&String, &ChipType)>> = BTreeMap::new();

    for (type_name, chip_type) in &config.chip_types {
        // Plugins have no family - they are not chips.
        let Some(family) = chip_family(type_name, chip_type) else {
            continue;
        };

        families
            .entry(family.key)
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
        "| Data (D0-D{}) | {} | {} data lines |\n",
        chip_type.data.len() - 1,
        data_pins.join(","),
        chip_type.data.len()
    ));

    // Control lines
    let mut control_lines: Vec<_> = chip_type.control.iter().collect();
    control_lines.sort_by_key(|(name, _)| *name);

    for (name, control) in control_lines {
        let polarity = match control.line_type {
            ControlLineType::Configurable => "Configurable polarity",
            ControlLineType::FixedActiveLow => "Active low",
            ControlLineType::FixedActiveHigh => "Active high",
        };
        doc.push_str(&format!(
            "| {} | {} | {} |\n",
            format_control_line_name(name, control),
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
    if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

/// Format a control line name with its polarity prefix
///
/// Active low lines are prefixed with `/`. Active high and configurable lines
/// are not - a configurable line has no polarity until the user assigns one.
fn format_control_line_name(name: &str, control: &ControlLine) -> String {
    let prefix = match control.line_type {
        ControlLineType::FixedActiveLow => "/",
        ControlLineType::FixedActiveHigh | ControlLineType::Configurable => "",
    };
    format!("{}{}", prefix, name.to_uppercase())
}

fn format_control_lines(chip_type: &ChipType) -> String {
    let mut lines = Vec::new();
    let mut control_vec: Vec<_> = chip_type.control.iter().collect();
    control_vec.sort_by_key(|(name, _)| *name);

    for (name, control) in control_vec {
        lines.push(format!(
            "{} (pin {})",
            format_control_line_name(name, control),
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
    if chip_type.control.is_empty() {
        return "None".to_string();
    }

    // Configurable lines are summarised by count, as their polarity is not
    // known until the user configures them. Fixed lines are named, as their
    // polarity is a property of the chip. A chip may have both - the 23C1001,
    // for instance, has fixed /CE and /OE alongside configurable CS1 and CS2.
    // BTreeMap iterates in key order, so the named lines come out sorted.
    let mut configurable = 0usize;
    let mut fixed = Vec::new();

    for (name, control) in &chip_type.control {
        if control.line_type == ControlLineType::Configurable {
            configurable += 1;
        } else {
            fixed.push(format_control_line_name(name, control));
        }
    }

    let mut parts = Vec::new();
    if configurable > 0 {
        parts.push(format!(
            "{} configurable CS line{}",
            configurable,
            if configurable > 1 { "s" } else { "" }
        ));
    }
    if !fixed.is_empty() {
        parts.push(fixed.join(", "));
    }

    parts.join(", ")
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

        if let Some(ref pe) = prog.pe {
            parts.push(format!(
                "PE: pin {} ({})",
                pe.pin,
                format_read_state(&pe.read_state)
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
        "x" => "Don't care during read".to_string(),
        "word_size" => "Selects word size during read".to_string(),
        _ => state.to_string(),
    }
}

fn get_pin_function(pin: u8, chip_type: &ChipType) -> String {
    let mut functions = Vec::new();

    // A pin may carry more than one function, so every source is checked rather
    // than returning on the first match.  On 40-pin parts the lowest address
    // line shares a pin with the highest data line.

    // Check address lines
    if let Some(pos) = chip_type.address.iter().position(|&p| p == pin) {
        functions.push(format!("A{}", pos));
    }

    // Check data lines
    if let Some(pos) = chip_type.data.iter().position(|&p| p == pin) {
        functions.push(format!("D{}", pos));
    }

    // Check control lines
    for (name, control) in &chip_type.control {
        if control.pin == pin {
            functions.push(format_control_line_name(name, control));
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
