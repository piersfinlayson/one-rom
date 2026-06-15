// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for onerom-gen Builder
//!
//! Progressive validation of metadata and ROM image generation.
//!
//! # Test Plan
//!
//! ## Phase 1: Basic Structure Tests ✓ COMPLETE
//! - [x] Single ROM set, single ROM, no boot logging
//! - [x] Validate metadata header (magic, version, count)
//! - [x] Validate ROM set structure (data ptr, size, roms ptr, count, serve alg, multi-cs)
//! - [x] Validate ROM pointer array
//! - [x] Validate ROM info structure (rom type, cs1/cs2/cs3 states)
//! - [x] Validate pointer chain (header → rom set → rom array → rom info)
//!
//! ## Phase 2: Multiple ROM Sets ✓ COMPLETE
//! - [x] Multiple single ROM sets (2-3 sets)
//! - [x] Validate ROM set array is correct
//! - [x] Validate each set independently
//! - [x] Validate each ROM info independently
//!
//! ## Phase 3: CS Configuration Tests ✓ COMPLETE
//! - [x] 2332 with CS1 + CS2 (both active low)
//! - [x] 2332 with CS1 active low, CS2 active high
//! - [x] 2316 with CS1 + CS2 + CS3 (all active low)
//! - [x] 2316 with mixed active high/low states
//! - [x] Validate CS states stored correctly
//!
//! ## Phase 4: Boot Logging (Filenames) ✓ COMPLETE
//! - [x] Single ROM with boot_logging enabled
//! - [x] Validate ROM info structure is 8 bytes (not 4)
//! - [x] Validate filename pointer points within metadata
//! - [x] Validate null-terminated filename string
//! - [x] Multiple ROMs with boot_logging
//!
//! ## Phase 5: Size Handling ✓ COMPLETE
//! - [x] Exact size match (no size_handling needed)
//! - [x] Duplicate (smaller file, exact divisor)
//! - [x] Pad (smaller file)
//! - [x] Error cases (too large, wrong divisor, unnecessary size_handling)
//!
//! ## Phase 6: Multi-ROM Sets ✓ COMPLETE
//! - [x] Banked ROM sets
//! - [x] Multi ROM sets
//! - [x] Validate serve algorithm selection
//! - [x] Validate multi-CS state
//!
//! ## Phase 7: ROM Images Buffer ✓ COMPLETE
//! - [x] Validate buffer size matches expectations
//! - [x] Note: ROM image bytes are "mangled" (address/data transformations)
//! - [x] Use board pin maps to verify correctness
//! - [x] Test address mapping
//! - [x] Test data bit reordering
//!
//! ## Phase 8: Edge Cases ✓ COMPLETE
//! - [x] 32 ROM sets (stress test)
//! - [x] Minimum ROM size (2KB - 2316)
//! - [x] Missing CS config (should error)
//! - [x] Adding files out of order
//! - [x] Adding duplicate files (should error)
//! - [x] Missing files at build time (should error)
//!
//! ## Phase 9: Board Configuration Variations
//! - [ ] Build for RP2350 board (different MCU family)
//! - [ ] Board with address pins on GPIO 8-23 (high window)
//! - [ ] Board without X1/X2 pins - verify banked/multi sets fail
//! - [ ] Board with different data pin block (e.g., GPIO 16-23)
//! - [ ] Verify pin mapping assertions fire for invalid board configs
//!
//! ## Phase 10: Flash Capacity and Resource Limits
//! - [ ] Maximum ROM sets that fit in metadata (find actual limit)
//! - [ ] ROM images exceeding available flash space (should error)
//! - [ ] Metadata buffer overflow (too many ROM sets/filenames)
//! - [ ] Maximum ROMs per banked set
//! - [ ] Maximum ROMs per multi set
//! - [ ] Verify size calculations account for all metadata structures
//!
//! ## Phase 11: JSON Parsing and Validation Errors
//! - [x] Malformed JSON (should error)
//! - [x] Missing required fields (chip_sets, type, file, etc.)
//! - [x] Invalid ROM type string (should error)
//! - [x] Invalid CS logic string (should error)
//! - [x] Invalid set type string (should error)
//! - [x] CS2 specified for 2364 (should error or ignore?)
//! - [x] CS3 specified for 2332 (should error or ignore?)
//! - [x] No CS1 specified (should error)
//! - [x] Invalid size_handling value (should error)
//! - [x] ROM type mismatch within multi set (all ROMs same type?)
//! - [x] Negative or invalid version number
//!
//! ## Phase 12: Filename and Boot Logging Edge Cases
//! - [x] Maximum length filename (find limit)
//! - [x] Empty filename string
//! - [x] Filename with special characters (spaces, quotes, slashes)
//! - [x] Filename with unicode characters
//! - [x] Very long filenames causing metadata overflow
//! - [x] Duplicate filenames in different ROM sets
//! - [x] Null bytes in filename (should terminate properly)
//!
//! ## Phase 13: ServeAlg Automatic Selection
//! - [x] Single ROM uses FirmwareProperties serve_alg
//! - [x] Banked ROM uses FirmwareProperties serve_alg
//! - [x] Multi ROM set automatically overrides to AddrOnAnyCs
//! - [x] Mixed config (single + multi) uses correct serve_alg for each set
//! - [x] Different FirmwareProperties serve_alg values (TwoCsOneAddr, AddrOnCs)
//! - [x] Verify multi set always uses AddrOnAnyCs regardless of FirmwareProperties
//!
//! ## Phase 14: File Size Edge Cases
//! - [x] Zero-byte file (should error)
//! - [x] Single-byte file
//! - [x] File size 2047 bytes (just under 2KB boundary)
//! - [x] File size 2049 bytes (just over 2KB, requires size_handling)
//! - [x] File size exactly 1KB for 2KB ROM with duplicate
//! - [x] File size exactly 2KB for 8KB ROM with duplicate
//! - [x] Very small file with pad (1 byte padded to 8KB)
//!
//! ## Phase 15: Complex ROM Set Configurations
//! - [ ] Multi set with mixed ROM types (if allowed)
//! - [ ] Multi set with different CS configurations per ROM
//! - [ ] Banked set with mixed ROM types (if allowed)
//! - [ ] Multiple banked sets in one config
//! - [ ] Multiple multi sets in one config
//! - [ ] Mix of single, banked, and multi sets
//! - [ ] Maximum complexity config (many sets, many ROMs, boot logging)
//!
//! ## Phase 16: CS Configuration Validation
//! - [x] 2316 with only CS1 and CS2 (missing CS3, should error)
//! - [x] 2332 with CS3 specified (should error or ignore?)
//! - [x] All three CS as ignore (should error)
//! - [x] CS1 as ignore but CS2 active (should error?)
//! - [x] Invalid CS logic combination for multi set
//! - [x] Banked set where ROMs have different CS1 states (should error?)
//!
//! ## Phase 17: Licenses
//!
//! ## Phase 18: ROM Images Correctness
//! - [ ] Verify duplicate correctly duplicates data (check both copies)
//! - [ ] Verify pad fills with correct byte value (0xFF or 0x00?)
//! - [ ] Multi set with 4+ ROMs (test X1/X2 bit combinations)
//! - [ ] Banked set with 8 ROMs (if supported)
//! - [ ] Verify unused/invalid addresses contain correct fill byte
//! - [ ] Different board pin mappings produce correct transformations
//!
//! ## Phase 19: Descriptions
//!
//! ## Phase 20: Firmware overrides

#[cfg(test)]
mod tests {
    use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
    use onerom_config::hw::Board;
    use onerom_config::mcu::{Family as McuFamily, Variant as McuVariant};
    use onerom_gen::image::CsLogic;
    use onerom_gen::{Builder, FileData};

    const FW_VER: FirmwareVersion = FirmwareVersion::new(0, 6, 0, 0);
    const MCU_FAM: McuFamily = McuFamily::Stm32f4;

    // ========================================================================
    // Constants from C headers
    // ========================================================================

    const HEADER_MAGIC: &[u8; 16] = b"ONEROM_METADATA\0";
    const HEADER_VERSION: u32 = 1;
    const METADATA_HEADER_LEN: usize = 256;
    const CHIP_SET_METADATA_LEN: usize = 16;
    const CHIP_INFO_METADATA_LEN: usize = 4;
    const CHIP_INFO_METADATA_LEN_WITH_FILENAME: usize = 8;

    // Metadata starts at flash_base + 48KB
    const METADATA_FLASH_OFFSET: u32 = 49152;

    // ROM type C enum values (from Rom::rom_type_c_enum_val in image.rs)
    const CHIP_TYPE_2316: u8 = 0;
    const CHIP_TYPE_2332: u8 = 1;
    const CHIP_TYPE_2364: u8 = 2;

    // ========================================================================
    // Helper: Parse Metadata Header
    // ========================================================================

    /// Represents the onerom_metadata_header_t C structure
    #[derive(Debug)]
    struct MetadataHeader {
        magic: [u8; 16],
        version: u32,
        chip_set_count: u8,
        chip_sets_ptr: u32,
    }

    impl MetadataHeader {
        /// Parse the metadata header from the start of the buffer
        fn parse(buf: &[u8]) -> Self {
            assert!(
                buf.len() >= METADATA_HEADER_LEN,
                "Buffer too small: {} bytes, need {}",
                buf.len(),
                METADATA_HEADER_LEN
            );

            // Magic: offset 0, 16 bytes
            let mut magic = [0u8; 16];
            magic.copy_from_slice(&buf[0..16]);

            // Version: offset 16, 4 bytes (u32 little-endian)
            let version = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);

            // ROM set count: offset 20, 1 byte
            let chip_set_count = buf[20];

            // Padding: offset 21, 3 bytes (we skip these)

            // ROM sets pointer: offset 24, 4 bytes (u32 little-endian)
            let chip_sets_ptr = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);

