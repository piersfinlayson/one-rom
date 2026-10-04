// tests/linker_script.rs
//
// Tests for the two linker-script fragments, which provide the firmware's and
// plugins' linker scripts with the schema constants they use.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::{
    ONEROM_INFO_OFFSET, SYSTEM_PLUGIN_OFFSET, SYSTEM_PLUGIN_SIZE, USER_PLUGIN_OFFSET,
    USER_PLUGIN_SIZE,
};
use onerom_metadata_gen::schema::Schema;
use onerom_metadata_gen::{linker_constants_gen, linker_gen};

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

/// The plugin regions reach a plugin's linker script through the plugin-facing
/// fragment, so its numbers must match the Rust constants.  A constant outside
/// the plugin API must not reach the file at all.
#[test]
fn the_plugin_fragment_defines_the_plugin_regions() {
    let schema = Schema::parse(include_str!("../metadata_schema.toml"))
        .expect("the shipped schema should have been accepted");
    let fragment = linker_constants_gen::generate(&schema);

    for line in [
        format!("ORA_SYSTEM_PLUGIN_OFFSET = {SYSTEM_PLUGIN_OFFSET:#X};"),
        format!("ORA_SYSTEM_PLUGIN_SIZE = {SYSTEM_PLUGIN_SIZE:#X};"),
        format!("ORA_USER_PLUGIN_OFFSET = {USER_PLUGIN_OFFSET:#X};"),
        format!("ORA_USER_PLUGIN_SIZE = {USER_PLUGIN_SIZE:#X};"),
    ] {
        assert!(
            fragment.contains(&line),
            "the plugin fragment doesn't contain '{line}':\n{fragment}"
        );
    }

    for (name, _) in fragment.lines().filter_map(|l| l.split_once(" = ")) {
        let constant = name
            .strip_prefix("ORA_")
            .and_then(|name| schema.constants.iter().find(|c| c.name == name));
        assert!(
            constant.is_some_and(|c| c.ora_api && c.linker_script),
            "the plugin fragment defines {name}, which isn't an ora_api linker_script constant"
        );
    }
}
