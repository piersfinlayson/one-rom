// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The gating fixture's device types.
//!
//! Neither real schema has a field gated on a metadata generation, so this is
//! the only place such a field reaches the device generator.  The test is that
//! the generated code compiles.

// The generated code names `crate::Ptr` and `crate::RuntimeCell`, as it does
// in onerom-metadata, whose schema also fills the info slot.
#[allow(unused_imports)]
use onerom_metadata::{Ptr, RuntimeCell};

#[allow(dead_code, non_camel_case_types)]
mod fixture {
    include!(concat!(
        env!("OUT_DIR"),
        "/gating_fixture_device_generated.rs"
    ));
}

#[test]
fn the_fixtures_device_types_exist() {
    assert!(size_of::<fixture::onerom_metadata_header_t>() > 0);
}
