// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the GPIO plugin API (`ORA_ID_GPIO_QUERY`).
//!
//! `ora_gpio_set`'s register writes are compiled out under `TEST_BUILD`, but it
//! records what it drove into the test build's pad model and `ora_gpio_query`
//! reads that back, so the set side is reachable from here.  What these tests
//! cover is the query side, and in particular its `use` classification, which is
//! where the firmware can silently disagree with itself.
//!
//! # Where the expected answer comes from
//!
//! The classification lives in `pio_get_gpio_use()`
//! (`firmware/src/piodma/piorom2.c`) and duplicates knowledge the PIO setup
//! path also holds, so it can silently desync when slot layouts change.  What
//! these tests compare it against therefore has to be something other than a
//! restatement of the same derivation.  Two independent sources are used:
//!
//! 1. **The serving algorithm configuration in the generated firmware
//!    metadata**, read back through the same `onerom_gen` build the firmware
//!    under test was built from (see [`onerom_fw_tester::geometry`]).  This is
//!    a Rust reimplementation of the span arithmetic the C firmware does in
//!    `retrieve_gpio_init()`, over the same bytes, so an arithmetic slip in
//!    either shows up as a mismatch.  It covers every pin the serving
//!    algorithms name, including ones `retrieve_gpio_init()` does not itself
//!    collect (the `ALG_CS_2` qualifier pins and `ALG_DATA_1`'s A-1 pin), so a
//!    pin serving genuinely reads but the classifier reports free is a
//!    failure, not an invisible gap.  The slot's `gpio_override_config`
//!    identifies the pins whose input is forced or inverted.
//!
//! 2. **The apio emulation's own record of what the serving setup configured**
//!    — `_apio_emulated_gpios.output_block[]`, written by
//!    `APIO_GPIO_INPUT_OUTPUT` in `setup_serving_gpios()`.  This is what
//!    serving actually did rather than what its configuration says, and it
//!    pins down the driven (data) pins exactly.
//!
//!    Note the companion `input_only` bit cannot stand as the expected answer
//!    for the read pins: `setup_initial_gpios()` (`firmware/src/rp235x.c`) applies
//!    `APIO_GPIO_INPUT_ONLY` to every GPIO at boot, so after boot every pin
//!    except the data pins has it set, whether serving reads it or not.  That
//!    is the same reason the firmware needs this API at all — an address or CS
//!    pin is indistinguishable from a free one by register inspection.

use std::path::Path;

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::RpVariant;
use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_fw_tester::geometry;
use onerom_gen::Config;
use onerom_metadata::{
    GPIO_NONE, GpioOverride, MaybeKnown, OneromAlgAddrConfig, OneromAlgConfig, OneromAlgCsConfig,
    OneromAlgDataConfig, OneromMetadataHeader, RomSlotType,
};

/// GPIOs on the running RP2350 variant, mirroring the firmware's `max_gpios[]`
/// (`firmware/src/constants.c`), which `MAX_GPIOS` indexes by variant.
///
/// A board with no RP variant boots the emulation as an RP235xA, matching
/// [`Emulator::set_rp_variant`].
pub fn max_gpios(board: Board) -> u8 {
    match board.rp_variant() {
        Some(RpVariant::Rp235xB) => 48,
        _ => 30,
    }
}

/// A bitmask of the `count` GPIOs starting at `base`, or 0 if there are none.
///
/// `base` is `GPIO_NONE` when the slot has no such span; the firmware's own
/// span tests are all guarded by `< MAX_GPIOS`, so an absent span contributes
/// nothing.
fn span(base: u8, count: u8) -> u64 {
    if base == GPIO_NONE || count == 0 || base as u32 + count as u32 > 64 {
        return 0;
    }
    let mask = if count >= 64 {
        u64::MAX
    } else {
        (1u64 << count) - 1
    };
    mask << base
}

/// A single GPIO as a bitmask, or 0 if it is absent.
fn pin(gpio: u8) -> u64 {
    if gpio == GPIO_NONE || gpio >= 64 {
        0
    } else {
        1u64 << gpio
    }
}

