// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Constants from onerom-gen.
//!
//! onerom-gen doesn't list its constants as the metadata schema does, so each
//! one a document states is listed here.

/// Each constant a document can state, with its value.
const CONSTANTS: &[(&str, u64)] = &[(
    "MAX_ADDR_MON_MIN_CS_ACT_NS",
    onerom_gen::MAX_ADDR_MON_MIN_CS_ACT_NS as u64,
)];

/// The value of an onerom-gen constant, as text.
pub fn resolve(name: &str) -> Result<String, String> {
    CONSTANTS
        .iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| {
            let known: Vec<&str> = CONSTANTS.iter().map(|(name, _)| *name).collect();
            format!(
                "unknown onerom-gen constant '{name}' - known constants are {}",
                known.join(", ")
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_constant_resolves_to_its_value() {
        assert_eq!(
            resolve("MAX_ADDR_MON_MIN_CS_ACT_NS").unwrap(),
            onerom_gen::MAX_ADDR_MON_MIN_CS_ACT_NS.to_string()
        );
    }

    #[test]
    fn an_unlisted_name_lists_the_ones_there_are() {
        let err = resolve("MIN_ADDR_MON_MIN_CS_ACT_VERSION").unwrap_err();
        assert!(err.contains("MIN_ADDR_MON_MIN_CS_ACT_VERSION"), "{err}");
        assert!(err.contains("MAX_ADDR_MON_MIN_CS_ACT_NS"), "{err}");
    }
}
