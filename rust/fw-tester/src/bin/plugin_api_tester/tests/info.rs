// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for firmware info queries: device version, device metadata, and the
//! options the firmware was compiled with.

use std::path::Path;

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, OraResult, build_options, ffi};
use onerom_fw_tester::geometry;
use onerom_gen::Config;
use onerom_metadata::{
    OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_BOOT_FLAGS0_ROW, OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT,
    OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT, OTP_FLASH_DEVINFO_ROW, OTP_FLASH_DEVINFO_SIZE_BITS,
    OneromBoardSize, OneromFlashSize,
};

use crate::setup::setup;
use onerom_fw_tester::jumpers::Jumpers;

/// Verify that get_device_version returns a string that matches the parsed
/// firmware version, and that it writes only into a buffer big enough for it.
///
/// The size check is the one a plugin gets wrong: the firmware copies the whole
/// string including its terminator, so a buffer of exactly `strlen` bytes is one
/// short and must be refused rather than filled without a NUL. The two calls
/// either side of that boundary are what make it a boundary and not just a
/// refusal — one byte fewer is `INVALID_SIZE`, exactly enough is the string.
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

    // The terminator is part of what gets copied, so the smallest buffer that
    // works is one byte longer than the string.
    let needed = version_str.len() as u32 + 1;
    let (result, got) = emu.get_device_version(needed);
    if !result.is_ok() || got.as_deref() != Some(version_str.as_str()) {
        return Err(format!(
            "a {needed} byte buffer: got {result:?}/{got:?}, want Ok and '{version_str}'"
        ));
    }
    for max_len in [0, needed - 1] {
        let (result, _) = emu.get_device_version(max_len);
        if result != OraResult::InvalidSize {
            return Err(format!(
                "a {max_len} byte buffer: expected InvalidSize, got {result:?}"
            ));
        }
    }

    println!("  version: {} ({} bytes)", version_str, needed);
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

    // A NULL out pointer is refused rather than written through, and refused
    // for a key this firmware knows - so the code says "your call was wrong",
    // not "this firmware does not have that key".
    let result = emu.get_metadata_str_null_out(ffi::ora_metadata_key_t_ORA_METADATA_KEY_UNIT_NAME);
    if result != OraResult::InvalidArg {
        return Err(format!(
            "UNIT_NAME with NULL out: expected InvalidArg, got {:?}",
            result
        ));
    }

    Ok(())
}

/// Verify device-level unsigned metadata retrieval via the keyed getter, and
/// that the string and unsigned getters discriminate on datum type across the
/// shared key space.
pub fn test_metadata_uint(emu: &Emulator, config: &Config, board: Board) -> Result<(), String> {
    // turbo_boot and reserved_pins come from the config, so unlike the
    // board-specific keys below they have expected values rather than only a
    // contract.
    let reserved = config
        .reserved_pads(board)
        .map_err(|e| format!("reserved_pins: {e}"))?;
    let from_config: &[(ffi::ora_metadata_key_t, &str, u32)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_TURBO_BOOT,
            "TURBO_BOOT",
            u32::from(config.turbo_boot),
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_RESERVED_SEL_PINS,
            "RESERVED_SEL_PINS",
            u32::from(reserved.select_bits()),
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_RESERVED_X_PINS,
            "RESERVED_X_PINS",
            u32::from(reserved.x_bits()),
        ),
    ];
    for (key, label, expected) in from_config {
        let (result, value) = emu.get_metadata_uint(*key);
        if !result.is_ok() {
            return Err(format!("{label}: expected OK, got {result:?}"));
        }
        let value = value.ok_or_else(|| format!("{label}: OK but no value"))?;
        if value != *expected {
            return Err(format!(
                "{label}: got {value}, expected {expected} from the config"
            ));
        }
        println!("  {label}: {value}");
    }

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
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_FIRMWARE_FLAGS,
            "FIRMWARE_FLAGS",
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

    // As above: a NULL out pointer on a key this firmware knows is InvalidArg,
    // which is the answer that tells a plugin to fix the call rather than to
    // fall back to another key.
    let result =
        emu.get_metadata_uint_null_out(ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS);
    if result != OraResult::InvalidArg {
        return Err(format!(
            "GPIO_STATUS with NULL out: expected InvalidArg, got {:?}",
            result
        ));
    }

    Ok(())
}

