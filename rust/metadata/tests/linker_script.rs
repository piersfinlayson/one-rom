// tests/linker_script.rs
//
// Tests for the linker-script fragment, which gives One ROM's linker scripts
// the schema constants they use.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::ONEROM_INFO_OFFSET;
use onerom_metadata_gen::linker_gen;
use onerom_metadata_gen::schema::Schema;

/// The linker script places onerom_info_t with this line, and a host reads
/// onerom_info_t at the Rust constant, so both must hold the same number.
#[test]
fn the_fragment_defines_onerom_info_offset() {
    let schema = Schema::parse(include_str!("../metadata_schema.toml"))
        .expect("the shipped schema should have been accepted");
    let fragment = linker_gen::generate(&schema);
    let line = format!("ONEROM_INFO_OFFSET = {ONEROM_INFO_OFFSET:#X};");
    assert!(
        fragment.contains(&line),
        "the fragment carries no '{line}':\n{fragment}"
    );
}
