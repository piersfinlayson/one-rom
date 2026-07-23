// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A [`ChipType`] paired with the exact string the user wrote for it.

use alloc::string::{String, ToString};
use onerom_config::chip::ChipType;

/// A [`ChipType`] paired with the exact string the user wrote for it.
///
/// Many spellings resolve to a single [`ChipType`] - for example `27512`,
/// `27C512`, `27LC512` and `27SF512` all resolve to
/// [`ChipType::Chip27512`].  The resolved type drives all behaviour (RBCP
/// type, size, timing, chip-select layout, ...), while `raw` preserves the
/// user's exact spelling so it can be echoed back verbatim in the generated
/// metadata rather than a canonicalised name.
///
/// It deserialises from, and serialises to, a plain string - identical on the
/// wire to [`ChipType`] - so it is a drop-in replacement in configuration
/// structures.  Deserialisation resolves the string by delegating to
/// [`ChipType`]'s own [`Deserialize`](serde::Deserialize) implementation, so
/// the set of accepted spellings (and their case-sensitivity) is exactly that
/// of [`ChipType`]; an unrecognised spelling is a hard error.
#[derive(Debug, Clone)]
pub struct ChipTypeSpec {
    raw: String,
    resolved: ChipType,
}

impl ChipTypeSpec {
    /// Pairs a raw spelling with the [`ChipType`] it resolves to.
    ///
    /// The caller is responsible for `raw` genuinely resolving to `resolved`;
    /// this is intended for call sites that have already resolved the type
    /// while still holding the original string (for example the CLI `--slot`
    /// parser).
    pub fn new(raw: String, resolved: ChipType) -> Self {
        Self { raw, resolved }
    }

    /// Returns the resolved chip type.  Use this for all behaviour and
    /// semantics - the raw spelling is only for display/metadata.
    pub fn resolved(&self) -> ChipType {
        self.resolved
    }

    /// Returns a reference to the resolved chip type.  Lets call sites that
    /// previously exposed `&ChipType` keep doing so unchanged.
    pub fn resolved_ref(&self) -> &ChipType {
        &self.resolved
    }

    /// Returns the exact string the user entered for this chip type.
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

/// Wraps a [`ChipType`] using its canonical name as the raw spelling.  Used
/// where there is no user-entered string - for example synthetic plugin chip
/// types constructed by tooling.
impl From<ChipType> for ChipTypeSpec {
    fn from(resolved: ChipType) -> Self {
        Self {
            raw: resolved.name().to_string(),
            resolved,
        }
    }
}

impl serde::Serialize for ChipTypeSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Emit the user's exact spelling so configurations round-trip
        // faithfully.
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> serde::Deserialize<'de> for ChipTypeSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Capture the string verbatim, then resolve it.
        let raw = String::deserialize(deserializer)?;
        // Resolve case-insensitively via `try_from_str`, matching the CLI
        // `--slot` parser and the intent that any accepted spelling
        // round-trips through the config (serialise emits `raw`, so a
        // case-insensitive spelling such as `27lc512` must deserialise again).
        // Fall back to ChipType's own Deserialize so its debug names (e.g.
        // `Chip27512`) still resolve and invalid values get its familiar
        // "expected one of ..." error - ChipType deserialises a borrowed
        // `&str`, hence the borrowed-str deserialiser.
        let resolved = match ChipType::try_from_str(&raw) {
            Some(chip_type) => chip_type,
            None => ChipType::deserialize(
                serde::de::value::BorrowedStrDeserializer::<D::Error>::new(&raw),
            )?,
        };
        Ok(Self { raw, resolved })
    }
}

// Delegate the JSON schema entirely to ChipType so the generated
// `schema.json` is byte-identical - the field references the same
// `$defs/ChipType` definition and no separate ChipTypeSpec definition is
// emitted.
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ChipTypeSpec {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        ChipType::schema_name()
    }

    fn schema_id() -> alloc::borrow::Cow<'static, str> {
        ChipType::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ChipType::json_schema(generator)
    }

    fn inline_schema() -> bool {
        ChipType::inline_schema()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_preserves_raw_spelling() {
        // A non-canonical alias must resolve correctly yet keep its spelling.
        let spec: ChipTypeSpec = serde_json::from_str("\"27SF512\"").unwrap();
        assert_eq!(spec.raw(), "27SF512");
        assert_eq!(spec.resolved(), ChipType::Chip27512);
    }

    #[test]
    fn deserialize_canonical_spelling() {
        let spec: ChipTypeSpec = serde_json::from_str("\"27512\"").unwrap();
        assert_eq!(spec.raw(), "27512");
        assert_eq!(spec.resolved(), ChipType::Chip27512);
    }

    #[test]
    fn serialize_emits_raw_spelling() {
        let spec = ChipTypeSpec::new("27LC512".to_string(), ChipType::Chip27512);
        assert_eq!(serde_json::to_string(&spec).unwrap(), "\"27LC512\"");
    }

    #[test]
    fn round_trip_is_faithful() {
        let json = "\"27C512\"";
        let spec: ChipTypeSpec = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&spec).unwrap(), json);
    }

    #[test]
    fn deserialize_is_case_insensitive_and_round_trips() {
        // The CLI --slot parser accepts case-insensitive spellings; a config
        // carrying such a spelling must deserialise (and round-trip) rather
        // than fail when the composed config is re-parsed.
        let spec: ChipTypeSpec = serde_json::from_str("\"27lc512\"").unwrap();
        assert_eq!(spec.raw(), "27lc512");
        assert_eq!(spec.resolved(), ChipType::Chip27512);

        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(json, "\"27lc512\"");
        let again: ChipTypeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(again.raw(), "27lc512");
        assert_eq!(again.resolved(), ChipType::Chip27512);
    }

    #[test]
    fn debug_name_still_resolves() {
        // ChipType's own debug name must keep resolving via the fallback.
        let spec: ChipTypeSpec = serde_json::from_str("\"Chip27512\"").unwrap();
        assert_eq!(spec.resolved(), ChipType::Chip27512);
    }

    #[test]
    fn unrecognised_spelling_is_error() {
        assert!(serde_json::from_str::<ChipTypeSpec>("\"not-a-rom\"").is_err());
    }

    #[test]
    fn from_chip_type_uses_canonical_name() {
        let spec = ChipTypeSpec::from(ChipType::Chip27512);
        assert_eq!(spec.raw(), "27512");
        assert_eq!(spec.resolved(), ChipType::Chip27512);
    }
}
