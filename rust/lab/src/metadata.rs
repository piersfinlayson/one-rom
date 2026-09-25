// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! What a host reads to identify a One ROM Lab.
//!
//! `ONEROM_LAB_INFO` is Lab's `onerom_info_t`, which the linker script places
//! at `ONEROM_INFO_OFFSET` in flash.  Its `firmware_type` says the
//! firmware is Lab, and its `metadata` and `runtime` pointers point at Lab's
//! own structures.  All the layouts are generated from the metadata schemas.

use core::ffi::{CStr, c_char};

use onerom_config::hw::Board;
use onerom_lab_metadata::{
    LAB_METADATA_VERSION, LAB_RUNTIME_INFO_VERSION, ONEROM_LAB_RUNTIME_INFO_SIZE,
    onerom_lab_hardware_info_t, onerom_lab_info_t, onerom_lab_metadata_header_t,
    onerom_lab_runtime_info_t,
};
use onerom_metadata::{ONEROM_INFO_VERSION, Ptr, RuntimeCell, firmware_type_t};

// BAKED_BOARD, VERSION_*, BUILD_DATE, COMMIT and board_c_name, from build.rs.
include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

unsafe extern "C" {
    /// rtt-target's control block, which `logs::init_rtt` sets up.
    static _SEGGER_RTT: u8;
}

#[unsafe(link_section = ".onerom_info")]
#[used]
static ONEROM_LAB_INFO: onerom_lab_info_t = onerom_lab_info_t {
    magic: onerom_lab_info_t::MAGIC,
    major_version: VERSION_MAJOR,
    minor_version: VERSION_MINOR,
    patch_version: VERSION_PATCH,
    build_number: 0,
    build_date: Ptr::from_cstr(BUILD_DATE),
    commit: COMMIT,
    version: ONEROM_INFO_VERSION,
    metadata: Some(Ptr::new(&LAB_METADATA)),
    // SAFETY: only the address is taken.
    rtt: Some(Ptr::new(unsafe { &_SEGGER_RTT }).cast()),
    runtime: Some(Ptr::new(&LAB_RUNTIME)),
    firmware_type: firmware_type_t::FIRMWARE_TYPE_LAB,
    reserved: [0xFF; 22],
};

static LAB_METADATA: onerom_lab_metadata_header_t = onerom_lab_metadata_header_t {
    magic: onerom_lab_metadata_header_t::MAGIC,
    version: LAB_METADATA_VERSION,
    hw: Ptr::new(&LAB_HW),
    reserved: [0xFF; 232],
};

static LAB_HW: onerom_lab_hardware_info_t = onerom_lab_hardware_info_t {
    hw_rev: BAKED_HW_REV,
    reserved: [0xFF; 252],
};

static LAB_RUNTIME: RuntimeCell<onerom_lab_runtime_info_t> =
    RuntimeCell::new(onerom_lab_runtime_info_t {
        magic: onerom_lab_runtime_info_t::MAGIC,
        version: LAB_RUNTIME_INFO_VERSION,
        runtime_info_size: ONEROM_LAB_RUNTIME_INFO_SIZE as u32,
        hw_rev: BAKED_HW_REV,
        reserved: [0xFF; 240],
    });

/// The baked board's name, the starting value of both `hw_rev` fields.
const BAKED_HW_REV: Option<Ptr<c_char>> = match BAKED_BOARD {
    Some(board) => Some(Ptr::from_cstr(board_c_name(board))),
    None => None,
};

/// Record `board` in the runtime structure as the board Lab is running as.
pub fn set_board(board: Board) {
    let hw_rev = Some(Ptr::from_cstr(board_c_name(board)));
    // SAFETY: LAB_RUNTIME lives for the whole program, and nothing else writes
    // it.  The write is volatile because a host reads it.
    unsafe { (&raw mut (*LAB_RUNTIME.as_mut_ptr()).hw_rev).write_volatile(hw_rev) }
}
