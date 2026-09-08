// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Constants from the metadata schema.
//!
//! `rust/metadata/metadata_schema.toml` declares each of these once and emits
//! it to the firmware, to Rust and - where a plugin needs it - to the plugin
//! API. A document that states one is a fourth copy, and this is what compares
//! it with the other three.

/// The value of a schema constant, as text.
pub fn resolve(name: &str) -> Result<String, String> {
    onerom_metadata::ALL_CONSTANTS
        .iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, value)| (*value).to_string())
        .ok_or_else(|| {
            format!(
                "no constant '{name}' in the metadata schema - check \
                 rust/metadata/metadata_schema.toml"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_constant_resolves_to_its_value() {
        assert_eq!(resolve("GPIO_RESET_DEFAULT_HOLD_MS").unwrap(), "100");
        assert_eq!(resolve("LED_MAX_HOLD_MS").unwrap(), "60000");
    }

    #[test]
    fn a_name_no_longer_declared_says_where_to_look() {
        let err = resolve("LED_MAX_HOLD_SECONDS").unwrap_err();
        assert!(err.contains("LED_MAX_HOLD_SECONDS"), "{err}");
        assert!(err.contains("metadata_schema.toml"), "{err}");
    }
}
