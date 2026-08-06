// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::inspect::{
    InspectGpioArgs, InspectHeaderArgs, InspectImageArgs, InspectInfoArgs, InspectPeekLiveArgs,
    InspectPeekMemoryArgs, InspectSlotsArgs, InspectSocketArgs, InspectTelemetryArgs,
};
use crate::board_view::{gpio_header_role, gpio_rom_function, gpio_system_functions};
use crate::utils::{
    active_chip_type, check_device, check_device_running, check_fire_board,
    check_fire_board_optional, check_live_read_write, print_hex_dump, resolve_board,
    resolve_board_optional,
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

/// What the table shows when a column has nothing to say.
const GPIO_NONE: &str = "-";

/// The `Function` column: everything this GPIO is, in ROM or board terms.
///
/// One column rather than two, because splitting the ROM socket signal from the
/// header pad splits on *provenance* rather than on anything a reader needs: on
/// a 32-pin board the `A17` header pad and the socket's `A17` line are the same
/// net, and everywhere else exactly one of the two is populated. An X pad or an
/// image-select pad is as much a function of the pin as `A11` is.
///
/// Every name that applies is listed, in a fixed order - the ROM socket signal
/// for the chip being served, then the board peripheral, then the header pad -
/// rather than first-match-wins, deduplicated so the shared `A17` net is not
/// named twice. A GPIO can genuinely be two things: `fire-24-f` drives its
/// status LED and its NeoPixel from GPIO 29, and both belong here.
///
/// Falls back to the bare socket pin number when the served chip type could not
/// be resolved, so a socket pin is never shown as unused.
fn gpio_function_label(board: Option<&Board>, chip: Option<ChipType>, gpio: u8) -> String {
    let Some(board) = board else {
        return GPIO_NONE.to_string();
    };

    // Deduplication is by name, not by source: a 32-pin board's high address
    // lines are broken out on header pads, so the socket signal and the pad are
    // one net and must not be listed twice.
    let mut names: Vec<String> = Vec::new();
    let mut add = |name: String| {
        if !names.contains(&name) {
            names.push(name);
        }
    };

    // 1. The ROM socket signal under the chip being served. With no resolvable
    //    chip type the socket position is still worth stating.
    match chip.and_then(|chip| gpio_rom_function(board, chip, gpio)) {
        Some(function) => add(function),
        None => {
            if let Some(socket_pin) = board.socket_pin_for_gpio(gpio) {
                add(format!("socket pin {socket_pin}"));
            }
        }
    }

    // 2. The board peripheral(s).
    for system in gpio_system_functions(board, gpio) {
        add(system.to_string());
    }

    // 3. The header pad. Named last because it is where the signal surfaces
    //    rather than what it carries - but named, because "which GPIO is X1" is
    //    the main thing this table is read to answer before wiring a reset line.
    if let Some(role) = gpio_header_role(board, gpio) {
        add(role);
    }

    if names.is_empty() {
        GPIO_NONE.to_string()
    } else {
        names.join(", ")
    }
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
const GPIO_HEADINGS: [&str; 6] = ["GPIO", "Function", "Dir", "Level", "Max V", "Current use"];

/// The `Function` column's index in a row, which the "is this GPIO connected to
/// anything?" filter reads.
const GPIO_FUNCTION_COLUMN: usize = 1;

/// Render the `inspect gpio` table for `entries`, which describe the run of
/// GPIOs starting at `first_gpio`.
///
/// Pure so the layout can be tested without a device: everything about the pins
/// comes from `entries`, and everything about their names from `board` and
/// `chip`. Both are optional, because a board revision or ROM type this build
/// does not recognise must cost names rather than the listing.
///
/// `show_unconnected` includes the GPIOs with no function at all - thirteen of a
/// fire-28-c's forty-eight, which bury the rows a reader came for. `verbose`
/// adds the legend explaining where each column comes from.
fn render_gpio_table(
    board: Option<&Board>,
    chip: Option<ChipType>,
    first_gpio: u8,
    entries: &[GpioEntry],
    show_unconnected: bool,
    verbose: bool,
) -> String {
    // Rows are built first so every column can be sized to its own content.
    // Filtering happens here rather than at the caller so the columns are sized
    // to what is actually shown.
    let all_rows: Vec<[String; GPIO_HEADINGS.len()]> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let gpio = first_gpio.saturating_add(i as u8);
            [
                gpio.to_string(),
                gpio_function_label(board, chip, gpio),
                if entry.is_output != 0 { "out" } else { "in" }.to_string(),
                entry.level.to_string(),
                gpio_tolerance_label(board, gpio),
                gpio_use_label(entry),
            ]
        })
        .collect();

    // "Connected to something" is a question about the board, not about what
    // the device reports: X1, X2 and the image-select pads are `free`, and they
    // are precisely the pins someone reads this table to find. Without a board
    // nothing can be ruled out, so nothing is.
    let connected = |row: &[String; GPIO_HEADINGS.len()]| {
        row[GPIO_FUNCTION_COLUMN] != GPIO_NONE || board.is_none()
    };
    let rows: Vec<&[String; GPIO_HEADINGS.len()]> = all_rows
        .iter()
        .filter(|row| show_unconnected || connected(row))
        .collect();
    let hidden = all_rows.len() - rows.len();

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
        out.push_str(&line(*row));
    }

    // Rows dropped by the filter are always accounted for: a listing that
    // silently omits GPIOs is worse than a noisy one.
    if hidden > 0 {
        out.push('\n');
        out.push_str(&format!(
            "  {hidden} GPIO{} with no function {} hidden - use --all to show {}.\n",
            if hidden == 1 { "" } else { "s" },
            if hidden == 1 { "is" } else { "are" },
            if hidden == 1 { "it" } else { "them" },
        ));
    }

    // The legend explains where each column comes from, which is worth saying
    // once and tiresome on every run - so it is behind --verbose. Nothing safety
    // -critical lives here: the cost of taking a serving pin over is stated at
    // the point of action, by 'control pin' itself.
    if verbose {
        out.push('\n');
        out.push_str(
            "  Function is derived by this CLI from the board and the ROM being served,\n",
        );
        out.push_str(
            "  and lists the socket signal, the board peripheral and the header pad, in\n",
        );
        out.push_str("  that order; Current use, Dir and Level are what the device reports.\n");
        out.push('\n');
        out.push_str("  Dir is the pin's output driver - 'out' if enabled, 'in' if not.\n");
        out.push('\n');
        out.push_str(
            "  Current use is what One ROM is doing with the pin now, which can change:\n",
        );
        out.push_str(
            "  the image select pins are read at start of day and released, so they show\n",
        );
        out.push_str("  free while serving.\n");
        out.push('\n');
        out.push_str(
            "  serving (read) pins can be driven and released; serving (driven) pins cannot\n",
        );
        out.push_str("  be given back without a reboot.  See 'onerom control pin'.\n");
        out.push('\n');
        out.push_str(
            "  Function names only what a GPIO is; a pad may also carry SWCLK or SWDIO,\n",
        );
        out.push_str("  which are dedicated pins - run 'onerom inspect header' for the pads.\n");
        out.push('\n');
        if board.is_some_and(|b| b.rp_variant().is_some()) {
            out.push_str("  3V3 = 3.3V-only (ADC pin, keep ≤3.3V)    5V = 5V-tolerant\n");
        } else {
            out.push_str("  5V tolerance is not characterised pin by pin on this board.\n");
        }
        if board.is_some_and(|b| b.jumper_header().is_none()) {
            out.push_str(
                "  This board's header layout is not characterised, so pad names come from its\n",
            );
            out.push_str(
                "  pin assignments alone - run 'onerom inspect header' for what is known.\n",
            );
        }
    }

    out
}

