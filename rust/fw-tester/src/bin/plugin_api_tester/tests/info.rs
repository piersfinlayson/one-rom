// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for firmware info queries: device version, device metadata, and the
//! options the firmware was compiled with.

use std::path::Path;

use onerom_config::fw::FirmwareVersion;
use onerom_fw_emulator::{Emulator, OraResult, build_options, ffi};
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
pub fn test_metadata_uint(emu: &Emulator, config: &Config) -> Result<(), String> {
    // turbo_boot comes from the config, so unlike the board-specific keys
    // below it has an expected value rather than only a contract.
    let (result, value) =
        emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_TURBO_BOOT);
    if !result.is_ok() {
        return Err(format!("TURBO_BOOT: expected OK, got {:?}", result));
    }
    let value = value.ok_or_else(|| "TURBO_BOOT: OK but no value".to_string())?;
    let expected = u32::from(config.turbo_boot);
    if value != expected {
        return Err(format!(
            "TURBO_BOOT: got {}, expected {} from the config",
            value, expected
        ));
    }
    println!("  TURBO_BOOT: {}", value);

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

/// Verify the options the firmware reports being compiled with.
///
/// Covers the values, the type split across the two accessors, and the
/// rejections: an option this firmware does not know, and a NULL out pointer.
///
/// The logging expectations come from `build_options`, which `build.rs` sets
/// from the `TEST_LOGGING` it passed to the C build. The path under test runs
/// from that setting, through the `-D` in `test.mk`, through the firmware and
/// out through the API, and an expectation taken from the firmware's own answer
/// would check none of it. So a run with logging off is the one that catches an
/// option answered from the wrong gate, or from no gate at all.
pub fn test_compile_options(emu: &Emulator, base_dir: &Path) -> Result<(), String> {
    let logging: &[(ffi::ora_compile_option_t, &str, bool)] = &[
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_PLUGIN_LOGGING,
            "PLUGIN_LOGGING",
            build_options::PLUGIN_LOGGING,
        ),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_DEBUG_LOGGING,
            "DEBUG_LOGGING",
            build_options::DEBUG_LOGGING,
        ),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_BOOT_LOGGING,
            "BOOT_LOGGING",
            build_options::BOOT_LOGGING,
        ),
    ];

    for (option, label, built) in logging {
        let expected = u32::from(*built);
        let (result, value) = emu.get_compile_option_uint(*option);
        if !result.is_ok() {
            return Err(format!("{}: expected OK, got {:?}", label, result));
        }
        let value = value.ok_or_else(|| format!("{}: OK but no value", label))?;
        if value != expected {
            return Err(format!(
                "{}: got {}, expected {} — this library was built with it {}",
                label,
                value,
                expected,
                if *built { "on" } else { "off" }
            ));
        }
        println!("  {}: {}", label, value);
    }

    // The build number travels Makefile -> test.mk -> -D -> firmware -> API,
    // so the Makefile is an expectation independent of the firmware. It cannot
    // go stale against the built library either: onerom-fw-emulator's build.rs
    // declares a rerun dependency on that Makefile, so editing the number
    // rebuilds the C.
    let expected_build = makefile_build_number(base_dir)?;
    let (result, value) =
        emu.get_compile_option_uint(ffi::ora_compile_option_t_ORA_COMPILE_OPTION_BUILD_NUMBER);
    if !result.is_ok() {
        return Err(format!("BUILD_NUMBER: expected OK, got {:?}", result));
    }
    let value = value.ok_or_else(|| "BUILD_NUMBER: OK but no value".to_string())?;
    if value != expected_build {
        return Err(format!(
            "BUILD_NUMBER: got {}, expected {} from the root Makefile",
            value, expected_build
        ));
    }
    println!("  BUILD_NUMBER: {}", value);

    // The commit is whatever HEAD was when the C was built, and nothing
    // rebuilds it when HEAD moves, so comparing it against the working tree's
    // HEAD would fail for a reason that is not a firmware fault. Check the
    // shape instead: test.mk substitutes "unknown" when git has no answer.
    let (result, commit) =
        emu.get_compile_option_str(ffi::ora_compile_option_t_ORA_COMPILE_OPTION_GIT_COMMIT);
    if !result.is_ok() {
        return Err(format!("GIT_COMMIT: expected OK, got {:?}", result));
    }
    let commit = commit.ok_or_else(|| "GIT_COMMIT: OK but NULL pointer".to_string())?;
    let plausible =
        commit == "unknown" || (commit.len() >= 4 && commit.chars().all(|c| c.is_ascii_hexdigit()));
    if !plausible {
        return Err(format!(
            "GIT_COMMIT: '{}' is neither an abbreviated hash nor 'unknown'",
            commit
        ));
    }
    println!("  GIT_COMMIT: {}", commit);

    // The two accessors discriminate on the type of the option, over one key
    // space: the string option through the unsigned accessor...
    let result = emu
        .get_compile_option_uint(ffi::ora_compile_option_t_ORA_COMPILE_OPTION_GIT_COMMIT)
        .0;
    if result != OraResult::TypeMismatch {
        return Err(format!(
            "GIT_COMMIT via uint: expected TypeMismatch, got {:?}",
            result
        ));
    }
    // ...and every unsigned option through the string accessor.
    let unsigned: &[(ffi::ora_compile_option_t, &str)] = &[
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_PLUGIN_LOGGING,
            "PLUGIN_LOGGING",
        ),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_DEBUG_LOGGING,
            "DEBUG_LOGGING",
        ),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_BOOT_LOGGING,
            "BOOT_LOGGING",
        ),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_BUILD_NUMBER,
            "BUILD_NUMBER",
        ),
    ];
    for (option, label) in unsigned {
        let (result, value) = emu.get_compile_option_str(*option);
        if result != OraResult::TypeMismatch {
            return Err(format!(
                "{} via str: expected TypeMismatch, got {:?}",
                label, result
            ));
        }
        if value.is_some() {
            return Err(format!("{} via str: wrote a pointer on failure", label));
        }
    }

    // An option this firmware does not know - the case a plugin built against
    // a newer header hits - is NotSupported from both accessors, and not the
    // InvalidArg a NULL out pointer earns below. The two codes are what tells
    // a plugin "this firmware is older than my header, fall back" from "you
    // called this wrong", so the test pins each to its own case.
    // UNKNOWN_OPTION stands for an option added after this firmware was built.
    // INVALID is the sentinel a zeroed or defaulted variable is most likely to
    // hold.
    const UNKNOWN_OPTION: ffi::ora_compile_option_t = 99;
    let unknown: &[(ffi::ora_compile_option_t, &str)] = &[
        (UNKNOWN_OPTION, "an unknown option"),
        (
            ffi::ora_compile_option_t_ORA_COMPILE_OPTION_INVALID,
            "INVALID",
        ),
    ];
    for (option, label) in unknown {
        let (result, value) = emu.get_compile_option_uint(*option);
        if result != OraResult::NotSupported {
            return Err(format!(
                "{} via uint: expected NotSupported, got {:?}",
                label, result
            ));
        }
        if value.is_some() {
            return Err(format!("{} via uint: wrote a value on failure", label));
        }
        let (result, value) = emu.get_compile_option_str(*option);
        if result != OraResult::NotSupported {
            return Err(format!(
                "{} via str: expected NotSupported, got {:?}",
                label, result
            ));
        }
        if value.is_some() {
            return Err(format!("{} via str: wrote a pointer on failure", label));
        }
    }

    // A NULL out pointer is refused rather than written through. Asked with an
    // option of the accessor's own type, so the answer is about the pointer
    // and nothing else.
    let result = emu.get_compile_option_uint_null_out(
        ffi::ora_compile_option_t_ORA_COMPILE_OPTION_BUILD_NUMBER,
    );
    if result != OraResult::InvalidArg {
        return Err(format!(
            "uint with NULL out: expected InvalidArg, got {:?}",
            result
        ));
    }
    let result = emu
        .get_compile_option_str_null_out(ffi::ora_compile_option_t_ORA_COMPILE_OPTION_GIT_COMMIT);
    if result != OraResult::InvalidArg {
        return Err(format!(
            "str with NULL out: expected InvalidArg, got {:?}",
            result
        ));
    }

    Ok(())
}

/// `BUILD_NUMBER` as the root Makefile declares it.
fn makefile_build_number(base_dir: &Path) -> Result<u32, String> {
    let path = base_dir.join("Makefile");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("BUILD_NUMBER") {
            let value = rest
                .trim_start()
                .strip_prefix(":=")
                .or_else(|| rest.trim_start().strip_prefix('='))
                .ok_or_else(|| format!("cannot parse BUILD_NUMBER from '{}'", line))?;
            return value
                .trim()
                .parse()
                .map_err(|e| format!("cannot parse BUILD_NUMBER from '{}': {}", line, e));
        }
    }

    Err(format!("no BUILD_NUMBER in {}", path.display()))
}
