// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for firmware info queries: device version.

use onerom_config::fw::FirmwareVersion;
use onerom_fw_emulator::{Emulator, OraResult, ffi};
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

/// Verify device-level unsigned metadata retrieval via the keyed getter, and
/// that the string and unsigned getters discriminate on datum type across the
/// shared key space.
pub fn test_metadata_uint(emu: &Emulator) -> Result<(), String> {
    // Numeric keys resolve OK. Values are board-specific, so confirm the
    // contract and print them rather than asserting exact numbers.
    let numeric: &[(ffi::ora_metadata_key_t, &str)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS,
            "GPIO_STATUS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_NEOPIXEL,
            "GPIO_NEOPIXEL",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_NUM_PHYS_PINS,
            "NUM_PHYS_PINS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_STATUS_LED_STATE,
            "STATUS_LED_STATE",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_BOOT_LOGGING,
            "BOOT_LOGGING",
        ),
    ];
    for (key, label) in numeric {
        let (result, value) = emu.get_metadata_uint(*key);
        if !result.is_ok() {
            return Err(format!("{}: expected OK, got {:?}", label, result));
        }
        let value = value.ok_or_else(|| format!("{}: OK but no value", label))?;
        println!("  {}: {}", label, value);
    }

    // A string key must be TypeMismatch through the unsigned getter...
    let (result, _) = emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_HW_REV);
    if result != OraResult::TypeMismatch {
        return Err(format!(
            "HW_REV via uint: expected TypeMismatch, got {:?}",
            result
        ));
    }
    // ...and a numeric key must be TypeMismatch through the string getter.
    let (result, _) = emu.get_metadata_str(ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS);
    if result != OraResult::TypeMismatch {
        return Err(format!(
            "GPIO_STATUS via str: expected TypeMismatch, got {:?}",
            result
        ));
    }

    // Unknown / sentinel keys are NOT_SUPPORTED.
    let unknown: &[(ffi::ora_metadata_key_t, &str)] = &[
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_INVALID, "INVALID"),
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_NONE, "NONE"),
    ];
    for (key, label) in unknown {
        let (result, _) = emu.get_metadata_uint(*key);
        if result != OraResult::NotSupported {
            return Err(format!(
                "{}: expected NotSupported, got {:?}",
                label, result
            ));
        }
    }

    Ok(())
}
