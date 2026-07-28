// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::inspect::{
    InspectGpioArgs, InspectHeaderArgs, InspectImageArgs, InspectInfoArgs, InspectPeekLiveArgs,
    InspectPeekMemoryArgs, InspectSlotsArgs, InspectSocketArgs, InspectTelemetryArgs,
};
use crate::board_view::{gpio_header_role, gpio_rom_function, gpio_system_function};
use crate::utils::{
    active_chip_type, check_device, check_device_running, check_live_read_write, print_hex_dump,
    resolve_board,
};
use onerom_cli::CliFetch;
use onerom_cli::LIVE_ROM_BASE;
use onerom_cli::plugin::{PluginOrigin, PluginType, resolve_plugin_display};
use onerom_cli::usb::{GpioEntry, GpioUse, get_caps, gpio_query, gpio_query_all, read_memory};
use onerom_cli::{Device, Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::PinTolerance;
use onerom_fw_parser::{ParsedDevice, SdrrCsState, SlotKind};

pub async fn cmd_info(options: &Options, args: &InspectInfoArgs) -> Result<(), Error> {
    // Print the device summary
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();

    println!("{device}");

    // Print the detailed device information as JSON if available
    if let Some(onerom) = device.onerom.as_ref() {
        if let Some(sdrr) = onerom.as_original() {
            if let Some(info) = sdrr.flash.as_ref() {
                let json =
                    serde_json::to_string_pretty(info).map_err(|e| Error::Other(e.to_string()))?;
                println!("Flash information:");
                println!("{json}");
            }
            if let Some(info) = sdrr.ram.as_ref() {
                let json =
                    serde_json::to_string_pretty(info).map_err(|e| Error::Other(e.to_string()))?;
                println!("Runtime information:");
                println!("{json}");
            }
        } else if let Some(schema) = onerom.as_schema() {
            // A schema device dumps as a single tree: unlike the original
            // format, whose flash and RAM information are siblings, the
            // metadata and runtime information are both nested within the info
            // header, so one dump covers the lot.
            if let Some(info) = schema.info() {
                let json =
                    serde_json::to_string_pretty(info).map_err(|e| Error::Other(e.to_string()))?;
                println!("Device information:");
                println!("{json}");
            }
        }
    }

    Ok(())
}

pub async fn cmd_telemetry(options: &Options, args: &InspectTelemetryArgs) -> Result<(), Error> {
    check_device(options, args, true)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("inspect telemetry".into()))
}

