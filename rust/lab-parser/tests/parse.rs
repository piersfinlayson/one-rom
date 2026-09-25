// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The Lab parser against images with Lab's layout.  The header is at
//! `ONEROM_INFO_OFFSET`.  The metadata block follows it.  The runtime structure
//! is in RAM.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use airfrog_rpc::io::Reader;
use onerom_lab_metadata::{
    LAB_METADATA_MAGIC, LAB_METADATA_SIZE, LAB_METADATA_VERSION, LAB_RUNTIME_INFO_MAGIC,
    LAB_RUNTIME_INFO_VERSION, ONEROM_LAB_RUNTIME_INFO_SIZE, OneromLabHardwareInfo,
    OneromLabMetadataHeader, SerializeContext,
};
use onerom_lab_parser::{Lab, LabParser};
use onerom_metadata::{
    NewerGeneration, ONEROM_FAMILY_MAGIC, ONEROM_INFO_BUILD_DATE_OFFSET,
    ONEROM_INFO_FIRMWARE_TYPE_OFFSET, ONEROM_INFO_MAGIC, ONEROM_INFO_MAJOR_VERSION_OFFSET,
    ONEROM_INFO_METADATA_OFFSET, ONEROM_INFO_MINOR_VERSION_OFFSET, ONEROM_INFO_OFFSET,
    ONEROM_INFO_RUNTIME_OFFSET, ONEROM_INFO_VERSION, ONEROM_INFO_VERSION_OFFSET, ParseError,
    RuntimeAbsence, firmware_type_t,
};

const FLASH: u32 = 0x1000_0000;
const HEADER: u32 = FLASH + ONEROM_INFO_OFFSET;
const BLOCK: u32 = HEADER + 0x40;
const BUILD_DATE: u32 = FLASH + 0x1800;
const RAM: u32 = 0x2000_1000;
const IMAGE_LEN: usize = 0x2000;

/// A board name in the block beyond what the serializer writes.  The runtime
/// structure points at it.
const LIVE_BOARD: u32 = BLOCK + 0x800;

/// Runs a future to completion.  The reader is backed by memory and never
/// pends so one poll always completes.
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

/// Memory regions at fixed addresses.  A read outside them fails like a read
/// of RAM from an image file.
struct Regions(Vec<(u32, Vec<u8>)>);

impl Reader for Regions {
    type Error = ();

    async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), ()> {
        for (base, data) in &self.0 {
            let Some(offset) = addr.checked_sub(*base).map(|o| o as usize) else {
                continue;
            };
            if offset + buf.len() <= data.len() {
                buf.copy_from_slice(&data[offset..offset + buf.len()]);
                return Ok(());
            }
        }
        Err(())
    }

    fn update_base_address(&mut self, _base_address: u32) {}
}

/// A Lab as its firmware writes one: running as a board other than the one
/// it was built for.
struct Image {
    flash: Vec<u8>,
    ram: Option<Vec<u8>>,
}

impl Image {
    fn lab() -> Self {
        let mut flash = vec![0xFFu8; IMAGE_LEN];
        let header = (HEADER - FLASH) as usize;
        let put = |flash: &mut Vec<u8>, offset: usize, bytes: &[u8]| {
            flash[header + offset..header + offset + bytes.len()].copy_from_slice(bytes);
        };
        put(&mut flash, 0, ONEROM_FAMILY_MAGIC.as_bytes());
        put(
            &mut flash,
            ONEROM_INFO_MAJOR_VERSION_OFFSET,
            &0u16.to_le_bytes(),
        );
        put(
            &mut flash,
            ONEROM_INFO_MINOR_VERSION_OFFSET,
            &4u16.to_le_bytes(),
        );
        put(
            &mut flash,
            ONEROM_INFO_BUILD_DATE_OFFSET,
            &BUILD_DATE.to_le_bytes(),
        );
        put(
            &mut flash,
            ONEROM_INFO_VERSION_OFFSET,
            &ONEROM_INFO_VERSION.to_le_bytes(),
        );
        put(
            &mut flash,
            ONEROM_INFO_METADATA_OFFSET,
            &BLOCK.to_le_bytes(),
        );
        put(&mut flash, ONEROM_INFO_RUNTIME_OFFSET, &RAM.to_le_bytes());
        put(
            &mut flash,
            ONEROM_INFO_FIRMWARE_TYPE_OFFSET,
            &firmware_type_t::FIRMWARE_TYPE_LAB.0.to_le_bytes(),
        );

        let date = b"Sep 25 2026 12:00:00Z\0";
        let at = (BUILD_DATE - FLASH) as usize;
        flash[at..at + date.len()].copy_from_slice(date);

        let block = compose_block();
        let at = (BLOCK - FLASH) as usize;
        flash[at..at + block.len()].copy_from_slice(&block);

        let mut image = Self {
            flash,
            ram: Some(runtime()),
        };
        image.write_flash(LIVE_BOARD, b"fire-24-e\0");
        image
    }