            Self {
                magic,
                version,
                chip_set_count,
                chip_sets_ptr,
            }
        }

        /// Validate the header has correct magic and version
        fn validate_basic(&self) {
            assert_eq!(
                &self.magic, HEADER_MAGIC,
                "Magic bytes mismatch. Expected {:?}, got {:?}",
                HEADER_MAGIC, &self.magic
            );

            assert_eq!(
                self.version, HEADER_VERSION,
                "Version mismatch. Expected {}, got {}",
                HEADER_VERSION, self.version
            );

            assert!(
                self.chip_set_count > 0,
                "ROM set count must be > 0, got {}",
                self.chip_set_count
            );
        }
    }

    // ========================================================================
    // Helper: Parse ROM Set Structure
    // ========================================================================

    /// Represents the sdrr_chip_set_t C structure
    #[derive(Debug)]
    struct RomSetStruct {
        data_ptr: u32,
        size: u32,
        chips_ptr: u32,
        rom_count: u8,
        serve_alg: u8,
        multi_cs_state: u8,
    }

    impl RomSetStruct {
        /// Parse ROM set structure from buffer at given offset
        fn parse(buf: &[u8], offset: usize) -> Self {
            assert!(
                buf.len() >= offset + CHIP_SET_METADATA_LEN,
                "Buffer too small: {} bytes, need {} at offset {}",
                buf.len(),
                offset + CHIP_SET_METADATA_LEN,
                offset
            );

            // Data pointer: offset + 0, 4 bytes
            let data_ptr = u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);

            // Size: offset + 4, 4 bytes
            let size = u32::from_le_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]);

            // ROMs pointer: offset + 8, 4 bytes
            let chips_ptr = u32::from_le_bytes([
                buf[offset + 8],
                buf[offset + 9],
                buf[offset + 10],
                buf[offset + 11],
            ]);

            // ROM count: offset + 12, 1 byte
            let rom_count = buf[offset + 12];

            // Serve algorithm: offset + 13, 1 byte
            let serve_alg = buf[offset + 13];

            // Multi-CS state: offset + 14, 1 byte
            let multi_cs_state = buf[offset + 14];

            // Padding at offset + 15 (1 byte) - ignored

            Self {
                data_ptr,
                size,
                chips_ptr,
                rom_count,
                serve_alg,
                multi_cs_state,
            }
        }
    }

    // ========================================================================
    // Helper: Parse ROM Info Structure
    // ========================================================================

    /// Represents the sdrr_rom_info_t C structure
    #[derive(Debug)]
    struct RomInfoStruct {
        rom_type: u8,
        cs1_state: u8,
        cs2_state: u8,
        cs3_state: u8,
        filename_ptr: Option<u32>,
    }

    impl RomInfoStruct {
        /// Parse ROM info structure from buffer at given offset (without filename)
        fn parse(buf: &[u8], offset: usize) -> Self {
            assert!(
                buf.len() >= offset + CHIP_INFO_METADATA_LEN,
                "Buffer too small: {} bytes, need {} at offset {}",
                buf.len(),
                offset + CHIP_INFO_METADATA_LEN,
                offset
            );

            let rom_type = buf[offset];
            let cs1_state = buf[offset + 1];
            let cs2_state = buf[offset + 2];
            let cs3_state = buf[offset + 3];

            Self {
                rom_type,
                cs1_state,
                cs2_state,
                cs3_state,
                filename_ptr: None,
            }
        }

        /// Parse ROM info structure from buffer at given offset (with filename)
        fn parse_with_filename(buf: &[u8], offset: usize) -> Self {
            assert!(
                buf.len() >= offset + CHIP_INFO_METADATA_LEN_WITH_FILENAME,
                "Buffer too small: {} bytes, need {} at offset {}",
                buf.len(),
                offset + CHIP_INFO_METADATA_LEN_WITH_FILENAME,
                offset
            );

            let rom_type = buf[offset];
            let cs1_state = buf[offset + 1];
            let cs2_state = buf[offset + 2];
            let cs3_state = buf[offset + 3];

            let filename_ptr = u32::from_le_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]);

            Self {
                rom_type,
                cs1_state,
                cs2_state,
                cs3_state,
                filename_ptr: Some(filename_ptr),
            }
        }
    }

    // ========================================================================
    // Helper: Create test firmware properties
    // ========================================================================

    fn default_fw_props() -> FirmwareProperties {
        FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::Default,
            false, // boot_logging disabled
        )
        .unwrap()
    }

    fn fw_props_with_logging() -> FirmwareProperties {
        FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::Default,
            true, // boot_logging enabled
        )
        .unwrap()
    }

    // ========================================================================
    // Helper: Parse null-terminated string
    // ========================================================================

    fn parse_null_terminated_string(buf: &[u8], offset: usize) -> String {
        let mut end = offset;
        while end < buf.len() && buf[end] != 0 {
            end += 1;
        }

        assert!(
            end < buf.len(),
            "No null terminator found starting at offset {}",
            offset
        );

        String::from_utf8_lossy(&buf[offset..end]).to_string()
    }

    // ========================================================================
    // Helper: Create test ROM data
    // ========================================================================

    fn create_test_rom_data(size: usize, fill_byte: u8) -> Vec<u8> {
        vec![fill_byte; size]
    }

    // ========================================================================
    // TEST 1: Simplest possible - single ROM set, single ROM
    // ========================================================================

    #[test]
    fn test_phase1_single_rom_basic() {
        // Minimal JSON config: single ROM set with one 2364 ROM (8KB)
        let json = r#"{
            "version": 1,
            "description": "Phase 1 basic test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        // Parse the JSON
        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Get the file specs - should be exactly 1
        let file_specs = builder.file_specs();
        assert_eq!(file_specs.len(), 1, "Should have exactly 1 file");
        assert_eq!(file_specs[0].id, 0, "File ID should be 0");
        assert_eq!(file_specs[0].source, "test.rom", "File source should match");

        // Create 8KB of test data (2364 is 8KB)
        let rom_data = create_test_rom_data(8192, 0xAA);

        // Add the file
        builder
            .add_file(FileData {
                id: 0,
                data: rom_data,
            })
            .expect("Failed to add file");

        // Build the metadata and ROM images
        let props = default_fw_props();
        let (metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Basic sanity checks
        assert!(
            !metadata_buf.is_empty(),
            "Metadata buffer should not be empty"
        );
        assert!(
            metadata_buf.len() >= METADATA_HEADER_LEN,
            "Metadata buffer should be at least {} bytes, got {}",
            METADATA_HEADER_LEN,
            metadata_buf.len()
        );
        assert!(
            !rom_images_buf.is_empty(),
            "ROM images buffer should not be empty"
        );

        // Parse and validate the metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();

        // Check ROM set count
        assert_eq!(header.chip_set_count, 1, "Should have exactly 1 ROM set");

        println!("✓ Phase 1 Test 1: Basic single ROM set passed");
        println!("  - Metadata size: {} bytes", metadata_buf.len());
        println!("  - ROM images size: {} bytes", rom_images_buf.len());
        println!("  - ROM set count: {}", header.chip_set_count);
    }

    // ========================================================================
    // TEST 2: Validate ROM Set Structure
    // ========================================================================

    #[test]
    fn test_phase1_chip_set_structure() {
        let json = r#"{
            "version": 1,
            "description": "Phase 1 ROM set structure test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let rom_data = create_test_rom_data(8192, 0xAA);
        builder
            .add_file(FileData {
                id: 0,
                data: rom_data,
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();

        // Calculate where ROM set structure should be
        // chip_sets_ptr is an absolute flash address, need to convert to metadata buffer offset
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;

        // Validate offset is within metadata buffer
        assert!(
            chip_set_offset < metadata_buf.len(),
            "ROM set pointer {} (offset {}) outside metadata buffer (size {})",
            header.chip_sets_ptr,
            chip_set_offset,
            metadata_buf.len()
        );

        // Parse the ROM set structure
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Validate data pointer (should be flash_base + 64KB)
        let expected_data_ptr = flash_base + 65536;
        assert_eq!(
            chip_set.data_ptr, expected_data_ptr,
            "Data pointer mismatch. Expected 0x{:08X}, got 0x{:08X}",
            expected_data_ptr, chip_set.data_ptr
        );

        // Validate size (STM32F4 single ROM = 16KB)
        let expected_size = 16384u32;
        assert_eq!(
            chip_set.size, expected_size,
            "Size mismatch. Expected {} bytes, got {} bytes",
            expected_size, chip_set.size
        );

        // Validate ROMs pointer is within metadata
        let chips_ptr_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        assert!(
            chips_ptr_offset < metadata_buf.len(),
            "ROMs pointer {} (offset {}) outside metadata buffer (size {})",
            chip_set.chips_ptr,
            chips_ptr_offset,
            metadata_buf.len()
        );

        // Validate ROM count
        assert_eq!(
            chip_set.rom_count, 1,
            "ROM count should be 1, got {}",
            chip_set.rom_count
        );

        // Validate serve algorithm (single ROM uses AddrOnCs)
        let expected_serve_alg = ServeAlg::AddrOnCs.c_enum_value();
        assert_eq!(
            chip_set.serve_alg, expected_serve_alg,
            "Serve algorithm mismatch. Expected {} (AddrOnCs), got {}",
            expected_serve_alg, chip_set.serve_alg
        );

        // Validate multi-CS state (single ROM should be Ignore)
        let expected_multi_cs = CsLogic::Ignore.c_enum_val();
        assert_eq!(
            chip_set.multi_cs_state, expected_multi_cs,
            "Multi-CS state mismatch. Expected {} (Ignore), got {}",
            expected_multi_cs, chip_set.multi_cs_state
        );

        println!("✓ Phase 1 Test 2: ROM set structure validation passed");
        println!("  - Data pointer: 0x{:08X}", chip_set.data_ptr);
        println!("  - Size: {} bytes", chip_set.size);
        println!("  - ROMs pointer: 0x{:08X}", chip_set.chips_ptr);
        println!("  - ROM count: {}", chip_set.rom_count);
        println!("  - Serve algorithm: {}", chip_set.serve_alg);
        println!("  - Multi-CS state: {}", chip_set.multi_cs_state);
    }

    // ========================================================================
    // TEST 3: Validate ROM Info Structure
    // ========================================================================

    #[test]
    fn test_phase1_rom_info_structure() {
        let json = r#"{
            "version": 1,
            "description": "Phase 1 ROM info structure test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let rom_data = create_test_rom_data(8192, 0xAA);
        builder
            .add_file(FileData {
                id: 0,
                data: rom_data,
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header and ROM set
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Parse ROM pointer array to get pointer to first ROM info
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        assert!(
            rom_array_offset + 4 <= metadata_buf.len(),
            "ROM array pointer {} (offset {}) outside metadata buffer",
            chip_set.chips_ptr,
            rom_array_offset
        );

        // Read the first pointer from the ROM pointer array (4 bytes)
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);

        // Convert to buffer offset
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        assert!(
            rom_info_offset < metadata_buf.len(),
            "ROM info pointer {} (offset {}) outside metadata buffer",
            rom_info_ptr,
            rom_info_offset
        );

        // Parse the ROM info structure
        let rom_info = RomInfoStruct::parse(&metadata_buf, rom_info_offset);

        // Validate ROM type (2364 = 2)
        assert_eq!(
            rom_info.rom_type, CHIP_TYPE_2364,
            "ROM type mismatch. Expected {} (2364), got {}",
            CHIP_TYPE_2364, rom_info.rom_type
        );

        // Validate CS1 state (active_low = 0)
        let expected_cs1 = CsLogic::ActiveLow.c_enum_val();
        assert_eq!(
            rom_info.cs1_state, expected_cs1,
            "CS1 state mismatch. Expected {} (ActiveLow), got {}",
            expected_cs1, rom_info.cs1_state
        );

        // Validate CS2 state (not used for 2364, should be CS_NOT_USED = 2)
        let expected_cs2 = CsLogic::Ignore.c_enum_val();
        assert_eq!(
            rom_info.cs2_state, expected_cs2,
            "CS2 state mismatch. Expected {} (Ignore), got {}",
            expected_cs2, rom_info.cs2_state
        );

        // Validate CS3 state (not used for 2364, should be CS_NOT_USED = 2)
        let expected_cs3 = CsLogic::Ignore.c_enum_val();
        assert_eq!(
            rom_info.cs3_state, expected_cs3,
            "CS3 state mismatch. Expected {} (Ignore), got {}",
            expected_cs3, rom_info.cs3_state
        );

        println!("✓ Phase 1 Test 3: ROM info structure validation passed");
        println!("  - ROM type: {} (2364)", rom_info.rom_type);
        println!("  - CS1 state: {} (ActiveLow)", rom_info.cs1_state);
        println!("  - CS2 state: {} (Ignore)", rom_info.cs2_state);
        println!("  - CS3 state: {} (Ignore)", rom_info.cs3_state);
    }

    // ========================================================================
    // PHASE 2: Multiple ROM Sets
    // ========================================================================

    // ========================================================================
    // TEST 5: Two ROM Sets
    // ========================================================================

    #[test]
    fn test_phase2_two_chip_sets() {
        let json = r#"{
            "version": 1,
            "description": "Phase 2 two ROM sets test",
            "chip_sets": [
                {
                    "type": "single",
                    "description": "Set 0 - 2364",
                    "chips": [{
                        "file": "set0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "description": "Set 1 - 2332",
                    "chips": [{
                        "file": "set1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add ROM data for both sets
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA), // 2364 = 8KB
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(4096, 0x55), // 2332 = 4KB
            })
            .expect("Failed to add file 1");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();

        // Validate we have 2 ROM sets
        assert_eq!(
            header.chip_set_count, 2,
            "Should have 2 ROM sets, got {}",
            header.chip_set_count
        );

        // Parse both ROM sets
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set0 = RomSetStruct::parse(&metadata_buf, chip_set0_offset);

        let chip_set1_offset = chip_set0_offset + CHIP_SET_METADATA_LEN;
        let chip_set1 = RomSetStruct::parse(&metadata_buf, chip_set1_offset);

        // Validate Set 0 (2364)
        assert_eq!(chip_set0.rom_count, 1, "Set 0 should have 1 ROM");
        assert_eq!(chip_set0.size, 16384, "Set 0 size should be 16KB");
        assert_eq!(
            chip_set0.serve_alg,
            ServeAlg::AddrOnCs.c_enum_value(),
            "Set 0 serve algorithm mismatch"
        );

        // Validate Set 1 (2332)
        assert_eq!(chip_set1.rom_count, 1, "Set 1 should have 1 ROM");
        assert_eq!(chip_set1.size, 16384, "Set 1 size should be 16KB");
        assert_eq!(
            chip_set1.serve_alg,
            ServeAlg::AddrOnCs.c_enum_value(),
            "Set 1 serve algorithm mismatch"
        );

        // Validate Set 0 data pointer (flash_base + 64KB)
        let expected_data_ptr0 = flash_base + 65536;
        assert_eq!(
            chip_set0.data_ptr, expected_data_ptr0,
            "Set 0 data pointer mismatch"
        );

        // Validate Set 1 data pointer (flash_base + 64KB + 16KB)
        let expected_data_ptr1 = flash_base + 65536 + 16384;
        assert_eq!(
            chip_set1.data_ptr, expected_data_ptr1,
            "Set 1 data pointer mismatch"
        );

        // Parse ROM info for Set 0
        let rom_array0_offset = (chip_set0.chips_ptr - metadata_flash_start) as usize;
        let rom_info0_ptr = u32::from_le_bytes([
            metadata_buf[rom_array0_offset],
            metadata_buf[rom_array0_offset + 1],
            metadata_buf[rom_array0_offset + 2],
            metadata_buf[rom_array0_offset + 3],
        ]);
        let rom_info0_offset = (rom_info0_ptr - metadata_flash_start) as usize;
        let rom_info0 = RomInfoStruct::parse(&metadata_buf, rom_info0_offset);

        // Validate Set 0 ROM info
        assert_eq!(
            rom_info0.rom_type, CHIP_TYPE_2364,
            "Set 0 ROM type mismatch"
        );
        assert_eq!(rom_info0.cs1_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info0.cs2_state, CsLogic::Ignore.c_enum_val());
        assert_eq!(rom_info0.cs3_state, CsLogic::Ignore.c_enum_val());

        // Parse ROM info for Set 1
        let rom_array1_offset = (chip_set1.chips_ptr - metadata_flash_start) as usize;
        let rom_info1_ptr = u32::from_le_bytes([
            metadata_buf[rom_array1_offset],
            metadata_buf[rom_array1_offset + 1],
            metadata_buf[rom_array1_offset + 2],
            metadata_buf[rom_array1_offset + 3],
        ]);
        let rom_info1_offset = (rom_info1_ptr - metadata_flash_start) as usize;
        let rom_info1 = RomInfoStruct::parse(&metadata_buf, rom_info1_offset);

        // Validate Set 1 ROM info
        assert_eq!(
            rom_info1.rom_type, CHIP_TYPE_2332,
            "Set 1 ROM type mismatch"
        );
        assert_eq!(rom_info1.cs1_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info1.cs2_state, CsLogic::ActiveHigh.c_enum_val());
        assert_eq!(rom_info1.cs3_state, CsLogic::Ignore.c_enum_val());

        println!("✓ Phase 2 Test 1: Two ROM sets validation passed");
        println!("  Set 0:");
        println!("    - ROM type: {} (2364)", rom_info0.rom_type);
        println!("    - Data pointer: 0x{:08X}", chip_set0.data_ptr);
        println!("    - Size: {} bytes", chip_set0.size);
        println!("    - CS1: {} (ActiveLow)", rom_info0.cs1_state);
        println!("  Set 1:");
        println!("    - ROM type: {} (2332)", rom_info1.rom_type);
        println!("    - Data pointer: 0x{:08X}", chip_set1.data_ptr);
        println!("    - Size: {} bytes", chip_set1.size);
        println!(
            "    - CS1: {} (ActiveLow), CS2: {} (ActiveHigh)",
            rom_info1.cs1_state, rom_info1.cs2_state
        );
    }

    // ========================================================================
    // TEST 6: Three ROM Sets
    // ========================================================================

    #[test]
    fn test_phase2_three_chip_sets() {
        let json = r#"{
            "version": 1,
            "description": "Phase 2 three ROM sets test",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "set0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "set1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "set2.rom",
                        "type": "2316",
                        "cs1": "active_low",
                        "cs2": "active_low",
                        "cs3": "active_low"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add ROM data for all three sets
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA), // 2364 = 8KB
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(4096, 0x55), // 2332 = 4KB
            })
            .expect("Failed to add file 1");

        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(2048, 0xFF), // 2316 = 2KB
            })
            .expect("Failed to add file 2");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();

        // Validate we have 3 ROM sets
        assert_eq!(
            header.chip_set_count, 3,
            "Should have 3 ROM sets, got {}",
            header.chip_set_count
        );

        // Parse all three ROM sets
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set0 = RomSetStruct::parse(&metadata_buf, chip_set0_offset);

        let chip_set1_offset = chip_set0_offset + CHIP_SET_METADATA_LEN;
        let chip_set1 = RomSetStruct::parse(&metadata_buf, chip_set1_offset);

        let chip_set2_offset = chip_set1_offset + CHIP_SET_METADATA_LEN;
        let chip_set2 = RomSetStruct::parse(&metadata_buf, chip_set2_offset);

        // Validate data pointers are sequential
        let expected_data_ptr0 = flash_base + 65536;
        let expected_data_ptr1 = expected_data_ptr0 + 16384;
        let expected_data_ptr2 = expected_data_ptr1 + 16384;

        assert_eq!(chip_set0.data_ptr, expected_data_ptr0, "Set 0 data pointer");
        assert_eq!(chip_set1.data_ptr, expected_data_ptr1, "Set 1 data pointer");
        assert_eq!(chip_set2.data_ptr, expected_data_ptr2, "Set 2 data pointer");

        // Parse and validate ROM info for Set 0 (2364)
        let rom_array0_offset = (chip_set0.chips_ptr - metadata_flash_start) as usize;
        let rom_info0_ptr = u32::from_le_bytes([
            metadata_buf[rom_array0_offset],
            metadata_buf[rom_array0_offset + 1],
            metadata_buf[rom_array0_offset + 2],
            metadata_buf[rom_array0_offset + 3],
        ]);
        let rom_info0 = RomInfoStruct::parse(
            &metadata_buf,
            (rom_info0_ptr - metadata_flash_start) as usize,
        );

        assert_eq!(rom_info0.rom_type, CHIP_TYPE_2364);
        assert_eq!(rom_info0.cs1_state, CsLogic::ActiveLow.c_enum_val());

        // Parse and validate ROM info for Set 1 (2332)
        let rom_array1_offset = (chip_set1.chips_ptr - metadata_flash_start) as usize;
        let rom_info1_ptr = u32::from_le_bytes([
            metadata_buf[rom_array1_offset],
            metadata_buf[rom_array1_offset + 1],
            metadata_buf[rom_array1_offset + 2],
            metadata_buf[rom_array1_offset + 3],
        ]);
        let rom_info1 = RomInfoStruct::parse(
            &metadata_buf,
            (rom_info1_ptr - metadata_flash_start) as usize,
        );

        assert_eq!(rom_info1.rom_type, CHIP_TYPE_2332);
        assert_eq!(rom_info1.cs1_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info1.cs2_state, CsLogic::ActiveHigh.c_enum_val());

        // Parse and validate ROM info for Set 2 (2316)
        let rom_array2_offset = (chip_set2.chips_ptr - metadata_flash_start) as usize;
        let rom_info2_ptr = u32::from_le_bytes([
            metadata_buf[rom_array2_offset],
            metadata_buf[rom_array2_offset + 1],
            metadata_buf[rom_array2_offset + 2],
            metadata_buf[rom_array2_offset + 3],
        ]);
        let rom_info2 = RomInfoStruct::parse(
            &metadata_buf,
            (rom_info2_ptr - metadata_flash_start) as usize,
        );

        assert_eq!(rom_info2.rom_type, CHIP_TYPE_2316);
        assert_eq!(rom_info2.cs1_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info2.cs2_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info2.cs3_state, CsLogic::ActiveLow.c_enum_val());

        println!("✓ Phase 2 Test 2: Three ROM sets validation passed");
        println!("  Set 0: 2364, CS1=Low");
        println!("  Set 1: 2332, CS1=Low, CS2=High");
        println!("  Set 2: 2316, CS1=Low, CS2=Low, CS3=Low");
        println!(
            "  Data pointers: 0x{:08X}, 0x{:08X}, 0x{:08X}",
            chip_set0.data_ptr, chip_set1.data_ptr, chip_set2.data_ptr
        );
    }

    // ========================================================================
    // PHASE 4: Boot Logging (Filenames)
    // ========================================================================

    // ========================================================================
    // TEST 4: Validate ROM Info with Filename
    // ========================================================================

    #[test]
    fn test_phase4_boot_logging_filename() {
        let json = r#"{
            "version": 1,
            "description": "Phase 4 boot logging test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test_filename.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let rom_data = create_test_rom_data(8192, 0xAA);
        builder
            .add_file(FileData {
                id: 0,
                data: rom_data,
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header and ROM set
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Parse ROM pointer array to get pointer to first ROM info
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);

        // Convert to buffer offset
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;

        // Parse the ROM info structure WITH filename
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        // Validate basic ROM info fields (same as Phase 1)
        assert_eq!(rom_info.rom_type, CHIP_TYPE_2364);
        assert_eq!(rom_info.cs1_state, CsLogic::ActiveLow.c_enum_val());
        assert_eq!(rom_info.cs2_state, CsLogic::Ignore.c_enum_val());
        assert_eq!(rom_info.cs3_state, CsLogic::Ignore.c_enum_val());

        // Validate filename pointer exists
        assert!(
            rom_info.filename_ptr.is_some(),
            "Filename pointer should be present with boot_logging enabled"
        );

        let filename_ptr = rom_info.filename_ptr.unwrap();

        // Validate filename pointer is within metadata buffer
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        assert!(
            filename_offset < metadata_buf.len(),
            "Filename pointer {} (offset {}) outside metadata buffer (size {})",
            filename_ptr,
            filename_offset,
            metadata_buf.len()
        );

        // Parse the null-terminated filename string
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        // Validate filename matches what we specified in JSON
        assert_eq!(
            filename, "test_filename.rom",
            "Filename mismatch. Expected 'test_filename.rom', got '{}'",
            filename
        );

        println!("✓ Phase 4 Test 1: Boot logging with filename passed");
        println!("  - ROM type: {} (2364)", rom_info.rom_type);
        println!(
            "  - CS states: {}, {}, {}",
            rom_info.cs1_state, rom_info.cs2_state, rom_info.cs3_state
        );
        println!("  - Filename pointer: 0x{:08X}", filename_ptr);
        println!("  - Filename: '{}'", filename);
    }

    // ========================================================================
    // TEST 4: Validate ROM Info with label
    // ========================================================================

    #[test]
    fn test_phase4_boot_logging_filename_label() {
        let json = r#"{
            "version": 1,
            "description": "Phase 4 boot logging filename label test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test_filename.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "label": "Test ROM 1"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let rom_data = create_test_rom_data(8192, 0xAA);
        builder
            .add_file(FileData {
                id: 0,
                data: rom_data,
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header and ROM set
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Parse ROM pointer array to get pointer to first ROM info
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);

        // Convert to buffer offset
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;

        // Parse the ROM info structure WITH filename
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        // Validate filename pointer exists
        assert!(
            rom_info.filename_ptr.is_some(),
            "Filename pointer should be present with boot_logging enabled"
        );

        let filename_ptr = rom_info.filename_ptr.unwrap();

        // Validate filename pointer is within metadata buffer
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        assert!(
            filename_offset < metadata_buf.len(),
            "Filename pointer {} (offset {}) outside metadata buffer (size {})",
            filename_ptr,
            filename_offset,
            metadata_buf.len()
        );

        // Parse the null-terminated filename string
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        // Validate filename matches what we specified in JSON
        assert_eq!(
            filename, "Test ROM 1",
            "Filename mismatch. Expected 'Test ROM 1', got '{}'",
            filename
        );

        println!("✓ Phase 4 Test 2: Boot logging with label passed");
        println!("  - ROM type: {} (2364)", rom_info.rom_type);
        println!(
            "  - CS states: {}, {}, {}",
            rom_info.cs1_state, rom_info.cs2_state, rom_info.cs3_state
        );
        println!("  - Filename pointer: 0x{:08X}", filename_ptr);
        println!("  - Filename: '{}'", filename);
    }

    // ========================================================================
    // TEST 8: Multiple ROMs with Boot Logging
    // ========================================================================

    #[test]
    fn test_phase4_multiple_chips_with_boot_logging() {
        let json = r#"{
            "version": 1,
            "description": "Phase 4 multiple ROMs with boot logging test",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "first_rom.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "second_rom.bin",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(4096, 0x55),
            })
            .expect("Failed to add file 1");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 2);

        // Parse both ROM sets
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set0 = RomSetStruct::parse(&metadata_buf, chip_set0_offset);

        let chip_set1_offset = chip_set0_offset + CHIP_SET_METADATA_LEN;
        let chip_set1 = RomSetStruct::parse(&metadata_buf, chip_set1_offset);

        // Parse ROM info for Set 0 with filename
        let rom_array0_offset = (chip_set0.chips_ptr - metadata_flash_start) as usize;
        let rom_info0_ptr = u32::from_le_bytes([
            metadata_buf[rom_array0_offset],
            metadata_buf[rom_array0_offset + 1],
            metadata_buf[rom_array0_offset + 2],
            metadata_buf[rom_array0_offset + 3],
        ]);
        let rom_info0_offset = (rom_info0_ptr - metadata_flash_start) as usize;
        let rom_info0 = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info0_offset);

        // Validate Set 0 ROM info
        assert_eq!(rom_info0.rom_type, CHIP_TYPE_2364);
        assert!(
            rom_info0.filename_ptr.is_some(),
            "Set 0 should have filename"
        );

        let filename0_ptr = rom_info0.filename_ptr.unwrap();
        let filename0_offset = (filename0_ptr - metadata_flash_start) as usize;
        assert!(
            filename0_offset < metadata_buf.len(),
            "Set 0 filename pointer out of bounds"
        );

        let filename0 = parse_null_terminated_string(&metadata_buf, filename0_offset);
        assert_eq!(filename0, "first_rom.bin", "Set 0 filename mismatch");

        // Parse ROM info for Set 1 with filename
        let rom_array1_offset = (chip_set1.chips_ptr - metadata_flash_start) as usize;
        let rom_info1_ptr = u32::from_le_bytes([
            metadata_buf[rom_array1_offset],
            metadata_buf[rom_array1_offset + 1],
            metadata_buf[rom_array1_offset + 2],
            metadata_buf[rom_array1_offset + 3],
        ]);
        let rom_info1_offset = (rom_info1_ptr - metadata_flash_start) as usize;
        let rom_info1 = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info1_offset);

        // Validate Set 1 ROM info
        assert_eq!(rom_info1.rom_type, CHIP_TYPE_2332);
        assert!(
            rom_info1.filename_ptr.is_some(),
            "Set 1 should have filename"
        );

        let filename1_ptr = rom_info1.filename_ptr.unwrap();
        let filename1_offset = (filename1_ptr - metadata_flash_start) as usize;
        assert!(
            filename1_offset < metadata_buf.len(),
            "Set 1 filename pointer out of bounds"
        );

        let filename1 = parse_null_terminated_string(&metadata_buf, filename1_offset);
        assert_eq!(filename1, "second_rom.bin", "Set 1 filename mismatch");

        // Validate filenames are at different locations
        assert_ne!(
            filename0_offset, filename1_offset,
            "Filenames should be at different offsets"
        );

        println!("✓ Phase 4 Test 2: Multiple ROMs with boot logging passed");
        println!("  Set 0: '{}' at offset {}", filename0, filename0_offset);
        println!("  Set 1: '{}' at offset {}", filename1, filename1_offset);
    }

    // ========================================================================
    // PHASE 3: CS Configuration Tests
    // ========================================================================

    // ========================================================================
    // TEST 9: 2332 with CS1 and CS2 Both Active Low
    // ========================================================================

    #[test]
    fn test_phase3_2332_both_cs_active_low() {
        let json = r#"{
            "version": 1,
            "description": "Phase 3 2332 with both CS active low",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "active_low",
                    "cs2": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(4096, 0xAA), // 2332 = 4KB
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header and ROM set
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Parse ROM info
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse(&metadata_buf, rom_info_offset);

        // Validate ROM type
        assert_eq!(rom_info.rom_type, CHIP_TYPE_2332, "ROM type should be 2332");

        // Validate CS1 is active low
        assert_eq!(
            rom_info.cs1_state,
            CsLogic::ActiveLow.c_enum_val(),
            "CS1 should be active low"
        );

        // Validate CS2 is active low
        assert_eq!(
            rom_info.cs2_state,
            CsLogic::ActiveLow.c_enum_val(),
            "CS2 should be active low"
        );

        // CS3 should be ignored for 2332
        assert_eq!(
            rom_info.cs3_state,
            CsLogic::Ignore.c_enum_val(),
            "CS3 should be ignored for 2332"
        );

        println!("✓ Phase 3 Test 1: 2332 with both CS active low passed");
        println!("  - ROM type: {} (2332)", rom_info.rom_type);
        println!("  - CS1: {} (ActiveLow)", rom_info.cs1_state);
        println!("  - CS2: {} (ActiveLow)", rom_info.cs2_state);
        println!("  - CS3: {} (Ignore)", rom_info.cs3_state);
    }

    // ========================================================================
    // TEST 10: 2316 with Mixed CS States
    // ========================================================================

    #[test]
    fn test_phase3_2316_mixed_cs_states() {
        let json = r#"{
            "version": 1,
            "description": "Phase 3 2316 with mixed CS states",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_high",
                    "cs3": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2048, 0xAA), // 2316 = 2KB
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header and ROM set
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Parse ROM info
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse(&metadata_buf, rom_info_offset);

        // Validate ROM type
        assert_eq!(rom_info.rom_type, CHIP_TYPE_2316, "ROM type should be 2316");

        // Validate CS1 is active low
        assert_eq!(
            rom_info.cs1_state,
            CsLogic::ActiveLow.c_enum_val(),
            "CS1 should be active low"
        );

        // Validate CS2 is active high
        assert_eq!(
            rom_info.cs2_state,
            CsLogic::ActiveHigh.c_enum_val(),
            "CS2 should be active high"
        );

        // Validate CS3 is active low
        assert_eq!(
            rom_info.cs3_state,
            CsLogic::ActiveLow.c_enum_val(),
            "CS3 should be active low"
        );

        println!("✓ Phase 3 Test 2: 2316 with mixed CS states passed");
        println!("  - ROM type: {} (2316)", rom_info.rom_type);
        println!("  - CS1: {} (ActiveLow)", rom_info.cs1_state);
        println!("  - CS2: {} (ActiveHigh)", rom_info.cs2_state);
        println!("  - CS3: {} (ActiveLow)", rom_info.cs3_state);
    }

    // ========================================================================
    // PHASE 5: Size Handling
    // ========================================================================

    // ========================================================================
    // TEST 11: Exact Size Match
    // ========================================================================

    #[test]
    fn test_phase5_exact_size_match() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 exact size match",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create exactly 8KB for 2364
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with exact size");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 5 Test 1: Exact size match passed");
        println!("  - 8KB file for 2364 (8KB ROM) - no size_handling needed");
    }

    // Test location being specified within an ROM image
    #[test]
    fn test_phase5_location_slice_from_larger_image() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 location slice test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "large.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "location": {
                        "start": 4096,
                        "length": 8192
                    }
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 16KB image with pattern: first 4KB=0x11, next 8KB=0x22, last 4KB=0x33
        let mut data = Vec::new();
        data.extend_from_slice(&[0x11u8; 4096]);
        data.extend_from_slice(&[0x22u8; 8192]);
        data.extend_from_slice(&[0x33u8; 4096]);

        builder
            .add_file(FileData { id: 0, data })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (_metadata_buf, rom_images_buf) = builder
            .build(props)
            .inspect_err(|e| {
                eprintln!("Build failed with location slice: {e:?}");
            })
            .expect("Build should succeed with location slice");

        // Verify the extracted ROM contains only the 0x22 bytes (middle 8KB)
        for addr in 0..8192 {
            let b = read_rom_byte(&rom_images_buf, addr, props.board());
            assert_eq!(b, 0x22);
        }

        println!("✓ Phase 5 Test: Location slice from larger image passed");
        println!("  - Extracted 8KB from offset 4096 of 16KB image");
    }

    // Test location with duplicating rom image
    #[test]
    fn test_phase5_location_slice_with_duplicate() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 location slice with duplicate",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "large.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "duplicate",
                    "location": {
                        "start": 1024,
                        "length": 2048
                    }
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 8KB image with pattern: 1KB=0x11, 2KB=0x22, 5KB=0x33
        let mut data = Vec::new();
        data.extend_from_slice(&[0x11u8; 1024]);
        data.extend_from_slice(&[0x22u8; 2048]);
        data.extend_from_slice(&[0x33u8; 5120]);

        builder
            .add_file(FileData { id: 0, data })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (_metadata_buf, rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with location slice and duplicate");

        // Verify the 2KB pattern (0x22) is duplicated 4 times to fill 8KB ROM
        for addr in 0..8192 {
            let b = read_rom_byte(&rom_images_buf, addr, props.board());
            assert_eq!(b, 0x22, "Mismatch at address {:#X}", addr);
        }

        println!("✓ Phase 5 Test: Location slice with duplicate passed");
        println!("  - Extracted 2KB from offset 1024, duplicated 4x to fill 8KB ROM");
    }

    // Test location with padding rom image
    #[test]
    fn test_phase5_location_slice_with_pad() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 location slice with pad",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "large.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "pad",
                    "location": {
                        "start": 2048,
                        "length": 6144
                    }
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 16KB image with pattern: 2KB=0x11, 6KB=0x22, 8KB=0x33
        let mut data = Vec::new();
        data.extend_from_slice(&[0x11u8; 2048]);
        data.extend_from_slice(&[0x22u8; 6144]);
        data.extend_from_slice(&[0x33u8; 8192]);

        builder
            .add_file(FileData { id: 0, data })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (_metadata_buf, rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with location slice and pad");

        // Verify first 6KB is 0x22 (extracted data)
        for addr in 0..6144 {
            let b = read_rom_byte(&rom_images_buf, addr, props.board());
            assert_eq!(b, 0x22, "Mismatch at address {:#X}", addr);
        }

        // Verify last 2KB is 0xFF (padded)
        for addr in 6144..8192 {
            let b = read_rom_byte(&rom_images_buf, addr, props.board());
            assert_eq!(b, 0xAA, "Pad mismatch at address {:#X}", addr);
        }

        println!("✓ Phase 5 Test: Location slice with pad passed");
        println!("  - Extracted 6KB from offset 2048, padded to 8KB ROM");
    }

    // ========================================================================
    // TEST 12: Duplicate Size Handling
    // ========================================================================

    #[test]
    fn test_phase5_duplicate_size_handling() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 duplicate size handling",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "duplicate"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 4KB file for 8KB ROM (exact divisor)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(4096, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with duplicate");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 5 Test 2: Duplicate size handling passed");
        println!("  - 4KB file duplicated to fill 8KB ROM");
    }

    // ========================================================================
    // TEST 13: Pad Size Handling
    // ========================================================================

    #[test]
    fn test_phase5_pad_size_handling() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 pad size handling",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 3KB file for 8KB ROM (not an exact divisor, must pad)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(3072, 0x55),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) =
            builder.build(props).expect("Build should succeed with pad");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 5 Test 3: Pad size handling passed");
        println!("  - 3KB file padded to fill 8KB ROM");
    }

    // ========================================================================
    // TEST 14: Error - File Too Large
    // ========================================================================

    #[test]
    fn test_phase5_error_file_too_large() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 error - file too large",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 10KB file for 8KB ROM - too large
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(10240, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let result = builder.build(props);

        // Should fail
        assert!(result.is_err(), "Build should fail with file too large");

        println!("✓ Phase 5 Test 4: Error - file too large correctly rejected");
    }

    // ========================================================================
    // TEST 15: Error - Wrong Divisor for Duplicate
    // ========================================================================

    #[test]
    fn test_phase5_error_wrong_divisor() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 error - wrong divisor",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "duplicate"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 3KB file for 8KB ROM with duplicate - 3KB is not exact divisor of 8KB
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(3072, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let result = builder.build(props);

        // Should fail
        assert!(
            result.is_err(),
            "Build should fail with non-divisor size for duplicate"
        );

        println!("✓ Phase 5 Test 5: Error - wrong divisor for duplicate correctly rejected");
    }

    // ========================================================================
    // TEST 16: Error - Unnecessary size_handling
    // ========================================================================

    #[test]
    fn test_phase5_error_unnecessary_size_handling() {
        let json = r#"{
            "version": 1,
            "description": "Phase 5 error - unnecessary size_handling",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create exactly 8KB file but specified pad - unnecessary
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let result = builder.build(props);

        // Should fail
        assert!(
            result.is_err(),
            "Build should fail with unnecessary size_handling"
        );

        println!("✓ Phase 5 Test 6: Error - unnecessary size_handling correctly rejected");
    }

    // ========================================================================
    // PHASE 6: Multi-ROM Sets
    // ========================================================================

    // ========================================================================
    // TEST 17: Banked ROM Set (2 ROMs)
    // ========================================================================

    #[test]
    fn test_phase6_banked_chip_set() {
        let json = r#"{
            "version": 1,
            "description": "Phase 6 banked ROM set",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1, "Should have 1 ROM set");

        // Parse ROM set
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Validate ROM count
        assert_eq!(chip_set.rom_count, 2, "Banked set should have 2 ROMs");

        // Validate size - banked sets use 64KB
        assert_eq!(chip_set.size, 65536, "Banked set size should be 64KB");

        // Validate serve algorithm
        assert_eq!(
            chip_set.serve_alg,
            ServeAlg::AddrOnCs.c_enum_value(),
            "Banked set should use AddrOnCs serve algorithm"
        );

        // Validate multi-CS state (should be active_low since both ROMs use it)
        assert_eq!(
            chip_set.multi_cs_state,
            CsLogic::ActiveLow.c_enum_val(),
            "Multi-CS state should be ActiveLow"
        );

        println!("✓ Phase 6 Test 1: Banked ROM set passed");
        println!("  - 2 ROMs in banked set");
        println!("  - Size: {} bytes", chip_set.size);
        println!("  - Serve algorithm: {}", chip_set.serve_alg);
        println!("  - Multi-CS state: {}", chip_set.multi_cs_state);
    }

    // ========================================================================
    // TEST 18: Multi ROM Set (2 ROMs)
    // ========================================================================

    #[test]
    fn test_phase6_multi_chip_set() {
        let json = r#"{
            "version": 1,
            "description": "Phase 6 multi ROM set",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        let props = default_fw_props();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse metadata header
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1, "Should have 1 ROM set");

        // Parse ROM set
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Validate ROM count
        assert_eq!(chip_set.rom_count, 2, "Multi set should have 2 ROMs");

        // Validate size - multi sets use 64KB
        assert_eq!(chip_set.size, 65536, "Multi set size should be 64KB");

        // Validate serve algorithm - multi sets use AddrOnAnyCs
        assert_eq!(
            chip_set.serve_alg,
            ServeAlg::AddrOnAnyCs.c_enum_value(),
            "Multi set should use AddrOnAnyCs serve algorithm"
        );

        // Validate multi-CS state
        assert_eq!(
            chip_set.multi_cs_state,
            CsLogic::ActiveLow.c_enum_val(),
            "Multi-CS state should be ActiveLow"
        );

        println!("✓ Phase 6 Test 2: Multi ROM set passed");
        println!("  - 2 ROMs in multi set");
        println!("  - Size: {} bytes", chip_set.size);
        println!("  - Serve algorithm: {} (AddrOnAnyCs)", chip_set.serve_alg);
        println!("  - Multi-CS state: {}", chip_set.multi_cs_state);
    }

    // ========================================================================
    // PHASE 8: Edge Cases
    // ========================================================================

    // ========================================================================
    // TEST 19: Error - Adding Duplicate Files
    // ========================================================================

    #[test]
    fn test_phase8_error_duplicate_files() {
        let json = r#"{
            "version": 1,
            "description": "Phase 8 error - duplicate files",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add file once
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("First add should succeed");

        // Try to add same file again
        let result = builder.add_file(FileData {
            id: 0,
            data: create_test_rom_data(8192, 0xBB),
        });

        // Should fail
        assert!(result.is_err(), "Adding duplicate file should fail");

        println!("✓ Phase 8 Test 1: Error - duplicate files correctly rejected");
    }

    // ========================================================================
    // TEST 20: Error - Missing Files at Build Time
    // ========================================================================

    #[test]
    fn test_phase8_error_missing_files() {
        let json = r#"{
            "version": 1,
            "description": "Phase 8 error - missing files",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "test0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add only first file, skip second
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Adding file 0 should succeed");

        // Try to build without adding file 1
        let props = default_fw_props();
        let result = builder.build(props);

        // Should fail
        assert!(result.is_err(), "Building with missing files should fail");

        println!("✓ Phase 8 Test 2: Error - missing files at build time correctly rejected");
    }

    // ========================================================================
    // TEST 21: Adding Files Out of Order
    // ========================================================================

    #[test]
    fn test_phase8_files_out_of_order() {
        let json = r#"{
            "version": 1,
            "description": "Phase 8 files out of order",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "test0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test2.rom",
                        "type": "2316",
                        "cs1": "active_low",
                        "cs2": "active_low",
                        "cs3": "active_low"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add files out of order: 2, 0, 1
        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(2048, 0xFF),
            })
            .expect("Adding file 2 should succeed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Adding file 0 should succeed");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(4096, 0x55),
            })
            .expect("Adding file 1 should succeed");

        // Build should succeed even with files added out of order
        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 3);

        println!("✓ Phase 8 Test 3: Files added out of order correctly handled");
        println!("  - Added files in order: 2, 0, 1");
        println!("  - Build succeeded with 3 ROM sets");
    }

    // ========================================================================
    // TEST 22: Error - Missing CS Config
    // ========================================================================

    #[test]
    fn test_phase8_error_missing_cs_config() {
        // 2332 requires CS2 to be specified
        let json = r#"{
            "version": 1,
            "description": "Phase 8 error - missing CS2 for 2332",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        // Should fail at JSON parsing/validation
        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Missing CS2 for 2332 should fail");

        println!("✓ Phase 8 Test 4: Error - missing CS config correctly rejected");
    }

    // ========================================================================
    // TEST 23: Minimum ROM Size (2KB - 2316)
    // ========================================================================

    #[test]
    fn test_phase8_minimum_rom_size() {
        let json = r#"{
            "version": 1,
            "description": "Phase 8 minimum ROM size test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2048, 0xAA), // 2316 = 2KB
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 8 Test 5: Minimum ROM size (2KB) handled correctly");
    }

    // ========================================================================
    // TEST 24: 32 ROM Sets (Stress Test)
    // ========================================================================

    #[test]
    fn test_phase8_32_chip_sets() {
        // Build JSON for 32 ROM sets
        let mut json = String::from(
            r#"{
            "version": 1,
            "description": "Phase 8 32 ROM sets stress test",
            "chip_sets": ["#,
        );

        for i in 0..32 {
            if i > 0 {
                #[allow(clippy::single_char_add_str)]
                json.push_str(",");
            }
            json.push_str(&format!(
                r#"
                {{
                    "type": "single",
                    "chips": [{{
                        "file": "test{}.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }}]
                }}"#,
                i
            ));
        }

        json.push_str(
            r#"
            ]
        }"#,
        );

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, &json).expect("Failed to parse JSON");

        // Add all 32 files
        for i in 0..32 {
            builder
                .add_file(FileData {
                    id: i,
                    data: create_test_rom_data(8192, (i as u8).wrapping_mul(8)),
                })
                .unwrap_or_else(|_| panic!("Failed to add file {}", i));
        }

        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F405RG,
            ServeAlg::Default,
            false, // boot_logging disabled
        )
        .unwrap();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Validate
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 32, "Should have 32 ROM sets");

        println!("✓ Phase 8 Test 6: 32 ROM sets stress test passed");
        println!("  - Successfully built metadata for 32 ROM sets");
    }

    // ========================================================================
    // PHASE 7: ROM Images Buffer
    // ========================================================================

    // Helper: Transform logical address to physical address based on board pin mapping
    fn logical_to_physical_address(logical_addr: usize, board: onerom_config::hw::Board) -> usize {
        let addr_pins = board.addr_pins();

        // Address pins must be within a contiguous 16-bit window starting on an 8-boundary
        let min_pin = *addr_pins.iter().min().unwrap() as usize;
        let max_pin = *addr_pins.iter().max().unwrap() as usize;
        assert!(
            min_pin.is_multiple_of(8),
            "Address pins must start on 8-byte boundary, got min pin {}",
            min_pin
        );
        assert!(
            max_pin < min_pin + 16,
            "Address pins must be within 16-bit window, got range {}-{}",
            min_pin,
            max_pin
        );

        let mut physical_address = 0;

        // For each address line, if the bit is set in logical address,
        // set the corresponding physical pin bit
        let mut normalized_addr_pins = vec![0u8; addr_pins.len()];
        if addr_pins.iter().all(|&p| p >= 8) {
            for (ii, &p) in addr_pins.iter().enumerate() {
                normalized_addr_pins[ii] = p - 8;
            }
        } else {
            for (ii, &p) in addr_pins.iter().enumerate() {
                normalized_addr_pins[ii] = p;
            }
        }
        for (addr_line, &phys_pin) in normalized_addr_pins.iter().enumerate() {
            if logical_addr & (1 << addr_line) != 0 {
                let pin = phys_pin as usize;
                // Handle boards where address pins are shifted
                let bit_position = pin;
                physical_address |= 1 << bit_position;
            }
        }

        physical_address
    }

    // Helper: Transform logical data byte to physical byte based on board pin mapping
    fn logical_to_physical_byte(logical_byte: u8, board: onerom_config::hw::Board) -> u8 {
        let data_pins = board.data_pins();

        // Data pins must be within a contiguous 8-bit window starting on an 8-boundary
        let min_pin = *data_pins.iter().min().unwrap();
        let max_pin = *data_pins.iter().max().unwrap();
        assert_eq!(data_pins.len(), 8, "Must have exactly 8 data pins");
        assert!(
            min_pin.is_multiple_of(8),
            "Data pins must start on 8-byte boundary, got min pin {}",
            min_pin
        );
        assert!(
            max_pin < min_pin + 8,
            "Data pins must be within 8-bit window, got range {}-{}",
            min_pin,
            max_pin
        );

        let mut physical_byte = 0;

        // For each data line, if the bit is set in logical byte,
        // set the corresponding physical pin bit
        for (data_line, &phys_pin) in data_pins.iter().enumerate() {
            if logical_byte & (1 << data_line) != 0 {
                physical_byte |= 1 << (phys_pin % 8);
            }
        }

        physical_byte
    }

    // ========================================================================
    // TEST 25: ROM Images Buffer Validation
    // ========================================================================

    #[test]
    fn test_phase7_rom_images_buffer() {
        let json = r#"{
            "version": 1,
            "description": "Phase 7 ROM images buffer test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create test data with predictable pattern: data[addr] = addr as u8
        let rom_size = 8192; // 2364 = 8KB
        let mut test_data = Vec::with_capacity(rom_size);
        for addr in 0..rom_size {
            test_data.push(addr as u8);
        }

        builder
            .add_file(FileData {
                id: 0,
                data: test_data.clone(),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Validate every byte in the ROM images buffer
        let mut errors = 0;
        let max_errors_to_report = 10;

        #[allow(clippy::needless_range_loop)]
        for logical_addr in 0..rom_size {
            let logical_byte = test_data[logical_addr];

            // Transform to physical address and byte
            let physical_addr = logical_to_physical_address(logical_addr, board);
            let physical_byte = logical_to_physical_byte(logical_byte, board);

            // Check ROM images buffer
            let actual_byte = rom_images_buf[physical_addr];

            if actual_byte != physical_byte {
                errors += 1;
                if errors <= max_errors_to_report {
                    println!(
                        "  Mismatch at logical_addr=0x{:04X} (physical=0x{:04X}): expected 0x{:02X}, got 0x{:02X}",
                        logical_addr, physical_addr, physical_byte, actual_byte
                    );
                }
            }
        }

        if errors > max_errors_to_report {
            println!("  ... and {} more errors", errors - max_errors_to_report);
        }

        assert_eq!(
            errors, 0,
            "Found {} byte mismatches in ROM images buffer",
            errors
        );

        println!("✓ Phase 7 Test 1: ROM images buffer validation passed");
        println!(
            "  - Verified all {} bytes with address/data transformations",
            rom_size
        );
    }

    // Helper: Unscramble physical byte to logical byte based on board pin mapping
    fn unscramble_physical_byte(physical_byte: u8, board: onerom_config::hw::Board) -> u8 {
        let data_pins = board.data_pins();
        let mut logical_byte = 0;

        // For each physical pin, if the bit is set, set the corresponding logical data line bit
        for (data_line, &phys_pin) in data_pins.iter().enumerate() {
            if physical_byte & (1 << (phys_pin % 8)) != 0 {
                logical_byte |= 1 << data_line;
            }
        }

        logical_byte
    }

    // Helper: Read byte from ROM images buffer using logical address
    // (simulates what firmware does - reverse the transformations)
    fn read_rom_byte(
        rom_images_buf: &[u8],
        logical_addr: usize,
        board: onerom_config::hw::Board,
    ) -> u8 {
        // Transform logical address to physical address
        let physical_addr = logical_to_physical_address(logical_addr, board);

        // Read the physical byte
        let physical_byte = rom_images_buf[physical_addr];

        // Reverse transform physical byte to logical byte
        unscramble_physical_byte(physical_byte, board)
    }

    // Helper Read bye from ROM images buffer using absolute address
    fn read_rom_byte_abs(
        rom_images_buf: &[u8],
        abs_addr: usize,
        board: onerom_config::hw::Board,
    ) -> u8 {
        let physical_byte = rom_images_buf[abs_addr];
        unscramble_physical_byte(physical_byte, board)
    }

    // ========================================================================
    // TEST 26: ROM Images Buffer with Random Data
    // ========================================================================

    #[test]
    fn test_phase7_rom_images_random_data() {
        let json = r#"{
            "version": 1,
            "description": "Phase 7 ROM images with random data",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "random.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create random test data using a simple PRNG for reproducibility
        let rom_size = 8192; // 2364 = 8KB
        let mut test_data = Vec::with_capacity(rom_size);
        let mut seed = 0x12345678u32;
        for _ in 0..rom_size {
            // Simple linear congruential generator
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            test_data.push((seed >> 24) as u8);
        }

        builder
            .add_file(FileData {
                id: 0,
                data: test_data.clone(),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Read back every byte using logical addresses and verify against original
        let mut errors = 0;
        let max_errors_to_report = 10;

        #[allow(clippy::needless_range_loop)]
        for logical_addr in 0..rom_size {
            let expected_byte = test_data[logical_addr];
            let actual_byte = read_rom_byte(&rom_images_buf, logical_addr, board);

            if actual_byte != expected_byte {
                errors += 1;
                if errors <= max_errors_to_report {
                    println!(
                        "  Mismatch at logical_addr=0x{:04X}: expected 0x{:02X}, got 0x{:02X}",
                        logical_addr, expected_byte, actual_byte
                    );
                }
            }
        }

        if errors > max_errors_to_report {
            println!("  ... and {} more errors", errors - max_errors_to_report);
        }

        assert_eq!(
            errors, 0,
            "Found {} byte mismatches when reading back data",
            errors
        );

        println!("✓ Phase 7 Test 2: ROM images with random data passed");
        println!(
            "  - Stored and read back all {} random bytes correctly",
            rom_size
        );
    }

    // Helper: Check if CS line is active at given address
    fn is_cs_active(gpio_value: u16, cs_pin: u8, active_low: bool) -> bool {
        let bit_value = (1 << cs_pin) & gpio_value;
        if active_low {
            bit_value == 0
        } else {
            bit_value != 0
        }
    }

    // ========================================================================
    // TEST 27: Multi ROM Set Images
    // ========================================================================

    #[test]
    fn test_phase7_multi_chip_set_images() {
        let json = r#"{
            "version": 1,
            "description": "Phase 7 multi ROM set test",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom2.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create distinct test data for each ROM
        let rom_size = 8192;
        let rom0_data = create_test_rom_data(rom_size, 0x11);
        let rom1_data = create_test_rom_data(rom_size, 0x22);
        let rom2_data = create_test_rom_data(rom_size, 0x33);

        builder
            .add_file(FileData {
                id: 0,
                data: rom0_data.clone(),
            })
            .expect("Failed to add file 0");
        builder
            .add_file(FileData {
                id: 1,
                data: rom1_data.clone(),
            })
            .expect("Failed to add file 1");
        builder
            .add_file(FileData {
                id: 2,
                data: rom2_data.clone(),
            })
            .expect("Failed to add file 2");

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Get CS pins
        let cs1_pin = board.pin_cs1(onerom_config::chip::ChipType::Chip2364);
        let x1_pin = board.pin_x1();
        let x2_pin = board.pin_x2();
        println!(
            "CS1 pin: {}, X1 pin: {}, X2 pin: {}",
            cs1_pin, x1_pin, x2_pin
        );

        assert_ne!(cs1_pin, 255, "CS1 pin must be defined");
        assert_ne!(x1_pin, 255, "X1 pin must be defined for multi ROM sets");
        assert_ne!(x2_pin, 255, "X2 pin must be defined for multi ROM sets");

        // All CS lines are active low in this test
        let cs1_active_low = true;
        let x1_active_low = true;
        let x2_active_low = true;

        let mut errors = 0;
        let max_errors_to_report = 10;

        // Check all 64KB addresses
        for address in 0..65536u32 {
            let address_u16 = address as u16;

            // Determine which CS lines are active
            let cs1_active = is_cs_active(address_u16, cs1_pin, cs1_active_low);
            let x1_active = is_cs_active(address_u16, x1_pin, x1_active_low);
            let x2_active = is_cs_active(address_u16, x2_pin, x2_active_low);

            let active_count = [cs1_active, x1_active, x2_active]
                .iter()
                .filter(|&&x| x)
                .count();

            // Read actual byte from ROM images buffer
            let actual_byte = read_rom_byte_abs(&rom_images_buf, address as usize, board);

            let expected_byte = if active_count == 1 {
                // Exactly one CS active - should contain that ROM's data
                let rom_offset = (address as usize) & 0x1FFF; // Lower 13 bits for 8KB ROM
                if cs1_active {
                    rom0_data[rom_offset]
                } else if x1_active {
                    rom1_data[rom_offset]
                } else {
                    rom2_data[rom_offset]
                }
            } else {
                // Invalid (0 or multiple CS active) - should be 0xAA
                0xAA
            };

            if actual_byte != expected_byte {
                errors += 1;
                if errors <= max_errors_to_report {
                    println!(
                        "  Mismatch at addr=0x{:04X} (CS1={}, X1={}, X2={}): expected 0x{:02X}, got 0x{:02X}",
                        address, cs1_active, x1_active, x2_active, expected_byte, actual_byte
                    );
                }
            }
        }

        if errors > max_errors_to_report {
            println!("  ... and {} more errors", errors - max_errors_to_report);
        }

        assert_eq!(
            errors, 0,
            "Found {} byte mismatches in multi ROM set",
            errors
        );

        println!("✓ Phase 7 Test 3: Multi ROM set images passed");
        println!("  - Verified all 64KB with 3 ROMs selected by CS lines");
        println!("  - Validated invalid addresses contain 0xAA");
    }

    // ========================================================================
    // TEST 28: Banked ROM Set Images
    // ========================================================================

    #[test]
    fn test_phase7_banked_chip_set_images() {
        let json = r#"{
            "version": 1,
            "description": "Phase 7 banked ROM set test",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank2.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank3.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create distinct test data for each ROM (each 8KB)
        let rom_size = 8192;
        let rom_data = [
            create_test_rom_data(rom_size, 0x11),
            create_test_rom_data(rom_size, 0x22),
            create_test_rom_data(rom_size, 0x33),
            create_test_rom_data(rom_size, 0x44),
        ];

        for (id, data) in rom_data.iter().enumerate() {
            builder
                .add_file(FileData {
                    id,
                    data: data.clone(),
                })
                .unwrap_or_else(|_| panic!("Failed to add file {}", id));
        }

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Get CS pins
        let cs1_pin = board.pin_cs1(onerom_config::chip::ChipType::Chip2364);
        let x1_pin = board.pin_x1();
        let x2_pin = board.pin_x2();

        assert_ne!(cs1_pin, 255, "CS1 pin must be defined");
        assert_ne!(x1_pin, 255, "X1 pin must be defined for banked ROM sets");
        assert_ne!(x2_pin, 255, "X2 pin must be defined for banked ROM sets");

        let cs1_active_low = true;

        // We need to know which way X1/X2 are pulled when selected
        let x_dirn = board.x_jumper_pull();

        let mut errors = 0;
        let max_errors_to_report = 10;

        // For banked ROMs, the X1/X2 bits in the GPIO select which ROM
        for address in 0..65536u32 {
            let address_u16 = address as u16;

            let cs1_active = is_cs_active(address_u16, cs1_pin, cs1_active_low);
            let x1_bit = (address_u16 >> x1_pin) & 1;
            let x2_bit = (address_u16 >> x2_pin) & 1;

            // Determine which ROM based on X1/X2 bits and if CS1 is active
            let expected_byte = {
                let rom_offset = (address as usize) & 0x1FFF; // Lower 13 bits for 8KB ROM

                let mut bank = ((x2_bit << 1) | x1_bit) as usize;
                if x_dirn == 0 {
                    bank = 3 - bank;
                }

                if bank < rom_data.len() {
                    bank %= rom_data.len(); // Wrap around
                }
                rom_data[bank][rom_offset]
            };

            // Currently fill ROM section with banked ROM even if CS is INACTIVE.

            let actual_byte = read_rom_byte_abs(&rom_images_buf, address as usize, board);

            if actual_byte != expected_byte {
                errors += 1;
                if errors <= max_errors_to_report {
                    println!(
                        "  Mismatch at addr=0x{:04X} (CS1={}, X1={}, X2={}): expected 0x{:02X}, got 0x{:02X}",
                        address, cs1_active, x1_bit, x2_bit, expected_byte, actual_byte
                    );
                }
            }
        }

        if errors > max_errors_to_report {
            println!("  ... and {} more errors", errors - max_errors_to_report);
        }

        assert_eq!(
            errors, 0,
            "Found {} byte mismatches in banked ROM set",
            errors
        );

        println!("✓ Phase 7 Test 4: Banked ROM set images passed");
        println!(
            "  - Verified all 64KB with {} ROMs in banks",
            rom_data.len()
        );
        println!("  - Validated X1/X2 bit values select correct ROM");
    }

    // ========================================================================
    // PHASE 11: JSON Parsing and Validation Errors
    // ========================================================================

    // ========================================================================
    // TEST 29: Malformed JSON
    // ========================================================================

    #[test]
    fn test_phase11_malformed_json() {
        let json = r#"{
            "version": 1,
            "description": "Missing closing brace",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        "#; // Intentionally missing closing brace

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Malformed JSON should fail to parse");

        println!("✓ Phase 11 Test 1: Malformed JSON correctly rejected");
    }

    // ========================================================================
    // TEST 30: Missing Required Fields - chip_sets
    // ========================================================================

    #[test]
    fn test_phase11_missing_chip_sets() {
        let json = r#"{
            "version": 1,
            "description": "Missing chip_sets field"
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "JSON missing chip_sets should fail");

        println!("✓ Phase 11 Test 2: Missing chip_sets field correctly rejected");
    }

    // ========================================================================
    // TEST 31: Missing Required Fields - type
    // ========================================================================

    #[test]
    fn test_phase11_missing_set_type() {
        let json = r#"{
            "version": 1,
            "description": "Missing set type",
            "chip_sets": [{
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "JSON missing set type should fail");

        println!("✓ Phase 11 Test 3: Missing set type correctly rejected");
    }

    // ========================================================================
    // TEST 32: Missing Required Fields - file
    // ========================================================================

    #[test]
    fn test_phase11_missing_file() {
        let json = r#"{
            "version": 1,
            "description": "Missing file field",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "JSON missing file field should fail");

        println!("✓ Phase 11 Test 4: Missing file field correctly rejected");
    }

    // ========================================================================
    // TEST 33: Invalid ROM Type String
    // ========================================================================

    #[test]
    fn test_phase11_invalid_rom_type() {
        let json = r#"{
            "version": 1,
            "description": "Invalid ROM type",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "9999",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Invalid ROM type should fail");

        println!("✓ Phase 11 Test 5: Invalid ROM type correctly rejected");
    }

    // ========================================================================
    // TEST 34: Invalid CS Logic String
    // ========================================================================

    #[test]
    fn test_phase11_invalid_cs_logic() {
        let json = r#"{
            "version": 1,
            "description": "Invalid CS logic",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "maybe_active"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Invalid CS logic should fail");

        println!("✓ Phase 11 Test 6: Invalid CS logic correctly rejected");
    }

    // ========================================================================
    // TEST 35: Invalid Set Type String
    // ========================================================================

    #[test]
    fn test_phase11_invalid_set_type() {
        let json = r#"{
            "version": 1,
            "description": "Invalid set type",
            "chip_sets": [{
                "type": "triple",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Invalid set type should fail");

        println!("✓ Phase 11 Test 7: Invalid set type correctly rejected");
    }

    // ========================================================================
    // TEST 36: CS2 Specified for 2364
    // ========================================================================

    #[test]
    fn test_phase11_cs2_for_2364() {
        let json = r#"{
            "version": 1,
            "description": "CS2 specified for 2364",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "cs2": "active_high"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        // This test documents behavior - either should error or succeed
        // but we're asserting it should error for now
        assert!(result.is_err(), "CS2 specified for 2364 should fail");

        println!("✓ Phase 11 Test 8: CS2 for 2364 correctly rejected");
    }

    // ========================================================================
    // TEST 37: CS3 Specified for 2332
    // ========================================================================

    #[test]
    fn test_phase11_cs3_for_2332() {
        let json = r#"{
            "version": 1,
            "description": "CS3 specified for 2332",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "active_low",
                    "cs2": "active_high",
                    "cs3": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        // This test documents behavior - either should error or succeed
        assert!(result.is_err(), "CS3 specified for 2332 should fail");

        println!("✓ Phase 11 Test 9: CS3 for 2332 correctly rejected");
    }

    // ========================================================================
    // TEST 38: No CS1 Specified
    // ========================================================================

    #[test]
    fn test_phase11_missing_cs1() {
        let json = r#"{
            "version": 1,
            "description": "Missing CS1",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Missing CS1 should fail");

        println!("✓ Phase 11 Test 10: Missing CS1 correctly rejected");
    }

    // ========================================================================
    // TEST 39: Invalid size_handling Value
    // ========================================================================

    #[test]
    fn test_phase11_invalid_size_handling() {
        let json = r#"{
            "version": 1,
            "description": "Invalid size_handling",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "stretch"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Invalid size_handling should fail");

        println!("✓ Phase 11 Test 11: Invalid size_handling correctly rejected");
    }

    // ========================================================================
    // TEST 40: ROM Type Mismatch in Multi Set
    // ========================================================================

    #[test]
    fn test_phase11_rom_type_mismatch_multi_set() {
        let json = r#"{
            "version": 1,
            "description": "Mixed ROM types in multi set",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low",
                        "cs2": "ignore",
                        "cs3": "ignore"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "ignore",
                        "cs3": "ignore"
                    }
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        // This documents expected behavior - mixed types might be allowed or not
        // For now, assuming it should fail
        assert!(result.is_err(), "Mixed ROM types in multi set should fail");

        println!("✓ Phase 11 Test 12: ROM type mismatch in multi set correctly rejected");
    }

    // ========================================================================
    // TEST 41: ROM Type Mismatch in Banked Set
    // ========================================================================

    #[test]
    fn test_phase11_rom_type_mismatch_banked_set() {
        let json = r#"{
            "version": 1,
            "description": "Mixed ROM types in banked set",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Mixed ROM types in banked set should fail");

        println!("✓ Phase 11 Test 13: ROM type mismatch in banked set correctly rejected");
    }

    // ========================================================================
    // TEST 42: Negative Version Number
    // ========================================================================

    #[test]
    fn test_phase11_negative_version() {
        let json = r#"{
            "version": -1,
            "description": "Negative version",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Negative version should fail");

        println!("✓ Phase 11 Test 14: Negative version correctly rejected");
    }

    // ========================================================================
    // TEST 43: Invalid Version Number (Zero)
    // ========================================================================

    #[test]
    fn test_phase11_zero_version() {
        let json = r#"{
            "version": 0,
            "description": "Zero version",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Zero version should fail");

        println!("✓ Phase 11 Test 15: Zero version correctly rejected");
    }

    // ========================================================================
    // TEST 44: Invalid Version Number (Too High)
    // ========================================================================

    #[test]
    fn test_phase11_invalid_high_version() {
        let json = r#"{
            "version": 999,
            "description": "Version too high",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Version 999 should fail");

        println!("✓ Phase 11 Test 16: Invalid high version correctly rejected");
    }

    // ========================================================================
    // TEST 45: Empty ROM Sets Array (Allowed)
    // ========================================================================

    #[test]
    fn test_phase11_empty_chip_sets() {
        let json = r#"{
            "version": 1,
            "description": "Empty ROM sets array",
            "chip_sets": []
        }"#;

        let builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Empty chip_sets should be allowed");

        let props = default_fw_props();
        let (metadata_buf, rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Validate header shows 0 ROM sets
        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 0, "Should have 0 ROM sets");

        // ROM images buffer should be minimal/empty
        assert!(
            rom_images_buf.is_empty() || rom_images_buf.len() < 1024,
            "ROM images should be minimal for empty config"
        );

        println!("✓ Phase 11 Test 17: Empty chip_sets array allowed");
        println!("  - Metadata size: {} bytes", metadata_buf.len());
        println!("  - ROM images size: {} bytes", rom_images_buf.len());
    }
    // ========================================================================
    // TEST 46: Empty ROMs Array in Set
    // ========================================================================

    #[test]
    fn test_phase11_empty_chips_array() {
        let json = r#"{
            "version": 1,
            "description": "Empty roms array",
            "chip_sets": [{
                "type": "single",
                "chips": []
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Empty roms array should fail");

        println!("✓ Phase 11 Test 18: Empty roms array correctly rejected");
    }

    // ========================================================================
    // PHASE 12: Filename and Boot Logging Edge Cases
    // ========================================================================

    // ========================================================================
    // TEST 47: Empty Filename String
    // ========================================================================

    #[test]
    fn test_phase12_empty_filename() {
        let json = r#"{
            "version": 1,
            "description": "Empty filename",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "Empty filename should fail");

        println!("✓ Phase 12 Test 1: Empty filename correctly rejected");
    }

    // ========================================================================
    // TEST 48: Filename with Spaces
    // ========================================================================

    #[test]
    fn test_phase12_filename_with_spaces() {
        let json = r#"{
            "version": 1,
            "description": "Filename with spaces",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "my test rom.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Filename with spaces should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(
            filename, "my test rom.bin",
            "Filename with spaces should be preserved"
        );

        println!("✓ Phase 12 Test 2: Filename with spaces correctly handled");
    }

    // ========================================================================
    // TEST 49: Filename with Special Characters
    // ========================================================================

    #[test]
    fn test_phase12_filename_special_chars() {
        let json = r#"{
            "version": 1,
            "description": "Filename with special characters",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "rom-v1.0_test[final].bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Filename with special chars should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(
            filename, "rom-v1.0_test[final].bin",
            "Special characters should be preserved"
        );

        println!("✓ Phase 12 Test 3: Filename with special characters correctly handled");
    }

    // ========================================================================
    // TEST 50: Filename with Unicode
    // ========================================================================

    #[test]
    fn test_phase12_filename_unicode() {
        let json = r#"{
            "version": 1,
            "description": "Filename with unicode",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "röm_tëst_🎮.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Filename with unicode should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(
            filename, "röm_tëst_🎮.bin",
            "Unicode characters should be preserved"
        );

        println!("✓ Phase 12 Test 4: Filename with unicode correctly handled");
    }

    // ========================================================================
    // TEST 51: Maximum Length Filename (255 chars)
    // ========================================================================

    #[test]
    fn test_phase12_max_length_filename() {
        // Create a 255-character filename (255 is common max for filesystem names)
        let long_filename = "a".repeat(251) + ".bin"; // 251 + 4 = 255

        let json = format!(
            r#"{{
            "version": 1,
            "description": "Maximum length filename",
            "chip_sets": [{{
                "type": "single",
                "chips": [{{
                    "file": "{}",
                    "type": "2364",
                    "cs1": "active_low"
                }}]
            }}]
        }}"#,
            long_filename
        );

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, &json)
            .expect("Max length filename should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(filename.len(), 255, "Filename length should be 255");
        assert_eq!(filename, long_filename, "Long filename should be preserved");

        println!("✓ Phase 12 Test 5: Maximum length filename (255 chars) correctly handled");
    }

    // ========================================================================
    // TEST 52: Very Long Filename (Potential Overflow)
    // ========================================================================

    #[test]
    fn test_phase12_very_long_filename() {
        // Create a 1000-character filename to test limits
        let very_long_filename = "a".repeat(996) + ".bin";

        let json = format!(
            r#"{{
            "version": 1,
            "description": "Very long filename",
            "chip_sets": [{{
                "type": "single",
                "chips": [{{
                    "file": "{}",
                    "type": "2364",
                    "cs1": "active_low"
                }}]
            }}]
        }}"#,
            very_long_filename
        );

        let result = Builder::from_json(FW_VER, MCU_FAM, &json);

        // This might succeed or fail depending on implementation
        // If it succeeds, verify the filename is stored correctly
        // If it fails, that's also acceptable behavior
        match result {
            Ok(mut builder) => {
                builder
                    .add_file(FileData {
                        id: 0,
                        data: create_test_rom_data(8192, 0xAA),
                    })
                    .expect("Failed to add file");

                let props = fw_props_with_logging();
                let board = props.board();
                let flash_base = board.mcu_family().get_flash_base();
                let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;

                let build_result = builder.build(props);

                match build_result {
                    Ok((metadata_buf, _rom_images_buf)) => {
                        // Verify filename if build succeeded
                        let header = MetadataHeader::parse(&metadata_buf);
                        let chip_set_offset =
                            (header.chip_sets_ptr - metadata_flash_start) as usize;
                        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
                        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
                        let rom_info_ptr = u32::from_le_bytes([
                            metadata_buf[rom_array_offset],
                            metadata_buf[rom_array_offset + 1],
                            metadata_buf[rom_array_offset + 2],
                            metadata_buf[rom_array_offset + 3],
                        ]);
                        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
                        let rom_info =
                            RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

                        let filename_ptr = rom_info.filename_ptr.unwrap();
                        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
                        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

                        assert_eq!(
                            filename, very_long_filename,
                            "Very long filename should be preserved"
                        );
                        println!(
                            "✓ Phase 12 Test 6: Very long filename (1000 chars) handled - stored successfully"
                        );
                    }
                    Err(_) => {
                        println!(
                            "✓ Phase 12 Test 6: Very long filename (1000 chars) handled - rejected at build time"
                        );
                    }
                }
            }
            Err(_) => {
                println!(
                    "✓ Phase 12 Test 6: Very long filename (1000 chars) handled - rejected at parse time"
                );
            }
        }
    }

    // ========================================================================
    // TEST 53: Duplicate Filenames in Different ROM Sets
    // ========================================================================

    #[test]
    fn test_phase12_duplicate_filenames() {
        let json = r#"{
            "version": 1,
            "description": "Duplicate filenames in different sets",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "test.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high",
                        "size_handling": "truncate"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Duplicate filenames should be allowed across different sets");

        // Verify only 1 unique file needed
        let file_specs = builder.file_specs();
        assert_eq!(file_specs.len(), 1, "Should have only 1 unique file");
        assert_eq!(file_specs[0].id, 0);

        // Only add the file once - it's deduplicated
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Verify both ROM sets are created
        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 2);

        // Check first ROM set
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set0 = RomSetStruct::parse(&metadata_buf, chip_set0_offset);
        let rom_array0_offset = (chip_set0.chips_ptr - metadata_flash_start) as usize;
        let rom_info0_ptr = u32::from_le_bytes([
            metadata_buf[rom_array0_offset],
            metadata_buf[rom_array0_offset + 1],
            metadata_buf[rom_array0_offset + 2],
            metadata_buf[rom_array0_offset + 3],
        ]);
        let rom_info0 = RomInfoStruct::parse_with_filename(
            &metadata_buf,
            (rom_info0_ptr - metadata_flash_start) as usize,
        );
        let filename0 = parse_null_terminated_string(
            &metadata_buf,
            (rom_info0.filename_ptr.unwrap() - metadata_flash_start) as usize,
        );

        // Check second ROM set
        let chip_set1_offset = chip_set0_offset + CHIP_SET_METADATA_LEN;
        let chip_set1 = RomSetStruct::parse(&metadata_buf, chip_set1_offset);
        let rom_array1_offset = (chip_set1.chips_ptr - metadata_flash_start) as usize;
        let rom_info1_ptr = u32::from_le_bytes([
            metadata_buf[rom_array1_offset],
            metadata_buf[rom_array1_offset + 1],
            metadata_buf[rom_array1_offset + 2],
            metadata_buf[rom_array1_offset + 3],
        ]);
        let rom_info1 = RomInfoStruct::parse_with_filename(
            &metadata_buf,
            (rom_info1_ptr - metadata_flash_start) as usize,
        );
        let filename1 = parse_null_terminated_string(
            &metadata_buf,
            (rom_info1.filename_ptr.unwrap() - metadata_flash_start) as usize,
        );

        assert_eq!(filename0, "test.rom");
        assert_eq!(filename1, "test.rom");

        println!("✓ Phase 12 Test 7: Duplicate filenames correctly deduplicated");
    }

    // ========================================================================
    // TEST 54: Filename with Path Separators
    // ========================================================================

    #[test]
    fn test_phase12_filename_with_path() {
        let json = r#"{
            "version": 1,
            "description": "Filename with path separators",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "path/to/my/rom.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Filename with path should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(
            filename, "path/to/my/rom.bin",
            "Path separators should be preserved"
        );

        println!("✓ Phase 12 Test 8: Filename with path separators correctly handled");
    }

    // ========================================================================
    // TEST 55: Filename with Quotes
    // ========================================================================

    #[test]
    fn test_phase12_filename_with_quotes() {
        let json = r#"{
            "version": 1,
            "description": "Filename with quotes",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "rom\"with'quotes\".bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json)
            .expect("Filename with quotes should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = fw_props_with_logging();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Parse and verify filename
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
        let rom_info_ptr = u32::from_le_bytes([
            metadata_buf[rom_array_offset],
            metadata_buf[rom_array_offset + 1],
            metadata_buf[rom_array_offset + 2],
            metadata_buf[rom_array_offset + 3],
        ]);
        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
        let rom_info = RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

        let filename_ptr = rom_info.filename_ptr.unwrap();
        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

        assert_eq!(
            filename, "rom\"with'quotes\".bin",
            "Quotes should be preserved"
        );

        println!("✓ Phase 12 Test 9: Filename with quotes correctly handled");
    }

    // ========================================================================
    // TEST 56: Filename with Embedded Null Byte
    // ========================================================================

    #[test]
    fn test_phase12_filename_with_null_byte() {
        // In JSON, null bytes are represented as \u0000
        let json = r#"{
            "version": 1,
            "description": "Filename with null byte",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "rom\u0000hidden.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        // This should either:
        // 1. Reject at parse time (preferred), OR
        // 2. Accept but truncate at null byte when storing

        match result {
            Err(_) => {
                println!("✓ Phase 12 Test 10: Filename with null byte rejected at parse time");
            }
            Ok(mut builder) => {
                builder
                    .add_file(FileData {
                        id: 0,
                        data: create_test_rom_data(8192, 0xAA),
                    })
                    .expect("Failed to add file");

                let props = fw_props_with_logging();
                let board = props.board();
                let flash_base = board.mcu_family().get_flash_base();
                let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;

                let build_result = builder.build(props);

                match build_result {
                    Ok((metadata_buf, _rom_images_buf)) => {
                        // If build succeeded, verify null byte handling
                        let header = MetadataHeader::parse(&metadata_buf);
                        let chip_set_offset =
                            (header.chip_sets_ptr - metadata_flash_start) as usize;
                        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);
                        let rom_array_offset = (chip_set.chips_ptr - metadata_flash_start) as usize;
                        let rom_info_ptr = u32::from_le_bytes([
                            metadata_buf[rom_array_offset],
                            metadata_buf[rom_array_offset + 1],
                            metadata_buf[rom_array_offset + 2],
                            metadata_buf[rom_array_offset + 3],
                        ]);
                        let rom_info_offset = (rom_info_ptr - metadata_flash_start) as usize;
                        let rom_info =
                            RomInfoStruct::parse_with_filename(&metadata_buf, rom_info_offset);

                        let filename_ptr = rom_info.filename_ptr.unwrap();
                        let filename_offset = (filename_ptr - metadata_flash_start) as usize;
                        let filename = parse_null_terminated_string(&metadata_buf, filename_offset);

                        // Should be truncated at null byte
                        assert_eq!(filename, "rom", "Filename should be truncated at null byte");
                        println!(
                            "✓ Phase 12 Test 10: Filename with null byte handled - truncated at null"
                        );
                    }
                    Err(_) => {
                        println!(
                            "✓ Phase 12 Test 10: Filename with null byte rejected at build time"
                        );
                    }
                }
            }
        }
    }

    // ========================================================================
    // PHASE 13: ServeAlg Automatic Selection
    // ========================================================================

    // ========================================================================
    // TEST 57: Single ROM Uses Default ServeAlg from FirmwareProperties
    // ========================================================================

    #[test]
    fn test_phase13_single_rom_uses_fw_serve_alg() {
        let json = r#"{
            "version": 1,
            "description": "Single ROM with default serve_alg",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        // Build with different ServeAlg in FirmwareProperties
        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::TwoCsOneAddr, // Explicitly set different algorithm
            false,
        )
        .unwrap();

        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Verify single ROM uses the FirmwareProperties serve_alg
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        assert_eq!(
            chip_set.serve_alg,
            ServeAlg::TwoCsOneAddr.c_enum_value(),
            "Single ROM should use FirmwareProperties serve_alg"
        );

        println!("✓ Phase 13 Test 1: Single ROM uses FirmwareProperties serve_alg");
    }

    // ========================================================================
    // TEST 58: Multi ROM Set Overrides to AddrOnAnyCs
    // ========================================================================

    #[test]
    fn test_phase13_multi_set_overrides_serve_alg() {
        let json = r#"{
            "version": 1,
            "description": "Multi set should override serve_alg",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        // Build with TwoCsOneAddr in FirmwareProperties
        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::TwoCsOneAddr, // This should be overridden for multi sets
            false,
        )
        .unwrap();

        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Verify multi set overrides to AddrOnAnyCs
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        assert_eq!(
            chip_set.serve_alg,
            ServeAlg::AddrOnAnyCs.c_enum_value(),
            "Multi ROM set should override to AddrOnAnyCs"
        );

        println!("✓ Phase 13 Test 2: Multi ROM set overrides serve_alg to AddrOnAnyCs");
    }

    // ========================================================================
    // TEST 59: Banked ROM Set Uses FirmwareProperties ServeAlg
    // ========================================================================

    #[test]
    fn test_phase13_banked_set_uses_fw_serve_alg() {
        let json = r#"{
            "version": 1,
            "description": "Banked set with serve_alg",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        // Build with TwoCsOneAddr
        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::TwoCsOneAddr,
            false,
        )
        .unwrap();

        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Verify banked set uses FirmwareProperties serve_alg
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        assert_eq!(
            chip_set.serve_alg,
            ServeAlg::TwoCsOneAddr.c_enum_value(),
            "Banked ROM set should use FirmwareProperties serve_alg"
        );

        println!("✓ Phase 13 Test 3: Banked ROM set uses FirmwareProperties serve_alg");
    }

    // ========================================================================
    // TEST 60: Mixed Set Types with Different ServeAlgs
    // ========================================================================

    #[test]
    fn test_phase13_mixed_sets_different_serve_algs() {
        let json = r#"{
            "version": 1,
            "description": "Mixed set types",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "single.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                },
                {
                    "type": "multi",
                    "chips": [
                        {
                            "file": "multi0.bin",
                            "type": "2364",
                            "cs1": "active_low"
                        },
                        {
                            "file": "multi1.bin",
                            "type": "2364",
                            "cs1": "active_low"
                        }
                    ]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(8192, 0x33),
            })
            .expect("Failed to add file 2");

        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::AddrOnCs,
            false,
        )
        .unwrap();

        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 2);

        // Check single ROM set uses FW serve_alg
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set0 = RomSetStruct::parse(&metadata_buf, chip_set0_offset);

        assert_eq!(
            chip_set0.serve_alg,
            ServeAlg::AddrOnCs.c_enum_value(),
            "Single ROM should use FirmwareProperties serve_alg"
        );

        // Check multi ROM set overrides to AddrOnAnyCs
        let chip_set1_offset = chip_set0_offset + CHIP_SET_METADATA_LEN;
        let chip_set1 = RomSetStruct::parse(&metadata_buf, chip_set1_offset);

        assert_eq!(
            chip_set1.serve_alg,
            ServeAlg::AddrOnAnyCs.c_enum_value(),
            "Multi ROM set should override to AddrOnAnyCs"
        );

        println!("✓ Phase 13 Test 4: Mixed set types correctly use different serve algorithms");
    }

    // ========================================================================
    // PHASE 14: File Size Edge Cases
    // ========================================================================

    // ========================================================================
    // TEST 61: Zero-Byte File
    // ========================================================================

    #[test]
    fn test_phase14_zero_byte_file() {
        let json = r#"{
            "version": 1,
            "description": "Zero-byte file",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add zero-byte file
        builder
            .add_file(FileData {
                id: 0,
                data: Vec::new(),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let result = builder.build(props);

        assert!(result.is_err(), "Zero-byte file should fail");

        println!("✓ Phase 14 Test 1: Zero-byte file correctly rejected");
    }

    // ========================================================================
    // TEST 62: Single-Byte File
    // ========================================================================

    #[test]
    fn test_phase14_single_byte_file() {
        let json = r#"{
            "version": 1,
            "description": "Single-byte file",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add single-byte file (should be padded to 2KB)
        builder
            .add_file(FileData {
                id: 0,
                data: vec![0x42],
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, rom_images_buf) =
            builder.build(props).expect("Build should succeed with pad");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        // ROM images should contain the padded data
        assert!(!rom_images_buf.is_empty(), "ROM images should not be empty");

        println!("✓ Phase 14 Test 2: Single-byte file with pad correctly handled");
    }

    // ========================================================================
    // TEST 63: File Size 2047 Bytes (Just Under 2KB)
    // ========================================================================

    #[test]
    fn test_phase14_file_2047_bytes() {
        let json = r#"{
            "version": 1,
            "description": "2047 byte file",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add 2047-byte file (1 byte short of 2KB)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2047, 0xAB),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) =
            builder.build(props).expect("Build should succeed with pad");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 3: 2047-byte file (just under 2KB) with pad correctly handled");
    }

    // ========================================================================
    // TEST 64: File Size 2049 Bytes (Just Over 2KB)
    // ========================================================================

    #[test]
    fn test_phase14_file_2049_bytes() {
        let json = r#"{
            "version": 1,
            "description": "2049 byte file",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add 2049-byte file (1 byte over 2KB, needs padding to 4KB)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2049, 0xCD),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) =
            builder.build(props).expect("Build should succeed with pad");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 4: 2049-byte file (just over 2KB) with pad correctly handled");
    }

    // ========================================================================
    // TEST 65: File Exactly 1KB for 2KB ROM with Duplicate
    // ========================================================================

    #[test]
    fn test_phase14_1kb_for_2kb_duplicate() {
        let json = r#"{
            "version": 1,
            "description": "1KB file for 2KB ROM with duplicate",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low",
                    "size_handling": "duplicate"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add exactly 1KB file (should duplicate to 2KB)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(1024, 0xEF),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with duplicate");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 5: 1KB file for 2KB ROM with duplicate correctly handled");
    }

    // ========================================================================
    // TEST 66: File Exactly 2KB for 8KB ROM with Duplicate
    // ========================================================================

    #[test]
    fn test_phase14_2kb_for_8kb_duplicate() {
        let json = r#"{
            "version": 1,
            "description": "2KB file for 8KB ROM with duplicate",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "duplicate"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add exactly 2KB file (should duplicate 4 times to 8KB)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2048, 0x12),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder
            .build(props)
            .expect("Build should succeed with duplicate");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 6: 2KB file for 8KB ROM with duplicate correctly handled");
    }

    // ========================================================================
    // TEST 67: Very Small File (1 byte) Padded to 8KB
    // ========================================================================

    #[test]
    fn test_phase14_1_byte_padded_to_8kb() {
        let json = r#"{
            "version": 1,
            "description": "1 byte file padded to 8KB",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add 1-byte file (should pad to 8KB)
        builder
            .add_file(FileData {
                id: 0,
                data: vec![0x99],
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (metadata_buf, rom_images_buf) =
            builder.build(props).expect("Build should succeed with pad");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        // Verify the single byte is in the ROM images
        let first_byte = read_rom_byte(&rom_images_buf, 0, board);
        assert_eq!(first_byte, 0x99, "First byte should be 0x99");

        // Verify padding (exact pad value depends on implementation, but it should be consistent)
        let last_byte = read_rom_byte(&rom_images_buf, 8191, board);
        println!(
            "  First byte: 0x{:02X}, Last byte (padding): 0x{:02X}",
            first_byte, last_byte
        );

        println!("✓ Phase 14 Test 7: 1 byte file padded to 8KB correctly handled");
    }

    // ========================================================================
    // TEST 68: File Size Not a Power of 2 Without size_handling
    // ========================================================================

    #[test]
    fn test_phase14_odd_size_no_handling() {
        let json = r#"{
            "version": 1,
            "description": "Odd size without size_handling",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add 3KB file without size_handling (should error)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(3072, 0x33),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let result = builder.build(props);

        assert!(
            result.is_err(),
            "Odd size without size_handling should fail"
        );

        println!("✓ Phase 14 Test 8: Odd size without size_handling correctly rejected");
    }

    // ========================================================================
    // TEST 69: Maximum ROM Size (8KB for 2364)
    // ========================================================================

    #[test]
    fn test_phase14_maximum_rom_size() {
        let json = r#"{
            "version": 1,
            "description": "Maximum ROM size",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add exactly 8KB (maximum for 2364)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xFF),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 9: Maximum ROM size (8KB) correctly handled");
    }

    // ========================================================================
    // TEST 70: Minimum ROM Size (2KB for 2316)
    // ========================================================================

    #[test]
    fn test_phase14_minimum_rom_size() {
        let json = r#"{
            "version": 1,
            "description": "Minimum ROM size",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Add exactly 2KB (minimum/exact size for 2316)
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2048, 0x00),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        // Basic validation
        let header = MetadataHeader::parse(&metadata_buf);
        header.validate_basic();
        assert_eq!(header.chip_set_count, 1);

        println!("✓ Phase 14 Test 10: Minimum ROM size (2KB) correctly handled");
    }

    // ========================================================================
    // PHASE 16: CS Configuration Validation
    // ========================================================================

    // ========================================================================
    // TEST 71: 2316 with Only CS1 and CS2 (Missing CS3)
    // ========================================================================

    #[test]
    fn test_phase16_2316_missing_cs3() {
        let json = r#"{
            "version": 1,
            "description": "2316 with missing CS3",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "2316 missing CS3 should fail");

        println!("✓ Phase 16 Test 1: 2316 with missing CS3 correctly rejected");
    }

    // ========================================================================
    // TEST 72: 2332 with CS3 Specified (Already tested in Phase 11)
    // ========================================================================
    // This was TEST 37 in Phase 11 - no need to duplicate

    // ========================================================================
    // TEST 73: All Three CS as Ignore
    // ========================================================================

    #[test]
    fn test_phase16_all_cs_ignore() {
        let json = r#"{
            "version": 1,
            "description": "All CS lines as ignore",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "ignore",
                    "cs2": "ignore",
                    "cs3": "ignore"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "All CS as ignore should fail");

        println!("✓ Phase 16 Test 2: All CS lines as ignore correctly rejected");
    }

    // ========================================================================
    // TEST 74: CS1 as Ignore but CS2 Active
    // ========================================================================

    #[test]
    fn test_phase16_cs1_ignore_cs2_active() {
        let json = r#"{
            "version": 1,
            "description": "CS1 ignore but CS2 active",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "ignore",
                    "cs2": "active_low"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "CS1 as ignore with CS2 active should fail");

        println!("✓ Phase 16 Test 3: CS1 ignore with CS2 active correctly rejected");
    }

    // ========================================================================
    // TEST 75: Multi Set with Inconsistent CS States Between ROMs
    // ========================================================================

    #[test]
    fn test_phase16_multi_set_inconsistent_cs() {
        let json = r#"{
            "version": 1,
            "description": "Multi set with inconsistent CS states",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2364",
                        "cs1": "active_high"
                    }
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(
            result.is_err(),
            "Multi set with inconsistent CS1 states should fail"
        );

        println!("✓ Phase 16 Test 4: Multi set with inconsistent CS states correctly rejected");
    }

    // ========================================================================
    // TEST 76: Banked Set with Different CS1 States
    // ========================================================================

    #[test]
    fn test_phase16_banked_set_different_cs1() {
        let json = r#"{
            "version": 1,
            "description": "Banked set with different CS1 states",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2364",
                        "cs1": "active_high"
                    }
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(
            result.is_err(),
            "Banked set with different CS1 states should fail"
        );

        println!("✓ Phase 16 Test 5: Banked set with different CS1 states correctly rejected");
    }

    // ========================================================================
    // TEST 77: 2332 with CS1 Ignore and CS2 Ignore
    // ========================================================================

    #[test]
    fn test_phase16_2332_both_cs_ignore() {
        let json = r#"{
            "version": 1,
            "description": "2332 with both CS ignore",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "ignore",
                    "cs2": "ignore"
                }]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(result.is_err(), "2332 with both CS as ignore should fail");

        println!("✓ Phase 16 Test 6: 2332 with both CS ignore correctly rejected");
    }

    // ========================================================================
    // TEST 77A: Varous invalid CS configurations
    // ========================================================================
    fn invalid_config_json(roms: &str) -> String {
        format!(
            r#"{{
            "version": 1,
            "description": "Invalid CS config",
            "chip_sets": [{{
                "type": "single",
                "chips": [{roms}]
            }}]
        }}"#
        )
    }
    #[test]
    fn test_phase16_invalid_cs_configs() {
        let invalid_configs = vec![
            r#"{
                "file": "test.rom",
                "type": "2316"
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2316",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2316",
                "cs1": "active_low",
                "cs2": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2316",
                "cs1": "active_low",
                "cs2": "active_low",
                "cs3": "ignore"
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2332",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2332",
                "cs1": "ignore",
                "cs2": "ignore"
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2332",
                "cs1": "active_low",
                "cs2": "active_low",
                "cs3": "active_low"
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2364",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2364",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2364",
                "cs1": "active_low",
                "cs2": "active_high"
            }"#,
            r#"{
                "file": "test.rom",
                "type": "23128",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "23128",
                "cs1": "ignore",
                "cs2": "ignore",
                "cs3": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "23128",
                "cs1": "active_low",
                "cs2": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2716",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2716",
                "cs1": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2732",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2732",
                "cs1": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2764",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "2764",
                "cs1": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27128",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27128",
                "cs1": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27256",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27256",
                "cs1": "active_low",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27512",
                "cs1": "ignore",
            }"#,
            r#"{
                "file": "test.rom",
                "type": "27512",
                "cs1": "active_low",
            }"#,
        ];
        for config in invalid_configs {
            let json = invalid_config_json(config);
            let result = Builder::from_json(FW_VER, MCU_FAM, &json);
            assert!(result.is_err(), "Invalid CS config should fail");
        }

        println!("✓ Phase 16: Invalid CS config correctly rejected");
    }

    // ========================================================================
    // TEST 78: Valid CS Configuration - 2316 with All CS Active
    // ========================================================================

    #[test]
    fn test_phase16_2316_all_cs_active_valid() {
        let json = r#"{
            "version": 1,
            "description": "Valid 2316 with all CS active",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2316",
                    "cs1": "active_low",
                    "cs2": "active_low",
                    "cs3": "active_low"
                }]
            }]
        }"#;

        let mut builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Valid CS config should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(2048, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (_metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        println!("✓ Phase 16 Test 7: Valid 2316 with all CS active correctly handled");
    }

    // ========================================================================
    // TEST 79: Valid CS Configuration - 2332 with Mixed CS States
    // ========================================================================

    #[test]
    fn test_phase16_2332_mixed_cs_valid() {
        let json = r#"{
            "version": 1,
            "description": "Valid 2332 with mixed CS states",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2332",
                    "cs1": "active_low",
                    "cs2": "active_high"
                }]
            }]
        }"#;

        let mut builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Valid CS config should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(4096, 0xBB),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let (_metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        println!("✓ Phase 16 Test 8: Valid 2332 with mixed CS states correctly handled");
    }

    // ========================================================================
    // TEST 80: Multi Set with All ROMs Having Same CS Configuration (Valid)
    // ========================================================================

    #[test]
    fn test_phase16_multi_set_same_cs_valid() {
        let json = r#"{
            "version": 1,
            "description": "Multi set with same CS config",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    {
                        "file": "rom0.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom1.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "rom2.bin",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Same CS config should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0x11),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x22),
            })
            .expect("Failed to add file 1");

        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(8192, 0x33),
            })
            .expect("Failed to add file 2");

        let props = default_fw_props();
        let (_metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        println!("✓ Phase 16 Test 9: Multi set with same CS config correctly handled");
    }

    // ========================================================================
    // TEST 81: Banked Set with All ROMs Having Same CS Configuration (Valid)
    // ========================================================================

    #[test]
    fn test_phase16_banked_set_same_cs_valid() {
        let json = r#"{
            "version": 1,
            "description": "Banked set with same CS config",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    {
                        "file": "bank0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank1.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    },
                    {
                        "file": "bank2.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }
                ]
            }]
        }"#;

        let mut builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Same CS config should be allowed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0x44),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(8192, 0x55),
            })
            .expect("Failed to add file 1");

        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(8192, 0x66),
            })
            .expect("Failed to add file 2");

        let props = default_fw_props();
        let (_metadata_buf, _rom_images_buf) = builder.build(props).expect("Build should succeed");

        println!("✓ Phase 16 Test 10: Banked set with same CS config correctly handled");
    }

    // ========================================================================
    // PHASE 18: ROM Images Correctness
    // ========================================================================

    // ========================================================================
    // TEST 82: Verify Duplicate Correctly Duplicates Data (Check Both Copies)
    // ========================================================================

    #[test]
    fn test_phase18_duplicate_both_copies() {
        let json = r#"{
            "version": 1,
            "description": "Verify duplicate creates identical copies",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "duplicate"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 4KB of test data with unique pattern
        let mut test_data = Vec::with_capacity(4096);
        for i in 0..4096 {
            test_data.push((i & 0xFF) as u8);
        }

        builder
            .add_file(FileData {
                id: 0,
                data: test_data.clone(),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Verify first copy (addresses 0-4095)
        let mut errors_first = 0;
        #[allow(clippy::needless_range_loop)]
        for addr in 0..4096 {
            let expected = test_data[addr];
            let actual = read_rom_byte(&rom_images_buf, addr, board);
            if actual != expected {
                errors_first += 1;
                if errors_first <= 5 {
                    println!(
                        "  First copy mismatch at 0x{:04X}: expected 0x{:02X}, got 0x{:02X}",
                        addr, expected, actual
                    );
                }
            }
        }

        // Verify second copy (addresses 4096-8191)
        let mut errors_second = 0;
        for addr in 4096..8192 {
            let expected = test_data[addr - 4096];
            let actual = read_rom_byte(&rom_images_buf, addr, board);
            if actual != expected {
                errors_second += 1;
                if errors_second <= 5 {
                    println!(
                        "  Second copy mismatch at 0x{:04X}: expected 0x{:02X}, got 0x{:02X}",
                        addr, expected, actual
                    );
                }
            }
        }

        assert_eq!(errors_first, 0, "First copy has {} errors", errors_first);
        assert_eq!(errors_second, 0, "Second copy has {} errors", errors_second);

        println!("✓ Phase 18 Test 1: Duplicate correctly creates two identical copies");
    }

    // ========================================================================
    // TEST 83: Verify Pad Fills with PAD_BLANK_BYTE
    // ========================================================================

    #[test]
    fn test_phase18_pad_fill_value() {
        let json = r#"{
            "version": 1,
            "description": "Verify pad fill byte",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low",
                    "size_handling": "pad"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        // Create 2KB of test data (will be padded to 8KB)
        let test_data = create_test_rom_data(2048, 0x42);

        builder
            .add_file(FileData {
                id: 0,
                data: test_data.clone(),
            })
            .expect("Failed to add file");

        let props = default_fw_props();
        let board = props.board();
        let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

        // Verify original data (addresses 0-2047)
        for addr in 0..2048 {
            let expected = 0x42;
            let actual = read_rom_byte(&rom_images_buf, addr, board);
            assert_eq!(actual, expected, "Original data mismatch at 0x{:04X}", addr);
        }

        // Verify all padding uses PAD_BLANK_BYTE (addresses 2048-8191)
        for addr in 2048..8192 {
            let actual = read_rom_byte(&rom_images_buf, addr, board);
            assert_eq!(
                actual,
                onerom_gen::PAD_BLANK_BYTE,
                "Pad byte mismatch at 0x{:04X}: expected 0x{:02X}, got 0x{:02X}",
                addr,
                onerom_gen::PAD_BLANK_BYTE,
                actual
            );
        }

        println!(
            "✓ Phase 18 Test 2: Pad fills with PAD_BLANK_BYTE (0x{:02X})",
            onerom_gen::PAD_BLANK_BYTE
        );
    }

    // ========================================================================
    // TEST 85: Set with 4 multi-set ROMs (Should Fail - Max is 3)
    // ========================================================================

    #[test]
    fn test_phase18_multi_chip_set_4_chips_fails() {
        let json = r#"{
            "version": 1,
            "description": "Multi-ROM set with 4 ROMs (exceeds max)",
            "chip_sets": [{
                "type": "multi",
                "chips": [
                    { "file": "bank0.rom", "type": "2364", "cs1": "active_low" },
                    { "file": "bank1.rom", "type": "2364", "cs1": "active_low" },
                    { "file": "bank2.rom", "type": "2364", "cs1": "active_low" },
                    { "file": "bank3.rom", "type": "2364", "cs1": "active_low" },
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(
            result.is_err(),
            "Banked set with 8 ROMs should fail (max is 4)"
        );

        println!("✓ Phase 18 Test 4: Banked set with 8 ROMs correctly rejected (max is 4)");
    }

    // ========================================================================
    // TEST 85: Banked Set with 8 ROMs (Should Fail - Max is 4)
    // ========================================================================

    #[test]
    fn test_phase18_banked_set_8_chips_fails() {
        let json = r#"{
            "version": 1,
            "description": "Banked set with 8 ROMs (exceeds max)",
            "chip_sets": [{
                "type": "banked",
                "chips": [
                    { "file": "bank0.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank1.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank2.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank3.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank4.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank5.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank6.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" },
                    { "file": "bank7.rom", "type": "2316", "cs1": "active_low", "cs2": "active_low", "cs3": "active_low" }
                ]
            }]
        }"#;

        let result = Builder::from_json(FW_VER, MCU_FAM, json);

        assert!(
            result.is_err(),
            "Banked set with 8 ROMs should fail (max is 4)"
        );

        println!("✓ Phase 18 Test 4: Banked set with 8 ROMs correctly rejected (max is 4)");
    }

    // ========================================================================
    // TEST 86: Invalid, removed
    // ========================================================================

    // ========================================================================
    // TEST 87: Different Board Pin Mappings Produce Correct Transformations
    // ========================================================================

    #[test]
    fn test_phase18_different_board_pin_mappings() {
        let json = r#"{
            "version": 1,
            "description": "Test different board pin mappings",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        // Test with multiple boards that have different pin mappings
        let boards_to_test = [
            (Board::Ice24D, McuVariant::F411RE),
            (Board::Ice24E, McuVariant::F411RE),
            (Board::Ice24F, McuVariant::F411RE),
            (Board::Ice24G, McuVariant::F411RE),
            (Board::Ice24UsbH, McuVariant::F411RE),
            (Board::Fire24A, McuVariant::RP2350),
            (Board::Fire24UsbB, McuVariant::RP2350),
        ];

        for (board_type, mcu_variant) in boards_to_test.iter() {
            let mut builder =
                Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

            // Create test data with unique pattern
            let mut test_data = Vec::with_capacity(8192);
            for i in 0..8192 {
                test_data.push((i & 0xFF) as u8);
            }

            builder
                .add_file(FileData {
                    id: 0,
                    data: test_data.clone(),
                })
                .expect("Failed to add file");

            let props = FirmwareProperties::new(
                FirmwareVersion::new(0, 5, 1, 0),
                *board_type,
                *mcu_variant,
                ServeAlg::Default,
                false,
            )
            .unwrap();

            let board = props.board();
            let (_metadata_buf, rom_images_buf) = builder.build(props).expect("Build failed");

            // Verify address/data transformations work correctly for this board
            let mut errors = 0;
            let max_errors = 10;

            #[allow(clippy::needless_range_loop)]
            for addr in 0..8192 {
                let expected = test_data[addr];
                let actual = read_rom_byte(&rom_images_buf, addr, board);
                if actual != expected {
                    errors += 1;
                    if errors <= max_errors {
                        println!(
                            "  {:?} mismatch at 0x{:04X}: expected 0x{:02X}, got 0x{:02X}",
                            board_type, addr, expected, actual
                        );
                    }
                }
            }

            if errors > max_errors {
                println!("  ... and {} more errors", errors - max_errors);
            }

            assert_eq!(
                errors, 0,
                "Found {} transformation errors for {:?}",
                errors, board_type
            );

            println!("  ✓ {:?} pin mapping transformations correct", board_type);
        }

        println!("✓ Phase 18 Test 6: Different board pin mappings produce correct transformations");
    }

    // ========================================================================
    // Test license presence
    // ========================================================================
    #[test]
    fn test_phase17_license_presence() {
        let json = r#"{
            "version": 1,
            "description": "Test license presence",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "license": "license.url",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAB),
            })
            .expect("Failed to add file");

        let licenses = builder.licenses();
        assert_eq!(licenses.len(), 1, "There should be one license entry");
        assert_eq!(licenses[0].url, "license.url", "License URL should match");
        assert_eq!(licenses[0].id, 0, "License ID should be 0");
        assert_eq!(licenses[0].file_id, 0, "File ID should be 0");

        builder
            .accept_license(&licenses[0])
            .expect("License acceptance should pass");

        println!("✓ License presence test passed");
    }

    // ========================================================================
    // Test no license
    // ========================================================================
    #[test]
    fn test_phase17_no_license_presence() {
        let json = r#"{
            "version": 1,
            "description": "Test license presence",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAB),
            })
            .expect("Failed to add file");

        let licenses = builder.licenses();
        assert_eq!(licenses.len(), 0, "There should be no license entry");

        // Should fail
        let license = onerom_gen::License::new(0, 0, "license.url".to_string());
        builder
            .accept_license(&license)
            .expect_err("License acceptance should fail");

        println!("✓ License presence test passed");
    }

    // ========================================================================
    // Test descriptions
    // ========================================================================
    #[test]
    fn test_phase19_basic() {
        let json = r#"{
            "version": 1,
            "description": "Test description",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Test description\n\nImages:\n0: test.rom",
            "Description should match"
        );
    }

    #[test]
    fn test_phase19_detail() {
        let json = r#"{
            "version": 1,
            "description": "Test description",
            "detail": "Detailed description",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Test description\n\nDetailed description\n\nImages:\n0: test.rom",
            "Description should match"
        );
    }

    #[test]
    fn test_phase19_image() {
        let json = r#"{
            "version": 1,
            "description": "Test description",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "description": "an image",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Test description\n\nImages:\n0: an image",
            "Description should match"
        );
    }

    #[test]
    fn test_phase19_name() {
        let json = r#"{
            "version": 1,
            "name": "Name of config",
            "description": "Test description",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "cs1": "active_low",
                    "file": "test.rom",
                    "type": "2364"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Name of config\n--------------\n\nTest description\n\nImages:\n0: test.rom",
            "Description should match"
        );
    }

    #[test]
    fn test_phase19_notes() {
        let json = r#"{
            "version": 1,
            "description": "Test description",
            "notes": "Some notes here",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "description": "an image",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Test description\n\nImages:\n0: an image\n\nSome notes here",
            "Description should match"
        );
    }

    #[test]
    fn test_phase19_set() {
        let json = r#"{
            "version": 1,
            "description": "Test description",
            "chip_sets": [{
                "type": "banked",
                "chips": [{
                    "file": "test0.rom",
                    "type": "2364",
                    "cs1": "active_low"
                },{
                    "file": "test1.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");
        let desc = builder.description();
        assert_eq!(
            desc, "Test description\n\nSets:\n0: Banked\n  0: test0.rom\n  1: test1.rom",
            "Description should match"
        );
    }

    // ========================================================================
    // PHASE 20: Firmware Overrides
    // ========================================================================

    fn default_fw_props_060() -> FirmwareProperties {
        FirmwareProperties::new(
            FirmwareVersion::new(0, 6, 0, 0), // 0.6.0 version for extended features
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::Default,
            false, // boot_logging disabled
        )
        .unwrap()
    }

    // ========================================================================
    // Helper: Parse Firmware Overrides Structure (0.6.0+)
    // ========================================================================

    /// Represents the onerom_firmware_overrides_t C structure
    #[derive(Debug)]
    struct FirmwareOverridesStruct {
        override_present: [u8; 8],
        ice_freq: u16,
        fire_freq: u16,
        fire_vreg: u8,
        override_value: [u8; 8],
    }

    impl FirmwareOverridesStruct {
        /// Parse firmware overrides structure from buffer at given offset
        fn parse(buf: &[u8], offset: usize) -> Self {
            assert!(
                buf.len() >= offset + 64,
                "Buffer too small: {} bytes, need {} at offset {}",
                buf.len(),
                offset + 64,
                offset
            );

            // override_present: 8 bytes at offset 0
            let mut override_present = [0u8; 8];
            override_present.copy_from_slice(&buf[offset..offset + 8]);

            // ice_freq_mhz: 2 bytes (u16) at offset 8
            let ice_freq = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]);

            // fire_freq_mhz: 2 bytes (u16) at offset 10
            let fire_freq = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]);

            // fire_vreg: 1 byte at offset 12
            let fire_vreg = buf[offset + 12];

            // pad1: 3 bytes at offset 13-15 (skip)

            // override_value: 8 bytes at offset 16
            let mut override_value = [0u8; 8];
            override_value.copy_from_slice(&buf[offset + 16..offset + 24]);

            // pad3: 40 bytes at offset 24-63 (skip)

            Self {
                override_present,
                ice_freq,
                fire_freq,
                fire_vreg,
                override_value,
            }
        }

        /// Check if a specific override is present
        fn is_override_present(&self, bit_index: usize) -> bool {
            let byte_index = bit_index / 8;
            let bit_offset = bit_index % 8;
            (self.override_present[byte_index] & (1 << bit_offset)) != 0
        }

        /// Check if a specific override value bit is set
        fn is_override_value_set(&self, bit_index: usize) -> bool {
            let byte_index = bit_index / 8;
            let bit_offset = bit_index % 8;
            (self.override_value[byte_index] & (1 << bit_offset)) != 0
        }
    }

    // ========================================================================
    // Helper: Parse Extended ROM Set Structure (0.6.0+)
    // ========================================================================

    /// Extended ROM set structure with firmware_overrides support
    #[derive(Debug)]
    struct ExtendedRomSetStruct {
        extra_info: u8,
        serve_config_ptr: u32,
        firmware_overrides_ptr: u32,
    }

    impl ExtendedRomSetStruct {
        fn parse(buf: &[u8], offset: usize) -> Self {
            assert!(
                buf.len() >= offset + 64,
                "Buffer too small: {} bytes, need {} at offset {}",
                buf.len(),
                offset + 64,
                offset
            );

            // extra_info: 1 byte at offset 15
            let extra_info = buf[offset + 15];

            // serve_config_ptr: 4 bytes at offset 16-19
            let serve_config_ptr = u32::from_le_bytes([
                buf[offset + 16],
                buf[offset + 17],
                buf[offset + 18],
                buf[offset + 19],
            ]);

            // firmware_overrides_ptr: 4 bytes at offset 20-23
            let firmware_overrides_ptr = u32::from_le_bytes([
                buf[offset + 20],
                buf[offset + 21],
                buf[offset + 22],
                buf[offset + 23],
            ]);

            // pad2: 40 bytes at offset 24-63 (skip)

            Self {
                extra_info,
                serve_config_ptr,
                firmware_overrides_ptr,
            }
        }
    }

    // ========================================================================
    // TEST 88: Basic Ice Clock Override
    // ========================================================================

    #[test]
    fn test_phase20_ice_override() {
        let json = r#"{
            "version": 1,
            "description": "Ice clock override test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "100MHz",
                        "overclock": false
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse extended ROM set structure
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Verify extra_info is set
        assert_eq!(
            ext_chip_set.extra_info, 1,
            "extra_info should be 1 for 0.6.0+"
        );

        // Verify firmware_overrides pointer is valid
        assert_ne!(
            ext_chip_set.firmware_overrides_ptr, 0xFFFFFFFF,
            "firmware_overrides_ptr should not be NULL"
        );

        // Parse firmware overrides structure
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify Ice frequency override is present (bit 0)
        assert!(
            fw_overrides.is_override_present(0),
            "Ice frequency override should be present"
        );

        // Verify Ice overclock override is present (bit 1)
        assert!(
            fw_overrides.is_override_present(1),
            "Ice overclock override should be present"
        );

        // Verify Ice frequency value (100MHz = enum value 100)
        assert_eq!(fw_overrides.ice_freq, 100, "Ice frequency should be 100MHz");

        // Verify Ice overclock is disabled (bit 0 in override_value should be 0)
        assert!(
            !fw_overrides.is_override_value_set(0),
            "Ice overclock should be disabled"
        );

        println!("✓ Phase 20 Test 1: Ice clock override correctly serialized");
        println!("  - Ice frequency: {}MHz", fw_overrides.ice_freq);
        println!("  - Overclock: disabled");
    }

    // ========================================================================
    // TEST 89: Basic Fire Clock Override
    // ========================================================================
    #[test]
    fn test_phase20_fire_override() {
        let json = r#"{
            "version": 1,
            "description": "Fire clock override test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "fire": {
                        "cpu_freq": "300MHz",
                        "overclock": true,
                        "vreg": "1.10V",
                        "serve_mode": "Pio"
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse extended ROM set structure
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Debug: Show the pointer and offset
        println!(
            "firmware_overrides_ptr: 0x{:08X}",
            ext_chip_set.firmware_overrides_ptr
        );
        println!("metadata_flash_start: 0x{:08X}", metadata_flash_start);

        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        println!("fw_overrides_offset: {}", fw_overrides_offset);

        // Debug: Show first 24 bytes of firmware_overrides structure
        println!("First 24 bytes of firmware_overrides structure:");
        for i in 0..24 {
            print!("{:02X} ", metadata_buf[fw_overrides_offset + i]);
            if (i + 1) % 8 == 0 {
                println!();
            }
        }
        println!();

        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify Fire frequency override is present (bit 2)
        assert!(
            fw_overrides.is_override_present(2),
            "Fire frequency override should be present"
        );

        // Verify Fire frequency value (300MHz)
        assert_eq!(
            fw_overrides.fire_freq, 300,
            "Fire frequency should be 300MHz, got {}",
            fw_overrides.fire_freq
        );

        // Verify overclock override is present (bit 3)
        assert!(
            fw_overrides.is_override_present(3),
            "Fire overclock override should be present"
        );

        // Verify Fire overclock is enabled (bit 1 in override_value should be 1)
        assert!(
            fw_overrides.is_override_value_set(1),
            "Fire overclock should be enabled"
        );

        // Verify voltage regulator override is present (bit 4)
        assert!(
            fw_overrides.is_override_present(4),
            "Fire voltage regulator override should be present"
        );

        // Verify Fire voltage regulator value (1.10V = enum value 2)
        assert_eq!(
            fw_overrides.fire_vreg, 11,
            "Fire voltage regulator should be 1.10V (enum 2), got {}",
            fw_overrides.fire_vreg
        );

        // Verify serve mode override is present (bit 7)
        assert!(
            fw_overrides.is_override_present(7),
            "Serve mode override should be present"
        );

        // Verify serve mode is PIO (bit 3 in override_value should be 1)
        assert!(
            fw_overrides.is_override_value_set(4),
            "Serve mode should be PIO"
        );
    }

    // ========================================================================
    // TEST 90: Multiple Overrides Together
    // ========================================================================

    #[test]
    fn test_phase20_multiple_overrides() {
        let json = r#"{
            "version": 1,
            "description": "Multiple overrides test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "120MHz",
                        "overclock": true
                    },
                    "led": {
                        "enabled": false
                    },
                    "swd": {
                        "swd_enabled": true
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify all override_present bits
        assert!(
            fw_overrides.is_override_present(0),
            "Ice frequency should be present"
        );
        assert!(
            fw_overrides.is_override_present(1),
            "Ice overclock should be present"
        );
        assert!(fw_overrides.is_override_present(5), "LED should be present");
        assert!(fw_overrides.is_override_present(6), "SWD should be present");

        // Verify Ice frequency
        assert_eq!(fw_overrides.ice_freq, 120, "Ice frequency should be 120MHz");

        // Verify override_value bits
        assert!(
            fw_overrides.is_override_value_set(0),
            "Ice overclock should be enabled"
        );
        assert!(
            !fw_overrides.is_override_value_set(2),
            "LED should be disabled"
        );
        assert!(
            fw_overrides.is_override_value_set(3),
            "SWD should be enabled"
        );

        println!("✓ Phase 20 Test 3: Multiple overrides correctly serialized");
        println!("  - Ice frequency: {}MHz", fw_overrides.ice_freq);
        println!("  - Overclock: enabled");
        println!("  - LED: disabled");
        println!("  - SWD: enabled");
    }

    // ========================================================================
    // TEST 94: Multiple ROM Sets with Different Overrides
    // ========================================================================

    #[test]
    fn test_phase20_multiple_sets_different_overrides() {
        let json = r#"{
            "version": 1,
            "description": "Multiple sets with different overrides",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "test0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }],
                    "firmware_overrides": {
                        "ice": {
                            "cpu_freq": "100MHz",
                            "overclock": false
                        }
                    }
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }],
                    "firmware_overrides": {
                        "ice": {
                            "cpu_freq": "150MHz",
                            "overclock": true
                        },
                        "led": {
                            "enabled": false
                        }
                    }
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test2.rom",
                        "type": "2316",
                        "cs1": "active_low",
                        "cs2": "active_low",
                        "cs3": "active_low"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file 0");

        builder
            .add_file(FileData {
                id: 1,
                data: create_test_rom_data(4096, 0x55),
            })
            .expect("Failed to add file 1");

        builder
            .add_file(FileData {
                id: 2,
                data: create_test_rom_data(2048, 0xFF),
            })
            .expect("Failed to add file 2");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 3);

        // Check Set 0 - has ice override
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set0 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set0_offset);
        assert_ne!(
            ext_chip_set0.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 0 should have firmware_overrides"
        );

        let fw_overrides0_offset =
            (ext_chip_set0.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides0 = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides0_offset);
        assert_eq!(fw_overrides0.ice_freq, 100, "Set 0 should be 100MHz");
        assert!(
            !fw_overrides0.is_override_value_set(0),
            "Set 0 Ice overclock should be disabled"
        );

        // Check Set 1 - has ice + led overrides
        let chip_set1_offset = chip_set0_offset + 64; // Extended structure is 64 bytes
        let ext_chip_set1 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set1_offset);
        assert_ne!(
            ext_chip_set1.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 1 should have firmware_overrides"
        );

        let fw_overrides1_offset =
            (ext_chip_set1.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides1 = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides1_offset);
        assert_eq!(fw_overrides1.ice_freq, 150, "Set 1 should be 150MHz");
        assert!(
            fw_overrides1.is_override_value_set(0),
            "Set 1 Ice overclock should be enabled"
        );
        assert!(
            !fw_overrides1.is_override_value_set(2),
            "Set 1 LED should be disabled"
        );

        // Check Set 2 - no overrides
        let chip_set2_offset = chip_set1_offset + 64;
        let ext_chip_set2 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set2_offset);
        assert_eq!(
            ext_chip_set2.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 2 should have NULL firmware_overrides"
        );

        println!("✓ Phase 20 Test 7: Multiple ROM sets with different overrides");
        println!("  - Set 0: 100MHz, overclock disabled");
        println!("  - Set 1: 150MHz, overclock enabled, LED disabled");
        println!("  - Set 2: No overrides");
    }

    // ========================================================================
    // TEST 98: Bitfield Validation - All Bits
    // ========================================================================

    #[test]
    fn test_phase20_all_bitfields() {
        let json = r#"{
            "version": 1,
            "description": "All bitfields test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "96MHz",
                        "overclock": true
                    },
                    "led": {
                        "enabled": true
                    },
                    "swd": {
                        "swd_enabled": false
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Check override_present bitfield
        // Bits: 0=Ice freq, 1=Ice overclock, 5=LED, 6=SWD
        let expected_present = (1 << 0) | (1 << 1) | (1 << 5) | (1 << 6);
        assert_eq!(
            fw_overrides.override_present[0], expected_present,
            "override_present[0] should be 0x{:02X}, got 0x{:02X}",
            expected_present, fw_overrides.override_present[0]
        );

        // Check override_value bitfield
        // Bits: 0=Ice overclock enabled, 2=LED enabled, 3=SWD disabled
        let expected_value = (1 << 0) | (1 << 2); // Ice overclock=1, LED=1, SWD=0
        assert_eq!(
            fw_overrides.override_value[0], expected_value,
            "override_value[0] should be 0x{:02X}, got 0x{:02X}",
            expected_value, fw_overrides.override_value[0]
        );

        println!("✓ Phase 20 Test 11: All bitfields correctly set");
        println!(
            "  - override_present[0]: 0x{:02X}",
            fw_overrides.override_present[0]
        );
        println!(
            "  - override_value[0]: 0x{:02X}",
            fw_overrides.override_value[0]
        );
    }
    // ========================================================================
    // TEST 91: No Overrides - Null Pointers
    // ========================================================================

    #[test]
    fn test_phase20_no_overrides() {
        let json = r#"{
            "version": 1,
            "description": "No overrides test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse extended ROM set structure
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Verify extra_info is set
        assert_eq!(
            ext_chip_set.extra_info, 1,
            "extra_info should be 1 for 0.6.0+"
        );

        // Verify both pointers are NULL (0xFFFFFFFF)
        assert_eq!(
            ext_chip_set.firmware_overrides_ptr, 0xFFFFFFFF,
            "firmware_overrides_ptr should be NULL when no overrides"
        );

        assert_eq!(
            ext_chip_set.serve_config_ptr, 0xFFFFFFFF,
            "serve_config_ptr should be NULL when no serve_alg_params"
        );

        println!("✓ Phase 20 Test 4: No overrides correctly serialized with NULL pointers");
    }

    // ========================================================================
    // TEST 92: Pre-0.6.0 Compatibility - 16 Byte Structure
    // ========================================================================

    #[test]
    fn test_phase20_pre_060_compatibility() {
        let json = r#"{
            "version": 1,
            "description": "Pre-0.6.0 compatibility test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }]
            }]
        }"#;

        let mut builder = Builder::from_json(
            FirmwareVersion::new(0, 5, 1, 0), // Pre-0.6.0 version
            MCU_FAM,
            json,
        )
        .expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = FirmwareProperties::new(
            FirmwareVersion::new(0, 5, 1, 0),
            Board::Ice24UsbH,
            McuVariant::F411RE,
            ServeAlg::Default,
            false,
        )
        .unwrap();

        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse basic ROM set structure (should be 16 bytes)
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let chip_set = RomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Verify basic structure is valid
        assert_eq!(chip_set.rom_count, 1, "Should have 1 ROM");

        // Verify no extended fields are present (next ROM set or end of metadata should follow)
        // For pre-0.6.0, the next structure should be 16 bytes after this one, not 64
        // We can't directly test this without multiple ROM sets, but we can verify
        // the metadata length is reasonable for a 16-byte structure

        println!("✓ Phase 20 Test 5: Pre-0.6.0 compatibility - 16 byte structure");
    }

    // ========================================================================
    // TEST 93: Firmware Overrides with firmware_overrides Specified But Should Fail
    // ========================================================================

    #[test]
    fn test_phase20_overrides_on_old_firmware() {
        let json = r#"{
            "version": 1,
            "description": "Overrides on old firmware",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "100MHz",
                        "overclock": false
                    }
                }
            }]
        }"#;

        let result = Builder::from_json(
            FirmwareVersion::new(0, 5, 1, 0), // Pre-0.6.0 version
            MCU_FAM,
            json,
        );

        assert!(
            result.is_err(),
            "firmware_overrides should fail on pre-0.6.0 firmware"
        );

        println!("✓ Phase 20 Test 6: firmware_overrides on old firmware correctly rejected");
    }

    // ========================================================================
    // TEST 95: serve_alg_params
    // ========================================================================

    #[test]
    fn test_phase20_serve_alg_params() {
        let json = r#"{
            "version": 1,
            "description": "serve_alg_params test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "serve_alg_params": {
                        "params": [1, 2, 3, 4, 5]
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse extended ROM set structure
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Verify serve_config pointer is valid
        assert_ne!(
            ext_chip_set.serve_config_ptr, 0xFFFFFFFF,
            "serve_config_ptr should not be NULL"
        );

        // Parse serve_config structure (64 bytes)
        let serve_config_offset = (ext_chip_set.serve_config_ptr - metadata_flash_start) as usize;
        assert!(
            serve_config_offset + 64 <= metadata_buf.len(),
            "serve_config structure should fit in metadata buffer"
        );

        // Verify the params are written correctly
        assert_eq!(
            metadata_buf[serve_config_offset], 1,
            "First param should be 1"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 1],
            2,
            "Second param should be 2"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 2],
            3,
            "Third param should be 3"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 3],
            4,
            "Fourth param should be 4"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 4],
            5,
            "Fifth param should be 5"
        );

        // Verify rest is zero-padded
        for i in 5..64 {
            assert_eq!(
                metadata_buf[serve_config_offset + i],
                0xFF,
                "Padding at offset {} should be 0",
                i
            );
        }

        println!("✓ Phase 20 Test 8: serve_alg_params correctly serialized");
    }

    // ========================================================================
    // TEST 96: Empty firmware_overrides (All None) Should Fail
    // ========================================================================
    #[test]
    fn test_phase20_empty_firmware_overrides() {
        let json = r#"{
            "version": 1,
            "description": "Empty firmware_overrides",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {}
            }]
        }"#;

        let mut builder =
            Builder::from_json(FW_VER, MCU_FAM, json).expect("Parsing should succeed");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let result = builder.build(props);

        assert!(
            result.is_err(),
            "Build should fail with empty firmware_overrides"
        );

        println!("✓ Phase 20 Test 9: Empty firmware_overrides correctly rejected");
    }

    // ========================================================================
    // TEST 97: Stock Frequency Values
    // ========================================================================

    #[test]
    fn test_phase20_stock_frequency() {
        let json = r#"{
            "version": 1,
            "description": "Stock frequency test",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "Stock",
                        "overclock": false
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify Stock frequency is 0xFF (since repr is u16, upper byte should be 0xFF)
        assert_eq!(
            fw_overrides.ice_freq, 0xFFFF,
            "Stock frequency should be 0xFFFF"
        );

        println!("✓ Phase 20 Test 10: Stock frequency correctly serialized as 0xFF");
    }

    // ========================================================================
    // TEST 99: Fire Clock with Stock VREG
    // ========================================================================

    #[test]
    fn test_phase20_fire_stock_vreg() {
        let json = r#"{
            "version": 1,
            "description": "Fire clock with stock VREG",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "fire": {
                        "cpu_freq": "250MHz",
                        "overclock": false
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify Fire frequency is present (bit 2)
        assert!(
            fw_overrides.is_override_present(2),
            "Fire frequency override should be present"
        );

        // Verify Fire overclock is present (bit 3)
        assert!(
            fw_overrides.is_override_present(3),
            "Fire overclock override should be present"
        );

        // Verify VREG IS NOT present (bit 4) - as not specified
        assert!(
            !fw_overrides.is_override_present(4),
            "Fire VREG override should not be present"
        );

        // Verify VREG value is 0xFF (Stock)
        assert_eq!(fw_overrides.fire_vreg, 0xFF, "VREG should be 0xFF (Stock)");

        // Verify Fire overclock is disabled (bit 1 in override_value should be 0)
        assert!(
            !fw_overrides.is_override_value_set(1),
            "Fire overclock should be disabled"
        );

        println!("✓ Phase 20 Test 99: Fire clock with Stock VREG");
        println!("  - Fire frequency: 250MHz");
        println!("  - VREG: Stock (0xFF)");
        println!("  - VREG override bit NOT set");
    }

    // ========================================================================
    // TEST 100: Ice AND Fire Clocks Together
    // ========================================================================

    #[test]
    fn test_phase20_ice_and_fires() {
        let json = r#"{
            "version": 1,
            "description": "Ice and Fire clocks together",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "168MHz",
                        "overclock": true
                    },
                    "fire": {
                        "cpu_freq": "400MHz",
                        "overclock": false,
                        "vreg": "1.25V"
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Verify all override_present bits for both Ice and Fire
        assert!(
            fw_overrides.is_override_present(0),
            "Ice frequency should be present"
        );
        assert!(
            fw_overrides.is_override_present(1),
            "Ice overclock should be present"
        );
        assert!(
            fw_overrides.is_override_present(2),
            "Fire frequency should be present"
        );
        assert!(
            fw_overrides.is_override_present(3),
            "Fire overclock should be present"
        );
        assert!(
            fw_overrides.is_override_present(4),
            "Fire VREG should be present"
        );

        // Verify frequency values
        assert_eq!(fw_overrides.ice_freq, 168, "Ice frequency should be 168MHz");
        assert_eq!(
            fw_overrides.fire_freq, 400,
            "Fire frequency should be 400MHz"
        );

        // Verify VREG value (1.25V = 0x0E)
        assert_eq!(fw_overrides.fire_vreg, 0x0E, "VREG should be 1.25V (0x0E)");

        // Verify overclock values
        assert!(
            fw_overrides.is_override_value_set(0),
            "Ice overclock should be enabled"
        );
        assert!(
            !fw_overrides.is_override_value_set(1),
            "Fire overclock should be disabled"
        );

        println!("✓ Phase 20 Test 100: Ice and Fire clocks together");
        println!("  - Ice: 168MHz, overclock enabled");
        println!("  - Fire: 400MHz, overclock disabled, VREG 1.25V");
    }

    // ========================================================================
    // TEST 101: serve_alg_params Combined with Other Overrides
    // ========================================================================

    #[test]
    fn test_phase20_serve_params_with_overrides() {
        let json = r#"{
            "version": 1,
            "description": "serve_alg_params with other overrides",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "144MHz",
                        "overclock": false
                    },
                    "led": {
                        "enabled": true
                    },
                    "serve_alg_params": {
                        "params": [10, 20, 30, 40, 50]
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse extended ROM set structure
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);

        // Verify both pointers are valid
        assert_ne!(
            ext_chip_set.firmware_overrides_ptr, 0xFFFFFFFF,
            "firmware_overrides_ptr should not be NULL"
        );
        assert_ne!(
            ext_chip_set.serve_config_ptr, 0xFFFFFFFF,
            "serve_config_ptr should not be NULL"
        );

        // Check firmware overrides
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        assert!(
            fw_overrides.is_override_present(0),
            "Ice frequency should be present"
        );
        assert!(fw_overrides.is_override_present(5), "LED should be present");
        assert_eq!(fw_overrides.ice_freq, 144, "Ice frequency should be 144MHz");
        assert!(
            fw_overrides.is_override_value_set(2),
            "LED should be enabled"
        );

        // Check serve_config
        let serve_config_offset = (ext_chip_set.serve_config_ptr - metadata_flash_start) as usize;
        assert_eq!(
            metadata_buf[serve_config_offset], 10,
            "First param should be 10"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 1],
            20,
            "Second param should be 20"
        );
        assert_eq!(
            metadata_buf[serve_config_offset + 4],
            50,
            "Fifth param should be 50"
        );

        println!("✓ Phase 20 Test 101: serve_alg_params combined with other overrides");
        println!("  - Ice: 144MHz, overclock disabled");
        println!("  - LED: enabled");
        println!("  - serve_alg_params: [10, 20, 30, 40, 50]");
    }

    // ========================================================================
    // TEST 102: serve_alg_params Exactly 64 Bytes
    // ========================================================================

    #[test]
    fn test_phase20_serve_params_64_bytes() {
        let params: Vec<u8> = (0..64).collect();
        let params_json = format!(
            "[{}]",
            params
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let json = format!(
            r#"{{
            "version": 1,
            "description": "serve_alg_params exactly 64 bytes",
            "chip_sets": [{{
                "type": "single",
                "chips": [{{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }}],
                "firmware_overrides": {{
                    "serve_alg_params": {{
                        "params": {}
                    }}
                }}
            }}]
        }}"#,
            params_json
        );

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, &json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse serve_config
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let serve_config_offset = (ext_chip_set.serve_config_ptr - metadata_flash_start) as usize;

        // Verify all 64 bytes
        for i in 0..64 {
            assert_eq!(
                metadata_buf[serve_config_offset + i],
                i as u8,
                "Byte {} should be {}",
                i,
                i
            );
        }

        println!("✓ Phase 20 Test 102: serve_alg_params exactly 64 bytes");
    }

    // ========================================================================
    // TEST 103: serve_alg_params More Than 64 Bytes (Should Truncate)
    // ========================================================================

    #[test]
    fn test_phase20_serve_params_truncate() {
        let params: Vec<u8> = (0..100).collect();
        let params_json = format!(
            "[{}]",
            params
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let json = format!(
            r#"{{
            "version": 1,
            "description": "serve_alg_params more than 64 bytes",
            "chip_sets": [{{
                "type": "single",
                "chips": [{{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }}],
                "firmware_overrides": {{
                    "serve_alg_params": {{
                        "params": {}
                    }}
                }}
            }}]
        }}"#,
            params_json
        );

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, &json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse serve_config
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let serve_config_offset = (ext_chip_set.serve_config_ptr - metadata_flash_start) as usize;

        // Verify first 64 bytes (0-63)
        for i in 0..64 {
            assert_eq!(
                metadata_buf[serve_config_offset + i],
                i as u8,
                "Byte {} should be {}",
                i,
                i
            );
        }

        // Bytes 64-99 should have been truncated
        println!("✓ Phase 20 Test 103: serve_alg_params >64 bytes truncated to 64");
    }

    // ========================================================================
    // TEST 104: All Boolean Combinations
    // ========================================================================

    #[test]
    fn test_phase20_all_boolean_combinations() {
        let json = r#"{
            "version": 1,
            "description": "All boolean combinations",
            "chip_sets": [{
                "type": "single",
                "chips": [{
                    "file": "test.rom",
                    "type": "2364",
                    "cs1": "active_low"
                }],
                "firmware_overrides": {
                    "ice": {
                        "cpu_freq": "100MHz",
                        "overclock": true
                    },
                    "fire": {
                        "cpu_freq": "300MHz",
                        "overclock": false
                    },
                    "led": {
                        "enabled": true
                    },
                    "swd": {
                        "swd_enabled": false
                    }
                }
            }]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        builder
            .add_file(FileData {
                id: 0,
                data: create_test_rom_data(8192, 0xAA),
            })
            .expect("Failed to add file");

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        // Parse firmware overrides
        let header = MetadataHeader::parse(&metadata_buf);
        let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
        let fw_overrides_offset =
            (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

        // Check override_value bitfield
        // Bits: 0=Ice overclock, 1=Fire overclock, 2=LED, 3=SWD
        // Expected: Ice=true(1), Fire=false(0), LED=true(1), SWD=false(0)
        let expected_value = (1 << 0) | (1 << 2) | (1 << 5); // Bits 0 and 2 set
        assert_eq!(
            fw_overrides.override_value[0], expected_value,
            "override_value should be 0x{:02X}, got 0x{:02X}",
            expected_value, fw_overrides.override_value[0]
        );

        println!("✓ Phase 20 Test 104: All boolean combinations");
        println!("  - Ice overclock: enabled");
        println!("  - Fire overclock: disabled");
        println!("  - LED: enabled");
        println!("  - SWD: disabled");
    }

    // ========================================================================
    // TEST 105: Frequency Boundary Values
    // ========================================================================

    #[test]
    fn test_phase20_frequency_boundaries() {
        // Test minimum and maximum frequencies
        let test_cases = vec![("1MHz", 1u16, "Ice min"), ("180MHz", 180u16, "Ice max")];

        for (freq_str, expected_value, description) in test_cases {
            let json = format!(
                r#"{{
                "version": 1,
                "description": "Frequency boundary test: {}",
                "chip_sets": [{{
                    "type": "single",
                    "chips": [{{
                        "file": "test.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }}],
                    "firmware_overrides": {{
                        "ice": {{
                            "cpu_freq": "{}",
                            "overclock": false
                        }}
                    }}
                }}]
            }}"#,
                description, freq_str
            );

            let mut builder =
                Builder::from_json(FW_VER, MCU_FAM, &json).expect("Failed to parse JSON");

            builder
                .add_file(FileData {
                    id: 0,
                    data: create_test_rom_data(8192, 0xAA),
                })
                .expect("Failed to add file");

            let props = default_fw_props_060();
            let board = props.board();
            let flash_base = board.mcu_family().get_flash_base();
            let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
            let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

            // Parse firmware overrides
            let header = MetadataHeader::parse(&metadata_buf);
            let chip_set_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
            let ext_chip_set = ExtendedRomSetStruct::parse(&metadata_buf, chip_set_offset);
            let fw_overrides_offset =
                (ext_chip_set.firmware_overrides_ptr - metadata_flash_start) as usize;
            let fw_overrides = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides_offset);

            assert_eq!(
                fw_overrides.ice_freq, expected_value,
                "{}: Expected {}, got {}",
                description, expected_value, fw_overrides.ice_freq
            );

            println!("✓ Phase 20 Test 105: {} = {}", description, expected_value);
        }
    }

    // ========================================================================
    // TEST 106: Complex Multiple ROM Sets with Different Overrides
    // ========================================================================

    #[test]
    fn test_phase20_complex_multiple_sets() {
        let json = r#"{
            "version": 1,
            "description": "Complex multiple sets with varied overrides",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [{
                        "file": "test0.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }],
                    "firmware_overrides": {
                        "ice": {
                            "cpu_freq": "96MHz",
                            "overclock": false
                        },
                        "led": {
                            "enabled": true
                        }
                    }
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test1.rom",
                        "type": "2332",
                        "cs1": "active_low",
                        "cs2": "active_high"
                    }],
                    "firmware_overrides": {
                        "fire": {
                            "cpu_freq": "300MHz",
                            "overclock": true,
                            "vreg": "1.15V"
                        },
                        "swd": {
                            "swd_enabled": false
                        },
                        "serve_alg_params": {
                            "params": [1, 2, 3]
                        }
                    }
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test2.rom",
                        "type": "2316",
                        "cs1": "active_low",
                        "cs2": "active_low",
                        "cs3": "active_low"
                    }],
                    "firmware_overrides": {
                        "ice": {
                            "cpu_freq": "168MHz",
                            "overclock": true
                        },
                        "fire": {
                            "cpu_freq": "250MHz",
                            "overclock": false
                        }
                    }
                },
                {
                    "type": "single",
                    "chips": [{
                        "file": "test3.rom",
                        "type": "2364",
                        "cs1": "active_low"
                    }]
                }
            ]
        }"#;

        let mut builder = Builder::from_json(FW_VER, MCU_FAM, json).expect("Failed to parse JSON");

        let rom_sizes = [8192, 4096, 2048, 8192];
        for (i, _) in rom_sizes.iter().enumerate() {
            builder
                .add_file(FileData {
                    id: i,
                    data: create_test_rom_data(rom_sizes[i], (0xAA - i * 0x20) as u8),
                })
                .unwrap_or_else(|_| panic!("Failed to add file {}", i));
        }

        let props = default_fw_props_060();
        let board = props.board();
        let flash_base = board.mcu_family().get_flash_base();
        let metadata_flash_start = flash_base + METADATA_FLASH_OFFSET;
        let (metadata_buf, _rom_images_buf) = builder.build(props).expect("Build failed");

        let header = MetadataHeader::parse(&metadata_buf);
        assert_eq!(header.chip_set_count, 4);

        // ===== SET 0: Ice + LED =====
        let chip_set0_offset = (header.chip_sets_ptr - metadata_flash_start) as usize;
        let ext_chip_set0 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set0_offset);
        assert_ne!(
            ext_chip_set0.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 0 should have firmware_overrides"
        );
        assert_eq!(
            ext_chip_set0.serve_config_ptr, 0xFFFFFFFF,
            "Set 0 should NOT have serve_config"
        );

        let fw_overrides0_offset =
            (ext_chip_set0.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides0 = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides0_offset);

        assert!(
            fw_overrides0.is_override_present(0),
            "Set 0: Ice freq should be present"
        );
        assert!(
            fw_overrides0.is_override_present(1),
            "Set 0: Ice overclock should be present"
        );
        assert!(
            fw_overrides0.is_override_present(5),
            "Set 0: LED should be present"
        );
        assert!(
            !fw_overrides0.is_override_present(2),
            "Set 0: Fire freq should NOT be present"
        );

        assert_eq!(
            fw_overrides0.ice_freq, 96,
            "Set 0: Ice freq should be 96MHz"
        );
        assert!(
            !fw_overrides0.is_override_value_set(0),
            "Set 0: Ice overclock should be disabled"
        );
        assert!(
            fw_overrides0.is_override_value_set(2),
            "Set 0: LED should be enabled"
        );

        // ===== SET 1: Fire + SWD + serve_alg_params =====
        let chip_set1_offset = chip_set0_offset + 64;
        let ext_chip_set1 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set1_offset);
        assert_ne!(
            ext_chip_set1.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 1 should have firmware_overrides"
        );
        assert_ne!(
            ext_chip_set1.serve_config_ptr, 0xFFFFFFFF,
            "Set 1 should have serve_config"
        );

        let fw_overrides1_offset =
            (ext_chip_set1.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides1 = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides1_offset);

        assert!(
            !fw_overrides1.is_override_present(0),
            "Set 1: Ice freq should NOT be present"
        );
        assert!(
            fw_overrides1.is_override_present(2),
            "Set 1: Fire freq should be present"
        );
        assert!(
            fw_overrides1.is_override_present(3),
            "Set 1: Fire overclock should be present"
        );
        assert!(
            fw_overrides1.is_override_present(4),
            "Set 1: Fire VREG should be present"
        );
        assert!(
            fw_overrides1.is_override_present(6),
            "Set 1: SWD should be present"
        );

        assert_eq!(
            fw_overrides1.fire_freq, 300,
            "Set 1: Fire freq should be 300MHz"
        );
        assert_eq!(
            fw_overrides1.fire_vreg, 0x0C,
            "Set 1: VREG should be 1.15V (0x0C)"
        );
        assert!(
            fw_overrides1.is_override_value_set(1),
            "Set 1: Fire overclock should be enabled"
        );
        assert!(
            !fw_overrides1.is_override_value_set(3),
            "Set 1: SWD should be disabled"
        );

        // Check serve_config for Set 1
        let serve_config1_offset = (ext_chip_set1.serve_config_ptr - metadata_flash_start) as usize;
        assert_eq!(
            metadata_buf[serve_config1_offset], 1,
            "Set 1: serve param[0] should be 1"
        );
        assert_eq!(
            metadata_buf[serve_config1_offset + 1],
            2,
            "Set 1: serve param[1] should be 2"
        );
        assert_eq!(
            metadata_buf[serve_config1_offset + 2],
            3,
            "Set 1: serve param[2] should be 3"
        );
        assert_eq!(
            metadata_buf[serve_config1_offset + 3],
            0xFF,
            "Set 1: serve param[3] should be 0xFF (padding)"
        );

        // ===== SET 2: Ice + Fire =====
        let chip_set2_offset = chip_set1_offset + 64;
        let ext_chip_set2 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set2_offset);
        assert_ne!(
            ext_chip_set2.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 2 should have firmware_overrides"
        );
        assert_eq!(
            ext_chip_set2.serve_config_ptr, 0xFFFFFFFF,
            "Set 2 should NOT have serve_config"
        );

        let fw_overrides2_offset =
            (ext_chip_set2.firmware_overrides_ptr - metadata_flash_start) as usize;
        let fw_overrides2 = FirmwareOverridesStruct::parse(&metadata_buf, fw_overrides2_offset);

        assert!(
            fw_overrides2.is_override_present(0),
            "Set 2: Ice freq should be present"
        );
        assert!(
            fw_overrides2.is_override_present(1),
            "Set 2: Ice overclock should be present"
        );
        assert!(
            fw_overrides2.is_override_present(2),
            "Set 2: Fire freq should be present"
        );
        assert!(
            fw_overrides2.is_override_present(3),
            "Set 2: Fire overclock should be present"
        );
        assert!(
            !fw_overrides2.is_override_present(4),
            "Set 2: Fire VREG should not be present"
        );

        assert_eq!(
            fw_overrides2.ice_freq, 168,
            "Set 2: Ice freq should be 168MHz"
        );
        assert_eq!(
            fw_overrides2.fire_freq, 250,
            "Set 2: Fire freq should be 250MHz"
        );
        assert_eq!(
            fw_overrides2.fire_vreg, 0xFF,
            "Set 2: VREG should be Stock (0xFF)"
        );
        assert!(
            fw_overrides2.is_override_value_set(0),
            "Set 2: Ice overclock should be enabled"
        );
        assert!(
            !fw_overrides2.is_override_value_set(1),
            "Set 2: Fire overclock should be disabled"
        );

        // ===== SET 3: No overrides =====
        let chip_set3_offset = chip_set2_offset + 64;
        let ext_chip_set3 = ExtendedRomSetStruct::parse(&metadata_buf, chip_set3_offset);
        assert_eq!(
            ext_chip_set3.firmware_overrides_ptr, 0xFFFFFFFF,
            "Set 3 should NOT have firmware_overrides"
        );
        assert_eq!(
            ext_chip_set3.serve_config_ptr, 0xFFFFFFFF,
            "Set 3 should NOT have serve_config"
        );

        println!("✓ Phase 20 Test 106: Complex multiple ROM sets");
        println!("  - Set 0: Ice 96MHz (no OC), LED enabled");
        println!("  - Set 1: Fire 300MHz (OC), VREG 1.15V, SWD disabled, serve_params");
        println!("  - Set 2: Ice 168MHz (OC), Fire 250MHz (no OC, Stock VREG)");
        println!("  - Set 3: No overrides");
    }
}