/// Print a device's slot configuration.
///
/// Plugins are presented separately from ROM slots and by their friendly name:
/// an official plugin (image source under `images.onerom.org`) shows its
/// manifest display name, a user/sideloaded one its file stem. The manifest
/// lookup is best-effort - a network failure degrades the name to the slug, it
/// never fails the listing. ROM slots are numbered from 0, excluding plugins,
/// so the first real ROM is "Slot 0".
///
/// `--verbose` adds, per plugin, its image source and (for official plugins)
/// version and description; and, per ROM slot, its flash location.
pub async fn output_slot_info(
    device: &Device,
    options: &Options,
    prefix: &str,
) -> Result<(), Error> {
    print!("{prefix}");
    println!("{device}");

    let verbose = options.verbose;

    // Device identity sits directly beneath the header, before slot detail.
    if verbose && let Some(line) = device.mcu_chip_id_line() {
        print!("{prefix}");
        println!("  {line}");
    }

    let parsed = device.onerom.as_ref().ok_or_else(|| {
        Error::Other("No recognised information found on device flash".to_string())
    })?;

    // First pass over the neutral slot view: split plugin slots from ROM slots.
    // ROM slots are renumbered from 0 via the view's `user_index` (which counts
    // ROM slots only); the absolute `slot_index` is retained so the
    // format-specific detail below can be read from the matching Original/Schema
    // slot. Plugin slots keep only their image source, resolved to a name later.
    let mut plugin_slots: Vec<(usize, Option<String>)> = Vec::new();
    let mut rom_slots: Vec<(usize, usize, bool)> = Vec::new();
    for slot in parsed.slots() {
        match slot.kind {
            SlotKind::Plugin => {
                let source = slot
                    .roms()
                    .next()
                    .and_then(|r| r.filename.map(|s| s.to_string()));
                plugin_slots.push((slot.slot_index, source));
            }
            SlotKind::Rom => {
                // A ROM slot always has a user_index.
                let user_index = slot.user_index.unwrap_or(0);
                rom_slots.push((slot.slot_index, user_index, slot.active));
            }
        }
    }

    // Plugins, presented separately and by friendly name.
    if !plugin_slots.is_empty() {
        print!("{prefix}");
        println!("  Plugins:");
        for (slot_index, source) in &plugin_slots {
            output_plugin(prefix, verbose, *slot_index, source.as_deref()).await;
        }
    }

    // ROM slot count and the active marker both use the plugin-excluding
    // numbering.
    let rom_count = rom_slots.len();
    let active_user_index = rom_slots
        .iter()
        .find(|(_, _, active)| *active)
        .map(|(_, user_index, _)| *user_index);
    let active_str = active_user_index
        .map(|i| format!(" - Slot {i} is active"))
        .unwrap_or_default();
    print!("{prefix}");
    println!(
        "  Configured with {rom_count} slot{}{}",
        if rom_count == 1 { "" } else { "s" },
        active_str
    );

    // Second pass: print each ROM slot's detail, reaching into the
    // format-specific data by absolute `slot_index`.
    match parsed {
        ParsedDevice::Original(sdrr) => {
            let info = sdrr.flash.as_ref().ok_or_else(|| {
                Error::Other("No recognised information found on device flash".to_string())
            })?;

            for (slot_index, user_index, active) in &rom_slots {
                let set = &info.rom_sets[*slot_index];
                let active_marker = if *active { " (active)" } else { "" };
                print!("{prefix}");
                println!("  Slot {user_index}{active_marker}:");

                if verbose {
                    print!("{prefix}");
                    println!(
                        "    Flash location 0x{:08x} size 0x{:08x} bytes",
                        set.data_ptr, set.size
                    );
                }

                if let Some(overrides) = &set.firmware_overrides {
                    print!("{prefix}");
                    println!("    Firmware overrides:");
                    if let Some(led) = &overrides.led {
                        print!("{prefix}");
                        println!(
                            "      Status LED: {}",
                            if led.enabled { "on" } else { "off" }
                        );
                    }
                    if let Some(fire) = &overrides.fire {
                        if let Some(freq) = fire.cpu_freq {
                            print!("{prefix}");
                            println!("      CPU frequency: {freq}");
                        }
                        if let Some(vreg) = &fire.vreg {
                            print!("{prefix}");
                            println!("      CPU voltage: {vreg}");
                        }
                        if let Some(serve_mode) = &fire.serve_mode {
                            print!("{prefix}");
                            println!("      Serve mode: {serve_mode}");
                        }
                        if !fire.rom_dma_preload {
                            print!("{prefix}");
                            println!("      ROM DMA preload disabled");
                        }
                        if fire.force_16_bit {
                            print!("{prefix}");
                            println!("      Force 16-bit ROM enabled");
                        }
                    }
                    if let Some(debug) = &overrides.swd {
                        print!("{prefix}");
                        println!(
                            "      SWD: {}",
                            if debug.swd_enabled { "on" } else { "off" }
                        );
                    }
                }

                for (j, rom) in set.roms.iter().enumerate() {
                    let mut cs = String::new();
                    if rom.cs1_state != SdrrCsState::NotUsed {
                        cs.push_str(&format!("Chip Select 1: {} ", rom.cs1_state));
                    }
                    if rom.cs2_state != SdrrCsState::NotUsed {
                        cs.push_str(&format!("Chip Select 2: {} ", rom.cs2_state));
                    }
                    if rom.cs3_state != SdrrCsState::NotUsed {
                        cs.push_str(&format!("Chip Select 3: {} ", rom.cs3_state));
                    }
                    let rom_type = rom.rom_type;
                    print!("{prefix}");
                    println!("    Chip {j}: {rom_type} {cs}");
                    if let Some(filename) = &rom.filename {
                        print!("{prefix}");
                        println!("      Image source: {filename}");
                    }
                }
            }
            Ok(())
        }

        ParsedDevice::Schema(onerom) => {
            let metadata = onerom
                .metadata()
                .ok_or_else(|| Error::Other("No metadata found on device flash".to_string()))?;

            for (slot_index, user_index, active) in &rom_slots {
                let slot = &metadata.rom_slots[*slot_index];
                let active_marker = if *active { " (active)" } else { "" };
                print!("{prefix}");
                println!("  Slot {user_index}{active_marker}:");

                if verbose {
                    print!("{prefix}");
                    let data_addr = slot
                        .data
                        .addr()
                        .map(|a| format!("{a:#010x}"))
                        .unwrap_or_else(|| "(null)".to_string());
                    println!(
                        "    Flash location {data_addr}  size {:#x} bytes",
                        slot.size
                    );
                }

                #[allow(clippy::collapsible_if)]
                if let Some(overrides) = &slot.firmware_overrides {
                    if overrides.any_present() {
                        print!("{prefix}");
                        println!("    Firmware overrides:");
                        if let Some(enabled) = overrides.led_enabled() {
                            print!("{prefix}");
                            println!("      Status LED: {}", if enabled { "on" } else { "off" });
                        }
                        if let Some(freq) = overrides.cpu_freq() {
                            print!("{prefix}");
                            println!("      CPU frequency: {freq}MHz");
                        }
                        if let Some(vreg) = overrides.vreg() {
                            print!("{prefix}");
                            println!("      CPU voltage: {vreg}");
                        }
                        if let Some(overclock) = overrides.overclock_enabled() {
                            print!("{prefix}");
                            println!(
                                "      Overclock: {}",
                                if overclock { "enabled" } else { "disabled" }
                            );
                        }
                        if let Some(swd) = overrides.swd_enabled() {
                            print!("{prefix}");
                            println!("      SWD: {}", if swd { "on" } else { "off" });
                        }
                    }
                }

                for (j, rom) in slot.roms.iter().enumerate() {
                    print!("{prefix}");
                    println!("    Chip {j}: {}", rom.rom_type);
                    if let Some(filename) = &rom.filename {
                        print!("{prefix}");
                        println!("      Image source: {filename}");
                    }
                }
            }
            Ok(())
        }
    }
}