/// Verify that the BOARD_SIZE key reports the board size from the total flash
/// OTP configures, and that it agrees with the runtime value the firmware
/// recorded.  The cases are OTP unwritten, as an M board's is, and
/// FLASH_DEVINFO with:
/// - 2MB and no chip, which is M
/// - 2MB on each chip select, which is L
/// - 2MB and 4MB, which is neither
/// - 4MB and no chip, which is L
/// - 2MB and a code above 16MB, which counts as no chip
///
/// Chip select 1 counts only on a board with a secondary flash chip select.
///
/// Each boot replaces the firmware's state, so this runs ahead of the boot the
/// rest of a slot's suite uses.  OTP outlives a boot so it is cleared once the
/// boots are done.
pub fn test_metadata_board_size(
    board: Board,
    jumpers: &Jumpers,
    log_enabled: bool,
    sel_image: u8,
) -> Result<(), String> {
    use OneromBoardSize::{BoardSizeL, BoardSizeM, BoardSizeOther};

    let none = OneromFlashSize::FlashSizeNone as u16;
    let mb2 = OneromFlashSize::FlashSize2mb as u16;
    let mb4 = OneromFlashSize::FlashSize4mb as u16;
    // `size` where chip select 1 counts.  Each case using this has 2MB on
    // chip select 0, which is M on its own.
    let if_cs1_counts = |size| {
        if board.external_flash_cs_pin().is_some() {
            size
        } else {
            BoardSizeM
        }
    };

    // Each case's chip select 0 and 1 size codes, or `None` for OTP unwritten.
    let cases = [
        ("OTP unwritten", None, BoardSizeM),
        ("2MB and no chip", Some((mb2, none)), BoardSizeM),
        ("2MB and 2MB", Some((mb2, mb2)), if_cs1_counts(BoardSizeL)),
        (
            "2MB and 4MB",
            Some((mb2, mb4)),
            if_cs1_counts(BoardSizeOther),
        ),
        ("4MB and no chip", Some((mb4, none)), BoardSizeL),
        (
            "2MB and a code above 16MB",
            Some((mb2, OTP_FLASH_DEVINFO_SIZE_BITS)),
            BoardSizeM,
        ),
    ];

    let mut result = Ok(());
    for (label, sizes, expected) in cases {
        Emulator::clear_otp();
        if let Some((cs0, cs1)) = sizes {
            Emulator::set_otp_raw(
                OTP_BOOT_FLAGS0_ROW,
                &[OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE; 3],
            );
            let devinfo = (cs0 << OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT)
                | (cs1 << OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT);
            Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[devinfo]);
        }
        let (emu, _) = setup(board, jumpers, log_enabled, sel_image);

        let expected = expected as u32;
        let (status, value) =
            emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_BOARD_SIZE);
        let recorded = u32::from(emu.board_size());
        if !status.is_ok() || value != Some(expected) || recorded != expected {
            result = Err(format!(
                "BOARD_SIZE with {label}: got {status:?}/{value:?}, the firmware recorded \
                 {recorded}, expected {expected}"
            ));
            break;
        }
        println!("  BOARD_SIZE with {label}: {expected}");
    }

    Emulator::clear_otp();
    result
}

/// Verify that the FLASH_CS0_SIZE and FLASH_CS1_SIZE keys report the flash on
/// each chip select from OTP.  The cases are OTP unwritten and FLASH_DEVINFO
/// with:
/// - 2MB and no chip, an M board
/// - 2MB on each chip select, an L board
/// - 2MB and a code above 16MB, which counts as no chip
///
/// Chip select 1 counts only on a board with a secondary flash chip select.  On
/// a board whose gpio_ext_flash_cs is GPIO_NONE, 2MB on each chip select
/// reports no chip on chip select 1.
///
/// The keys read OTP on each call, so the cases change OTP under one boot.
/// OTP is cleared afterwards, as it was at boot.  Also checks the keys are
/// TypeMismatch through the string and indexed getters, and that a NULL out
/// pointer is InvalidArg.
pub fn test_metadata_flash_sizes(emu: &Emulator, board: Board) -> Result<(), String> {
    let result = check_flash_sizes(emu, board);
    Emulator::clear_otp();
    result
}

