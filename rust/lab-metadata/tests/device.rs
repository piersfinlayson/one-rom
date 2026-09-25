// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Lab's structures and header declared as statics, with the header's
//! pointers at Lab's own structures.  That this compiles is most of the test:
//! it holds the type parameters, the runtime cell, the `Sync` bounds and the
//! const constructors to what a firmware needs.

use core::ffi::CStr;

use onerom_lab_metadata::{
    LAB_METADATA_VERSION, LAB_RUNTIME_INFO_VERSION, ONEROM_LAB_RUNTIME_INFO_SIZE,
    onerom_lab_hardware_info_t, onerom_lab_info_t, onerom_lab_metadata_header_t,
    onerom_lab_runtime_info_t, rp235x_variant_t,
};
use onerom_metadata::{FirmwareType, ONEROM_INFO_VERSION, Ptr, RuntimeCell, firmware_type_t};

static HW: onerom_lab_hardware_info_t = onerom_lab_hardware_info_t {
    hw_rev: Some(Ptr::from_cstr(c"fire-40-a")),
    rp235x: rp235x_variant_t::RP235XB,
    reserved: [0xFF; 251],
};

static HEADER: onerom_lab_metadata_header_t = onerom_lab_metadata_header_t {
    magic: onerom_lab_metadata_header_t::MAGIC,
    version: LAB_METADATA_VERSION,
    hw: Ptr::new(&HW),
    reserved: [0xFF; 232],
};

static RUNTIME: RuntimeCell<onerom_lab_runtime_info_t> =
    RuntimeCell::new(onerom_lab_runtime_info_t {
        magic: onerom_lab_runtime_info_t::MAGIC,
        version: LAB_RUNTIME_INFO_VERSION,
        runtime_info_size: ONEROM_LAB_RUNTIME_INFO_SIZE as u32,
        hw_rev: None,
        reserved: [0xFF; 240],
    });

static INFO: onerom_lab_info_t = onerom_lab_info_t {
    magic: onerom_lab_info_t::MAGIC,
    major_version: 0,
    minor_version: 4,
    patch_version: 0,
    build_number: 0,
    build_date: Ptr::from_cstr(c"Sep 23 2026 12:00:00Z"),
    commit: [0; 8],
    version: ONEROM_INFO_VERSION,
    metadata: Some(Ptr::new(&HEADER)),
    rtt: None,
    runtime: Some(Ptr::new(&RUNTIME)),
    firmware_type: firmware_type_t::FIRMWARE_TYPE_LAB,
    reserved: [0xFF; 22],
};

#[test]
fn the_anchor_points_at_labs_structures() {
    let metadata = INFO.metadata.expect("metadata is set");
    let runtime = INFO.runtime.expect("runtime is set");
    assert!(core::ptr::eq(metadata.as_ptr(), &HEADER));
    assert!(core::ptr::eq(runtime.as_ptr(), &RUNTIME));
    assert!(core::ptr::eq(HEADER.hw.as_ptr(), &HW));
    assert_eq!(
        FirmwareType::try_from(INFO.firmware_type.0),
        Ok(FirmwareType::FirmwareTypeLab)
    );
}

#[test]
fn string_fields_hold_their_strings() {
    let hw_rev = HW.hw_rev.expect("hw_rev is set").as_ptr();
    // SAFETY: both point at static C string literals.
    let (hw_rev, build_date) = unsafe {
        (
            CStr::from_ptr(hw_rev),
            CStr::from_ptr(INFO.build_date.as_ptr()),
        )
    };
    assert_eq!(hw_rev, c"fire-40-a");
    assert_eq!(build_date, c"Sep 23 2026 12:00:00Z");
}
