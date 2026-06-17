// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration tests for the v2 (RP2350/Fire) builder path.
//!
//! Uses `onerom_metadata::DeviceMemoryView` to parse the serialised output,
//! reading fields at the absolute flash addresses derived from the schema.
//!
//! Board/chip combinations are selected from confirmed layout derivations in
//! `addr_layout::tests` and `rom_slot::tests`.

#[cfg(test)]
mod tests {
    use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
    use onerom_config::hw::Board;
    use onerom_config::mcu::{Family as McuFamily, Variant as McuVariant};
    use onerom_gen::{Builder, FileData};
    use onerom_metadata::{
        CURRENT_METADATA_VERSION, DeviceMemoryView, METADATA_BASE, METADATA_SIZE,
        ONEROM_METADATA_MAGIC,
    };

    // ROM images are placed immediately after the 16KB metadata region.
    const ROM_DATA_BASE: u32 = METADATA_BASE + METADATA_SIZE as u32;

    // rom_slot_type_t discriminants (schema: rom_slot_type_t)
    const SLOT_TYPE_SINGLE_ROM: u8 = 3;
    const SLOT_TYPE_BANKED_ROM: u8 = 5;

    // bit_modes_t values (schema: bit_modes_t)
    const BIT_MODE_8: u8 = 1;
    const BIT_MODE_16: u8 = 2;

    // onerom_alg_cs_t discriminants (schema: onerom_alg_cs_t)
    const ALG_CS_0: u8 = 0;
    const ALG_CS_2: u8 = 2;

    // onerom_alg_data_t discriminants (schema: onerom_alg_data_t)
    const ALG_DATA_1: u8 = 1;

    // Sentinel for nullable pointer fields. The v2 serializer writes 0 for
    // null; the parser (DeviceMemoryView) also accepts 0xFFFF_FFFF as null.
    const NULL_PTR: u32 = 0;

    // ========================================================================
    // Field byte offsets derived from the schema
    // ========================================================================

    // onerom_metadata_header_t (size = 256, placed at METADATA_BASE)
    const HDR_MAGIC: u32 = METADATA_BASE; // [u8; 16]
    const HDR_VERSION: u32 = METADATA_BASE + 16; // u32
    // hw_ptr at +20
    const HDR_FW_PTR: u32 = METADATA_BASE + 24; // u32 → onerom_firmware_config_t
    const HDR_SLOT_COUNT: u32 = METADATA_BASE + 28; // u8
    const HDR_BOOT_LOGGING: u32 = METADATA_BASE + 29; // u8
    const HDR_SWD_ENABLED: u32 = METADATA_BASE + 30; // u8
    const HDR_TURBO_BOOT: u32 = METADATA_BASE + 31; // u8
    const HDR_SLOTS_PTR: u32 = METADATA_BASE + 32; // u32 → [onerom_rom_slot_t]

    // onerom_firmware_config_t (size = 8)
    const FW_CFG_NAME: u32 = 0; // cstr_ptr u32 (nullable)
    const FW_CFG_SERIAL: u32 = 4; // cstr_ptr u32 (nullable)

    // onerom_rom_slot_t (size = 32, laid out as a contiguous array)
    const SLOT_DATA: u32 = 0; // opaque_ptr u32
    const SLOT_SIZE: u32 = 4; // u32
    const SLOT_ROMS: u32 = 8; // struct_ptr_array_ptr u32
    const SLOT_ROM_COUNT: u32 = 12; // u8
    const SLOT_TYPE: u32 = 13; // u8 (rom_slot_type_t)
    // reserved1 at +14 (2 bytes)
    const SLOT_ALG: u32 = 16; // struct_ptr u32 → onerom_alg_config_t
    const SLOT_FW_OVRD: u32 = 20; // struct_ptr u32, nullable