/// [`test_metadata_flash_sizes`], less clearing OTP.
fn check_flash_sizes(emu: &Emulator, board: Board) -> Result<(), String> {
    use OneromFlashSize::{FlashSize2mb, FlashSizeNone};

    let mb2 = FlashSize2mb as u16;
    let none = FlashSizeNone as u16;
    let cs1_2mb = if board.external_flash_cs_pin().is_some() {
        FlashSize2mb
    } else {
        FlashSizeNone
    };

    // Each case's chip select 0 and 1 size codes, or `None` for OTP
    // unwritten, then the sizes the keys report.
    let cases = [
        ("OTP unwritten", None, (FlashSize2mb, FlashSizeNone)),
        (
            "2MB and no chip",
            Some((mb2, none)),
            (FlashSize2mb, FlashSizeNone),
        ),
        ("2MB and 2MB", Some((mb2, mb2)), (FlashSize2mb, cs1_2mb)),
        (
            "2MB and a code above 16MB",
            Some((mb2, OTP_FLASH_DEVINFO_SIZE_BITS)),
            (FlashSize2mb, FlashSizeNone),
        ),
    ];
    let keys = [
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_FLASH_CS0_SIZE,
            "FLASH_CS0_SIZE",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_FLASH_CS1_SIZE,
            "FLASH_CS1_SIZE",
        ),
    ];

    for (label, sizes, (cs0, cs1)) in cases {
        Emulator::clear_otp();
        if let Some((cs0, cs1)) = sizes {
            Emulator::set_otp_raw(
                OTP_BOOT_FLAGS0_ROW,
                &[OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE; 3],
            );
            let devinfo = (cs0 << OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT)
                | (cs1 << OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT);
            Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[devinfo]);
        }
        for ((key, name), expected) in keys.iter().zip([cs0, cs1]) {
            let expected = expected as u32;
            let (status, value) = emu.get_metadata_uint(*key);
            if !status.is_ok() || value != Some(expected) {
                return Err(format!(
                    "{name} with {label}: got {status:?}/{value:?}, expected {expected}"
                ));
            }
        }
        println!("  FLASH_CS0_SIZE/FLASH_CS1_SIZE with {label}: {cs0}/{cs1}");
    }

    for (key, name) in keys {
        let (status, _) = emu.get_metadata_str(key);
        if status != OraResult::TypeMismatch {
            return Err(format!(
                "{name} via str: expected TypeMismatch, got {status:?}"
            ));
        }
        let (status, _) = emu.get_metadata_uint_at(key, 0);
        if status != OraResult::TypeMismatch {
            return Err(format!(
                "{name} via uint_at: expected TypeMismatch, got {status:?}"
            ));
        }
        let status = emu.get_metadata_uint_null_out(key);
        if status != OraResult::InvalidArg {
            return Err(format!(
                "{name} with NULL out: expected InvalidArg, got {status:?}"
            ));
        }
    }
    Ok(())
}