/// The pins the active slot's serving algorithms name, split by whether
/// serving drives them or only reads them.
struct ServingSet {
    /// Pins PIO drives — the data pins.
    driven: u64,
    /// Pins serving reads: the address span, the chip-select span, the /BYTE
    /// pin, `ALG_CS_2`'s qualifier pins and `ALG_DATA_1`'s A-1 pin.
    read: u64,
    /// The chip-select span and the `ALG_CS_0` /BYTE pin.  The firmware
    /// classifies these ahead of the forced pins.
    selects: u64,
    /// Pins whose input is forced to 0 or 1.
    forced: u64,
    /// Pins whose input is inverted.
    inverted: u64,
}

impl ServingSet {
    /// Pins `ora_gpio_query` reports as serving.
    fn in_use(&self) -> u64 {
        self.driven | self.selects | (self.read & !self.forced)
    }
}

/// The serving algorithm configuration of the `flash_slot`-th non-plugin ROM
/// slot in the generated metadata.
fn rom_slot_alg(
    header: &OneromMetadataHeader,
    flash_slot: usize,
) -> Result<&OneromAlgConfig, String> {
    let is_plugin = |t: MaybeKnown<RomSlotType>| {
        matches!(
            t,
            MaybeKnown::Known(
                RomSlotType::RomSlotTypePluginSystem
                    | RomSlotType::RomSlotTypePluginUser
                    | RomSlotType::RomSlotTypePluginPio
            )
        )
    };

    let slot = header
        .rom_slots
        .iter()
        .filter(|s| !is_plugin(s.slot_type))
        .nth(flash_slot)
        .ok_or_else(|| format!("metadata doesn't have flash slot {flash_slot}"))?;
    slot.alg
        .as_ref()
        .ok_or_else(|| format!("flash slot {flash_slot} doesn't have an alg config"))
}

/// Assemble the serving set for a ROM slot with `alg`.
///
/// Every GPIO field in the algorithm configuration is relative to that
/// algorithm's `gpio_base` (the PIO block's `GPIOBASE`), so each is offset by
/// its own base — which is exactly the arithmetic `retrieve_gpio_init()` does,
/// including only offsetting the /BYTE pin when one is present.
fn serving_set(alg: &OneromAlgConfig) -> Result<ServingSet, String> {
    // Chip select and data.  The common fields are repeated per variant
    // because each variant is a distinct enum shape; the extras differ.
    let (cs_base, cs_pins, data_base, data_pins, cs_byte, cs_extra) = match alg.alg_cs {
        OneromAlgCsConfig::AlgCs0 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            byte_pin,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            // The /BYTE pin sits outside the CS span, so it has to be named
            // specifically or it would fall through to free.
            if byte_pin == GPIO_NONE {
                0
            } else {
                pin(gpio_base + byte_pin)
            },
            0,
        ),
        OneromAlgCsConfig::AlgCs1 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            // cs_ignore_index names a position the select field masks out; it
            // is still inside the CS span and still sampled, so it needs no
            // separate handling.
            0,
            0,
        ),
        OneromAlgCsConfig::AlgCs2 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            base_qualifier_pin,
            num_qualifier_pins,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            0,
            // The qualifier pins are address lines the CS state machine
            // samples to decide whether this bank is selected.
            span(gpio_base + base_qualifier_pin, num_qualifier_pins),
        ),
        // The pins a variant samples beyond the common fields are its own, and
        // an expectation missing them would pass against the wrong pin set.
        OneromAlgCsConfig::Unknown { alg, .. } => {
            return Err(format!(
                "CS algorithm {alg} is not one this build knows the sampled pins of"
            ));
        }
    };

    // Every field the address window needs is common to the family.
    let (OneromAlgAddrConfig::AlgAddr0 {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }
    | OneromAlgAddrConfig::Unknown {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }) = alg.alg_addr;
    let addr = span(gpio_base + base_addr_pin, num_addr_pins);

    // The data algorithm names the /BYTE pin again, plus the A-1 pin the
    // 16-bit data state machine reads to pick a half-word.
    let data_extra = match alg.alg_data {
        OneromAlgDataConfig::AlgData0 { .. } => 0,
        OneromAlgDataConfig::AlgData1 {
            gpio_base,
            byte_pin,
            a_minus_1_pin,
            ..
        } => pin(gpio_base + byte_pin) | pin(gpio_base + a_minus_1_pin),
        // As for the CS algorithm above.
        OneromAlgDataConfig::Unknown { alg, .. } => {
            return Err(format!(
                "data algorithm {alg} is not one this build knows the sampled pins of"
            ));
        }
    };

    // Entries are encoded as in `onerom_alg_override_config_t`.
    let overrides = |modes: &[GpioOverride]| -> u64 {
        alg.gpio_override_config
            .iter()
            .flat_map(|config| config.params.iter())
            .filter(|&&entry| modes.iter().any(|&mode| entry >> 6 == mode as u8))
            .fold(0, |mask, &entry| mask | pin(entry & 0x3F))
    };
    let forced = overrides(&[GpioOverride::GpioOverLow, GpioOverride::GpioOverHigh]);
    let inverted = overrides(&[GpioOverride::GpioOverInvert]);

    let driven = span(data_base, data_pins);
    let selects = (span(cs_base, cs_pins) | cs_byte) & !driven;
    let read = (addr | selects | cs_extra | data_extra) & !driven;

    Ok(ServingSet {
        driven,
        read,
        selects,
        forced,
        inverted,
    })
}