    // onerom_firmware_overrides_t (size = 32)
    // +0:  override_present [u8; 8]
    // +8:  ice_freq u16
    // +10: fire_freq u16
    // +12: fire_vreg u8
    // +13: pad1 [u8; 3]
    // +16: override_value [u8; 8]
    // +24: pad3 [u8; 8]
    const FW_OVRD_PRESENT: u32 = 0; // first byte of override_present
    const FW_OVRD_FIRE_FREQ: u32 = 10; // u16
    const FW_OVRD_FIRE_VREG: u32 = 12; // u8
    const FW_OVRD_VALUE: u32 = 16; // first byte of override_value

    // override_present[0] bit positions (from build_firmware_overrides)
    const OVR_FIRE_CPU_FREQ: u8 = 1 << 2;
    const OVR_FIRE_OVERCLOCK: u8 = 1 << 3;
    const OVR_FIRE_VREG: u8 = 1 << 4;
    const OVR_LED: u8 = 1 << 5;

    // override_value[0] bit positions
    const VAL_FIRE_OVERCLOCK: u8 = 1 << 1;
    const VAL_LED_ENABLED: u8 = 1 << 2;

    // onerom_alg_config_t (size = 32)
    const ALG_CS_PTR: u32 = 0; // tagged_fam_ptr u32 → onerom_alg_cs_config_t
    const ALG_DATA_PTR: u32 = 8; // tagged_fam_ptr u32 → onerom_alg_data_config_t
    const ALG_DMA_PTR: u32 = 12; // tagged_fam_ptr u32 → onerom_alg_dma_config_t
    const ALG_PULL_PTR: u32 = 16; // simple_fam_ptr u32, nullable
    const ALG_OVERRIDE_PTR: u32 = 20; // simple_fam_ptr u32, nullable

    // onerom_alg_override_config_t simple FAM binary layout:
    //   [param_len(1)] [params(param_len)]
    // Each param byte: (gpio_override_t << 6) | (gpio & 0x3F)
    // GpioOverInvert (value=1): top 2 bits = 0b01
    const OVERRIDE_PARAM_LEN: u32 = 0; // u8
    const OVERRIDE_TYPE_INVERT: u8 = 1; // GpioOverride::GpioOverInvert discriminant

    // onerom_alg_cs_config_t tagged FAM binary layout:
    //   [discriminant(1)] [param_len(1)] [clkdiv_int(2)] [clkdiv_frac(1)]
    //   [gpio_base(1)] [base_cs_pin(1)] [num_cs_pins(1)] [base_data_pin(1)]
    //   [num_data_pins(1)] [cs_active_delay(1)] [cs_inactive_delay(1)]
    //   [params(param_len)]
    // ALG_CS_0 params (param_len=4): serve_cs_low_0, byte_pin,
    //   first_rom_cs_base, first_rom_num_cs_pins
    // ALG_CS_2 params (param_len=3): base_qualifier_pin, num_qualifier_pins,
    //   qualifier_inactive_pattern
    const CS_DISCRIMINANT: u32 = 0; // u8 (onerom_alg_cs_t)
    const CS0_SERVE_CS_LOW_0: u32 = 12; // u8 — first ALG_CS_0 param byte
    const CS2_BASE_QUALIFIER_PIN: u32 = 12; // u8 — first ALG_CS_2 param byte
    const CS2_NUM_QUALIFIER_PINS: u32 = 13; // u8
    const CS2_QUALIFIER_INACTIVE_PATTERN: u32 = 14; // u8

    // onerom_alg_data_config_t tagged FAM binary layout:
    //   [discriminant(1)] [param_len(1)] [clkdiv_int(2)] [clkdiv_frac(1)]
    //   [gpio_base(1)] [base_data_pin(1)] [word_size(1)] [params(param_len)]
    // ALG_DATA_1 params (param_len=2): byte_pin, a_minus_1_pin
    const DATA_DISCRIMINANT: u32 = 0; // u8 (onerom_alg_data_t)
    const DATA_WORD_SIZE: u32 = 7; // u8 — same offset for both AlgData0/1

