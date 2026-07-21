// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for firmware info queries: device version.

use onerom_config::fw::FirmwareVersion;
use onerom_fw_emulator::{ffi, Emulator, OraResult};
use onerom_gen::Config;

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

/// Verify device-level metadata string retrieval via the keyed getter.
///
/// Checks that:
/// - known string keys return OK with the value stored in the config verbatim,
///   or None (OK with a NULL pointer) when the optional field is unset;
/// - an unknown key, and the NONE sentinel, return NOT_SUPPORTED - the
///   forward-compatibility contract a newer plugin relies on against older
///   firmware.
pub fn test_metadata_str(emu: &Emulator, config: &Config) -> Result<(), String> {
    let known: &[(ffi::ora_metadata_key_t, &str, &Option<String>)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_UNIT_NAME,
            "UNIT_NAME",
            &config.instance_name,
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_SERIAL_OVERRIDE,
            "SERIAL_OVERRIDE",
            &config.serial_override,
        ),
    ];

    for (key, label, expected) in known {
        let (result, value) = emu.get_metadata_str(*key);
        if !result.is_ok() {
            return Err(format!("{}: expected OK, got {:?}", label, result));
        }
        if &value != *expected {
            return Err(format!(
                "{}: value mismatch: got {:?} expected {:?}",
                label, value, expected
            ));
        }
        println!("  {}: {:?}", label, value);
    }

    let unknown: &[(ffi::ora_metadata_key_t, &str)] = &[
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_INVALID, "INVALID"),
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_NONE, "NONE"),
    ];

    for (key, label) in unknown {
        let (result, _) = emu.get_metadata_str(*key);
        if result != OraResult::NotSupported {
            return Err(format!(
                "{}: expected NotSupported, got {:?}",
                label, result
            ));
        }
    }

    Ok(())
}