/// GPIOs the apio emulation records as PIO-driven outputs, i.e. those
/// `setup_serving_gpios()` passed to `APIO_GPIO_INPUT_OUTPUT`.
pub fn apio_driven_pins(max_gpios: u8) -> u64 {
    // SAFETY: `_apio_emulated_gpios` is a plain C global written by the
    // firmware under emulation; this reads it through a raw pointer without
    // forming a reference to the `static mut`.
    let output_block = unsafe { (*core::ptr::addr_of!(ffi::_apio_emulated_gpios)).output_block };
    let mut mask = 0u64;
    for gpio in 0..max_gpios {
        if output_block[gpio as usize] >= 0 {
            mask |= 1u64 << gpio;
        }
    }
    mask
}

/// The board's own system GPIOs, read back through the metadata getters.
///
/// These are the pins `ora_gpio_query` reports as `ORA_GPIO_USE_SYSTEM` when
/// serving is not using them.  Each is `GPIO_NONE` on a board without it.
fn system_pins(emu: &Emulator) -> Result<u64, String> {
    let keys: &[(ffi::ora_metadata_key_t, &str)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS,
            "GPIO_STATUS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_NEOPIXEL,
            "GPIO_NEOPIXEL",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_VBUS,
            "GPIO_VBUS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_EXT_FLASH_CS,
            "GPIO_EXT_FLASH_CS",
        ),
    ];

    let mut mask = 0u64;
    for (key, label) in keys {
        let (result, value) = emu.get_metadata_uint(*key);
        if !result.is_ok() {
            return Err(format!("{label}: expected OK, got {result:?}"));
        }
        let value = value.ok_or_else(|| format!("{label}: OK but no value"))?;
        mask |= pin(value as u8);
    }
    Ok(mask)
}

/// The other GPIOs wired to the same X pin of `board` as `gpio`, as a mask.
fn x_pin_others(board: Board, gpio: u8) -> u64 {
    [1, 2]
        .into_iter()
        .map(|x_pin| board.gpios_for_x_pin(x_pin))
        .filter(|gpios| gpios.contains(&gpio))
        .flatten()
        .filter(|&&other| other != gpio)
        .fold(0, |mask, &other| mask | pin(other))
}

fn use_name(value: u8) -> String {
    match value as ffi::ora_gpio_use_t {
        ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE => "FREE".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ => "SERVING_READ".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN => "SERVING_DRIVEN".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM => "SYSTEM".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_INPUT_FORCED => "INPUT_FORCED".to_string(),
        other => format!("<unknown {other}>"),
    }
}

