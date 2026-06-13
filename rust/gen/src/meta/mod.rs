// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

mod metav1;

pub use metav1::*;

use crate::{FireServeMode, FirmwareConfig};

/// Serialize the 24-byte core of `onerom_firmware_overrides_t`.
///
/// Called by both the v1 writer (which appends 40 pad bytes to reach 64)
/// and the v2 writer (which appends 8 pad bytes to reach 32).
#[allow(clippy::collapsible_if)]
pub(crate) fn fw_overrides_core(config: &FirmwareConfig) -> [u8; 24] {
    let mut out = [PAD_METADATA_BYTE; 24];
    let mut offset = 0usize;

    // ── override_present bitfield (8 bytes) ─────────────────────────────
    // Bit positions in override_present[0]:
    // 0 = Ice MCU frequency
    // 1 = Ice overclock overridden
    // 2 = Fire MCU frequency
    // 3 = Fire overclock overridden
    // 4 = Fire VREQ overridden
    // 5 = Status LED overridden
    // 6 = SWD overridden
    // 7 = Fire serve mode overridden
    //
    // Bit positions in override_present[1]:
    // 0 = Fire ROM DMA preload overridden
    // 1 = Force 16 bit mode overridden
    let mut override_present = [0u8; 8];

    if let Some(ref ice_config) = config.ice {
        if ice_config.cpu_freq.is_some() {
            override_present[0] |= 1 << 0; // Ice frequency
        }
        if ice_config.overclock.is_some() {
            override_present[0] |= 1 << 1; // Ice overclock
        }
    }

    if let Some(ref fire_config) = config.fire {
        if fire_config.cpu_freq.is_some() {
            override_present[0] |= 1 << 2; // Fire frequency
        }
        if fire_config.overclock.is_some() {
            override_present[0] |= 1 << 3; // Fire overclock
        }
        if fire_config.vreg.is_some() {
            override_present[0] |= 1 << 4; // Fire VREQ
        }
        if fire_config.serve_mode.is_some() {
            override_present[0] |= 1 << 7; // Fire serve mode
        }
        // Always include ROM DMA preload
        override_present[1] |= 1 << 0;
        if fire_config.force_16_bit {
            override_present[1] |= 1 << 1; // Force 16-bit mode
        }
    }

    if config.led.is_some() {
        override_present[0] |= 1 << 5; // Status LED
    }

    if config.swd.is_some() {
        override_present[0] |= 1 << 6; // SWD
    }

    out[offset..offset + 8].copy_from_slice(&override_present);
    offset += 8;

    // ── frequencies (u16 LE, 2 bytes each) ──────────────────────────────
    let ice_freq = config
        .ice
        .as_ref()
        .and_then(|c| c.cpu_freq.as_ref())
        .map(|f| f.get())
        .unwrap_or(0xFFFF);
    out[offset..offset + 2].copy_from_slice(&ice_freq.to_le_bytes());
    offset += 2;

    let fire_freq = config
        .fire
        .as_ref()
        .and_then(|c| c.cpu_freq.as_ref())
        .map(|f| f.get())
        .unwrap_or(0xFFFF);
    out[offset..offset + 2].copy_from_slice(&fire_freq.to_le_bytes());
    offset += 2;

    // ── fire_vreq (1 byte) + pad1 (3 bytes) ─────────────────────────────
    out[offset] = config
        .fire
        .as_ref()
        .and_then(|c| c.vreg.as_ref())
        .map(|v| v.clone() as u8)
        .unwrap_or(0xFF);
    offset += 1;

    // pad1[3] — already PAD_METADATA_BYTE from initialisation above
    offset += 3;

    debug_assert_eq!(offset, 16);

    // ── override_value bitfield (8 bytes) ────────────────────────────────
    // Bit positions in override_value[0]:
    // 0 = Ice overclocking enabled
    // 1 = Fire overclocking enabled
    // 2 = Status LED enabled
    // 3 = SWD enabled
    // 4 = Fire serve mode (1 = PIO, 0 = CPU)
    // 5 = Fire ROM DMA preload enabled
    // 6 = Force 16-bit mode
    let mut override_value = [0u8; 8];

    if let Some(ref ice_config) = config.ice {
        if let Some(overclock) = ice_config.overclock {
            if overclock {
                override_value[0] |= 1 << 0;
            }
        }
    }

    if let Some(ref fire_config) = config.fire {
        if let Some(overclock) = fire_config.overclock {
            if overclock {
                override_value[0] |= 1 << 1;
            }
        }
        if let Some(ref serve_mode) = fire_config.serve_mode {
            if *serve_mode == FireServeMode::Pio {
                override_value[0] |= 1 << 4;
            }
        }
        if fire_config.rom_dma_preload {
            override_value[0] |= 1 << 5;
        }
        if fire_config.force_16_bit {
            override_value[0] |= 1 << 6;
        }
    }

    if let Some(ref led) = config.led {
        if led.enabled {
            override_value[0] |= 1 << 2;
        }
    }

    if let Some(ref swd) = config.swd {
        if swd.swd_enabled {
            override_value[0] |= 1 << 3;
        }
    }

    out[offset..offset + 8].copy_from_slice(&override_value);
    // offset would be 24 here — matches the array length

    out
}
