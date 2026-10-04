// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the board size a device's runtime info records.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_fw_parser::readers::{MemoryReader, RegionKind};
use onerom_fw_parser::{
    McuLine, McuStorage, ParsedDevice, Parser, SDRR_INFO_FW_OFFSET, Sdrr, SdrrInfo, SdrrRuntimeInfo,
};
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, FirmwareType, MaybeKnown, ONEROM_FAMILY_MAGIC,
    ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF, ONEROM_INFO_RUNTIME_OFFSET as RUNTIME_OFF,
    ONEROM_INFO_VERSION, ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
    ONEROM_RUNTIME_INFO_BOARD_SIZE_OFFSET, OneromBoardSize, RUNTIME_INFO_VERSION,
};

const RP235X_FLASH_BASE: u32 = FLASH_CS0_BASE_ADDR;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

/// Runs a future to completion.  The reader is backed by memory and never
/// pends, so one poll always completes.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory reader pended"),
    }
}

/// A v0.8.0 One ROM parsed from flash containing `onerom_info_t` without
/// metadata, and from RAM containing `runtime` where it's `Some`.
fn schema_device(runtime: Option<Vec<u8>>) -> ParsedDevice {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let mut put = |offset: usize, bytes: &[u8]| {
        image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, ONEROM_FAMILY_MAGIC.as_bytes());
    put(6, &8u16.to_le_bytes()); // minor version
    // build_date, into the zeroed tail of the image
    put(12, &(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    put(RUNTIME_OFF, &RP235X_RAM_BASE.to_le_bytes());
    put(VERSION_OFF, &ONEROM_INFO_VERSION.to_le_bytes());
    put(
        TYPE_OFF,
        &(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes(),
    );

    let mut reader = MemoryReader::new(image, RP235X_FLASH_BASE);
    if let Some(ram) = runtime {
        reader.add_region(RegionKind::Ram, ram, RP235X_RAM_BASE);
    }
    block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
            .parse_device(),
    )
}

/// `onerom_runtime_info_t` recording board size `size`.
fn runtime_info(size: u8) -> Vec<u8> {
    let mut ram = vec![0u8; 64];
    ram[0..4].copy_from_slice(b"sdrr");
    ram[4..8].copy_from_slice(&RUNTIME_INFO_VERSION.to_le_bytes());
    ram[38] = 1; // bit_mode: 8-bit
    ram[ONEROM_RUNTIME_INFO_BOARD_SIZE_OFFSET] = size;
    ram
}

/// A pre-v0.7.0 One ROM, with runtime info where `running`.
fn original_device(running: bool) -> ParsedDevice {
    let ram = running.then_some(SdrrRuntimeInfo {
        image_sel: 0,
        rom_set_index: 0,
        count_rom_access: 0,
        last_parsed_access_count: 0,
        account_count_address: 0,
        rom_table_address: 0,
        rom_table_size: 0,
        overclock_enabled: None,
        status_led_enabled: None,
        swd_enabled: None,
        fire_vreg: None,
        ice_freq_mhz: None,
        fire_freq_mhz: None,
        sysclk_mhz: None,
        fire_serve_mode: None,
        bit_mode: None,
        rom_dma_copy: None,
        num_data_pins: None,
        force_16_bit: None,
        peri_en: None,
        limp_mode: None,
    });
    ParsedDevice::Original(Sdrr {
        flash: Some(SdrrInfo {
            major_version: 0,
            minor_version: 6,
            patch_version: 0,
            build_number: 0,
            commit: [0; 8],
            stm_line: McuLine::Rp2350,
            stm_storage: McuStorage::Storage2MB,
            freq: 150,
            overclock: false,
            swd_enabled: true,
            preload_image_to_ram: true,
            bootloader_capable: false,
            status_led_enabled: false,
            boot_logging_enabled: false,
            mco_enabled: false,
            rom_set_count: 0,
            count_rom_access: false,
            boot_config: [0; 4],
            build_date: None,
            hw_rev: None,
            rom_sets: Vec::new(),
            pins: None,
            parse_errors: Vec::new(),
            extra_info: None,
            metadata_present: true,
            version: onerom_config::fw::FirmwareVersion::new(0, 6, 0, 0),
            board: None,
            model: None,
            mcu_variant: None,
            runtime_info_ptr: 0,
        }),
        ram,
    })
}

#[test]
fn a_running_one_rom_has_the_size_its_runtime_info_records() {
    for (raw, size) in [
        (1, MaybeKnown::Known(OneromBoardSize::BoardSizeM)),
        (2, MaybeKnown::Known(OneromBoardSize::BoardSizeL)),
        (0, MaybeKnown::Known(OneromBoardSize::BoardSizeUnknown)),
        (0x37, MaybeKnown::Unknown(0x37)),
    ] {
        let device = schema_device(Some(runtime_info(raw)));
        assert!(device.is_running(), "{raw}");
        assert_eq!(device.runtime_board_size(), Some(size), "{raw}");
    }
}

#[test]
fn a_stopped_one_rom_doesnt_have_a_runtime_size() {
    let device = schema_device(None);
    assert!(device.is_recognised());
    assert!(!device.is_running());
    assert_eq!(device.runtime_board_size(), None);
}

/// Firmware before 0.7.0 doesn't record a board size.
#[test]
fn an_original_format_one_rom_running_has_an_unknown_size() {
    assert_eq!(
        original_device(true).runtime_board_size(),
        Some(MaybeKnown::Known(OneromBoardSize::BoardSizeUnknown))
    );
    assert_eq!(original_device(false).runtime_board_size(), None);
}

#[test]
fn a_lab_doesnt_have_a_runtime_size() {
    assert_eq!(ParsedDevice::Lab.runtime_board_size(), None);
}
