// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Build `OneromFirmwareConfig` (top-level name/serial overrides) and
//! `OneromFirmwareOverrides` (per-slot firmware overrides), from the raw
//! `Config`/`FirmwareConfig` types.
//!
//! Per the V2 metadata schema, several bits of `override_present`/
//! `override_value` are reserved and always left `0`:
//! - `override_present[0]` bits 0/1 (Ice cpu_freq/overclock): Ice overrides
//!   are rejected outright at config-validation time
//!   (`validate_config_for_v2`) - v2 is Fire-only.
//! - `override_present[0]` bit 7 / `override_value[0]` bit 4 (Fire serve
//!   mode): v2 is PIO-only, so this is meaningless and not encoded.
//! - `override_present[1]` bits 0/1 (`rom_dma_preload`/`force_16_bit`): not
//!   supported on V2.
//!
//! `ice_freq` is always `0` for the same reason as the Ice bits above.

use onerom_metadata::{
    FireFreq, FireVreg as OneromFireVreg, OneromFirmwareConfig, OneromFirmwareOverrides,
};

use crate::{Config, FirmwareConfig};

pub fn build_firmware_config(config: &Config) -> OneromFirmwareConfig {
    OneromFirmwareConfig {
        name: config.instance_name.clone(),
        serial_number: config.serial_override.clone(),
    }
}

// Bit positions within override_present[0]/override_value[0].
const PRESENT_FIRE_CPU_FREQ: u8 = 1 << 2;
const PRESENT_FIRE_OVERCLOCK: u8 = 1 << 3;
const PRESENT_FIRE_VREG: u8 = 1 << 4;
const PRESENT_LED: u8 = 1 << 5;
const PRESENT_SWD: u8 = 1 << 6;

const VALUE_FIRE_OVERCLOCK: u8 = 1 << 1;
const VALUE_LED_ENABLED: u8 = 1 << 2;
const VALUE_SWD_ENABLED: u8 = 1 << 3;

/// Build the per-slot `OneromFirmwareOverrides` from `FirmwareConfig`.
pub fn build_firmware_overrides(overrides: &FirmwareConfig) -> OneromFirmwareOverrides {
    let mut override_present = [0u8; 8];
    let mut override_value = [0u8; 8];

    let mut fire_freq: FireFreq = 0; // FIRE_FREQ_NONE
    let mut fire_vreg = OneromFireVreg::FireVregNone;

    if let Some(fire) = &overrides.fire {
        if let Some(cpu_freq) = fire.cpu_freq {
            override_present[0] |= PRESENT_FIRE_CPU_FREQ;
            fire_freq = cpu_freq.get();
        }

        if let Some(overclock) = fire.overclock {
            override_present[0] |= PRESENT_FIRE_OVERCLOCK;
            if overclock {
                override_value[0] |= VALUE_FIRE_OVERCLOCK;
            }
        }

        if let Some(vreg) = &fire.vreg {
            override_present[0] |= PRESENT_FIRE_VREG;
            fire_vreg = OneromFireVreg::try_from(vreg.clone() as u8)
                .expect("onerom_config::FireVreg values are a subset of onerom_metadata::FireVreg");
        }

        // fire.serve_mode, fire.rom_dma_preload, fire.force_16_bit: not
        // encoded - reserved/not supported on V2 (see module doc comment).
    }

    if let Some(led) = &overrides.led {
        override_present[0] |= PRESENT_LED;
        if led.enabled {
            override_value[0] |= VALUE_LED_ENABLED;
        }
    }

    if let Some(swd) = &overrides.swd {
        override_present[0] |= PRESENT_SWD;
        if swd.swd_enabled {
            override_value[0] |= VALUE_SWD_ENABLED;
        }
    }

    OneromFirmwareOverrides {
        override_present,
        ice_freq: 0, // Ice rejected by validate_config_for_v2 - always 0.
        fire_freq,
        fire_vreg,
        override_value,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    use crate::{DebugConfig, FireConfig, FireCpuFreq, FireVreg, LedConfig};

    #[test]
    fn firmware_config_name_and_serial() {
        let config = Config {
            version: 1,
            name: None,
            description: "test".to_string(),
            detail: None,
            chip_sets: vec![],
            notes: None,
            categories: None,
            instance_name: Some("My One ROM".to_string()),
            serial_override: Some("ABC123".to_string()),
            boot_logging: false,
            swd_enabled: true,
            turbo_boot: false,
        };

        let fw = build_firmware_config(&config);
        assert_eq!(fw.name, Some("My One ROM".to_string()));
        assert_eq!(fw.serial_number, Some("ABC123".to_string()));
    }

    #[test]
    fn firmware_overrides_empty() {
        let overrides = FirmwareConfig {
            ice: None,
            fire: None,
            led: None,
            swd: None,
            serve_alg_params: None,
        };

        let result = build_firmware_overrides(&overrides);

        assert_eq!(result.override_present, [0u8; 8]);
        assert_eq!(result.ice_freq, 0);
        assert_eq!(result.fire_freq, 0);
        assert_eq!(result.fire_vreg, OneromFireVreg::FireVregNone);
        assert_eq!(result.override_value, [0u8; 8]);
    }

    #[test]
    fn firmware_overrides_fire_led_swd() {
        let overrides = FirmwareConfig {
            ice: None,
            fire: Some(FireConfig {
                cpu_freq: Some(FireCpuFreq::mhz(200).unwrap()),
                overclock: Some(true),
                vreg: Some(FireVreg::V1_10),
                serve_mode: None,
                rom_dma_preload: true,
                force_16_bit: false,
            }),
            led: Some(LedConfig { enabled: true }),
            swd: Some(DebugConfig { swd_enabled: true }),
            serve_alg_params: None,
        };

        let result = build_firmware_overrides(&overrides);

        assert_eq!(
            result.override_present[0],
            PRESENT_FIRE_CPU_FREQ
                | PRESENT_FIRE_OVERCLOCK
                | PRESENT_FIRE_VREG
                | PRESENT_LED
                | PRESENT_SWD
        );
        assert_eq!(result.override_present[1..], [0u8; 7]);
        assert_eq!(result.ice_freq, 0);
        assert_eq!(result.fire_freq, 200);
        assert_eq!(result.fire_vreg, OneromFireVreg::FireVreg110v);
        assert_eq!(
            result.override_value[0],
            VALUE_FIRE_OVERCLOCK | VALUE_LED_ENABLED | VALUE_SWD_ENABLED
        );
        assert_eq!(result.override_value[1..], [0u8; 7]);
    }
}