    fn write_flash(&mut self, addr: u32, bytes: &[u8]) {
        let at = (addr - FLASH) as usize;
        self.flash[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn write_header(&mut self, offset: usize, bytes: &[u8]) {
        self.write_flash(HEADER + offset as u32, bytes);
    }

    fn parse(self) -> Result<Lab, String> {
        let mut regions = vec![(FLASH, self.flash)];
        if let Some(ram) = self.ram {
            regions.push((RAM, ram));
        }
        let mut reader = Regions(regions);
        block_on(LabParser::new(&mut reader).parse())
    }
}

/// Lab's metadata block from Lab's generated serializer.
fn compose_block() -> Vec<u8> {
    let mut magic = [0u8; 16];
    magic[..LAB_METADATA_MAGIC.len()].copy_from_slice(LAB_METADATA_MAGIC.as_bytes());
    let header = OneromLabMetadataHeader {
        magic,
        version: LAB_METADATA_VERSION,
        hw: OneromLabHardwareInfo {
            hw_rev: Some("fire-40-a".into()),
        },
    };

    let mut buf = vec![0xFFu8; LAB_METADATA_SIZE as usize];
    header.check_generation(header.version).unwrap();
    let mut ctx = SerializeContext::new(BLOCK, header.version, &mut buf);
    header.layout(&mut ctx).unwrap();
    header.write(&mut ctx, BLOCK);
    buf
}

/// The runtime structure as a running Lab holds it.  It doesn't have a serializer so
/// its fields are written by hand in the schema's order.
fn runtime() -> Vec<u8> {
    let mut ram = vec![0xFFu8; ONEROM_LAB_RUNTIME_INFO_SIZE];
    ram[0..4].copy_from_slice(LAB_RUNTIME_INFO_MAGIC.as_bytes());
    ram[4..8].copy_from_slice(&LAB_RUNTIME_INFO_VERSION.to_le_bytes());
    ram[8..12].copy_from_slice(&(ONEROM_LAB_RUNTIME_INFO_SIZE as u32).to_le_bytes());
    ram[12..16].copy_from_slice(&LIVE_BOARD.to_le_bytes());
    ram
}

#[test]
fn a_lab_parses() {
    let lab = Image::lab().parse().expect("a Lab should parse");
    let metadata = lab.metadata.as_ref().expect("metadata should parse");
    assert_eq!(metadata.hw.hw_rev.as_deref(), Some("fire-40-a"));
    let runtime = lab.runtime.as_ref().expect("runtime should parse");
    assert_eq!(runtime.hw_rev.as_deref(), Some("fire-24-e"));
    assert!(lab.newer_generations().is_empty());
}

#[test]
fn a_one_rom_is_not_a_lab() {
    let mut image = Image::lab();
    image.write_header(ONEROM_INFO_FIRMWARE_TYPE_OFFSET, &0xFFFFu16.to_le_bytes());
    let err = image.parse().unwrap_err();
    assert!(err.contains("Not a One ROM Lab"), "unexpected error: {err}");
}

/// A generation 2 header has only padding where firmware_type later went.
/// Padding holding Lab's value must not make it a Lab.
#[test]
fn a_header_older_than_firmware_type_is_not_a_lab() {
    let mut image = Image::lab();
    image.write_header(0, ONEROM_INFO_MAGIC.as_bytes());
    image.write_header(ONEROM_INFO_MINOR_VERSION_OFFSET, &7u16.to_le_bytes());
    image.write_header(ONEROM_INFO_VERSION_OFFSET, &2u32.to_le_bytes());
    let err = image.parse().unwrap_err();
    assert!(err.contains("Not a One ROM Lab"), "unexpected error: {err}");
}

#[test]
fn another_magic_is_not_the_family() {
    let mut image = Image::lab();
    image.write_header(0, b"WXYZ");
    let err = image.parse().unwrap_err();
    assert!(
        err.contains("Not a One ROM family image"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_null_metadata_pointer_is_an_error() {
    let mut image = Image::lab();
    image.write_header(ONEROM_INFO_METADATA_OFFSET, &0u32.to_le_bytes());
    let lab = image.parse().expect("a Lab should parse");
    assert_eq!(
        lab.metadata.unwrap_err(),
        ParseError::NullPointer { field: "metadata" }
    );
}

/// The parser reads only the block.
#[test]
fn metadata_reaching_outside_the_block_does_not_parse() {
    let mut image = Image::lab();
    // `hw` follows the 16-byte magic and the 4-byte version.
    let outside = BLOCK + LAB_METADATA_SIZE;
    image.write_flash(BLOCK + 20, &outside.to_le_bytes());
    let lab = image.parse().expect("a Lab should parse");
    assert!(matches!(
        lab.metadata,
        Err(ParseError::OutOfBounds { addr, .. }) if addr == outside
    ));
}

#[test]
fn a_null_runtime_pointer_says_so() {
    let mut image = Image::lab();
    image.write_header(ONEROM_INFO_RUNTIME_OFFSET, &0u32.to_le_bytes());
    let lab = image.parse().expect("a Lab should parse");
    assert_eq!(lab.runtime.unwrap_err(), RuntimeAbsence::NoPointer);
}

/// An image file doesn't hold RAM.
#[test]
fn an_image_file_has_metadata_and_no_runtime() {
    let mut image = Image::lab();
    image.ram = None;
    let lab = image.parse().expect("a Lab should parse");
    assert!(lab.metadata.is_ok());
    assert_eq!(lab.runtime.unwrap_err(), RuntimeAbsence::Unreadable);
}

#[test]
fn ram_without_the_runtime_magic_is_a_stopped_lab() {
    let mut image = Image::lab();
    image.ram = Some(vec![0u8; ONEROM_LAB_RUNTIME_INFO_SIZE]);
    let lab = image.parse().expect("a Lab should parse");
    assert_eq!(lab.runtime.unwrap_err(), RuntimeAbsence::NotRunning);
}

#[test]
fn a_runtime_reaching_outside_the_block_is_unparsed() {
    let mut image = Image::lab();
    let mut ram = runtime();
    ram[12..16].copy_from_slice(&BUILD_DATE.to_le_bytes());
    image.ram = Some(ram);
    let lab = image.parse().expect("a Lab should parse");
    assert_eq!(lab.runtime.unwrap_err(), RuntimeAbsence::Unparsed);
}

#[test]
fn a_newer_metadata_generation_is_reported() {
    let mut image = Image::lab();
    // `version` follows the 16-byte magic.
    image.write_flash(BLOCK + 16, &(LAB_METADATA_VERSION + 1).to_le_bytes());
    let lab = image.parse().expect("a Lab should parse");
    assert_eq!(
        lab.newer_generations(),
        [NewerGeneration {
            structure: "onerom_lab_metadata_header_t",
            device_generation: LAB_METADATA_VERSION + 1,
            known_generation: LAB_METADATA_VERSION,
        }]
    );
}
