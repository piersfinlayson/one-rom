// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Where a value in a document comes from.
//!
//! A span names its source - `const:GPIO_MAX_HOLD_MS`, `version:cli` - so a
//! value that comes from somewhere new needs a provider here and no change to
//! any marker already written. Without the prefix every name would share one
//! namespace fed by one crate, and the first value from elsewhere would have to
//! be smuggled in under a false name.

mod constants;
mod version;

/// Resolve one name against one source.
///
/// The value comes back as text, because that is what a document states and
/// what [`crate::format`] renders. An unknown source or name is an error: a
/// span that resolves to nothing checks nothing.
pub fn resolve(source: &str, name: &str) -> Result<String, String> {
    match source {
        "const" => constants::resolve(name),
        "version" => version::resolve(name),
        other => Err(format!(
            "unknown source '{other}' - known sources are const and version"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_answers_for_its_own_names() {
        assert_eq!(resolve("const", "GPIO_MAX_HOLD_MS").unwrap(), "60000");
        assert!(resolve("version", "cli").unwrap().starts_with('0'));
    }

    #[test]
    fn an_unknown_source_names_the_ones_there_are() {
        let err = resolve("firmware", "0.7.2").unwrap_err();
        assert!(err.contains("firmware"), "{err}");
        assert!(err.contains("const"), "{err}");
    }
}
