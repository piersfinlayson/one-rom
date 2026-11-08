// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Checks and metadata encoding for `addr_mon_min_cs_act_ns`.

use alloc::string::{String, ToString};
use onerom_config::fw::FirmwareVersion;
use onerom_metadata::{ADDR_MON_MIN_CS_ACT_NOT_SET, ADDR_MON_MIN_CS_ACT_UNIT_NS};

use crate::plugin::{file_stem, official_plugin};
use crate::{
    ChipConfig, Config, Error, FirmwareConfig, MAX_ADDR_MON_MIN_CS_ACT_NS,
    MIN_ADDR_MON_MIN_CS_ACT_VERSION, Result,
};

/// A chip set's own value.
fn set_value(overrides: Option<&FirmwareConfig>) -> Option<u16> {
    overrides?.fire.as_ref()?.addr_mon_min_cs_act_ns
}

fn plugin_name(chip: &ChipConfig) -> String {
    if let Some(name) = &chip.plugin {
        return name.clone();
    }
    let source = chip.filename();
    official_plugin(&source)
        .map_or_else(|| file_stem(&source), |official| official.name)
        .to_string()
}

pub(crate) fn check_config(version: FirmwareVersion, config: &Config) -> Result<()> {
    if let Some(ns) = config.addr_mon_min_cs_act_ns
        && ns > MAX_ADDR_MON_MIN_CS_ACT_NS
    {
        return Err(Error::AddrMonMinCsActOutOfRange { slot: None, ns });
    }

    let mut used = config.addr_mon_min_cs_act_ns.is_some();
    let mut slot = 0;
    for chip_set in &config.chip_sets {
        let plugin = chip_set
            .chips
            .iter()
            .find(|chip| chip.chip_type.resolved().is_plugin());
        if let Some(ns) = set_value(chip_set.firmware_overrides.as_ref()) {
            if let Some(plugin) = plugin {
                return Err(Error::AddrMonMinCsActOnPlugin {
                    plugin: plugin_name(plugin),
                });
            }
            if ns > MAX_ADDR_MON_MIN_CS_ACT_NS {
                return Err(Error::AddrMonMinCsActOutOfRange {
                    slot: Some(slot),
                    ns,
                });
            }
            used = true;
        }
        if plugin.is_none() {
            slot += 1;
        }
    }

    if used && version < MIN_ADDR_MON_MIN_CS_ACT_VERSION {
        return Err(Error::FirmwareTooOld {
            feat: "addr_mon_min_cs_act_ns",
            version,
            minimum: MIN_ADDR_MON_MIN_CS_ACT_VERSION,
        });
    }
    Ok(())
}

pub(crate) fn metadata_value(ns: Option<u16>) -> u8 {
    match ns {
        None => ADDR_MON_MIN_CS_ACT_NOT_SET,
        Some(ns) => {
            debug_assert!(ns <= MAX_ADDR_MON_MIN_CS_ACT_NS, "check_config checks it");
            ns.div_ceil(u16::from(ADDR_MON_MIN_CS_ACT_UNIT_NS)) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metadata_value_is_rounded_up_to_whole_units() {
        for (ns, value) in [
            (Some(0), 0),
            (Some(1), 1),
            (Some(9), 1),
            (Some(10), 1),
            (Some(11), 2),
            (Some(140), 14),
            (Some(2531), 254),
            (Some(MAX_ADDR_MON_MIN_CS_ACT_NS), 254),
            (None, ADDR_MON_MIN_CS_ACT_NOT_SET),
        ] {
            assert_eq!(metadata_value(ns), value, "{ns:?}");
        }
    }
}