/// Print one plugin line (and, when verbose, its detail).
///
/// Resolves the plugin's image `source` to a friendly name via `onerom-app`.
/// The manifest lookup for official plugins is best-effort: any fetch or parse
/// failure degrades the name to the slug rather than erroring. A plugin slot
/// with no recorded source falls back to its slot-derived type.
async fn output_plugin(prefix: &str, verbose: bool, slot_index: usize, source: Option<&str>) {
    let Some(source) = source else {
        let label = PluginType::from_slot_index(slot_index)
            .map(|t| t.short())
            .unwrap_or("unknown");
        print!("{prefix}");
        println!("    {label} plugin (no image source)");
        return;
    };

    match resolve_plugin_display(slot_index, source, &CliFetch).await {
        Some(display) => {
            print!("{prefix}");
            println!("    {}", display.display_label());
            if verbose {
                print!("{prefix}");
                println!("      Source: {source}");
                if let PluginOrigin::Manifest { plugin, version } = &display.origin {
                    print!("{prefix}");
                    println!("      Version: {version}");
                    if let Some(description) = &plugin.description {
                        print!("{prefix}");
                        println!("      Description: {description}");
                    }
                }
            }
        }
        None => {
            // slot_index was not a plugin slot; show the raw source rather than
            // inventing a name.
            print!("{prefix}");
            println!("    {source}");
        }
    }
}

