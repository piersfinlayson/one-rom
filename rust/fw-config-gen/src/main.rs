// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! pio-test-gen - Generate host C metadata for PIO testing

mod args;

use anyhow::{Result, anyhow};
use clap::Parser as _;
use std::io::Write as _;

use log::debug;
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant as McuVariant};
use onerom_fw::{get_rom_files, read_rom_config};
use onerom_gen::Builder;
use onerom_metadata::{
    DeviceMemoryView, METADATA_BASE, METADATA_SIZE, OneromMetadataHeader, generate_host_metadata_c,
    serialize,
};

use args::Args;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    let version = FirmwareVersion::try_from_str(&args.version)
        .map_err(|e| anyhow!("Invalid firmware version '{}': {e}", args.version))?;

    let board = Board::try_from_str(&args.board)
        .ok_or_else(|| anyhow!("Invalid board '{}'", args.board))?;

    let config_json = read_rom_config(&args.config)
        .map_err(|e| anyhow!("Failed to read ROM config '{}': {e}", args.config))?;

    let mut builder = Builder::from_json(version, Family::Rp2350, &config_json)
        .map_err(|e| anyhow!("Failed to parse ROM config: {e}"))?;

    // Licenses are not supported by this tool
    let licenses = builder.licenses();
    assert!(
        licenses.is_empty(),
        "ROM config contains licenses — not supported by fw-config-gen"
    );

    get_rom_files(&mut builder).map_err(|e| anyhow!("Failed to load ROM files: {e}"))?;

    let props = FirmwareProperties::new(
        version,
        board,
        McuVariant::RP2350,
        ServeAlg::default(),
        args.boot_logging,
    )
    .map_err(|e| anyhow!("Failed to create firmware properties: {e}"))?;

    let (metadata_buf, rom_data_buf) = builder
        .build(props)
        .map_err(|e| anyhow!("Build failed: {e}"))?;

    // Parse metadata back
    let view = DeviceMemoryView::new(&metadata_buf, METADATA_BASE);
    let header = OneromMetadataHeader::parse(&view, METADATA_BASE)
        .map_err(|e| anyhow!("Failed to parse generated metadata: {e:?}"))?;

    // Round-trip check: re-serialize and compare bytes
    let mut re_serialized = vec![0u8; METADATA_SIZE];
    serialize(&header, METADATA_BASE, &mut re_serialized)
        .map_err(|e| anyhow!("Round-trip re-serialize failed: {e:?}"))?;
    assert_eq!(
        metadata_buf, re_serialized,
        "Round-trip check failed: re-serialized bytes do not match original"
    );

    debug!("Round-trip check passed");

    // Split flat ROM data into per-slot chunks using sizes from parsed header
    let mut offset = 0usize;
    let slot_data: Vec<Vec<u8>> = header
        .rom_slots
        .iter()
        .map(|slot| {
            let sz = slot.size as usize;
            let chunk = rom_data_buf[offset..offset + sz].to_vec();
            offset += sz;
            chunk
        })
        .collect();

    // Generate C source and write to output
    let c_src = generate_host_metadata_c(&header, slot_data);

    std::fs::write(&args.output, c_src.as_bytes())
        .map_err(|e| anyhow!("Failed to write '{}': {e}", args.output))?;

    println!("Generated host metadata C: {}", args.output);

    Ok(())
}

fn init_logging(verbose: bool) {
    let mut b = env_logger::Builder::from_default_env();
    b.filter_level(if verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
    b.format(|buf, record| {
        let level = format!("{}: ", record.level());
        writeln!(buf, "{:07}{}", level, record.args())
    });
    b.init();
}
