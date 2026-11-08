// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The firmware's checks of the plugin slots before it starts the plugins.
//!
//! - A plugin header fails the plugin check with a wrong magic, plugin API
//!   version or plugin type, a minimum firmware version above the running
//!   firmware's, or a USB controller DPRAM reservation above
//!   `USB_CONTROLLER_DPRAM_MAX`. A minimum equal to the running firmware's and
//!   a reservation of `USB_CONTROLLER_DPRAM_MAX` pass.
//! - The firmware starts a user plugin only if both plugin headers pass the
//!   check.
//! - Launch doesn't start a plugin from a slot that isn't a plugin slot or from
//!   a slot past the slot count.
//! - A plugin slot's ROM filename doesn't change which plugins start.

use onerom_config::fw::FirmwareVersion;
use onerom_fw_emulator::{Emulator, ffi};
use onerom_metadata::USB_CONTROLLER_DPRAM_MAX;

use crate::flash_range::PLUGINS;
use crate::report::TestReport;

pub fn run(report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    Emulator::set_sel_image(0);
    let emulator = Emulator::boot();

    report.add_check("plugin launch: ROM slots", check_rom_slots());
    Emulator::install_plugin_slots();
    report.add_check(
        "plugin launch: header fields",
        firmware_version(&emulator).and_then(check_header_fields),
    );
    report.add_check("plugin launch: filenames", check_filenames());
    // Last because only a restore puts the slot count back.
    report.add_check("plugin launch: slot count", check_slot_count());

    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

fn starts(started: bool) -> &'static str {
    if started { "starts" } else { "doesn't start" }
}

fn place_both() {
    for p in &PLUGINS {
        p.place(true, 0, 0);
    }
}

fn firmware_version(emu: &Emulator) -> Result<FirmwareVersion, String> {
    let (r, version) = emu.get_device_version(64);
    let version = version.ok_or(format!("reading the firmware version returned {r:?}"))?;
    let stripped = version.strip_prefix('v').unwrap_or(&version);
    FirmwareVersion::try_from_str(stripped)
        .map_err(|e| format!("the firmware version {version:?}: {e}"))
}

fn check_rom_slots() -> Result<(), String> {
    let system = Emulator::system_plugin_to_launch();
    let user = Emulator::user_plugin_to_launch(true);
    if system || user {
        return Err(format!(
            "from ROM slots the firmware {} the system plugin and {} the user plugin",
            starts(system),
            starts(user)
        ));
    }
    Ok(())
}

/// A label, a change to the plugin header in a slot and whether the plugin
/// still passes the check with it.
type HeaderCase<'a> = (&'a str, &'a dyn Fn(u8), bool);

fn check_header_fields(version: FirmwareVersion) -> Result<(), String> {
    let (major, minor, patch) = (version.major(), version.minor(), version.patch());
    let max_dpram = USB_CONTROLLER_DPRAM_MAX as u16;
    let cases: [HeaderCase; 8] = [
        (
            "a wrong magic",
            &|i| Emulator::set_plugin_magic(i, ffi::ORA_PLUGIN_MAGIC ^ 1),
            false,
        ),
        (
            "the next plugin API version",
            &|i| Emulator::set_plugin_api_version(i, ffi::ORA_PLUGIN_VERSION_1 + 1),
            false,
        ),
        (
            "the running firmware's version as its minimum",
            &|i| Emulator::set_plugin_min_fw_version(i, major, minor, patch),
            true,
        ),
        (
            "a minimum major version above the running firmware's",
            &|i| Emulator::set_plugin_min_fw_version(i, major + 1, 0, 0),
            false,
        ),
        (
            "a minimum minor version above the running firmware's",
            &|i| Emulator::set_plugin_min_fw_version(i, major, minor + 1, 0),
            false,
        ),
        (
            "a minimum patch version above the running firmware's",
            &|i| Emulator::set_plugin_min_fw_version(i, major, minor, patch + 1),
            false,
        ),
        (
            "the maximum USB controller DPRAM",
            &|i| Emulator::set_plugin_usb_controller_dpram_size(i, max_dpram),
            true,
        ),
        (
            "USB controller DPRAM above the maximum",
            &|i| Emulator::set_plugin_usb_controller_dpram_size(i, max_dpram + 1),
            false,
        ),
    ];

    for p in &PLUGINS {
        for (label, set_field, passes) in &cases {
            place_both();
            set_field(p.index);
            let checked = Emulator::check_plugin_valid(p.index, p.plugin_type, p.index);
            let started = Emulator::user_plugin_to_start();
            if (checked, started) != (*passes, *passes) {
                return Err(format!(
                    "the {} plugin with {label}: the plugin check {} and the firmware {} \
                     the user plugin",
                    p.label,
                    if checked { "passes" } else { "fails" },
                    starts(started)
                ));
            }
        }

        place_both();
        let other = &PLUGINS[usize::from(p.index ^ 1)];
        if Emulator::check_plugin_valid(p.index, other.plugin_type, p.index) {
            return Err(format!(
                "the {} plugin passes the plugin check as a {} plugin",
                p.label, other.label
            ));
        }
    }
    Ok(())
}

fn check_filenames() -> Result<(), String> {
    place_both();
    let filenames = [c"system.bin", c"user.bin"];
    for (p, filename) in PLUGINS.iter().zip(filenames) {
        Emulator::set_plugin_filename(p.index, Some(filename));
    }
    let system = Emulator::system_plugin_to_launch();
    let user = Emulator::user_plugin_to_launch(true);
    for p in &PLUGINS {
        Emulator::set_plugin_filename(p.index, None);
    }

    if !system || !user {
        return Err(format!(
            "with filenames the firmware {} the system plugin and {} the user plugin",
            starts(system),
            starts(user)
        ));
    }
    Ok(())
}

fn check_slot_count() -> Result<(), String> {
    place_both();
    for (count, system_starts) in [(1, true), (0, false)] {
        Emulator::set_rom_slot_count(count);
        let system = Emulator::system_plugin_to_launch();
        let user = Emulator::user_plugin_to_launch(true) || Emulator::user_plugin_to_start();
        if (system, user) != (system_starts, false) {
            return Err(format!(
                "with a slot count of {count} the firmware {} the system plugin and {} the \
                 user plugin",
                starts(system),
                starts(user)
            ));
        }
    }
    Ok(())
}