pub async fn cmd_slots(options: &Options, args: &InspectSlotsArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();

    output_slot_info(device, options, "").await
}

pub async fn cmd_image(options: &Options, args: &InspectImageArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("inspect image".into()))
}

// Outputs some bytes of data read from the device, either to the console as a
// hex dump or to a file if an output path is provided.
//
// addr_offset is subtracted from the displayed addresses in the hex dump, so
// it can be used to convert from a physical memory address to an offset within
// a range.
async fn read_and_output(
    device: &Device,
    address: u32,
    length: u32,
    addr_offset: u32,
    out: Option<&String>,
) -> Result<(), Error> {
    let data = read_memory(device, address, length).await?;

    if let Some(filename) = out {
        std::fs::write(filename, &data).map_err(|e| Error::io(filename, e))?;
    } else {
        print_hex_dump(address - addr_offset, &data);
    }

    Ok(())
}

pub async fn cmd_peek_live(options: &Options, args: &InspectPeekLiveArgs) -> Result<(), Error> {
    let (address, length) = check_live_read_write(options, args.address, args.length, args)?;

    let device = options.device.as_ref().unwrap();
    read_and_output(device, address, length, LIVE_ROM_BASE, args.output.as_ref()).await
}

pub async fn cmd_peek_memory(options: &Options, args: &InspectPeekMemoryArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();
    read_and_output(device, args.address, args.length, 0, args.output.as_ref()).await
}

/// What One ROM is doing with a GPIO, as the device reports it.
///
/// The device's categories are deliberately coarse - they say what taking a pin
/// over would cost, not what the pin is - so this is the one column of the table
/// that does not come from local metadata. A category this build does not
/// recognise is shown raw rather than guessed at.
fn gpio_use_label(entry: &GpioEntry) -> String {
    match entry.gpio_use() {
        Some(GpioUse::Free) => "free".to_string(),
        Some(GpioUse::ServingRead) => "serving (read)".to_string(),
        Some(GpioUse::ServingDriven) => "serving (driven)".to_string(),
        Some(GpioUse::SystemPin) => "system".to_string(),
        None => format!("unknown ({})", entry.gpio_use_raw),
    }
}

/// The `Function` column: what this GPIO is, in ROM or board terms.
///
/// A socket pin is named by the ROM currently being served (`A5`, `D3`, `CS1`,
/// `BYTE`); a board system pin by what the board uses it for. Falls back to the
/// bare socket pin number when the served chip type could not be resolved, so a
/// socket pin is never shown as unused.
fn gpio_function_label(board: Option<&Board>, chip: Option<ChipType>, gpio: u8) -> String {
    let Some(board) = board else {
        return "-".to_string();
    };

    if let Some(chip) = chip
        && let Some(function) = gpio_rom_function(board, chip, gpio)
    {
        return function;
    }
    if let Some(system) = gpio_system_function(board, gpio) {
        return system.to_string();
    }
    if let Some(socket_pin) = board.socket_pin_for_gpio(gpio) {
        return format!("socket pin {socket_pin}");
    }
    "-".to_string()
}

/// The `5V` column, from static board metadata. Ice (STM32) boards are not
/// characterised pin by pin and report `?`.
fn gpio_tolerance_label(board: Option<&Board>, gpio: u8) -> String {
    match board.and_then(|b| b.gpio_tolerance(gpio)) {
        Some(PinTolerance::FiveVolt) => "5V".to_string(),
        Some(PinTolerance::ThreeVolt3) => "3V3".to_string(),
        None => "?".to_string(),
    }
}