/// Verify `ora_gpio_query`'s `use` field for every GPIO on the device against
/// the serving set the firmware was built from, cross-checked against what the
/// apio emulation recorded serving doing.
pub fn test_gpio_use(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
    set_idx: usize,
) -> Result<(), String> {
    let max_gpios = max_gpios(board);

    let header = geometry::build_header(config, board, fw_version, base_dir)?;
    let alg = rom_slot_alg(&header, crate::flash_slot(config, set_idx))?;
    let serving = serving_set(alg)?;
    let system = system_pins(emu)?;

    // The host's copy of the firmware's rule, which `program --reset-host`
    // checks an image with.
    let used = onerom_gen::used_gpios(alg);
    if used != serving.in_use() {
        return Err(format!(
            "onerom_gen::used_gpios says 0x{used:012X}, the serving set says 0x{:012X}",
            serving.in_use()
        ));
    }

    // Cross-check the metadata-derived data pins against what serving actually
    // handed to the PIO.  If these disagree the metadata-derived expectation
    // below is untrustworthy, so say so before reporting per-pin mismatches.
    let driven_by_apio = apio_driven_pins(max_gpios);
    if driven_by_apio != serving.driven {
        return Err(format!(
            "data pins disagree: metadata says 0x{:012X}, apio recorded serving configuring 0x{:012X}",
            serving.driven, driven_by_apio
        ));
    }

    let mut errors = Vec::new();
    let mut counts = [0usize; 5];

    for gpio in 0..max_gpios {
        let bit = 1u64 << gpio;
        // No cast: bindgen is given -fshort-enums to match the C, so this is
        // already the enum's own width.
        let expected: ffi::ora_gpio_use_t = if serving.driven & bit != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN
        } else if serving.selects & bit != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
        } else if serving.forced & bit != 0 {
            if system & bit != 0 {
                ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM
            } else {
                ffi::ora_gpio_use_t_ORA_GPIO_USE_INPUT_FORCED
            }
        } else if serving.read & bit != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
        } else if system & bit != 0 {
            // Serving takes precedence: a system pin the active slot also uses
            // is reported as what serving is using it for.
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM
        } else {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE
        };
        // Driving a GPIO drives every GPIO wired to the same X pin, so a GPIO
        // serving doesn't use reports what serving uses the other for.
        let x_pin_use = x_pin_others(board, gpio) & serving.in_use();
        let expected = if x_pin_use == 0
            || matches!(
                expected,
                ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN
                    | ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
                    | ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM
            ) {
            expected
        } else if x_pin_use & serving.driven != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN
        } else {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
        };

        let (result, info) = emu.gpio_query(gpio);
        if !result.is_ok() {
            errors.push(format!("gpio {gpio}: query failed: {result:?}"));
            continue;
        }
        if info.size as usize != size_of::<ffi::ora_gpio_info_t>() {
            errors.push(format!(
                "gpio {gpio}: wrote {} bytes, expected {}",
                info.size,
                size_of::<ffi::ora_gpio_info_t>()
            ));
        }
        if info.gpio_use != expected {
            errors.push(format!(
                "gpio {gpio}: use {} expected {}",
                use_name(info.gpio_use),
                use_name(expected)
            ));
        }
        if serving.inverted & bit != 0
            && info.gpio_use != ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
        {
            errors.push(format!(
                "gpio {gpio}: input inverted, use {} expected SERVING_READ",
                use_name(info.gpio_use)
            ));
        }
        if (info.gpio_use as usize) < counts.len() {
            counts[info.gpio_use as usize] += 1;
        }
    }

    // An out-of-range GPIO is rejected rather than silently classified, which
    // is what makes the loop above a whole-device sweep.
    for gpio in [max_gpios, 63, 255] {
        let (result, _) = emu.gpio_query(gpio);
        if result != OraResult::InvalidArg {
            errors.push(format!(
                "gpio {gpio} (out of range): expected InvalidArg, got {result:?}"
            ));
        }
    }

    // The forward-compatibility contract: the firmware writes no more than the
    // caller's own sizeof and reports how much it wrote.  0xFF is the sentinel
    // the wrapper pre-fills unwritten fields with.
    let (result, info) = emu.gpio_query_sized(0, 2);
    if !result.is_ok() {
        errors.push(format!("short query: expected OK, got {result:?}"));
    } else if info.size != 2 || info.level != 0xFF || info.is_output != 0xFF {
        errors.push(format!(
            "short query: wrote {} bytes and touched level={} is_output={}",
            info.size, info.level, info.is_output
        ));
    }
    let (result, _) = emu.gpio_query_sized(0, 0);
    if result != OraResult::InvalidSize {
        errors.push(format!(
            "zero-size query: expected InvalidSize, got {result:?}"
        ));
    }
    // The other end of the same contract, for a caller built against a header
    // with more fields than this firmware knows: the size is clamped to what
    // the firmware has to write, and reported back as that, rather than the
    // caller's own number being echoed and the extra fields left as whatever
    // the caller had there.
    let (result, info) = emu.gpio_query_sized(0, u8::MAX);
    if !result.is_ok() {
        errors.push(format!("oversized query: expected OK, got {result:?}"));
    } else if info.size as usize != size_of::<ffi::ora_gpio_info_t>() {
        errors.push(format!(
            "oversized query: reported {} bytes written, expected {}",
            info.size,
            size_of::<ffi::ora_gpio_info_t>()
        ));
    }

    if errors.is_empty() {
        println!(
            "  {} GPIOs: {} free, {} serving-read ({} inverted), {} serving-driven, {} system, {} input-forced",
            max_gpios,
            counts[0],
            counts[1],
            serving.inverted.count_ones(),
            counts[2],
            counts[3],
            counts[4]
        );
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Record a mismatch between what a call returned and what it had to return.
fn note(errors: &mut Vec<String>, what: &str, got: OraResult, want: OraResult) {
    if got != want {
        errors.push(format!("{what}: got {got:?}, want {want:?}"));
    }
}

/// The first GPIO `ora_gpio_query` classifies as `want`, or an error naming
/// what the sweep did find.
///
/// Which GPIO is free, read or driven depends on the board and the slot under
/// test, so every pin these tests touch is chosen from the firmware's own
/// classification rather than named here.
fn first_gpio_with_use(
    emu: &Emulator,
    max_gpios: u8,
    want: ffi::ora_gpio_use_t,
) -> Result<u8, String> {
    for gpio in 0..max_gpios {
        let (result, info) = emu.gpio_query(gpio);
        if result.is_ok() && info.gpio_use == want {
            return Ok(gpio);
        }
    }
    Err(format!("no GPIO classified {}", use_name(want)))
}

/// `ora_gpio_set` drives, releases and refuses.
///
/// Arm by asking the firmware which pin is free and which two it is serving
/// with.  Stimulate the free pin through all three states and read each back
/// through `ora_gpio_query`, which is a different code path reading the same
/// pad.  Fence the refusals: an out-of-range pin and a state that is not one
/// of the three are `INVALID_ARG`, and a pin serving is using is `GPIO_IN_USE`
/// without `ORA_GPIO_FLAG_FORCE`.  Discriminate on both sides — a refused call
/// must leave its pin exactly as it was, and the same call with the force flag
/// must go through, so the test cannot pass by refusing everything.
///
/// The pad the test build models is not the one serving reads: `stub_gpio_set`
/// writes an array only `ora_gpio_query` reads, so forcing a serving pin here
/// changes nothing the PIO emulation sees.  Every pin is put back the way it
/// was found regardless.
pub fn test_gpio_set(emu: &Emulator, board: Board) -> Result<(), String> {
    const LOW: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_LOW;
    const HIGH: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_HIGH;
    const INPUT: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_INPUT;
    /// Past the three `ora_gpio_state_t` values.
    const NOT_A_STATE: ffi::ora_gpio_state_t = 99;

    let max_gpios = max_gpios(board);
    let free = first_gpio_with_use(emu, max_gpios, ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE)?;
    let read = first_gpio_with_use(
        emu,
        max_gpios,
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ,
    )?;
    let driven = first_gpio_with_use(
        emu,
        max_gpios,
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN,
    )?;

    let mut errors = Vec::new();

    // What the two serving pins look like before anything is refused.
    let (_, read_before) = emu.gpio_query(read);
    let (_, driven_before) = emu.gpio_query(driven);

    // Drive the free pin high, then low, then release it.
    for (state, label, want_output, want_level) in [
        (HIGH, "high", 1u8, 1u8),
        (LOW, "low", 1, 0),
        (INPUT, "input", 0, 0),
    ] {
        note(
            &mut errors,
            &format!("set free GPIO {free} {label}"),
            emu.gpio_set(free, state, false),
            OraResult::Ok,
        );
        let (result, info) = emu.gpio_query(free);
        if !result.is_ok() {
            errors.push(format!("query after {label}: {result:?}"));
        } else if info.is_output != want_output || info.level != want_level {
            errors.push(format!(
                "GPIO {free} after {label}: is_output={} level={}, want is_output={want_output} level={want_level}",
                info.is_output, info.level
            ));
        }
    }

    // Refusals.
    for gpio in [max_gpios, 63, 255] {
        note(
            &mut errors,
            &format!("set out-of-range GPIO {gpio}"),
            emu.gpio_set(gpio, LOW, false),
            OraResult::InvalidArg,
        );
    }
    note(
        &mut errors,
        "set a state that is not one of the three",
        emu.gpio_set(free, NOT_A_STATE, false),
        OraResult::InvalidArg,
    );
    // Forcing does not excuse a bad state or a bad pin.
    note(
        &mut errors,
        "force a state that is not one of the three",
        emu.gpio_set(free, NOT_A_STATE, true),
        OraResult::InvalidArg,
    );
    note(
        &mut errors,
        &format!("set serving-read GPIO {read} unforced"),
        emu.gpio_set(read, HIGH, false),
        OraResult::GpioInUse,
    );
    note(
        &mut errors,
        &format!("set serving-driven GPIO {driven} unforced"),
        emu.gpio_set(driven, HIGH, false),
        OraResult::GpioInUse,
    );

    // A refused call left its pin alone.
    let (_, read_after) = emu.gpio_query(read);
    let (_, driven_after) = emu.gpio_query(driven);
    if read_after != read_before {
        errors.push(format!(
            "refused set moved serving-read GPIO {read}: {read_before:?} -> {read_after:?}"
        ));
    }
    if driven_after != driven_before {
        errors.push(format!(
            "refused set moved serving-driven GPIO {driven}: {driven_before:?} -> {driven_after:?}"
        ));
    }

    // ...and the same call with the force flag goes through.
    note(
        &mut errors,
        &format!("force serving-read GPIO {read} high"),
        emu.gpio_set(read, HIGH, true),
        OraResult::Ok,
    );
    let (_, forced) = emu.gpio_query(read);
    if forced.is_output != 1 || forced.level != 1 {
        errors.push(format!(
            "forced GPIO {read}: is_output={} level={}, want 1/1",
            forced.is_output, forced.level
        ));
    }
    // Releasing a forced serving-read pin restores it, which is the contract
    // that makes forcing one recoverable.
    note(
        &mut errors,
        &format!("release forced GPIO {read}"),
        emu.gpio_set(read, INPUT, true),
        OraResult::Ok,
    );
    let (_, released) = emu.gpio_query(read);
    if released != read_before {
        errors.push(format!(
            "released GPIO {read}: {released:?}, want {read_before:?}"
        ));
    }

    if errors.is_empty() {
        println!("  free GPIO {free}, serving read {read}, serving driven {driven}");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn first_input_forced(emu: &Emulator, board: Board) -> Option<u8> {
    first_gpio_with_use(
        emu,
        max_gpios(board),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_INPUT_FORCED,
    )
    .ok()
}

/// An input forced GPIO can be driven without `ORA_GPIO_FLAG_FORCE`.
pub fn test_gpio_set_input_forced(emu: &Emulator, gpio: u8) -> Result<(), String> {
    const FORCED: ffi::ora_gpio_use_t = ffi::ora_gpio_use_t_ORA_GPIO_USE_INPUT_FORCED;

    let mut errors = Vec::new();
    let (_, before) = emu.gpio_query(gpio);

    for (state, label, want_output, want_level) in [
        (ffi::ora_gpio_state_t_ORA_GPIO_STATE_HIGH, "high", 1u8, 1u8),
        (ffi::ora_gpio_state_t_ORA_GPIO_STATE_LOW, "low", 1, 0),
        (ffi::ora_gpio_state_t_ORA_GPIO_STATE_INPUT, "input", 0, 0),
    ] {
        note(
            &mut errors,
            &format!("set input-forced GPIO {gpio} {label} unforced"),
            emu.gpio_set(gpio, state, false),
            OraResult::Ok,
        );
        let (result, info) = emu.gpio_query(gpio);
        if !result.is_ok() {
            errors.push(format!("query after {label}: {result:?}"));
        } else if info.is_output != want_output
            || info.level != want_level
            || info.gpio_use != FORCED
        {
            errors.push(format!(
                "GPIO {gpio} after {label}: use={} is_output={} level={}, want use=INPUT_FORCED is_output={want_output} level={want_level}",
                use_name(info.gpio_use),
                info.is_output,
                info.level
            ));
        }
    }

    let (_, after) = emu.gpio_query(gpio);
    if after != before {
        errors.push(format!("released GPIO {gpio}: {after:?}, want {before:?}"));
    }

    if errors.is_empty() {
        println!("  input forced GPIO {gpio}");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// `ora_gpio_set` refuses each of `gpios`, so an X pin serving reads through
/// one GPIO can't be driven through the other.
///
/// Arm with [`serving_x_pin_gpios`].  Stimulate each with an unforced set,
/// which must be refused and leave the pin alone.  Discriminate with the same
/// set forced, which must go through, then release the pin and check it is as
/// it was.
pub fn test_gpio_set_x_pin(emu: &Emulator, gpios: &[u8]) -> Result<(), String> {
    const HIGH: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_HIGH;
    const INPUT: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_INPUT;

    let mut errors = Vec::new();
    for &gpio in gpios {
        let (_, before) = emu.gpio_query(gpio);
        note(
            &mut errors,
            &format!("set X pin GPIO {gpio} unforced"),
            emu.gpio_set(gpio, HIGH, false),
            OraResult::GpioInUse,
        );
        let (_, after) = emu.gpio_query(gpio);
        if after != before {
            errors.push(format!(
                "refused set moved X pin GPIO {gpio}: {before:?} -> {after:?}"
            ));
        }
        note(
            &mut errors,
            &format!("force X pin GPIO {gpio} high"),
            emu.gpio_set(gpio, HIGH, true),
            OraResult::Ok,
        );
        note(
            &mut errors,
            &format!("release X pin GPIO {gpio}"),
            emu.gpio_set(gpio, INPUT, true),
            OraResult::Ok,
        );
        let (_, released) = emu.gpio_query(gpio);
        if released != before {
            errors.push(format!(
                "released X pin GPIO {gpio}: {released:?}, want {before:?}"
            ));
        }
    }

    if errors.is_empty() {
        println!("  X pin GPIOs {gpios:?}");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Both GPIOs of each of `board`'s dual-wired X pins where `ora_gpio_query`
/// reports one of them as serving.
///
/// Chosen from the board's wiring rather than from each GPIO's own answer, so
/// a GPIO the query wrongly reports free is still tested.
pub fn serving_x_pin_gpios(emu: &Emulator, board: Board) -> Vec<u8> {
    let serving = |gpio: u8| {
        let (result, info) = emu.gpio_query(gpio);
        result.is_ok()
            && matches!(
                info.gpio_use,
                ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ
                    | ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN
            )
    };
    [1, 2]
        .into_iter()
        .map(|x_pin| board.gpios_for_x_pin(x_pin))
        .filter(|gpios| gpios.len() > 1 && gpios.iter().any(|&gpio| serving(gpio)))
        .flatten()
        .copied()
        .collect()
}

/// `ora_is_pin_output` answers 0xFF for a pin this device does not have, and
/// never contradicts `ora_gpio_query` about one it does.
///
/// The test drives a free pin so the two calls have something to disagree
/// about: with the pin driving, an answer of 0 would be wrong.  Under
/// `TEST_BUILD` the call reads `GPIO_STATUS`, which this build has no model
/// of, so it returns 0xFF for every pin — hence "0xFF or the same answer" and
/// not a bare equality.  The out-of-range half is the same on a device and
/// here, and it is what a wrong constant fails.
pub fn test_is_pin_output(emu: &Emulator, board: Board) -> Result<(), String> {
    /// The value `ora_is_pin_output` returns for a pin the device does not
    /// have — and, in the test build, for every pin.
    const NOT_A_PIN: u8 = 0xFF;

    let max_gpios = max_gpios(board);
    let free = first_gpio_with_use(emu, max_gpios, ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE)?;
    let mut errors = Vec::new();

    for gpio in [max_gpios, 63, 255] {
        let got = emu.is_pin_output(gpio);
        if got != NOT_A_PIN {
            errors.push(format!("GPIO {gpio} is out of range: got {got}, want 0xFF"));
        }
    }

    let result = emu.gpio_set(free, ffi::ora_gpio_state_t_ORA_GPIO_STATE_HIGH, false);
    if !result.is_ok() {
        return Err(format!("could not drive free GPIO {free}: {result:?}"));
    }

    let mut answered = 0usize;
    for gpio in 0..max_gpios {
        let got = emu.is_pin_output(gpio);
        let (result, info) = emu.gpio_query(gpio);
        if !result.is_ok() {
            errors.push(format!("GPIO {gpio}: query failed: {result:?}"));
            continue;
        }
        if got == NOT_A_PIN {
            continue;
        }
        answered += 1;
        if u8::from(got != 0) != info.is_output {
            errors.push(format!(
                "GPIO {gpio}: is_pin_output says {got}, gpio_query says is_output={}",
                info.is_output
            ));
        }
    }

    let result = emu.gpio_set(free, ffi::ora_gpio_state_t_ORA_GPIO_STATE_INPUT, false);
    if !result.is_ok() {
        errors.push(format!("could not release free GPIO {free}: {result:?}"));
    }

    if errors.is_empty() {
        println!("  0xFF out of range, {answered}/{max_gpios} pins answered in this build");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
