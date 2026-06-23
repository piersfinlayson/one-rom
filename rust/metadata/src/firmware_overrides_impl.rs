// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Handwritten accessor methods for [`OneromFirmwareOverrides`].
//!
//! The struct itself is auto-generated from `firmware/metadata_schema.toml`
//! and must not be edited. The bit layout encoded here is the authoritative
//! definition from that schema:
//!
//! `override_present[0]`:
//!   - bit 2: Fire MCU frequency override present
//!   - bit 3: Fire overclock override present
//!   - bit 4: Fire VREG override present
//!   - bit 5: Status LED override present
//!   - bit 6: SWD override present
//!
//! `override_value[0]`:
//!   - bit 1: Fire overclock enabled
//!   - bit 2: Status LED enabled
//!   - bit 3: SWD enabled

use crate::{FireFreq, FireVreg, OneromFirmwareOverrides};

impl OneromFirmwareOverrides {
    // override_present[0] bit positions
    const PRESENT_FIRE_FREQ: u8 = 1 << 2;
    const PRESENT_FIRE_OVERCLOCK: u8 = 1 << 3;
    const PRESENT_FIRE_VREG: u8 = 1 << 4;
    const PRESENT_LED: u8 = 1 << 5;
    const PRESENT_SWD: u8 = 1 << 6;

    // override_value[0] bit positions
    const VALUE_OVERCLOCK: u8 = 1 << 1;
    const VALUE_LED_ENABLED: u8 = 1 << 2;
    const VALUE_SWD_ENABLED: u8 = 1 << 3;

    /// Returns `true` if any override bit is set across the entire
    /// `override_present` array.
    pub fn any_present(&self) -> bool {
        self.override_present.iter().any(|&b| b != 0)
    }

    /// CPU frequency override in MHz, or `None` if not overridden.
    pub fn cpu_freq(&self) -> Option<FireFreq> {
        (self.override_present[0] & Self::PRESENT_FIRE_FREQ != 0).then_some(self.fire_freq)
    }

    /// VREG voltage override, or `None` if not overridden.
    pub fn vreg(&self) -> Option<FireVreg> {
        (self.override_present[0] & Self::PRESENT_FIRE_VREG != 0).then_some(self.fire_vreg)
    }

    /// Status LED enabled override, or `None` if not overridden.
    pub fn led_enabled(&self) -> Option<bool> {
        (self.override_present[0] & Self::PRESENT_LED != 0)
            .then(|| self.override_value[0] & Self::VALUE_LED_ENABLED != 0)
    }

    /// SWD enabled override, or `None` if not overridden.
    pub fn swd_enabled(&self) -> Option<bool> {
        (self.override_present[0] & Self::PRESENT_SWD != 0)
            .then(|| self.override_value[0] & Self::VALUE_SWD_ENABLED != 0)
    }

    /// Overclock enabled override, or `None` if not overridden.
    pub fn overclock_enabled(&self) -> Option<bool> {
        (self.override_present[0] & Self::PRESENT_FIRE_OVERCLOCK != 0)
            .then(|| self.override_value[0] & Self::VALUE_OVERCLOCK != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides_with(present: u8, value: u8) -> OneromFirmwareOverrides {
        OneromFirmwareOverrides {
            override_present: [present, 0, 0, 0, 0, 0, 0, 0],
            override_value: [value, 0, 0, 0, 0, 0, 0, 0],
            ice_freq: 0,
            fire_freq: 0,
            fire_vreg: FireVreg::FireVregStock,
        }
    }

    #[test]
    fn any_present_empty() {
        assert!(!overrides_with(0, 0).any_present());
    }

    #[test]
    fn any_present_nonempty() {
        assert!(overrides_with(1 << 5, 0).any_present());
    }

    #[test]
    fn cpu_freq_absent() {
        assert_eq!(overrides_with(0, 0).cpu_freq(), None);
    }

    #[test]
    fn cpu_freq_present() {
        let mut o = overrides_with(1 << 2, 0);
        o.fire_freq = 200;
        assert_eq!(o.cpu_freq(), Some(200));
    }

    #[test]
    fn vreg_absent() {
        assert_eq!(overrides_with(0, 0).vreg(), None);
    }

    #[test]
    fn vreg_present() {
        let mut o = overrides_with(1 << 4, 0);
        o.fire_vreg = FireVreg::FireVreg110v;
        assert_eq!(o.vreg(), Some(FireVreg::FireVreg110v));
    }

    #[test]
    fn led_absent() {
        assert_eq!(overrides_with(0, 0).led_enabled(), None);
    }

    #[test]
    fn led_present_on() {
        assert_eq!(overrides_with(1 << 5, 1 << 2).led_enabled(), Some(true));
    }

    #[test]
    fn led_present_off() {
        assert_eq!(overrides_with(1 << 5, 0).led_enabled(), Some(false));
    }

    #[test]
    fn swd_absent() {
        assert_eq!(overrides_with(0, 0).swd_enabled(), None);
    }

    #[test]
    fn swd_present_on() {
        assert_eq!(overrides_with(1 << 6, 1 << 3).swd_enabled(), Some(true));
    }

    #[test]
    fn swd_present_off() {
        assert_eq!(overrides_with(1 << 6, 0).swd_enabled(), Some(false));
    }

    #[test]
    fn overclock_absent() {
        assert_eq!(overrides_with(0, 0).overclock_enabled(), None);
    }

    #[test]
    fn overclock_present_on() {
        assert_eq!(
            overrides_with(1 << 3, 1 << 1).overclock_enabled(),
            Some(true)
        );
    }

    #[test]
    fn overclock_present_off() {
        assert_eq!(overrides_with(1 << 3, 0).overclock_enabled(), Some(false));
    }
}
