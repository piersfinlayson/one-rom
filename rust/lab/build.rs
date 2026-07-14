// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Re-run this build script if anything in git changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    // Set up linking
    println!("cargo:rustc-link-arg=-v");
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");

    // Re-run this build script of DEFMT_LOG changes.
    println!("cargo:rerun-if-env-changed=DEFMT_LOG");

    // Create a map file
    let out_dir = env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-arg=-Map={out_dir}/output.map");
    // Generate memory.x
    generate_rp2350_memory_x();

    // Generate built information
    built::write_built_file().expect("Failed to acquire build-time information");
}

fn generate_rp2350_memory_x() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let memory_path = Path::new(&out_dir).join("memory.x");

    let memory_x = r#"
MEMORY {
    /*
     * The RP2350 has either external or internal flash.
     *
     * 2 MiB is a safe default here, although a Pico 2 has 4 MiB.
     */
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /*
     * RAM consists of 8 banks, SRAM0-SRAM7, with a striped mapping.
     * This is usually good for performance, as it distributes load on
     * those banks evenly.
     */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /*
     * RAM banks 8 and 9 use a direct mapping. They can be used to have
     * memory areas dedicated for some specific job, improving predictability
     * of access times.
     * Example: Separate stacks for core0 and core1.
     */
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* ### Boot ROM info
     *
     * Goes after .vector_table, to keep it in the first 4K of flash
     * where the Boot ROM (and picotool) can find it
     */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* move .text to start /after/ the boot info */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* ### Picotool 'Binary Info' Entries
     *
     * Picotool looks through this block (as we have pointers to it in our
     * header) to find interesting information.
     */
    .bi_entries : ALIGN(4)
    {
        /* We put this in the header */
        __bi_entries_start = .;
        /* Here are the entries */
        KEEP(*(.bi_entries));
        /* Keep this block a nice round size */
        . = ALIGN(4);
        /* We put this in the header */
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* ### Boot ROM extra info
     *
     * Goes after everything in our program, so it can contain a signature.
     */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH

} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
    "#;

    fs::write(memory_path, memory_x).unwrap();
    println!("cargo:rustc-link-search={out_dir}");
}
