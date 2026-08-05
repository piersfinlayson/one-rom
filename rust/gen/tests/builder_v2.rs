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
    use onerom_gen::{Builder, ConfigOverrides, ConfigWarning, Error as GenError, FileData};
    use onerom_metadata::{
        CURRENT_METADATA_VERSION, DeviceMemoryView, METADATA_BASE, METADATA_SIZE,
        ONEROM_METADATA_MAGIC,
    };

    // ROM images are placed immediately after the 16KB metadata region.
    const ROM_DATA_BASE: u32 = METADATA_BASE + METADATA_SIZE as u32;

    // rom_slot_type_t discriminants (schema: rom_slot_type_t)
    const SLOT_TYPE_SINGLE_ROM: u8 = 3;
    const SLOT_TYPE_BANKED_ROM: u8 = 5;
    const SLOT_TYPE_MULTI_ROM: u8 = 4;

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

    // Common header (CS_CONFIG_LEN = 12 bytes):
    //   [discriminant(1)][param_len(1)][clkdiv_int(2)][clkdiv_frac(1)][gpio_base(1)]
    //   [base_cs_pin(1)][num_cs_pins(1)][base_data_pin(1)][num_data_pins(1)]
    //   [cs_active_delay(1)][cs_inactive_delay(1)]
    const CS_BASE_CS_PIN: u32 = 6; // u8 — offset of first CS GPIO from gpio_base
    const CS_NUM_CS_PINS: u32 = 7; // u8 — width of the CS-detect range

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
    const OVERRIDE_TYPE_LOW: u8 = 2; // GpioOverride::GpioOverLow discriminant

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
    const CS_CONFIG_LEN: u32 = 12; // Lengrh of onerom_alg_cs_config_t
    const CS0_SERVE_CS_LOW_0: u32 = CS_CONFIG_LEN; // u8 — first ALG_CS_0 param byte
    const CS0_FIRST_ROM_CS_BASE: u32 = CS_CONFIG_LEN + 2; // u8 — chip0's CS pin offset within the CS range
    const CS0_FIRST_ROM_NUM_CS_PINS: u32 = CS_CONFIG_LEN + 3; // u8 — always 1 for Multi (one pin per chip)
    const CS2_BASE_QUALIFIER_PIN: u32 = CS_CONFIG_LEN; // u8 — first ALG_CS_2 param byte
    const CS2_NUM_QUALIFIER_PINS: u32 = CS_CONFIG_LEN + 1; // u8
    const CS2_QUALIFIER_INACTIVE_PATTERN: u32 = CS_CONFIG_LEN + 2; // u8
    const CS0_BYTE_PIN: u32 = CS_CONFIG_LEN + 1; // u8 — 2nd ALG_CS_0 param byte
    const ALG_DATA_0: u8 = 0; // onerom_alg_data_t discriminant

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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

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

        // ALG: CS0, active-low (serve_cs_low_0=0), BitMode8, no pull config.
        // GPIO8 (X2) and GPIO9 (X1) are unused on a Single set but sit inside
        // the [0,16) address window, so both are forced low.
        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "unused X-pin GPIOs must be forced low");
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 2);
        assert_eq!(v.read_u8(ov + 1).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 8);
        assert_eq!(v.read_u8(ov + 2).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 9);

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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0x55u8; 8192])).unwrap();

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
        // not (bank 0 = default). 2-chip: X1 inverted; X2 (GPIO8) is unused on
        // a 2-chip set and forced low (param_len == 2).
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "banked on x_jumper_pull=0 board must have gpio_override_config"
        );
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 2);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 8);

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
        b.add_file(FileData::new(0, vec![0x11u8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0x22u8; 8192])).unwrap();
        b.add_file(FileData::new(2, vec![0x33u8; 8192])).unwrap();

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
        b.add_file(FileData::new(0, vec![0x11u8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0x22u8; 8192])).unwrap();
        b.add_file(FileData::new(2, vec![0x33u8; 8192])).unwrap();
        b.add_file(FileData::new(3, vec![0x44u8; 8192])).unwrap();

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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0x55u8; 8192])).unwrap();

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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_BOOT_LOGGING).unwrap(), 1);
        assert_eq!(v.read_u8(HDR_SWD_ENABLED).unwrap(), 1);
        assert_eq!(v.read_u8(HDR_TURBO_BOOT).unwrap(), 1);
    }

    /// Turbo boot with more than one non-plugin slot is refused by default,
    /// and accepted - reported as a warning - when the caller overrides it.
    ///
    /// Asserts the accepted build still sets the turbo boot header flag, so a
    /// build that quietly dropped turbo boot to make the config legal would
    /// not pass.
    #[test]
    fn v2_turbo_boot_multi_slot() {
        let json = r#"{
            "version": 1,
            "description": "turbo boot, two slots",
            "turbo_boot": true,
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "one.bin", "type": "2364", "cs1": "active_low" }]
            }, {
                "type": "single",
                "chips": [{ "file": "two.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let version = FirmwareVersion::new(0, 7, 0, 0);

        let err = Builder::from_json(version, McuFamily::Rp2350, json)
            .expect_err("turbo boot with two slots must be refused by default");
        assert!(
            matches!(err, GenError::TurboBootMultiSlot { slots: 2 }),
            "unexpected error: {err}"
        );

        let overrides = ConfigOverrides::default().allow_turbo_boot_multi_slot(true);
        let (mut b, warnings) =
            Builder::from_json_with_overrides(version, McuFamily::Rp2350, json, &overrides)
                .expect("the override must allow the config to build");
        assert!(
            matches!(
                warnings.as_slice(),
                [ConfigWarning::TurboBootMultiSlot { slots: 2 }]
            ),
            "expected one turbo boot warning, got {warnings:?}"
        );

        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 8192])).unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        assert_eq!(view(&meta).read_u8(HDR_TURBO_BOOT).unwrap(), 1);
    }

    /// A single non-plugin slot is the ordinary turbo boot case, and needs no
    /// override.
    #[test]
    fn v2_turbo_boot_single_slot_no_warning() {
        let json = r#"{
            "version": 1,
            "description": "turbo boot, one slot",
            "turbo_boot": true,
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let overrides = ConfigOverrides::default().allow_turbo_boot_multi_slot(true);
        let (_b, warnings) = Builder::from_json_with_overrides(
            FirmwareVersion::new(0, 7, 0, 0),
            McuFamily::Rp2350,
            json,
            &overrides,
        )
        .expect("from_json_with_overrides should succeed");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    /// Boot logging with SWD disabled is a supported combination: SWD stays up
    /// for the whole of boot, so the boot log is emitted in full, and is only
    /// shut off when serving starts.  This pairing used to be rejected by
    /// validate_config_v2.
    ///
    /// Asserts the build succeeds *and* that both flags reach the header with
    /// the values asked for - a build that quietly forced swd_enabled back to
    /// 1 would otherwise pass.
    #[test]
    fn v2_boot_logging_with_swd_disabled() {
        let json = r#"{
            "version": 1,
            "description": "boot logging, SWD off",
            "swd_enabled": false,
            "boot_logging": true,
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

        let (meta, _rom) = b
            .build(v2_props(Board::Fire24A))
            .expect("boot_logging with swd_enabled = false must build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_BOOT_LOGGING).unwrap(), 1);
        assert_eq!(v.read_u8(HDR_SWD_ENABLED).unwrap(), 0);
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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

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
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

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
        b.add_file(FileData::new(0, vec![0xAAu8; 49152])).unwrap();

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
        b.add_file(FileData::new(0, vec![0xAAu8; 524288])).unwrap();

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

    // ========================================================================
    // v2 multi 2-chip: Fire24E / 2x 2364
    // ========================================================================

    /// 2-chip Multi set on Fire24E (CS1 as per-chip select — 2364 has only
    /// one control line so no ignore needed on chips[1]).
    ///
    /// Key assertions:
    /// - slot_type = SLOT_TYPE_MULTI_ROM
    /// - AlgCs0 with serve_cs_low_0 = 1 (active-high, Multi convention)
    /// - first_rom_cs_base = 10 (CS1 at GPIO 10 - gpio_base 0)
    /// - 2 GpioOverInvert entries: CS1 and X1 (both active-low hardware,
    ///   inverted to active-high for the Multi CS PIO)
    /// - No gpio_pull_config (Multi X pins are CS inputs, not jumpers)
    #[test]
    fn v2_multi_2chip_fire24e_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi 2-chip 2364",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "chip1.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 8192])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24E)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_DATA).unwrap(), ROM_DATA_BASE);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // Table: 2^15 entries × 1 byte = 32768
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1 << 15);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        // Multi convention: serve_cs_low_0 = 1 (active-high after GpioOverInvert)
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);
        // chip0's per-chip select: CS1 at GPIO 10, gpio_base=0 → offset 10
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(), 10);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // No pull config: Multi X pins are driven CS selects, not jumpers
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // 2 GpioOverInvert entries: CS1 and X1 (active-low → active-high)
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "Multi set must have gpio_override_config");
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            2,
            "2-chip Multi must have 2 override entries (CS1 and X1)"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "2364"
        );
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr + 4).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "2364"
        );
    }

    // ========================================================================
    // v2 multi 3-chip: Fire24E / 3x 2364
    // ========================================================================

    /// 3-chip Multi set on Fire24E. Extends the 2-chip test with X2, giving
    /// 3 GpioOverInvert entries (CS1, X1, X2).
    #[test]
    fn v2_multi_3chip_fire24e_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi 3-chip 2364",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "chip1.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "chip2.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 8192])).unwrap();
        b.add_file(FileData::new(2, vec![0xCCu8; 8192])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24E)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 3);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1 << 16);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(), 10);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // 3 GpioOverInvert entries: CS1, X1, X2
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR);
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            3,
            "3-chip Multi must have 3 override entries (CS1, X1, X2)"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 3).unwrap() >> 6, OVERRIDE_TYPE_INVERT);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        for i in 0..3u32 {
            assert_eq!(
                v.read_cstr(v.read_u32_le(roms_arr + i * 4).unwrap() + ROM_INFO_TYPE_PTR)
                    .unwrap(),
                "2364"
            );
        }
    }

    // ========================================================================
    // v2 multi 2-chip CE-primary: Fire28C / 2x 27128, OE commoned
    // ========================================================================

    /// 2-chip Multi set on Fire28C, 27128 with CE as per-chip select (OE
    /// commoned). chips[1] has `"oe": "ignore"`.
    ///
    /// On Fire28C: CE=GPIO10, OE=GPIO11 (commoned, fills the span gap),
    /// X1=GPIO9. The CS range is {9,10,11} — contiguous.
    ///
    /// Key assertions:
    /// - serve_cs_low_0 = 1
    /// - first_rom_cs_base = 10 (CE at GPIO 10, gpio_base=0)
    /// - 2 GpioOverInvert entries: CE and X1
    /// - No pull config
    #[test]
    fn v2_multi_2chip_fire28c_27128_ce_primary() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi 2-chip 27128 CE-primary",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "27128" },
                    { "file": "chip1.bin", "type": "27128", "oe": "ignore" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 16384])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 16384])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);
        // CE at GPIO 10, gpio_base=0 → first_rom_cs_base = 10
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(), 10);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // 2 GpioOverInvert entries: CE and X1
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "Multi set must have gpio_override_config");
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 5);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // CE @ GPIO10
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // X1 @ GPIO9
        assert_eq!(v.read_u8(ov + 3).unwrap(), (OVERRIDE_TYPE_INVERT << 6) | 11); // OE commoned @ GPIO11
        assert_eq!(v.read_u8(ov + 4).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 12);
        assert_eq!(v.read_u8(ov + 5).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 18);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr + 4).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
    }

    // ========================================================================
    // v2 multi 2-chip OE-primary: Fire28C / 2x 27128, CE commoned
    // ========================================================================

    /// 2-chip Multi set on Fire28C, 27128 with OE as per-chip select (CE
    /// commoned). chips[1] has `"ce": "ignore"`.
    ///
    /// On Fire28C: OE=GPIO11, CE=GPIO10 (commoned), X1=GPIO9. The commoned
    /// CE fills the gap between OE and X1, making {9,10,11} contiguous.
    ///
    /// Key assertions:
    /// - serve_cs_low_0 = 1
    /// - first_rom_cs_base = 11 (OE at GPIO 11, gpio_base=0)
    /// - 2 GpioOverInvert entries: OE and X1
    /// - No pull config
    #[test]
    fn v2_multi_2chip_fire28c_27128_oe_primary() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi 2-chip 27128 OE-primary",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "27128" },
                    { "file": "chip1.bin", "type": "27128", "ce": "ignore" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 16384])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 16384])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);
        // OE at GPIO 11, gpio_base=0 → first_rom_cs_base = 11
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(), 11);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // 2 GpioOverInvert entries: OE and X1
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "Multi set must have gpio_override_config");
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 5);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // OE @ GPIO11
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // X1 @ GPIO9
        assert_eq!(v.read_u8(ov + 3).unwrap(), (OVERRIDE_TYPE_INVERT << 6) | 10); // CE commoned @ GPIO10
        assert_eq!(v.read_u8(ov + 4).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 12);
        assert_eq!(v.read_u8(ov + 5).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 18);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr + 4).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
    }

    // ========================================================================
    // v2 40-pin: byte_pin populated (AlgData1, default force_16_bit=false)
    // ========================================================================

    /// Single 27C400, Fire40A, default serving (AlgData1, force_16_bit=false).
    ///
    /// Extends the existing `v2_single_fire40a_27c400` scenario with the
    /// key assertion the existing test was missing: `CS0_BYTE_PIN` in the
    /// serialised AlgCs0 must be set to a real GPIO (not GPIO_NONE = 0xFF)
    /// because AlgData1 supplies its `byte_pin` to AlgCs0.
    ///
    /// Also asserts `serve_cs_low_0 = 0` (single set, active-low convention)
    /// and `word_size = 16`.
    #[test]
    fn v2_single_fire40a_27c400_byte_pin_populated() {
        let json = r#"{
            "version": 1,
            "description": "v2 40-pin byte_pin populated",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C400" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 524288]), /* 27C400 = 512KB byte-mode image */
        )
        .unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire40A)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        let alg = alg_base(&v, s0);

        // AlgCs0: serve_cs_low_0 = 0 (single set, active-low)
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);

        // byte_pin must be a real GPIO (not 0xFF / GPIO_NONE): AlgData1
        // supplies its /BYTE pin to the CS algorithm so the CS PIO knows
        // which GPIO signals byte-mode operation.
        let byte_pin = v.read_u8(cs + CS0_BYTE_PIN).unwrap();
        assert_ne!(
            byte_pin, 0xFF,
            "byte_pin must be a real GPIO (not GPIO_NONE) when AlgData1 is used"
        );

        // AlgData: discriminant=1 (AlgData1), word_size=16
        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_1);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 16);

        // DMA: BitMode16
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_16);
    }

    // ========================================================================
    // v2 40-pin: byte_pin absent (AlgData0, force_16_bit=true)
    // ========================================================================

    /// Single 27C400, Fire40A, `force_16_bit = true` (AlgData0, word_size=16).
    ///
    /// With `force_16_bit`, the chip is served in its native 16-bit/word mode
    /// without reading /BYTE. AlgData0 is used (not AlgData1), so AlgCs0's
    /// `byte_pin` must be GPIO_NONE (0xFF): no /BYTE is involved.
    ///
    /// The ROM table size is the same as the default AlgData1 case: 2^18
    /// entries × 2 bytes = 524288 bytes — `force_16_bit` only changes how
    /// the table is served, not its layout.
    #[test]
    fn v2_single_fire40a_27c400_force_16bit_byte_pin_none() {
        let json = r#"{
            "version": 1,
            "description": "v2 40-pin force_16_bit",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C400" }],
                "firmware_overrides": {
                    "fire": { "force_16_bit": true }
                }
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 524288])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire40A)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);

        // Table size unchanged: force_16_bit doesn't affect layout.
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 18 << 1); // 2^18 * 2 = 524288
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        // AlgData0 (not AlgData1) with word_size=16: /BYTE is ignored.
        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(
            v.read_u8(data + DATA_DISCRIMINANT).unwrap(),
            ALG_DATA_0,
            "force_16_bit must produce AlgData0, not AlgData1"
        );
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 16);

        // DMA: still BitMode16
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_16);

        // AlgCs0: byte_pin must be GPIO_NONE (0xFF) — AlgData0 has no /BYTE.
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(
            v.read_u8(cs + CS0_BYTE_PIN).unwrap(),
            0xFF,
            "byte_pin must be GPIO_NONE when force_16_bit=true (AlgData0, no /BYTE)"
        );
    }

    // ========================================================================
    // v2 40-pin: 27C200 (256KB, BitMode16)
    // ========================================================================

    /// Single 27C200, Fire40A, BitMode16 (AlgData1).
    ///
    /// 27C200 is a 256KB 16-bit EPROM: 2^17 word addresses (A0-A16),
    /// plus A-1 shared with D15. The ROM table has 2^17 entries × 2
    /// bytes/word = 2^18 = 262144 bytes. Verifies that a second 16-bit
    /// chip type follows the same BitMode16 path as 27C400, and that
    /// slot_size reflects the smaller address space correctly.
    #[test]
    fn v2_single_fire40a_27c200() {
        let json = r#"{
            "version": 1,
            "description": "v2 40-pin 27C200",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C200" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        // 27C200 = 256KB byte-mode image = 262144 bytes
        b.add_file(FileData::new(0, vec![0xAAu8; 262144])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire40A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);

        // 2^17 word entries × 2 bytes/word = 262144, but Fire 40 A requires an
        // extra pin
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << (17 + 1) << 1); // 2^17 * 2 = 262144
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        // AlgData1 (default, force_16_bit=false), word_size=16
        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_1);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 16);

        // DMA: BitMode16
        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_16);

        // byte_pin populated (AlgData1 supplies it to AlgCs0)
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_ne!(
            v.read_u8(cs + CS0_BYTE_PIN).unwrap(),
            0xFF,
            "byte_pin must be set for AlgData1 (27C200 on Fire40A)"
        );

        // Single set: no pull config
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27C200");
    }

    // ========================================================================
    // v2 single: Fire32B / 27C010
    // ========================================================================

    /// Single 27C010 (128KB EPROM) on Fire32B.
    ///
    /// 17 address lines → num_addr_pins=17, slot_size=2^17=131072.
    /// CE+OE fixed active-low: serve_cs_low_0=0, no overrides.
    #[test]
    fn v2_single_fire32b_27c010() {
        let json = r#"{
            "version": 1,
            "description": "v2 Fire32B 27C010",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C010" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 131072]), /* 27C010 = 128KB */
        )
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire32B)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 17); // 2^17 = 131072
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27C010");
    }

    // ========================================================================
    // v2 single: Fire32B / 27C040
    // ========================================================================

    /// Single 27C040 (512KB EPROM) on Fire32B.
    ///
    /// 19 address lines → num_addr_pins=19, slot_size=2^19=524288.
    /// A16/A17/A18 are dual-bonded; the layout picks GPIOs 18/17/16
    /// for a contiguous span [16,34]. CE+OE fixed active-low.
    #[test]
    fn v2_single_fire32b_27c040() {
        let json = r#"{
            "version": 1,
            "description": "v2 Fire32B 27C040",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C040" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 524288]), /* 27C040 = 512KB */
        )
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire32B)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 19); // 2^19 = 524288
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27C040");
    }

    // ========================================================================
    // v2 single: Fire32B / SST39SF040
    // ========================================================================

    /// Single SST39SF040 (512KB flash) on Fire32B.
    ///
    /// Same size as 27C040 but A18 is on pin 1 (dual-bonded [13,35]).
    /// The layout picks GPIO 35 for A18: span [17,35] is contiguous
    /// (score 19017) vs GPIO 13 which gives [13,34] with gaps (score 22013).
    /// slot_size=2^19=524288. CE+OE fixed active-low, same as 27C040.
    #[test]
    fn v2_single_fire32b_sst39sf040() {
        let json = r#"{
            "version": 1,
            "description": "v2 Fire32B SST39SF040",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "SST39SF040" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xBBu8; 524288]), /* SST39SF040 = 512KB */
        )
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire32B)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // Same table size as 27C040: 19 address lines → 2^19 bytes
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 19);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "SST39SF040");
    }

    // ========================================================================
    // v2 banked 2-chip: Fire28C / 2x 27128
    // ========================================================================

    /// 2-chip Banked 27128 on Fire28C.
    ///
    /// addr_layout: gpio_base=13, num_addr_pins=16, x1_gpio=Some(28).
    /// slot_size = 2^16 = 65536.
    ///
    /// Fire28C has x_jumper_pull=0: fitting the X1 jumper drives GPIO 28
    /// low, so bank 1 selection reads as 0 at the PIO — GpioOverInvert on
    /// X1 corrects this. Pull config also present for X1 (1 entry for
    /// 2-chip set).
    ///
    /// CE and OE are both fixed active-low on 27128; no CS override needed.
    #[test]
    fn v2_banked_2chip_fire28c_27128() {
        let json = r#"{
            "version": 1,
            "description": "v2 banked 2-chip 27128 Fire28C",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "bank0.bin", "type": "27128" },
                    { "file": "bank1.bin", "type": "27128" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 16384])).unwrap();
        b.add_file(FileData::new(1, vec![0x55u8; 16384])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_BANKED_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 16); // 2^16 = 65536
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0); // CE+OE active-low

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // 2-chip banked: pull entries for both bonds of X1 (GPIO 9 and GPIO 28)
        let pull = v.read_u32_le(alg + ALG_PULL_PTR).unwrap();
        assert_ne!(pull, NULL_PTR, "banked set must have gpio_pull_config");
        assert_eq!(v.read_u8(pull + PULL_PARAM_LEN).unwrap(), 2);

        // x_jumper_pull=0: X1 (GPIO 28) needs GpioOverInvert. GPIO18
        // (socket pin 1 / VPP) is a gap in the [13,29) address window and is
        // forced low — 2 entries.
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "banked on x_jumper_pull=0 board must have override"
        );
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 2);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        // Lower 6 bits must be X1's GPIO (28)
        assert_eq!(v.read_u8(ov + 1).unwrap() & 0x3F, 28);
        assert_eq!(v.read_u8(ov + 2).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 18);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr + 4).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "27128"
        );
    }

    // ========================================================================
    // v2 single: Fire32B / 27C080 — both halves
    // ========================================================================

    /// 27C080 half-select on Fire32B: tests both the lower half
    /// (cs1=active_low, no CS override) and the upper half
    /// (cs1=active_high, GpioOverInvert on GPIO 13 = A19).
    #[test]
    fn v2_single_fire32b_27c080_halves() {
        for (cs1_str, expect_override) in &[("active_low", false), ("active_high", true)] {
            let json = format!(
                r#"{{
                "version": 1,
                "description": "v2 Fire32B 27C080 {}",
                "chip_sets": [{{
                    "type": "single",
                    "chips": [{{ "file": "test.bin", "type": "27C080", "cs1": "{}" }}]
                }}]
            }}"#,
                cs1_str, cs1_str
            );

            let mut b = v2_builder(&json);
            b.add_file(FileData::new(0, vec![0xAAu8; 524288])).unwrap();

            let (meta, rom) = b
                .build(v2_props(Board::Fire32B))
                .unwrap_or_else(|e| panic!("build failed for cs1={cs1_str}: {e:?}"));
            let v = view(&meta);

            let s0 = slot_base(&v, 0);
            assert_eq!(
                v.read_u8(s0 + SLOT_TYPE).unwrap(),
                SLOT_TYPE_SINGLE_ROM,
                "cs1={cs1_str}: slot_type"
            );
            assert_eq!(
                v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(),
                1,
                "cs1={cs1_str}: rom_count"
            );

            // Both halves: same 19-bit table size
            let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
            assert_eq!(slot_size, 1u32 << 19, "cs1={cs1_str}: slot_size");
            assert_eq!(rom.len() as u32, slot_size, "cs1={cs1_str}: rom len");

            let alg = alg_base(&v, s0);
            let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
            assert_eq!(
                v.read_u8(cs + CS_DISCRIMINANT).unwrap(),
                ALG_CS_0,
                "cs1={cs1_str}: cs discriminant"
            );
            assert_eq!(
                v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(),
                0,
                "cs1={cs1_str}: serve_cs_low_0"
            );

            let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
            assert_eq!(
                v.read_u8(dma + DMA_BIT_MODE).unwrap(),
                BIT_MODE_8,
                "cs1={cs1_str}: bit_mode"
            );

            assert_eq!(
                v.read_u32_le(alg + ALG_PULL_PTR).unwrap(),
                NULL_PTR,
                "cs1={cs1_str}: no pull config"
            );

            let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
            if *expect_override {
                // active_high: GpioOverInvert on GPIO 13 (A19/CS1)
                assert_ne!(ov, NULL_PTR, "cs1={cs1_str}: override must be present");
                assert_eq!(
                    v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
                    1,
                    "cs1={cs1_str}: override param_len"
                );
                assert_eq!(
                    v.read_u8(ov + 1).unwrap() >> 6,
                    OVERRIDE_TYPE_INVERT,
                    "cs1={cs1_str}: override type"
                );
                assert_eq!(
                    v.read_u8(ov + 1).unwrap() & 0x3F,
                    13,
                    "cs1={cs1_str}: override GPIO must be 13 (A19)"
                );
            } else {
                // active_low: no override needed
                assert_eq!(ov, NULL_PTR, "cs1={cs1_str}: no override expected");
            }

            let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
            let rom0 = v.read_u32_le(roms_arr).unwrap();
            assert_eq!(
                v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(),
                "27C080",
                "cs1={cs1_str}: chip type string"
            );
        }
    }

    // ========================================================================
    // v2 cross-size: 2364 (24-pin) on Fire28C (28-pin), pin_offset=2
    // ========================================================================

    /// Single 2364 (24-pin mask ROM) served from a 28-pin One ROM (Fire28C),
    /// pin_offset=2.
    ///
    /// The 2364's 13 address lines (chip pins 1-8, 19, 21-23) land at socket
    /// pins 3-10, 21, 23-25, resolving to GPIOs {10,13,14,15,16,20-27}.
    /// The GPIO span [10,27] is 18 bits wide — 5 of those positions are
    /// occupied by data/CS lines, not address lines, and become don't-care
    /// bits in the ROM table. The resulting table is 2^18 = 262144 bytes
    /// (256KB), fitting within MAX_IMAGE_SIZE.
    ///
    /// CS1 (chip pin 20) lands at socket pin 22 → GPIO 11.
    /// Data lines (chip pins 9-11, 13-17) land at socket pins 11-13, 15-19
    /// → GPIOs 0-7.
    ///
    /// Key cross-size assertions:
    /// - slot_size = 2^18 = 262144 (18-bit GPIO span, not 16)
    /// - first_rom_cs_base = 11 (CS1 at GPIO 11, gpio_base=0)
    /// - first_rom_num_cs_pins = 1
    #[test]
    fn v2_single_fire28c_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 cross-size: 2364 on Fire28C",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // 18-bit GPIO span (not the usual 16): 5 gap positions add 2^5
        // redundancy to the table. The 8KB chip fills only 8192 of the
        // 262144 entries; the rest are PAD_NO_CHIP_BYTE.
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(
            slot_size,
            1u32 << 18,
            "2364 on Fire28C: 18-bit GPIO span → 256KB table"
        );
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        // CS1 at GPIO 11 (socket pin 22), gpio_base=0 → offset 11.
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(),
            11,
            "CS1 (chip pin 20 + offset 2 = socket 22 → GPIO 11)"
        );
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);
        assert_eq!(v.read_u8(cs + CS0_BYTE_PIN).unwrap(), 0xFF);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "unused window GPIOs forced low");
        // 24-pin 2364 centred in 28-pin socket (offset +2): GPIOs 12,17,18,19
        // are gaps in the [10,28) address window (NC / VCC socket pins).
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 4);
        assert_eq!(v.read_u8(ov + 1).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 12);
        assert_eq!(v.read_u8(ov + 2).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 17);
        assert_eq!(v.read_u8(ov + 3).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 18);
        assert_eq!(v.read_u8(ov + 4).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 19);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "2364");
    }

    // ========================================================================
    // v2 cross-size: 2732 (24-pin) on Fire32B (32-pin), pin_offset=4
    // ========================================================================

    /// Single 2732 (24-pin EPROM) served from a 32-pin One ROM (Fire32B),
    /// pin_offset=4.
    ///
    /// The 2732 works where 2364 does not because chip pin 18 differs between
    /// the two:
    ///   - 2364: chip pin 18 = A11 (address line) → socket 22 → GPIO 14.
    ///     GPIO 14 drags the addr span to [14,34], which straddles both PIO
    ///     windows and fits in neither.
    ///   - 2732: chip pin 18 = CE (control line, not an address pin). GPIO
    ///     14 never enters the address span. The 12 address lines resolve to
    ///     GPIOs {20-23, 27-34}, span [20,34] = 15 bits, padded to 16 —
    ///     fitting cleanly in [16,48).
    ///
    /// CE (chip pin 18 → socket 22 → GPIO 14) and OE (chip pin 20 → socket
    /// 24 → GPIO 15, taking the contiguous dual-bond option over GPIO 36)
    /// form a 2-pin contiguous CS range [14,15].
    ///
    /// Key cross-size assertions:
    /// - slot_size = 2^16 = 65536
    /// - first_rom_cs_base = 14 (CE at GPIO 14, gpio_base=0)
    /// - first_rom_num_cs_pins = 2 (CE + OE contiguous)
    #[test]
    fn v2_single_fire32b_2732() {
        let json = r#"{
            "version": 1,
            "description": "v2 cross-size: 2732 on Fire32B",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2732" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xBBu8; 4096])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire32B)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(
            slot_size,
            1u32 << 15,
            "2732 on Fire32B: 12 addr pins padded to 15 → 32KB table"
        );
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        // CE (chip pin 18 + offset 4 = socket 22 → GPIO 14), gpio_base=0.
        // OE (chip pin 20 + offset 4 = socket 24 → GPIO 15) is adjacent.
        // Together they form a 2-pin contiguous CS range.
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(),
            14,
            "CE at GPIO 14 (chip pin 18 + offset 4 = socket 22)"
        );
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(),
            2,
            "CE + OE span 2 contiguous pins [14,15]"
        );
        assert_eq!(v.read_u8(cs + CS0_BYTE_PIN).unwrap(), 0xFF);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "unused window GPIOs forced low");
        // 24-pin 2732 centred in 32-pin socket (offset +4): GPIOs 24,25,26
        // are gaps in the [20,35) address window.
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 3);
        assert_eq!(v.read_u8(ov + 1).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 24);
        assert_eq!(v.read_u8(ov + 2).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 25);
        assert_eq!(v.read_u8(ov + 3).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 26);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "2732");
    }

    // ========================================================================
    // v2 cross-size: 27256 (28-pin) on Fire32B (32-pin), pin_offset=2
    // ========================================================================

    /// Single 27256 (28-pin EPROM) served from a 32-pin One ROM (Fire32B),
    /// pin_offset=2.
    ///
    /// The 27256's 15 address lines (chip pins 2-10, 21, 23-27) land at
    /// socket pins 4-12, 23, 25-29, resolving to GPIOs {20-34} — 15
    /// contiguous values, padded to 16. GPIO span fits [16,48).
    ///
    /// Notably, the CE/OE layout is identical to 2732 on Fire32B (above):
    /// CE (chip pin 20 → socket 22 → GPIO 14) and OE (chip pin 22 → socket
    /// 24 → GPIO 15) form the same 2-pin CS range [14,15], and data lines
    /// resolve to GPIOs 0-7. This is not a coincidence — the same socket
    /// positions carry the same GPIOs regardless of which chip is installed.
    ///
    /// Key cross-size assertions:
    /// - slot_size = 2^16 = 65536
    /// - first_rom_cs_base = 14
    /// - first_rom_num_cs_pins = 2
    #[test]
    fn v2_single_fire32b_27256() {
        let json = r#"{
            "version": 1,
            "description": "v2 cross-size: 27256 on Fire32B",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27256" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xCCu8; 32768])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire32B)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(
            slot_size,
            1u32 << 15,
            "27256 on Fire32B: 15 addr pins padded to 15 → 32KB table"
        );
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);
        // CE (chip pin 20 + offset 2 = socket 22 → GPIO 14), gpio_base=0.
        // OE (chip pin 22 + offset 2 = socket 24 → GPIO 15).
        // Identical CS layout to 2732 on Fire32B: same socket positions,
        // same GPIOs, regardless of which chip is installed.
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(),
            14,
            "CE at GPIO 14 (chip pin 20 + offset 2 = socket 22)"
        );
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(),
            2,
            "CE + OE span 2 contiguous pins [14,15]"
        );
        assert_eq!(v.read_u8(cs + CS0_BYTE_PIN).unwrap(), 0xFF);

        let data = v.read_u32_le(alg + ALG_DATA_PTR).unwrap();
        assert_eq!(v.read_u8(data + DATA_DISCRIMINANT).unwrap(), ALG_DATA_0);
        assert_eq!(v.read_u8(data + DATA_WORD_SIZE).unwrap(), 8);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27256");
    }

    // ========================================================================
    // v2 cross-size: 2364 (24-pin) on Fire32B (32-pin) — incompatible layout
    // ========================================================================

    /// 2364 on Fire32B must fail: pin_offset=2 is a valid offset, but the
    /// resulting GPIO span is unservable.
    ///
    /// `socket_pin_offset(24, 32) = Some(4)`, so this is NOT an
    /// `IncompatiblePinCount` error. The failure occurs inside
    /// `derive_addr_layout`: chip pin 18 (A11) lands at socket pin 22 →
    /// GPIO 14, while chip pins 7/8 (A1/A0) land at socket pins 11/12 →
    /// GPIOs 33/34. The resulting span [14,34] is 21 bits wide, which
    /// straddles both PIO windows ([0,32) and [16,48)) and fits in neither.
    ///
    /// The same chip works on Fire28C (pin_offset=2) because the Fire28C
    /// GPIO layout keeps the equivalent span within [0,32). It works as
    /// 2732 on Fire32B because the 2732's chip pin 18 is CE (excluded from
    /// the address span), not an address line.
    ///
    /// A future Fire32C with a dual-bond at socket pin 22 → [14, 37] would
    /// resolve this: the combo scorer would pick GPIO 37 for A11, giving a
    /// span of [20,37] = 18 bits that fits [16,48), at the cost of a 256KB
    /// table.
    #[test]
    fn v2_single_fire32b_2364_incompatible_layout() {
        let json = r#"{
            "version": 1,
            "description": "v2 cross-size: 2364 on Fire32B (expected failure)",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2364", "cs1": "active_low" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();

        let result = b.build(v2_props(Board::Fire32B));

        assert!(
            result.is_err(),
            "2364 on Fire32B must fail: A11 (chip pin 18) at socket 22 → GPIO 14 \
             puts the addr span [14,34] outside both PIO windows"
        );
    }

    // ========================================================================
    // v2 fly-lead: 27128 (28-pin) on Fire24A (24-pin) — A13 in VCC socket
    // ========================================================================

    /// 27128 on Fire24A must fail: A13 (chip pin 26) maps to socket pin 24
    /// (pin_offset=-2: 26-2=24), which is VCC on Fire24A and carries no GPIO.
    /// `gpios_for_pin` returns `UnmappedPin` → build error.
    ///
    /// This is the canonical example of a fly-lead that cannot work: the extra
    /// address line falls inside the socket at a position the board already
    /// drives, rather than overhanging outside it.
    #[test]
    fn v2_single_fire24a_27128_a13_in_vcc_socket() {
        let json = r#"{
            "version": 1,
            "description": "v2 fly-lead: 27128 on Fire24A (expected failure)",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27128" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 16384]), /* 27128 = 16KB */
        )
        .unwrap();

        let result = b.build(v2_props(Board::Fire24A));
        assert!(
            result.is_err(),
            "27128 on Fire24A: A13 (chip pin 26) at socket pin 24 = VCC (GPIO_NONE) → must fail"
        );
    }

    // ========================================================================
    // v2 fly-lead: 28C512 (32-pin) on Fire28C (28-pin) — no fly-leads needed
    // ========================================================================

    /// 28C512 (64KB EEPROM, 32-pin) on Fire28C (28-pin), pin_offset=-2.
    ///
    /// All 16 address lines fall within socket pins 1-28. The overhanging
    /// chip positions (pins 1, 2, 31, 32 at socket positions -1, 0, 29, 30)
    /// carry only non-address signals, so no fly-leads are needed despite the
    /// chip having more pins than the socket.
    ///
    /// 16 address lines → slot_size = 2^16 = 65536.
    /// Single set: no pull config, no GPIO override config.
    #[test]
    fn v2_single_fire28c_28c512_fly_lead_none_required() {
        let json = r#"{
            "version": 1,
            "description": "v2 fly-lead: 28C512 on Fire28C (0 fly-leads)",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "28C512" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 65536]), /* 28C512 = 64KB */
        )
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // 16 address lines → 64KB table
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 16);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // No fly-lead wiring needed: no pull config, no GPIO override
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "28C512");
    }

    // ========================================================================
    // v2 fly-lead: 27C010 (32-pin) on Fire28C (28-pin) — one fly-lead to X1
    // ========================================================================

    /// 27C010 (128KB EPROM, 32-pin) on Fire28C (28-pin), pin_offset=-2.
    ///
    /// A16 (chip pin 2) maps to socket pin 0 (2-2=0 < 1) and overhangs —
    /// fly-leaded to X1. A15 (chip pin 3) maps to socket pin 1 and resolves
    /// normally from the socket. All remaining 15 address lines (A0-A14) are
    /// within socket pins 1-28.
    ///
    /// 17 address lines → slot_size = 2^17 = 131072.
    /// CE and OE are fixed active-low and match the Single set convention —
    /// no pull config, no GPIO override config.
    #[test]
    fn v2_single_fire28c_27c010_one_fly_lead() {
        let json = r#"{
            "version": 1,
            "description": "v2 fly-lead: 27C010 on Fire28C (1 fly-lead to X1)",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "27C010" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(
            FileData::new(0, vec![0xAAu8; 131072]), /* 27C010 = 128KB */
        )
        .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // 17 address lines → 128KB table
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 17);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        // Single set, CE+OE active-low: serve_cs_low_0 = 0
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 0);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // Fly-lead to X1 for A16 — no pull config (not banked), no CS override
        // (CE+OE are fixed active-low and match the Single set convention)
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        assert_eq!(v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap(), NULL_PTR);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "27C010");
    }

    // ========================================================================
    // v2 fly-lead: 2764 (28-pin) on Fire24A (24-pin) — one fly-lead to X1
    // ========================================================================

    /// 2764 (8KB EPROM, 28-pin) on Fire24A (24-pin), pin_offset=-2.
    ///
    /// A12 (chip pin 2) maps to socket pin 0 (2-2=0 < 1) and overhangs —
    /// fly-leaded to X1. All other address lines (A0-A11) are within socket
    /// pins 1-24. Single set: no pull config, no GPIO override config.
    #[test]
    fn v2_single_fire24a_2764_one_fly_lead() {
        let json = r#"{
            "version": 1,
            "description": "v2 fly-lead: 2764 on Fire24A (1 fly-lead to X1)",
            "chip_sets": [{
                "type": "single",
                "chips": [{ "file": "test.bin", "type": "2764" }]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192]) /* 2764 = 8KB */)
            .unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24A)).expect("build");
        let v = view(&meta);

        assert_eq!(v.read_u8(HDR_SLOT_COUNT).unwrap(), 1);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_SINGLE_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 1);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 15); // 32KB per compat table
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "unused window GPIO forced low");
        // 28-pin 2764 fly-leaded into 24-pin socket (offset -2): GPIO8 (X2)
        // is the sole gap in the [0,15) address window.
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 1);
        assert_eq!(v.read_u8(ov + 1).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 8);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        let rom0 = v.read_u32_le(roms_arr).unwrap();
        assert_eq!(v.read_cstr(rom0 + ROM_INFO_TYPE_PTR).unwrap(), "2764");
    }

    // ========================================================================
    // check_cs_v2: Multi set CS polarity consistency
    // ========================================================================

    /// CS2-primary Multi set must be accepted.
    ///
    /// Regression test for the bug where cs_primary_polarity blindly returned
    /// cs1 polarity, causing CS2-primary sets to fail with InconsistentCsLogic
    /// when chips[1+] legitimately had cs1=Ignore (commoned).
    ///
    /// Here: CS1 is commoned (active_low across all chips), CS2 is the
    /// per-chip select. chips[1] has cs1=ignore and cs2=active_low — exactly
    /// 1 active line, matching chip[0]'s cs2 polarity.
    #[test]
    fn check_cs_v2_multi_cs2_primary_accepted() {
        let json = r#"{
            "version": 1,
            "description": "CS2-primary Multi regression",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "a.bin", "type": "23128",
                      "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "b.bin", "type": "23128",
                      "cs1": "ignore", "cs2": "active_low", "cs3": "ignore" }
                ]
            }]
        }"#;
        v2_builder(json); // must not panic — from_json must succeed
    }

    // ========================================================================
    // check_cs_v2: Multi set accepts mixed per-chip select polarities
    // ========================================================================

    /// Multi sets with different CS polarities across chips must be accepted.
    ///
    /// Each chip in a Multi set has its per-chip select on a *different*
    /// physical GPIO: chip[0] uses the board's CS1 line; chips[1+] are
    /// fly-leaded to X1/X2. Because those are independent signals with
    /// independent GpioOverInvert handling, there is no physical constraint
    /// requiring them to share the same polarity.
    ///
    /// Previously rejected by the Multi polarity consistency check
    /// (active_low ≠ active_high); now accepted after restricting that
    /// check to Banked sets only.
    #[test]
    fn check_cs_v2_multi_mixed_polarity_accepted() {
        let json = r#"{
            "version": 1,
            "description": "Multi set with mixed per-chip select polarities",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "a.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "b.bin", "type": "2364", "cs1": "active_high" }
                ]
            }]
        }"#;
        v2_builder(json); // must not panic — from_json must succeed
    }

    /// Banked sets must still reject inconsistent CS polarities: all chips
    /// share the same physical CS line, so active_low and active_high are
    /// contradictory for the same signal.
    #[test]
    fn check_cs_v2_banked_mixed_polarity_rejected() {
        let json = r#"{
            "version": 1,
            "description": "Banked set with mixed CS polarity — invalid",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "a.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "b.bin", "type": "2364", "cs1": "active_high" }
                ]
            }]
        }"#;
        Builder::from_json(FirmwareVersion::new(0, 7, 0, 0), McuFamily::Rp2350, json)
            .expect_err("Banked set with mixed CS polarity must be rejected");
    }

    // ========================================================================
    // v2 multi mixed secondary chip types (2364 primary + 2332 secondary)
    //
    // The primary (chip[0]) is a 2364 with one control line (CS1); a 2332
    // secondary has two (CS1 + CS2), with CS2 ignored because it is tied to a
    // fixed level in the host machine (e.g. the C64 character ROM's CS2 -> 5V).
    // Validation must accept such a set regardless of where the 2332 sits among
    // the secondaries: `derive_multi_cs_config` anchors on chip[0], so ordering
    // is irrelevant to how the set is served.
    // ========================================================================

    /// 2332 secondary in the *middle* (chip[1]). This is the ordering that
    /// regressed in v0.7.0: the old check used chip[1] as its reference and
    /// rejected the trailing 2364 for "differing" on CS2.
    #[test]
    fn check_cs_v2_multi_2332_secondary_middle_accepted() {
        let json = r#"{
            "version": 1,
            "description": "2364 primary, 2332 secondary in the middle",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "kernal.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "char.bin",   "type": "2332", "cs1": "active_low", "cs2": "ignore" },
                    { "file": "basic.bin",  "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;
        v2_builder(json); // must not panic — from_json must succeed
    }

    /// The same three chips with the 2332 secondary *last* (as shipped in
    /// `onerom-config/set-c64.json`). Must remain accepted.
    #[test]
    fn check_cs_v2_multi_2332_secondary_last_accepted() {
        let json = r#"{
            "version": 1,
            "description": "2364 primary, 2332 secondary last",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "kernal.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "basic.bin",  "type": "2364", "cs1": "active_low" },
                    { "file": "char.bin",   "type": "2332", "cs1": "active_low", "cs2": "ignore" }
                ]
            }]
        }"#;
        v2_builder(json); // must not panic — from_json must succeed
    }

    /// A secondary must have exactly one active control line. A 2332 secondary
    /// that leaves CS2 active (rather than ignoring it) has two — its single
    /// fly-lead cannot drive both, so this is rejected.
    #[test]
    fn check_cs_v2_multi_secondary_two_active_lines_rejected() {
        let json = r#"{
            "version": 1,
            "description": "2332 secondary with CS2 active — invalid",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "kernal.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "char.bin",   "type": "2332", "cs1": "active_low", "cs2": "active_low" }
                ]
            }]
        }"#;
        let err = Builder::from_json(FirmwareVersion::new(0, 7, 0, 0), McuFamily::Rp2350, json)
            .expect_err("secondary with two active control lines must be rejected");
        assert!(
            err.to_string().contains("exactly one active control line"),
            "unexpected error: {err}"
        );
    }

    /// All secondaries must select on the same control line, because the deriver
    /// reads only chip[1] to fix the per-chip select for the whole set. Here a
    /// 27512 set has one secondary selecting on CE and another on OE.
    #[test]
    fn check_cs_v2_multi_secondaries_disagree_on_select_rejected() {
        let json = r#"{
            "version": 1,
            "description": "27512 multi, secondaries select different lines — invalid",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "a.bin", "type": "27512", "ce": "active_low", "oe": "active_low" },
                    { "file": "b.bin", "type": "27512", "ce": "active_low", "oe": "ignore" },
                    { "file": "c.bin", "type": "27512", "ce": "ignore", "oe": "active_low" }
                ]
            }]
        }"#;
        let err = Builder::from_json(FirmwareVersion::new(0, 7, 0, 0), McuFamily::Rp2350, json)
            .expect_err("secondaries selecting different lines must be rejected");
        assert!(
            err.to_string().contains("same per-chip select line"),
            "unexpected error: {err}"
        );
    }

    /// A secondary must have every control line the primary has. Here the
    /// primary is a 2332 (CS1 + CS2) but a secondary is a 2364 (CS1 only): the
    /// deriver would read the 2364's absent CS2 as active and misclassify it, so
    /// this is rejected.
    #[test]
    fn check_cs_v2_multi_secondary_missing_primary_line_rejected() {
        let json = r#"{
            "version": 1,
            "description": "2332 primary, 2364 secondary lacking CS2 — invalid",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "a.bin", "type": "2332", "cs1": "active_low", "cs2": "active_low" },
                    { "file": "b.bin", "type": "2364", "cs1": "active_low" }
                ]
            }]
        }"#;
        let err = Builder::from_json(FirmwareVersion::new(0, 7, 0, 0), McuFamily::Rp2350, json)
            .expect_err("secondary lacking a primary control line must be rejected");
        assert!(
            err.to_string().contains("lacks control line"),
            "unexpected error: {err}"
        );
    }

    // ========================================================================
    // v2 multi 2-chip CS2-primary: Fire28C / 2x 23128
    // ========================================================================

    /// 2-chip CS2-primary Multi set on Fire28C with 23128.
    ///
    /// chip[0]: cs1=active_low (commoned), cs2=active_low (per-chip select),
    ///          cs3=active_low (commoned).
    /// chip[1]: cs1=ignore, cs2=active_low, cs3=ignore.
    ///
    /// Regression: previously panicked at rom_image.rs because the Multi
    /// branch searched for SelectRole::Cs1|Ce|Oe and CS2-primary sets have
    /// SelectRole::Cs2 as their first select_line entry. Now fixed to use
    /// select_lines.first().
    ///
    /// first_rom_cs_base=11 (CS2 at GPIO 11) distinguishes this from
    /// CS1-primary sets (first_rom_cs_base=10) and CE-primary (=10) or
    /// OE-primary (=11) 27-series sets.
    #[test]
    fn v2_multi_2chip_fire28c_23128_cs2_primary() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi CS2-primary 23128 Fire28C",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "23128",
                      "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "chip1.bin", "type": "23128",
                      "cs1": "ignore", "cs2": "active_low", "cs3": "ignore" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 16384])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 16384])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire28C)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 2);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);

        // CS2 at GPIO 11 is the per-chip select: first_rom_cs_base=11.
        // This is the key assertion proving select_lines.first() returned
        // CS2 (not CS1 at GPIO 10).
        assert_eq!(
            v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(),
            11,
            "first_rom_cs_base must be 11 (CS2 at GPIO 11, not CS1 at GPIO 10)"
        );
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // CS2 and X1 both active_low → both need GpioOverInvert (2 entries)
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(
            ov, NULL_PTR,
            "CS2-primary Multi must have gpio_override_config"
        );
        assert_eq!(v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(), 5);
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // CS2 @ GPIO11
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT); // X1 @ GPIO9
        assert_eq!(v.read_u8(ov + 3).unwrap(), (OVERRIDE_TYPE_INVERT << 6) | 10); // CS1 commoned @ GPIO10
        assert_eq!(v.read_u8(ov + 4).unwrap(), (OVERRIDE_TYPE_INVERT << 6) | 12); // CS3 commoned @ GPIO12
        assert_eq!(v.read_u8(ov + 5).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 18);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "23128"
        );
        assert_eq!(
            v.read_cstr(v.read_u32_le(roms_arr + 4).unwrap() + ROM_INFO_TYPE_PTR)
                .unwrap(),
            "23128"
        );
    }

    // ========================================================================
    // v2 multi 2-chip mixed polarity: Fire24E / 2x 2364
    // ========================================================================

    /// Regression test for the mixed-polarity Multi override bug.
    ///
    /// chip[0] cs1=active_low, chip[1] cs1=active_high.  The Multi CS PIO
    /// requires active-high on all select lines, so:
    ///   - CS1 (chip[0], active_low) needs GpioOverInvert.
    ///   - X1  (chip[1], active_high) must NOT be inverted.
    ///
    /// Before the fix, `build_cs_overrides` used chip[0]'s cs_config for X1
    /// as well, producing two GpioOverInvert entries and inverting X1
    /// incorrectly.  With the fix, only one entry is emitted (CS1 only).
    ///
    /// Fails before fix: override param_len == 2.
    /// Passes after fix: override param_len == 1.
    #[test]
    fn v2_multi_2chip_mixed_polarity_fire24e_2364() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi mixed polarity regression",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "2364", "cs1": "active_low" },
                    { "file": "chip1.bin", "type": "2364", "cs1": "active_high" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 8192])).unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 8192])).unwrap();

        let (meta, _rom) = b.build(v2_props(Board::Fire24E)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);

        let alg = alg_base(&v, s0);

        // No pull config: Multi X pins are CS selects, not jumpers.
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // Only CS1 needs GpioOverInvert (active_low ≠ required active_high).
        // X1 carries chip[1]'s active_high CS — it already matches the
        // required convention and must not be inverted.
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "CS1 override must be present");
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            1,
            "only chip[0]'s CS1 (active_low) should be inverted; \
            chip[1]'s X1 (active_high) must not be"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
    }

    // ========================================================================
    // v2 multi 3-chip: Fire24F / 3x 2316 — truly-ignored CS2/CS3 (issue266)
    // ========================================================================

    /// 3-chip Multi 2316 on Fire24F — the issue266 layout.
    ///
    /// Every chip has cs1=active_low (per-chip select), cs2=ignore, cs3=ignore.
    /// Because cs2/cs3 are `Ignore` on chip0 as well as the secondaries, they are
    /// *truly ignored*, not commoned: CS1@10 is the select, X1@9/X2@8 the fly-lead
    /// selects, and CS2@11/CS3@12 carry no meaning for the set.
    ///
    /// Before the fix, `derive_multi_cs_config` looked only at a secondary and
    /// classified every non-select line as commoned, so CS2/CS3 were folded into
    /// the CS-detect span (cs=8+5, the gate reading GPIO 11/12) and never forced
    /// low. After the fix they are excluded from the span and forced low as
    /// address-window gaps.
    ///
    /// Key assertions vs. the pre-fix behaviour:
    /// - num_cs_pins = 3 (the three real selects {8,9,10}), NOT 5.
    /// - override config forces GPIO 11 and 12 low.
    #[test]
    fn v2_multi_3chip_fire24f_2316_cs2_cs3_ignored() {
        let json = r#"{
            "version": 1,
            "description": "v2 multi 3-chip 2316 Fire24F (issue266)",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "chip0.bin", "type": "2316", "allow_cs_ignore": true,
                    "cs1": "active_low", "cs2": "ignore", "cs3": "ignore" },
                    { "file": "chip1.bin", "type": "2316", "allow_cs_ignore": true,
                    "cs1": "active_low", "cs2": "ignore", "cs3": "ignore" },
                    { "file": "chip2.bin", "type": "2316", "allow_cs_ignore": true,
                    "cs1": "active_low", "cs2": "ignore", "cs3": "ignore" }
                ]
            }]
        }"#;

        let mut b = v2_builder(json);
        b.add_file(FileData::new(0, vec![0xAAu8; 2048]) /* 2316 = 2KB */)
            .unwrap();
        b.add_file(FileData::new(1, vec![0xBBu8; 2048])).unwrap();
        b.add_file(FileData::new(2, vec![0xCCu8; 2048])).unwrap();

        let (meta, rom) = b.build(v2_props(Board::Fire24F)).expect("build");
        let v = view(&meta);

        let s0 = slot_base(&v, 0);
        assert_eq!(v.read_u8(s0 + SLOT_TYPE).unwrap(), SLOT_TYPE_MULTI_ROM);
        assert_eq!(v.read_u8(s0 + SLOT_ROM_COUNT).unwrap(), 3);
        assert_eq!(v.read_u32_le(s0 + SLOT_FW_OVRD).unwrap(), NULL_PTR);

        // Address window is [8,24) → 16 bits → 64KB. Unchanged by the fix: CS2/CS3
        // sit between the X pins and the address block, so they were inside the
        // window (and part of its span) either way.
        let slot_size = v.read_u32_le(s0 + SLOT_SIZE).unwrap();
        assert_eq!(slot_size, 1u32 << 16);
        assert_eq!(rom.len() as u32, slot_size);

        let alg = alg_base(&v, s0);
        let cs = v.read_u32_le(alg + ALG_CS_PTR).unwrap();
        assert_eq!(v.read_u8(cs + CS_DISCRIMINANT).unwrap(), ALG_CS_0);
        assert_eq!(v.read_u8(cs + CS0_SERVE_CS_LOW_0).unwrap(), 1);

        // The fix: the CS-detect range is the three real selects {X2@8, X1@9,
        // CS1@10}, not {8..12}. Ignored CS2/CS3 are out of the gate.
        assert_eq!(v.read_u8(cs + CS_BASE_CS_PIN).unwrap(), 8);
        assert_eq!(
            v.read_u8(cs + CS_NUM_CS_PINS).unwrap(),
            3,
            "ignored CS2/CS3 must be excluded from the CS range (was 5 pre-fix)"
        );

        // chip0's per-chip select is CS1 at GPIO 10.
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_CS_BASE).unwrap(), 10);
        assert_eq!(v.read_u8(cs + CS0_FIRST_ROM_NUM_CS_PINS).unwrap(), 1);

        let dma = v.read_u32_le(alg + ALG_DMA_PTR).unwrap();
        assert_eq!(v.read_u8(dma + DMA_BIT_MODE).unwrap(), BIT_MODE_8);

        // Multi X pins are driven CS selects, not jumpers → no pull config.
        assert_eq!(v.read_u32_le(alg + ALG_PULL_PTR).unwrap(), NULL_PTR);

        // Override config: three GpioOverInvert (CS1@10, X1@9, X2@8 — all
        // active_low, inverted to active-high for the Multi gate), then two
        // GpioOverLow for the truly-ignored CS2@11 and CS3@12 that fall inside the
        // [8,24) address window. Invert entries precede forced-low entries, matching
        // the pattern in v2_multi_2chip_fire28c_27128_ce_primary.
        let ov = v.read_u32_le(alg + ALG_OVERRIDE_PTR).unwrap();
        assert_ne!(ov, NULL_PTR, "Multi set must have gpio_override_config");
        assert_eq!(
            v.read_u8(ov + OVERRIDE_PARAM_LEN).unwrap(),
            5,
            "3 inverts (selects) + 2 forced-low (ignored CS2/CS3)"
        );
        assert_eq!(v.read_u8(ov + 1).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 2).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        assert_eq!(v.read_u8(ov + 3).unwrap() >> 6, OVERRIDE_TYPE_INVERT);
        // The crux of the fix: the ignored lines are forced low.
        assert_eq!(v.read_u8(ov + 4).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 11);
        assert_eq!(v.read_u8(ov + 5).unwrap(), (OVERRIDE_TYPE_LOW << 6) | 12);

        let roms_arr = v.read_u32_le(s0 + SLOT_ROMS).unwrap();
        for i in 0..3u32 {
            assert_eq!(
                v.read_cstr(v.read_u32_le(roms_arr + i * 4).unwrap() + ROM_INFO_TYPE_PTR)
                    .unwrap(),
                "2316"
            );
        }
    }

    // ========================================================================
    // v2 flash-overflow guard: Fire32B / 16 x 27C010
    // ========================================================================

    /// The v2 builder must reject a config whose composed ROM data does not fit
    /// the target board's flash — matching the guard the v1 builder has always
    /// had. Sixteen 27C010 slots (each served 1:1 at 128KB, see
    /// `v2_single_fire32b_27c010`) total 2MB, which exceeds the ROM space left
    /// on the RP2350's 2MB flash after the firmware (48KB) and the metadata
    /// region (16KB). Without the guard, `build()` would silently return an
    /// over-large image; every consumer of the single onerom-gen `build()`
    /// (CLI, the onerom-fw tool, Studio, one-rom-wasm) relies on this check.
    #[test]
    #[allow(clippy::wildcard_enum_match_arm)]
    fn v2_rejects_oversized_rom_data() {
        const CHIP_BYTES: usize = 131_072; // 27C010 = 128KB, served 1:1
        const SLOTS: usize = 16; // 16 * 128KB = 2MB > flash minus fw+metadata

        let sets: Vec<String> = (0..SLOTS)
            .map(|i| {
                format!(
                    r#"{{ "type": "single", "chips": [{{ "file": "f{i}.bin", "type": "27C010" }}] }}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{ "version": 1, "description": "v2 flash overflow", "chip_sets": [{}] }}"#,
            sets.join(",")
        );

        let mut b = v2_builder(&json);
        for id in 0..SLOTS {
            b.add_file(FileData::new(id, vec![0xAAu8; CHIP_BYTES]))
                .unwrap();
        }

        let err = b
            .build(v2_props(Board::Fire32B))
            .expect_err("build must reject ROM data that overflows flash");

        match err {
            onerom_gen::Error::BufferTooSmall {
                location,
                expected,
                actual,
            } => {
                assert_eq!(location, "Flash");
                assert_eq!(expected, SLOTS * CHIP_BYTES);
                assert!(
                    expected > actual,
                    "expected ROM data ({expected}) should exceed available flash ({actual})"
                );
            }
            other => panic!("expected Error::BufferTooSmall, got {other:?}"),
        }
    }
}
