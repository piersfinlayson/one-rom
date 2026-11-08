// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect info` on a running One ROM whose flash contains `onerom_info_t`
//! but doesn't contain metadata.

use onerom_fw_parser::readers::{MemoryReader, RegionKind};
use onerom_fw_parser::{Parser, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, FirmwareType, ONEROM_FAMILY_MAGIC,
    ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF, ONEROM_INFO_RUNTIME_OFFSET as RUNTIME_OFF,
    ONEROM_INFO_VERSION, ONEROM_INFO_VERSION_OFFSET as VERSION_OFF, ONEROM_RUNTIME_INFO_SIZE,
    RUNTIME_INFO_VERSION,
};

use crate::inspect::print_onerom_info;

const RAM_BASE: u32 = 0x2008_0000;

#[tokio::test]
async fn runtime_info() {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let mut put = |offset: usize, bytes: &[u8]| {
        image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, ONEROM_FAMILY_MAGIC.as_bytes());
    put(6, &8u16.to_le_bytes()); // minor version
    // build_date points into the zeroed tail of the image so the string is empty
    put(12, &(FLASH_CS0_BASE_ADDR + 0x300).to_le_bytes());
    put(RUNTIME_OFF, &RAM_BASE.to_le_bytes());
    put(VERSION_OFF, &ONEROM_INFO_VERSION.to_le_bytes());
    put(
        TYPE_OFF,
        &(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes(),
    );

    let mut ram = vec![0u8; ONEROM_RUNTIME_INFO_SIZE];
    ram[0..4].copy_from_slice(b"sdrr");
    ram[4..8].copy_from_slice(&RUNTIME_INFO_VERSION.to_le_bytes());
    ram[38] = 1; // bit_mode: 8-bit

    let mut reader = MemoryReader::new(image, FLASH_CS0_BASE_ADDR);
    reader.add_region(RegionKind::Ram, ram, RAM_BASE);
    let parsed = Parser::with_base_flash_address(&mut reader, FLASH_CS0_BASE_ADDR, RAM_BASE)
        .parse_device()
        .await;

    println!("$ onerom inspect info");
    print_onerom_info(&parsed).unwrap();
}