/// The RP2350 variant a device's GPIO count implies.
///
/// The device reports its GPIO count, not its variant, but the two are one to
/// one: the RP235xA (QFN-60) has 30 and the RP235xB (QFN-80) has 48. Anything
/// else is a device this build does not understand, and is better left unnamed
/// than guessed at.
fn rp_variant_from_gpio_count(num_gpios: u8) -> Option<&'static str> {
    match num_gpios {
        30 => Some("RP235xA"),
        48 => Some("RP235xB"),
        _ => None,
    }
}

pub async fn cmd_gpio(options: &Options, args: &InspectGpioArgs) -> Result<(), Error> {
    check_device_running(options, args)?;
    let device = options.device.as_ref().unwrap();

    // Naming is entirely local: the board pin map plus the chip type of the ROM
    // being served. The board is also what turns a --pin pad name into a GPIO,
    // so it has to be settled before the device is queried.
    let board = resolve_board_optional(options, &args.board)?;
    // The GPIOs being named belong to the connected Fire, so an Ice --board
    // would relabel them against hardware that is not there.
    check_fire_board_optional(&board)?;
    let chip = active_chip_type(device);
    let pin = args
        .pin
        .map(|pin| pin.resolve(board.as_ref()))
        .transpose()?;

    // The capability probe carries num_gpios, which is what a whole-device query
    // is sized from - 30 on an RP2350A, 48 on an RP2350B, never a constant.
    let caps = get_caps(device).await?;
    let (first_gpio, entries) = match pin {
        Some(pin) => (pin.gpio(), gpio_query(device, &caps, pin.gpio(), 1).await?),
        None => (0, gpio_query_all(device, &caps).await?),
    };

    println!("{device}");
    println!();

    let mut title = "GPIO state".to_string();
    if let Some(board) = board.as_ref() {
        title.push_str(&format!("  ·  {}", board.description()));
    }
    // The silicon variant the device itself reports, not what the board
    // revision implies: it decides how many GPIOs there are (30 or 48) and
    // which of them are 3.3V-only, so the table's length and its Max V column
    // both only make sense once it is stated.
    if let Some(variant) = rp_variant_from_gpio_count(caps.num_gpios) {
        title.push_str(&format!("  ·  {variant}"));
    }
    if let Some(rom_type) = device.get_active_rom_type() {
        title.push_str(&format!("  ·  serving {rom_type}"));
    }
    println!("{title}");
    println!();

    print!(
        "{}",
        render_gpio_table(
            board.as_ref(),
            chip,
            first_gpio,
            &entries,
            // --pin already narrows to one GPIO; filtering it out again would
            // answer a direct question with an empty table.
            args.all || args.pin.is_some(),
            options.verbose,
        )
    );

    Ok(())
}