    // onerom_alg_dma_config_t tagged FAM binary layout:
    //   [discriminant(1)] [param_len(1)] [bit_mode(1)] [continuous(1)]
    const DMA_BIT_MODE: u32 = 2; // u8 (bit_modes_t)

    // onerom_alg_pull_config_t simple FAM binary layout:
    //   [param_len(1)] [params(param_len)]
    const PULL_PARAM_LEN: u32 = 0; // u8

    // onerom_rom_info_t (size = 16)
    const ROM_INFO_TYPE_PTR: u32 = 0; // cstr_ptr u32 → rom type string

    // ========================================================================
    // Helpers
    // ========================================================================

    fn v2_props(board: Board) -> FirmwareProperties {
        FirmwareProperties::new(
            FirmwareVersion::new(0, 7, 0, 0),
            board,
            McuVariant::RP2350,
            ServeAlg::Default,
            false,
        )
        .unwrap()
    }

    fn v2_builder(json: &str) -> Builder {
        Builder::from_json(FirmwareVersion::new(0, 7, 0, 0), McuFamily::Rp2350, json)
            .expect("from_json should succeed")
    }

    fn view(buf: &[u8]) -> DeviceMemoryView<'_> {
        DeviceMemoryView::new(buf, METADATA_BASE)
    }

    /// Absolute flash address of slot `n` in the contiguous slots array.
    fn slot_base(v: &DeviceMemoryView, n: u32) -> u32 {
        v.read_u32_le(HDR_SLOTS_PTR).unwrap() + n * 32
    }

    /// Absolute flash address of the alg_config pointed to by a slot.
    fn alg_base(v: &DeviceMemoryView, slot: u32) -> u32 {
        v.read_u32_le(slot + SLOT_ALG).unwrap()
    }

    // ========================================================================
    // v2 single sentinel: Fire24A / 2364
    // ========================================================================

    /// Baseline sentinel: confirms the v2 `Builder::build()` path produces a
    /// correctly structured `OneromMetadataHeader` for a single ROM slot.
    #[test]
    fn v2_single_fire24a_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 single sentinel",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        // Header
        let magic = v.read_bytes::<16>(HDR_MAGIC).unwrap();
        assert!(magic.starts_with(ONEROM_METADATA_MAGIC.as_bytes()));
        assert_eq!(
            v.read_u32_le(HDR_VERSION).unwrap(),
            CURRENT_METADATA_VERSION
        );
        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        // Slot 0 structural fields
        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // Slot size matches what was actually serialised into the ROM buffer
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);

        // ALG: CS0, active-low (serve_cs_low_0=0), BitMode8, no pull config,
        // no override config (Single sets never need X-pin inversion)
        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        // ROM info: chip type string
        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "2364");
    }

    // ========================================================================
    // v2 banked 2-chip: Fire24A / 2x 2364
    // ========================================================================

    /// End-to-end banked sentinel: confirms the full
    /// `Builder::build()` → `build_v2` → `build_rom_slot` → `build_rom_image`
    /// path produces the correct metadata for a 2-chip banked set.
    ///
    /// Key properties:
    /// - `slot_type` = `RomSlotTypeBankedRom`
    /// - `rom_count` = 2
    /// - `alg_cs` = `AlgCs0` with `serve_cs_low_0` = 0 (active-low)
    /// - `alg_dma` bit_mode = `BitMode8`
    /// - `gpio_pull_config` present with exactly 1 entry (X1 only, 2-chip)
    #[test]
    fn v2_banked_2chip_fire24a_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 banked 2-chip",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "bank0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank1.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 1,
            data: vec![0x55u8; 8192],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_BANKED_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);
        assert!(slot_size >= 1 << 16, "banked table must be at least 64KB");

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // 2-chip: X1 pull only (param_len == 1)
        let pull = v.read_u32_le(alg + ALG_PULL_PTR).unwrap();
        assert_ne!(pull, NULL_PTR, "banked set must have gpio_pull_config");
        assert_eq!(v.read_u8(pull + PULL_PARAM_LEN).unwrap(), 1);

        // Fire24A has x_jumper_pull=0: X1 needs GpioOverInvert so the address
        // PIO reads 1 when the jumper is fitted (bank 1 selected) and 0 when
        // not (bank 0 = default). 2-chip: X1 override only (param_len == 1).
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "banked on x_jumper_pull=0 board must have gpio_override_config"
        );
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 1);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        let rom1 = v.read_u32_le(roms_arr + 4).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "2364");
        assert_eq!(v.read_cstr(rom1 + ROM_INFO_TYPE_PTR).unwrap(), "2364");
    }

    // ========================================================================
    // v2 banked 3-chip: Fire24A / 3x 2364
    // ========================================================================

    /// 3-chip banked set. The key assertion is `PULL_PARAM_LEN == 2`
    /// (X1 and X2 both get pull entries), which is the end-to-end proof that
    /// the `num_chips >= 3` bug fix in `build_gpio_pull_config` flows all the
    /// way through serialisation. Bank index 3 (X1=1, X2=1) maps to
    /// `PAD_NO_CHIP_BYTE` in the ROM table; both jumpers still need pulls.
    #[test]
    fn v2_banked_3chip_fire24a_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 banked 3-chip",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "bank0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank1.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank2.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0x11u8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 1,
            data: vec![0x22u8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 2,
            data: vec![0x33u8; 8192],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_BANKED_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 3);
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);
        assert!(slot_size >= 1 << 16);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // 3-chip: X1 AND X2 pull entries (param_len == 2) — end-to-end proof
        // of the num_chips >= 3 bug fix in build_gpio_pull_config.
        let pull = v.read_u32_le(alg + ALG_PULL_PTR).unwrap();
        assert_ne!(pull, NULL_PTR, "banked set must have gpio_pull_config");
        assert_eq!(
            v.read_u8(pull + PULL_PARAM_LEN).unwrap(),
            2,
            "3-chip banked must have pull entries for both X1 and X2"
        );

        // Fire24A has x_jumper_pull=0: X1 and X2 both need GpioOverInvert.
        // 3-chip: param_len == 2.
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "banked on x_jumper_pull=0 board must have gpio_override_config"
        );
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            2,
            "3-chip banked must have override entries for both X1 and X2"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        for i in 0..3u32 {
            let rom_info = v.read_u32_le(roms_arr + i * 4).unwrap();
            assert_eq!(v.read_cstr(rom_info + ROM_INFO_TYPE_PTR).unwrap(), "2364");
        }
    }

    // ========================================================================
    // v2 banked 4-chip: Fire24A / 4x 2364
    // ========================================================================

    /// 4-chip banked set: all four banks occupied.
    /// Pull config still has 2 entries (X1 and X2); the distinction from 3-chip
    /// is that bank index 3 maps to chip 3 rather than PAD_NO_CHIP_BYTE.
    #[test]
    fn v2_banked_4chip_fire24a_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 banked 4-chip",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "bank0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank1.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank2.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "bank3.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0x11u8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 1,
            data: vec![0x22u8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 2,
            data: vec![0x33u8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 3,
            data: vec![0x44u8; 8192],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_BANKED_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 4);
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);
        assert!(slot_size >= 1 << 16);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // 4-chip: X1 and X2 pull entries (param_len == 2)
        let pull = v.read_u32_le(alg + ALG_PULL_PTR).unwrap();
        assert_ne!(pull, NULL_PTR, "banked set must have gpio_pull_config");
        assert_eq!(
            v.read_u8(pull + PULL_PARAM_LEN).unwrap(),
            2,
            "4-chip banked must have pull entries for both X1 and X2"
        );

        // Fire24A has x_jumper_pull=0: X1 and X2 both need GpioOverInvert.
        // 4-chip: param_len == 2 (same as 3-chip).
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "banked on x_jumper_pull=0 board must have gpio_override_config"
        );
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            2,
            "4-chip banked must have override entries for both X1 and X2"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        for i in 0..4u32 {
            let rom_info = v.read_u32_le(roms_arr + i * 4).unwrap();
            assert_eq!(v.read_cstr(rom_info + ROM_INFO_TYPE_PTR).unwrap(), "2364");
        }
    }

    // ========================================================================
    // v2 multiple slots: sequential data offsets
    // ========================================================================

    /// Two single-ROM slots. Verifies that each slot's `data` pointer is
    /// offset correctly: slot 0 at ROM_DATA_BASE, slot 1 at
    /// ROM_DATA_BASE + slot0_size. Also confirms the total ROM buffer length
    /// equals the sum of both slot sizes.
    #[test]
    fn v2_two_single_fire24a_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 two single slots",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{ "file": "a.bin", "type": "2364", "cs1": "active_low" }]
                },
                {
                    "type": "single",
                    "chips": [{ "file": "b.bin", "type": "2364", "cs1": "active_low" }]
                }
            ]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();
        b.add_file(FileData {
            id: 1,
            data: vec![0x55u8; 8192],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 2);

        let s0 = slot_base(&v, 0);
        let s1 = slot_base(&v, 1);

        let slot0_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        let slot1_size = v.read_u32_le(s1 + SLOT_SIZE).unwrap();

        // Slot 0 starts at ROM_DATA_BASE
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);

        // Slot 1 starts immediately after slot 0
        assert_eq!(
            v.read_u32_le(s1 + SLOT_DATA).unwrap(),
            ROM_DATA_BASE + slot0_size
        );
        assert_eq!(v.read_u8(s1 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s1 + SLOT_ROM_COUNT).unwrap(), 1);

        // Total ROM buffer == sum of both slot sizes
        assert_eq!(rom.len() as u32, slot0_size + slot1_size);
    }

    // ========================================================================
    // v2 header flags: boot_logging, swd_enabled, turbo_boot
    // ========================================================================

    /// Confirms that `boot_logging`, `swd_enabled`, and `turbo_boot` from the
    /// JSON config are serialised into the correct header byte positions.
    #[test]
    fn v2_header_flags() {
        let json = r#"{
            "version": 1,
            "description": "v2 header flags",
            "swd_enabled": true,
            "boot_logging": true,
            "turbo_boot": true,
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_BOOT_LOGGING).unwrap(), 1);
        assert_eq!(v.read_u8(HDR_SWD_ENABLED).unwrap(), 1);
        assert_eq!(v.read_u8(HDR_TURBO_BOOT).unwrap(), 1);
    }

    // ========================================================================
    // v2 firmware config: instance_name and serial_override
    // ========================================================================

    /// Confirms that `instance_name` and `serial_override` from the JSON config
    /// are serialised into the `onerom_firmware_config_t` struct and reachable
    /// via the `fw` pointer in the header.
    #[test]
    fn v2_firmware_config_name_serial() {
        let json = r#"{
            "version": 1,
            "description": "v2 firmware config",
            "instance_name": "My One ROM",
            "serial_override": "SN12345",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        let fw_ptr = v.read_u32_le(HDR_FW_PTR).unwrap();
        assert_ne!(fw_ptr, NULL_PTR, "fw pointer must not be null");

        assert_eq!(
            v.read_cstr_opt(fw_ptr + FW_CFG_NAME).unwrap(),
            Some("My One ROM".to_string())
        );
        assert_eq!(
            v.read_cstr_opt(fw_ptr + FW_CFG_SERIAL).unwrap(),
            Some("SN12345".to_string())
        );
    }

    // ========================================================================
    // v2 firmware overrides: Fire overrides
    // ========================================================================

    /// Confirms that per-slot Fire firmware overrides are serialised into the
    /// `onerom_firmware_overrides_t` struct reachable from SLOT_FW_OVRD, with
    /// the correct `override_present` and `override_value` bitfields and
    /// typed field values.
    #[test]
    fn v2_firmware_overrides_fire() {
        let json = r#"{
            "version": 1,
            "description": "v2 firmware overrides",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }],
                "firmware_overrides": {
                    "fire": {
                        "cpu_freq": "300MHz",
                        "overclock": true,
                        "vreg": "1.10V"
                    },
                    "led": { "enabled": true }
                }
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 8192],
        })
        .unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        let fw_ovrd = v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap();
        assert_ne!(fw_ovrd, NULL_PTR, "slot must have firmware_overrides");

        // override_present[0]: fire cpu_freq, fire overclock, fire vreg, led
        let expected_present = OVR_FIRE_CPU_FREQ | OVR_FIRE_OVERCLOCK | OVR_FIRE_VREG | OVR_LED;
        assert_eq!(
            v.read_u8(fw_ovrd + FW_OVRD_PRESENT).unwrap(),
            expected_present
        );

        // fire_freq = 300 MHz
        assert_eq!(v.read_u16_le(fw_ovrd + FW_OVRD_FIRE_FREQ).unwrap(), 300);

        // fire_vreg = FIRE_VREG_1_10V = 0x0B
        assert_eq!(v.read_u8(fw_ovrd + FW_OVRD_FIRE_VREG).unwrap(), 0x0B);

        // override_value[0]: fire overclock enabled, led enabled
        let expected_value = VAL_FIRE_OVERCLOCK | VAL_LED_ENABLED;
        assert_eq!(v.read_u8(fw_ovrd + FW_OVRD_VALUE).unwrap(), expected_value);
    }

    // ========================================================================
    // v2 AlgCs2: Fire28A / 23QL384
    // ========================================================================

    /// Single 23QL384 slot. The 23QL384 uses `ALG_CS_2` (enable +
    /// address-qualified): deselected when A14 and A15 are both high.
    /// Verifies the CS discriminant, qualifier pin count, and inactive pattern.
    #[test]
    fn v2_single_fire28a_23ql384() {
        let json = r#"{
            "version": 1,
            "description": "v2 AlgCs2 23QL384",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "23QL384", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        // 23QL384 = 48KB = 49152 bytes
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 49152],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);

        // AlgCs2: discriminant=2, A14+A15 as qualifiers (num=2, inactive=0b11)
        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_2);
        assert_eq!(v.read_u8(cs + CS2_NUM_QUALIFIER_PINS).unwrap(), 2);
        assert_eq!(
            v.read_u8(cs + CS2_QUALIFIER_INACTIVE_PATTERN).unwrap(),
            0b11
        );

        // base_qualifier_pin must be within the PIO window (< 32)
        assert!(
            v.read_u8(cs + CS2_BASE_QUALIFIER_PIN).unwrap() < 32,
            "base_qualifier_pin must be within PIO GPIO window"
        );

        // Single set: no pull config
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // DMA remains BitMode8
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "23QL384");
    }

    // ========================================================================
    // v2 BitMode16: Fire40A / 27C400
    // ========================================================================

    /// Single 27C400 slot (BitMode16). Verifies:
    /// - `alg_data` discriminant = `AlgData1` (byte-mode pin support)
    /// - `word_size` = 16
    /// - `alg_dma` bit_mode = `BitMode16`
    /// - slot size = 2^18 × 2 bytes = 524288 (18 word address lines, 2 bytes/word)
    #[test]
    fn v2_single_fire40a_27c400() {
        let json = r#"{
            "version": 1,
            "description": "v2 BitMode16 27C400",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C400" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        // 27C400 = 512KB byte-mode image = 524288 bytes
        b.add_file(FileData {
            id: 0,
            data: vec![0xAAu8; 524288],
        })
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire40A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);

        // 2^18 word entries × 2 bytes/word = 524288
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 18 << 1); // 2^18 * 2
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        // AlgData1: discriminant=1, word_size=16
        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_1);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 16);

        // DMA: BitMode16
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_16);

        // Single set: no pull config
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27C400");
    }
}
