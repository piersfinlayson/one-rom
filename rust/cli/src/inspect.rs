// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::inspect::{
    InspectGpioArgs, InspectImageArgs, InspectInfoArgs, InspectPeekLiveArgs, InspectPeekMemoryArgs,
    InspectSlotsArgs, InspectTelemetryArgs,
};
use crate::utils::{check_device, check_live_read_write, print_hex_dump};
use onerom_cli::LIVE_ROM_BASE;
use onerom_cli::usb::read_memory;
use onerom_cli::{Device, Error, Options};
use onerom_fw_parser::SdrrCsState;

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
            if let Some(info) = schema.info() {
                println!(
                    "Firmware version: {}.{}.{}",
                    info.major_version, info.minor_version, info.patch_version
                );
                println!("Format: Schema (v0.7.0+)");
            }
            if schema.runtime().is_some() {
                println!("Device is running.");
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

pub fn output_slot_info(device: &Device, options: &Options, prefix: &str) -> Result<(), Error> {
    print!("{prefix}");
    println!("{device}");

    let active_rom_set_index = device.get_active_rom_set_index();
    let verbose = options.verbose;

    match device.onerom.as_ref() {
        Some(onerom_fw_parser::ParsedDevice::Original(sdrr)) => {
            let info = sdrr.flash.as_ref().ok_or_else(|| {
                Error::Other("No recognised information found on device flash".to_string())
            })?;

            let set_count = info.rom_set_count;
            let active_str = active_rom_set_index
                .map(|i| format!(" - Slot {i} is active"))
                .unwrap_or_default();
            print!("{prefix}");
            println!(
                "  Configured with {set_count} slot{}{}",
                if set_count == 1 { "" } else { "s" },
                active_str
            );

            for (i, set) in info.rom_sets.iter().enumerate() {
                let active = if Some(i as u8) == active_rom_set_index {
                    " (active)"
                } else {
                    ""
                };
                print!("{prefix}");
                println!("  Slot {i}{active}:");
                let set_location = set.data_ptr;
                let set_image_size = set.size;
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
                    if verbose {
                        print!("{prefix}");
                        println!(
                            "      Flash location 0x{set_location:08x} size 0x{set_image_size:08x} bytes"
                        );
                    }
                    if let Some(filename) = &rom.filename {
                        print!("{prefix}");
                        println!("      Image source: {filename}");
                    }
                }
            }
            Ok(())
        }

        Some(onerom_fw_parser::ParsedDevice::Schema(onerom)) => {
            let metadata = onerom
                .metadata()
                .ok_or_else(|| Error::Other("No metadata found on device flash".to_string()))?;

            let set_count = metadata.rom_slot_count;
            let active_str = active_rom_set_index
                .map(|i| format!(" - Slot {i} is active"))
                .unwrap_or_default();
            print!("{prefix}");
            println!(
                "  Configured with {set_count} slot{}{}",
                if set_count == 1 { "" } else { "s" },
                active_str
            );

            for (i, slot) in metadata.rom_slots.iter().enumerate() {
                let active = if Some(i as u8) == active_rom_set_index {
                    " (active)"
                } else {
                    ""
                };
                print!("{prefix}");
                println!("  Slot {i}{active}:");
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

        _ => Err(Error::Other(
            "No recognised information found on device flash".to_string(),
        )),
    }
}

pub async fn cmd_slots(options: &Options, args: &InspectSlotsArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();

    output_slot_info(device, options, "")
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