pub async fn cmd_header(options: &Options, args: &InspectHeaderArgs) -> Result<(), Error> {
    let board = resolve_device_board(options, args, "header", &args.board)?;
    crate::board::show_pin_header(&board);
    Ok(())
}

pub async fn cmd_socket(options: &Options, args: &InspectSocketArgs) -> Result<(), Error> {
    let board = resolve_device_board(options, args, "socket", &args.board)?;
    crate::board::show_rom_socket(&board, &args.chip_type, args.gpio)
}

/// The board a device-side view draws, with `--board` overriding what the
/// connected One ROM reports.
///
/// These views describe the hardware of a *connected* One ROM, so the device is
/// required even when `--board` is given: `--board` names the board this build
/// failed to recognise the device as, it does not stand in for the device.
/// `onerom board <view> --board <board>` is what draws a board by name with
/// nothing plugged in.
///
/// The device check therefore comes first, which is what makes
/// [`Error::NoDeviceForBoardView`] mean "connected, but its board type is
/// unknown and you did not say" - and why that error's advice is `--board`
/// rather than to connect something.
///
/// The board being named belongs to the connected Fire, so an Ice `--board`
/// would draw hardware that is not there - the same reasoning as
/// `inspect gpio`.
fn resolve_device_board(
    options: &Options,
    args: &impl crate::args::CommandTrait,
    view: &str,
    board_arg: &Option<String>,
) -> Result<Board, Error> {
    check_device(options, args, false)?;
    let board = resolve_board(options, board_arg)?
        .ok_or_else(|| Error::NoDeviceForBoardView(view.to_string()))?;
    check_fire_board(&board)?;
    Ok(board)
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

    /// The number of GPIO rows in a rendered table. The body ends at the blank
    /// line before whatever follows it.
    fn row_count(table: &str) -> usize {
        table
            .lines()
            .skip(2) // headings and rule
            .take_while(|l| !l.is_empty())
            .count()
    }

    /// The `Function` cell for `gpio`, read out of a rendered table rather than
    /// from the labelling helper, so the assertions are about what a user sees.
    fn function_cell(table: &str, gpio: u8) -> String {
        let widths: Vec<usize> = table
            .lines()
            .nth(1)
            .expect("rule row")
            .split_whitespace()
            .map(str::len)
            .collect();
        let row = table
            .lines()
            .skip(2)
            .take_while(|l| !l.is_empty())
            .find(|l| l.split_whitespace().next() == Some(&gpio.to_string()))
            .unwrap_or_else(|| panic!("no row for GPIO{gpio}\n{table}"));
        // Two leading spaces, then the GPIO column and its two-space gutter.
        let start = 2 + widths[0] + 2;
        row[start..]
            .chars()
            .take(widths[GPIO_FUNCTION_COLUMN])
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn table_names_every_function_of_a_gpio_in_one_column() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::ServingRead as u8),
            true,
            true,
        );

        // One column, holding socket signals, board peripherals and header pads
        // alike - all of them things the GPIO is.
        assert!(!table.contains("Pad"), "{table}");
        assert_eq!(function_cell(&table, 16), "A7");
        assert_eq!(function_cell(&table, 10), "CS1");
        assert_eq!(function_cell(&table, 26), "SEL_A");
        assert_eq!(function_cell(&table, 9), "X1");
        assert_eq!(function_cell(&table, 8), "X2");

        // The device's own column, and the ADC pins' tolerance.
        assert!(table.contains("serving (read)"), "{table}");
        assert!(table.contains("3V3"), "{table}");
        assert!(table.contains("5V = 5V-tolerant"), "{table}");

        // One row per GPIO, with --all.
        assert_eq!(row_count(&table), 30, "{table}");
    }

    #[test]
    fn table_function_column_names_only_gpios() {
        // GPIO24/25 are the SEL_D/SEL_C pads, which share their nets with the
        // SWDIO/SWCLK debug pins. Those are dedicated RP2350 pins, not GPIOs, so
        // a GPIO-indexed table must not claim GPIO24 is SWDIO. (Not verbose:
        // the legend mentions both names, to explain their absence.)
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::Free as u8),
            true,
            false,
        );
        assert!(!table.contains("SWDIO"), "{table}");
        assert!(!table.contains("SWCLK"), "{table}");
        assert_eq!(function_cell(&table, 24), "SEL_D");
        assert_eq!(function_cell(&table, 25), "SEL_C");
    }

    #[test]
    fn table_names_both_functions_of_a_shared_system_gpio() {
        // fire-24-f drives the status LED and the NeoPixel from GPIO 29.
        let board = Board::try_from_str("fire-24-f").unwrap();
        assert_eq!(board.pin_status(), 29);
        assert_eq!(board.pin_neo(), Some(29));
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::SystemPin as u8),
            true,
            false,
        );
        assert_eq!(function_cell(&table, 29), "Status LED, RGB LED");

        // fire-28-c has them on distinct GPIOs, which must name one each.
        let b28 = Board::try_from_str("fire-28-c").unwrap();
        assert_eq!(b28.pin_status(), 45);
        assert_eq!(b28.pin_neo(), Some(44));
        let table = render_gpio_table(
            Some(&b28),
            Some(ChipType::Chip2364),
            0,
            &entries(48, GpioUse::SystemPin as u8),
            true,
            false,
        );
        assert_eq!(function_cell(&table, 45), "Status LED");
        assert_eq!(function_cell(&table, 44), "RGB LED");
    }

    #[test]
    fn table_does_not_name_one_net_twice() {
        // A 32-pin board breaks its high address lines out onto header pads, so
        // the socket signal and the pad are the same net under the same name.
        let board = Board::try_from_str("fire-32-b").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip27C040),
            0,
            &entries(48, GpioUse::ServingRead as u8),
            true,
            false,
        );
        for line in table.lines().skip(2).take_while(|l| !l.is_empty()) {
            let cell = line
                .split_whitespace()
                .nth(1)
                .expect("a function cell")
                .to_string();
            assert!(!line.contains(&format!("{cell}, {cell}")), "{line}");
        }
        // The shared pad/socket case is present in this board at all, so the
        // assertion above is not vacuous.
        let a_pad_gpio = board
            .addr_pins()
            .iter()
            .copied()
            .find(|&g| gpio_header_role(&board, g).is_some_and(|r| r.starts_with('A')))
            .expect("fire-32-b breaks out address lines");
        assert!(
            !function_cell(&table, a_pad_gpio).contains(','),
            "{}",
            function_cell(&table, a_pad_gpio)
        );
    }

    #[test]
    fn table_hides_unconnected_gpios_by_default() {
        let board = Board::try_from_str("fire-28-c").unwrap();
        let all = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(48, GpioUse::Free as u8),
            true,
            false,
        );
        let default = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(48, GpioUse::Free as u8),
            false,
            false,
        );

        assert_eq!(row_count(&all), 48, "{all}");
        assert!(row_count(&default) < row_count(&all), "{default}");

        // Every hidden row is accounted for, and the count is right.
        let hidden = row_count(&all) - row_count(&default);
        assert!(
            default.contains(&format!("{hidden} GPIOs with no function are hidden")),
            "{default}"
        );
        assert!(!all.contains("hidden"), "{all}");

        // The drivable pads report `free`, so a filter on the device's `use`
        // would drop exactly the rows this table exists to show. They must
        // survive the default view.
        for pad in ["X1", "X2", "SEL_A", "SEL_B", "SEL_C", "SEL_D"] {
            assert!(default.contains(pad), "{pad} missing\n{default}");
        }
    }

    #[test]
    fn table_legend_is_verbose_only() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        let quiet = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::Free as u8),
            true,
            false,
        );
        let loud = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(30, GpioUse::Free as u8),
            true,
            true,
        );
        assert!(!quiet.contains("derived by this CLI"), "{quiet}");
        assert!(!quiet.contains("5V-tolerant"), "{quiet}");
        assert!(loud.contains("derived by this CLI"), "{loud}");
        assert!(loud.contains("onerom control pin"), "{loud}");
        // The table itself is identical either way.
        assert_eq!(row_count(&quiet), row_count(&loud));
    }

    #[test]
    fn table_columns_line_up() {
        let board = Board::try_from_str("fire-32-b").unwrap();
        let table = render_gpio_table(
            Some(&board),
            Some(ChipType::Chip27512),
            0,
            &entries(48, GpioUse::Free as u8),
            true,
            true,
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
        // An unrecognised board loses the names, not the listing - including
        // under the default filter, which cannot rule anything out without a
        // board to ask.
        let table = render_gpio_table(
            None,
            None,
            0,
            &entries(30, GpioUse::Free as u8),
            false,
            true,
        );
        assert!(table.contains("Current use"), "{table}");
        assert!(table.contains("free"), "{table}");
        assert_eq!(row_count(&table), 30, "{table}");
        assert!(!table.contains("hidden"), "{table}");
        // Tolerance is unknown, and says so rather than claiming 5V.
        assert!(table.contains('?'), "{table}");
        assert!(table.contains("not characterised pin by pin"), "{table}");
    }

    #[test]
    fn table_degrades_without_a_chip_type() {
        // A ROM type this build cannot resolve still names socket pins by their
        // socket position, rather than showing them as nothing.
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(
            Some(&board),
            None,
            0,
            &entries(30, GpioUse::Free as u8),
            true,
            true,
        );
        assert!(table.contains("socket pin "), "{table}");
        assert!(table.contains("SEL_A"), "{table}");
    }

    #[test]
    fn table_notes_an_uncharacterised_header() {
        let board = Board::try_from_str("ice-24-d").unwrap();
        assert!(board.jumper_header().is_none());
        let table = render_gpio_table(
            Some(&board),
            None,
            0,
            &entries(16, GpioUse::Free as u8),
            true,
            true,
        );
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
            true,
            false,
        );
        // --pin gpio9 shows GPIO 9, not GPIO 0.
        assert!(table.contains("\n  9 "), "{table}");
        assert_eq!(function_cell(&table, 9), "X1");
    }

    #[test]
    fn table_shows_an_unrecognised_use_raw() {
        // A category from a device newer than this build must not be guessed at.
        let board = Board::try_from_str("fire-24-f").unwrap();
        let table = render_gpio_table(Some(&board), None, 0, &entries(4, 9), true, false);
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
                "RP2350A, 30 GPIOs, shared status LED / NeoPixel",
                Some("fire-24-f"),
                Some(ChipType::Chip2364),
                30u8,
            ),
            (
                "RP2350B, 48 GPIOs, status LED and NeoPixel distinct",
                Some("fire-28-c"),
                Some(ChipType::Chip2364),
                48,
            ),
            (
                "RP2350B, address lines broken out onto header pads",
                Some("fire-32-b"),
                Some(ChipType::Chip27C040),
                48,
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
            let entries = mixed(count, 0..=7, count.saturating_sub(1));
            // Both the default view and --all, and both with and without the
            // legend, so every shape the command can print is eyeballable.
            for (view, show_unconnected, verbose) in
                [("default", false, false), ("--all --verbose", true, true)]
            {
                println!("\n=== {label} ({view}) ===");
                println!(
                    "{}",
                    render_gpio_table(board.as_ref(), chip, 0, &entries, show_unconnected, verbose)
                );
            }
        }
    }
}
