// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for firmware info queries: device version.

use onerom_config::fw::FirmwareVersion;
use onerom_fw_emulator::Emulator;

/// Verify that get_device_version returns a string that matches the parsed
/// firmware version.
pub fn test_device_version(emu: &Emulator, fw_version: &FirmwareVersion) -> Result<(), String> {
    let (result, version_str) = emu.get_device_version(64);
    if !result.is_ok() {
        return Err(format!("{:?}", result));
    }
    let version_str = version_str.ok_or_else(|| "returned OK but no version string".to_string())?;

    let expected = format!("v{}", fw_version);
    if version_str != expected {
        return Err(format!(
            "version string mismatch: got '{}' expected '{}'",
            version_str, expected
        ));
    }

    println!("  version: {}", version_str);
    Ok(())
}