/// Verify indexed retrieval of the array-valued metadata keys.
///
/// The GPIO numbers are checked against two sources the firmware had no hand
/// in: `board.sel_pins()`, generated from the board's own configuration, and
/// the metadata header rebuilt here by `geometry::build_header`. `sel_pins()`
/// is the stronger of the two - it does not come from the metadata blob at all,
/// so it catches the blob and the accessor being wrong together.
///
/// Also covers the rejections, which are what a caller scanning for the
/// GPIO_NONE terminator relies on: an index past the end, a key that is not an
/// array through this accessor, an array key through the non-indexed accessor,
/// the sentinels, and a NULL out pointer.
pub fn test_metadata_uint_at(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
) -> Result<(), String> {
    let header = geometry::build_header(config, board, fw_version, base_dir)?;

    // The stored arrays, GPIO_NONE entries and all - this accessor reports
    // every slot, and the terminator is what tells a caller where to stop.
    let arrays: &[(ffi::ora_metadata_key_t, &str, &[u8])] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_SEL,
            "GPIO_SEL",
            &header.hw.gpio_sel,
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_X1,
            "GPIO_X1",
            &header.hw.gpio_x1,
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_X2,
            "GPIO_X2",
            &header.hw.gpio_x2,
        ),
    ];

    let mut errors = Vec::new();

    for (key, label, expected) in arrays {
        let mut got = Vec::with_capacity(expected.len());
        for index in 0..expected.len() {
            let (result, value) = emu.get_metadata_uint_at(*key, index as u32);
            if !result.is_ok() {
                errors.push(format!(
                    "{}[{}]: expected OK, got {:?}",
                    label, index, result
                ));
                continue;
            }
            let Some(value) = value else {
                errors.push(format!("{}[{}]: OK but no value", label, index));
                continue;
            };
            if value != u32::from(expected[index]) {
                errors.push(format!(
                    "{}[{}]: got {}, expected {} from the metadata",
                    label, index, value, expected[index]
                ));
            }
            got.push(value);
        }
        println!("  {}: {:?}", label, got);

        // One past the end is a caller error, not a terminator.
        let (result, _) = emu.get_metadata_uint_at(*key, expected.len() as u32);
        if result != OraResult::InvalidArg {
            errors.push(format!(
                "{}[{}]: expected InvalidArg past the end, got {:?}",
                label,
                expected.len(),
                result
            ));
        }

        // An array key is not readable through the non-indexed accessor.
        let (result, _) = emu.get_metadata_uint(*key);
        if result != OraResult::TypeMismatch {
            errors.push(format!(
                "{} via uint: expected TypeMismatch, got {:?}",
                label, result
            ));
        }

        let result = emu.get_metadata_uint_at_null_out(*key, 0);
        if result != OraResult::InvalidArg {
            errors.push(format!(
                "{} with NULL out: expected InvalidArg, got {:?}",
                label, result
            ));
        }
    }

    // The image select GPIOs, checked against the board configuration rather
    // than against the metadata built from it.  They are stored contiguously
    // from index 0, so the board's list must be a prefix of the array.
    let sel_pins = board.sel_pins();
    for (index, expected) in sel_pins.iter().enumerate() {
        let (result, value) = emu.get_metadata_uint_at(
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_SEL,
            index as u32,
        );
        if !result.is_ok() {
            errors.push(format!(
                "GPIO_SEL[{}]: expected OK, got {:?}",
                index, result
            ));
            continue;
        }
        if value != Some(u32::from(*expected)) {
            errors.push(format!(
                "GPIO_SEL[{}]: got {:?}, expected {} from the board configuration",
                index, value, expected
            ));
        }
    }
    // The entry after the board's last select pin terminates the list.
    if sel_pins.len() < header.hw.gpio_sel.len() {
        let (result, value) = emu.get_metadata_uint_at(
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_SEL,
            sel_pins.len() as u32,
        );
        if !result.is_ok() || value != Some(u32::from(onerom_metadata::GPIO_NONE)) {
            errors.push(format!(
                "GPIO_SEL[{}]: expected GPIO_NONE terminator, got {:?}/{:?}",
                sel_pins.len(),
                result,
                value
            ));
        }
    }

    // A scalar key and a string key are both TypeMismatch through this
    // accessor, whatever the index.
    let wrong_type: &[(ffi::ora_metadata_key_t, &str)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS,
            "GPIO_STATUS",
        ),
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_HW_REV, "HW_REV"),
    ];
    for (key, label) in wrong_type {
        let (result, _) = emu.get_metadata_uint_at(*key, 0);
        if result != OraResult::TypeMismatch {
            errors.push(format!(
                "{} via uint_at: expected TypeMismatch, got {:?}",
                label, result
            ));
        }
    }

    // Unknown / sentinel keys are NOT_SUPPORTED, as through the other
    // accessors - the key space is one space, whichever accessor asks.
    let unknown: &[(ffi::ora_metadata_key_t, &str)] = &[
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_INVALID, "INVALID"),
        (ffi::ora_metadata_key_t_ORA_METADATA_KEY_NONE, "NONE"),
    ];
    for (key, label) in unknown {
        let (result, _) = emu.get_metadata_uint_at(*key, 0);
        if result != OraResult::NotSupported {
            errors.push(format!(
                "{}: expected NotSupported, got {:?}",
                label, result
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
