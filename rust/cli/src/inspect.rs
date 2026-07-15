// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::inspect::{
    InspectGpioArgs, InspectImageArgs, InspectInfoArgs, InspectPeekLiveArgs, InspectPeekMemoryArgs,
    InspectSlotsArgs, InspectTelemetryArgs,
};
use crate::utils::{check_device, check_live_read_write, print_hex_dump};
use onerom_cli::CliFetch;
use onerom_cli::LIVE_ROM_BASE;
use onerom_cli::plugin::{PluginOrigin, PluginType, resolve_plugin_display};
use onerom_cli::usb::read_memory;
use onerom_cli::{Device, Error, Options};
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
                        println!("      Status LED: {}", if led.enabled { "on" } else { "off" });
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
                        println!("      SWD: {}", if debug.swd_enabled { "on" } else { "off" });
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
                    println!("    Flash location {data_addr}  size {:#x} bytes", slot.size);
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

pub async fn cmd_gpio(options: &Options, args: &InspectGpioArgs) -> Result<(), Error> {
    check_device(options, args, true)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("inspect gpio".into()))
}