/// Column headings for the `inspect gpio` table.
const GPIO_HEADINGS: [&str; 7] = [
    "GPIO",
    "Pad",
    "Function",
    "Dir",
    "Level",
    "5V",
    "One ROM use",
];

/// Render the `inspect gpio` table for `entries`, which describe the run of
/// GPIOs starting at `first_gpio`.
///
/// Pure so the layout can be tested without a device: everything about the pins
/// comes from `entries`, and everything about their names from `board` and
/// `chip`. Both are optional, because a board revision or ROM type this build
/// does not recognise must cost names rather than the listing.
fn render_gpio_table(
    board: Option<&Board>,
    chip: Option<ChipType>,
    first_gpio: u8,
    entries: &[GpioEntry],
) -> String {
    // Rows are built first so every column can be sized to its own content.
    let rows: Vec<[String; 7]> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let gpio = first_gpio.saturating_add(i as u8);
            [
                gpio.to_string(),
                board
                    .and_then(|b| gpio_header_role(b, gpio))
                    .unwrap_or_else(|| "-".to_string()),
                gpio_function_label(board, chip, gpio),
                if entry.is_output != 0 { "out" } else { "in" }.to_string(),
                entry.level.to_string(),
                gpio_tolerance_label(board, gpio),
                gpio_use_label(entry),
            ]
        })
        .collect();

    let widths: Vec<usize> = (0..GPIO_HEADINGS.len())
        .map(|c| {
            rows.iter()
                .map(|r| r[c].chars().count())
                .chain(std::iter::once(GPIO_HEADINGS[c].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let line = |cells: &[String]| {
        let mut out = String::from("  ");
        for (c, cell) in cells.iter().enumerate() {
            if c > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!("{cell:<width$}", width = widths[c]));
        }
        format!("{}\n", out.trim_end())
    };

    let mut out = String::new();
    out.push_str(&line(&GPIO_HEADINGS.map(String::from)));
    out.push_str(&line(
        &widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>(),
    ));
    for row in &rows {
        out.push_str(&line(row));
    }

    // Legend. The first line is the one that matters: the device says only what
    // it is doing with a pin, never what the pin is, so a reader must know which
    // columns are its word and which are this CLI's derivation.
    out.push('\n');
    out.push_str("  Pad and Function are derived by this CLI from the board and the ROM being\n");
    out.push_str("  served; One ROM use, Dir and Level are what the device reports.\n");
    out.push_str("  Dir is the pin's output driver - 'out' if enabled, 'in' if not.\n");
    out.push_str(
        "  serving (read) pins can be driven and released; serving (driven) pins cannot\n",
    );
    out.push_str("  be given back without a reboot.  See 'onerom control gpio'.\n");
    if board.is_some_and(|b| b.rp_variant().is_some()) {
        out.push_str("  3V3 = 3.3V-only (ADC pin, keep ≤3.3V)    5V = 5V-tolerant\n");
    } else {
        out.push_str("  5V tolerance is not characterised pin by pin on this board.\n");
    }
    if board.is_some_and(|b| b.jumper_header().is_none()) {
        out.push_str(
            "  This board's header layout is not characterised, so Pad names come from its\n",
        );
        out.push_str("  pin assignments alone - run 'onerom inspect header' for what is known.\n");
    }

    out
}

pub async fn cmd_gpio(options: &Options, args: &InspectGpioArgs) -> Result<(), Error> {
    check_device_running(options, args)?;
    let device = options.device.as_ref().unwrap();

    // The capability probe carries num_gpios, which is what a whole-device query
    // is sized from - 30 on an RP2350A, 48 on an RP2350B, never a constant.
    let caps = get_caps(device).await?;
    let (first_gpio, entries) = match args.pin {
        Some(pin) => (pin.gpio(), gpio_query(device, &caps, pin.gpio(), 1).await?),
        None => (0, gpio_query_all(device, &caps).await?),
    };

    // Naming is entirely local: the board pin map plus the chip type of the ROM
    // being served.
    let board = resolve_board(options, &None).ok().flatten();
    let chip = active_chip_type(device);

    println!("{device}");
    println!();

    let mut title = "GPIO state".to_string();
    if let Some(board) = board.as_ref() {
        title.push_str(&format!("  ·  {}", board.description()));
    }
    if let Some(rom_type) = device.get_active_rom_type() {
        title.push_str(&format!("  ·  serving {rom_type}"));
    }
    println!("{title}");
    println!();

    print!(
        "{}",
        render_gpio_table(board.as_ref(), chip, first_gpio, &entries)
    );

    Ok(())
}

pub async fn cmd_header(options: &Options, args: &InspectHeaderArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let board = resolve_board(options, &None)?.ok_or(Error::NoBoardOrDevice)?;
    crate::board::show_pin_header(&board);
    Ok(())
}

pub async fn cmd_socket(options: &Options, args: &InspectSocketArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let board = resolve_board(options, &None)?.ok_or(Error::NoBoardOrDevice)?;
    crate::board::show_rom_socket(&board, &args.chip_type, args.gpio)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device's worth of entries, using the same `use` category throughout so
    /// a test can pick out the column it cares about. The wire's fourth byte is
    /// reserved and is not represented here.
    fn entries(count: u8, gpio_use: u8) -> Vec<GpioEntry> {
        (0..count)
            .map(|gpio| GpioEntry {
                gpio_use_raw: gpio_use,
                level: u8::from(gpio.is_multiple_of(2)),
                is_output: u8::from(gpio.is_multiple_of(3)),
            })
            .collect()
    }

    #[test]
    fn table_names_pads_and_functions_from_local_metadata() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::ServingRead as u8),
        );

        // The socket, header and system columns, from the same board data the
        // socket and header diagrams draw with.
        assert!(table.contains("A7"), "{table}");
        assert!(table.contains("CS1"), "{table}");
        assert!(table.contains("SEL_A"), "{table}");
        assert!(table.contains("SEL_D/SWDIO"), "{table}");
        assert!(table.contains("X1"), "{table}");
        assert!(table.contains("status LED"), "{table}");

        // The device's own column, and the ADC pins' tolerance.
        assert!(table.contains("serving (read)"), "{table}");
        assert!(table.contains("3V3"), "{table}");
        assert!(table.contains("5V = 5V-tolerant"), "{table}");

        // One row per GPIO. The table body ends at the blank line before the
        // legend, so counting stops there.
        let rows = table
            .lines()
            .skip(2) // headings and rule
            .take_while(|l| !l.is_empty())
            .count();
        assert_eq!(rows, 30, "{table}");
    }

    #[test]
    fn table_columns_line_up() {
        let board = Board::try_from_str("fire-32-b").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip27512),
            0,
            &entries(48, GpioUse::Free as u8),
        );

        // Every table line - headings, rule and rows - starts each column at the
        // same offset. The rule row is the reference.
        let lines: Vec<&str> = table.lines().collect();
        let rule = lines.iter().find(|l| l.contains("----")).expect("rule row");
        let starts: Vec<usize> = rule
            .char_indices()
            .filter(|(i, c)| *c == '-' && (*i == 0 || !rule.starts_with('-')))
            .map(|(i, _)| i)
            .filter(|i| rule.as_bytes().get(i.wrapping_sub(1)) != Some(&b'-'))
            .collect();
        assert_eq!(starts.len(), GPIO_HEADINGS.len(), "{table}");

        let heading_line = lines[0];
        for (col, start) in starts.iter().enumerate() {
            assert_eq!(
                heading_line[*start..].split_whitespace().next(),
                Some(GPIO_HEADINGS[col].split(' ').next().unwrap()),
                "column {col} at {start}\n{table}"
            );
        }
    }

    #[test]
    fn table_degrades_without_a_board() {
        // An unrecognised board loses the names, not the listing.
        let table = render_gpio_table(None, None, 0, &entries(30, GpioUse::Free as u8));
        assert!(table.contains("One ROM use"), "{table}");
        assert!(table.contains("free"), "{table}");
        // Tolerance is unknown, and says so rather than claiming 5V.
        assert!(table.contains('?'), "{table}");
        assert!(table.contains("not characterised pin by pin"), "{table}");
    }

    #[test]
    fn table_degrades_without_a_chip_type() {
        // A ROM type this build cannot resolve still names socket pins by their
        // socket position, rather than showing them as nothing.
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(Some(&board), None, 0, &entries(30, GpioUse::Free as u8));
        assert!(table.contains("socket pin "), "{table}");
        assert!(table.contains("SEL_A"), "{table}");
    }

    #[test]
    fn table_notes_an_uncharacterised_header() {
        let board = Board::try_from_str("ice-24-d").unwrap();
        assert!(board.jumper_header().is_none());
        let table = render_gpio_table(Some(&board), None, 0, &entries(16, GpioUse::Free as u8));
        assert!(
            table.contains("header layout is not characterised"),
            "{table}"
        );
        // The pads it can still name are named.
        assert!(table.contains("SEL_A"), "{table}");
    }

    #[test]
    fn table_shows_a_single_gpio_at_its_own_number() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            9,
            &entries(1, GpioUse::Free as u8),
        );
        // --pin gpio9 shows GPIO 9, not GPIO 0.
        assert!(table.contains("\n  9 "), "{table}");
        assert!(table.contains("X1"), "{table}");
    }

    #[test]
    fn table_shows_an_unrecognised_use_raw() {
        // A category from a device newer than this build must not be guessed at.
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(Some(&board), None, 0, &entries(4, 9));
        assert!(table.contains("unknown (9)"), "{table}");
    }

    /// Print every shape the table takes, for eyeballing:
    /// `cargo test -p onerom-cli --bin onerom show_gpio_table -- --nocapture`.
    #[test]
    fn show_gpio_table() {
        // Mixed categories, so every rendering of the use column appears.
        let mixed =
            |count: u8, driven: std::ops::RangeInclusive<u8>, system: u8| -> Vec<GpioEntry> {
                (0..count)
                    .map(|gpio| GpioEntry {
                        gpio_use_raw: if driven.contains(&gpio) {
                            GpioUse::ServingDriven as u8
                        } else if gpio == system {
                            GpioUse::SystemPin as u8
                        } else if gpio > *driven.end() && gpio < system {
                            GpioUse::ServingRead as u8
                        } else {
                            GpioUse::Free as u8
                        },
                        level: u8::from(gpio.is_multiple_of(2)),
                        is_output: u8::from(driven.contains(&gpio)),
                    })
                    .collect()
            };

        for (label, board, chip, count) in [
            (
                "RP2350A, 30 GPIOs, fully characterised",
                Some("fire-24-f"),
                Some(ChipType::Chip2364),
                30u8,
            ),
            (
                "RP2350B, 48 GPIOs, 16-bit ROM with a /BYTE pin",
                Some("fire-40-b"),
                Some(ChipType::Chip27C400),
                48,
            ),
            (
                "no header descriptor, no per-pin tolerance",
                Some("ice-24-d"),
                Some(ChipType::Chip2364),
                16,
            ),
            ("board known, ROM type not", Some("fire-24-f"), None, 30),
            ("board unrecognised", None, None, 30),
        ] {
            let board = board.map(|b| Board::try_from_str(b).unwrap());
            println!("\n=== {label} ===");
            println!(
                "{}",
                render_gpio_table(
                    board.as_ref(),
                    chip,
                    0,
                    &mixed(count, 0..=7, count.saturating_sub(1))
                )
            );
        }
    }
}
