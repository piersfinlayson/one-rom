// src/released.rs
//
// The schema as the last release shipped it, read through the narrowest view
// that can answer where a byte sits and what a plugin-facing item said of the
// release it arrived in.

use serde::Deserialize;
use std::path::Path;

use crate::schema::{ConstantValue, FieldShape, NamedSizes, prim_size};

// ---------------------------------------------------------------------------
// The narrow view
// ---------------------------------------------------------------------------

// Nothing here denies unknown fields, and almost nothing is required.  The
// working schema's own reader is strict on purpose - a misspelled key there is
// a mechanism quietly doing nothing - but this reads a file written against an
// older set of keys, and asks only where a byte sits, what a constant holds,
// and which release a plugin-facing item said it arrived in.  A key since
// renamed, or one added after the copy was taken, bears on none of those, so
// it is skipped rather than refused.  That leaves the keys of this file free
// to move, which is what [schema] format_version is for.

/// The last released schema.
#[derive(Deserialize, Debug)]
pub struct Released {
    #[serde(default)]
    schema: ReleasedMetadata,
    #[serde(default)]
    pub constants: Vec<ReleasedConstant>,
    #[serde(default)]
    pub type_aliases: Vec<ReleasedAlias>,
    #[serde(default)]
    pub enums: Vec<ReleasedEnum>,
    #[serde(default)]
    pub structs: Vec<ReleasedStruct>,
    #[serde(default)]
    pub tagged_fams: Vec<ReleasedTaggedFam>,
}

#[derive(Deserialize, Debug, Default)]
struct ReleasedMetadata {
    /// Absent in every schema before 0.8.0, which is when the key arrived.
    firmware_release: Option<String>,
    /// Set where the crate has never released, so there is no shipped layout
    /// to compare against. Declared rather than inferred from an absent or
    /// empty file, both of which stay errors.
    #[serde(default)]
    unreleased: bool,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedConstant {
    pub name: String,
    pub value: ConstantValue,
    /// Whether the copy had this constant in the plugin API.
    #[serde(default)]
    pub ora_api: bool,
    /// Absent in every copy taken before `first_release` existed, and in every
    /// constant no plugin can see.
    pub first_release: Option<String>,
}

/// A plugin key, reduced to what says when it arrived.  A key's id is not a
/// byte of device state, and the name is all the comparison needs to find the
/// same key in both files.
#[derive(Deserialize, Debug)]
pub struct ReleasedPluginKey {
    pub name: String,
    /// Absent in every copy taken before `first_release` existed.
    pub first_release: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedAlias {
    pub name: String,
    pub underlying: String,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedEnum {
    pub name: String,
    pub size: u32,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedStruct {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<ReleasedField>,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedTaggedFam {
    pub name: String,
    pub discriminant_field: String,
    pub discriminant_type: String,
    pub param_len_field: String,
    #[serde(default)]
    pub common_fields: Vec<ReleasedField>,
    #[serde(default)]
    pub variants: Vec<ReleasedVariant>,
}

#[derive(Deserialize, Debug)]
pub struct ReleasedVariant {
    pub discriminant: String,
    #[serde(default)]
    pub fields: Vec<ReleasedField>,
}

/// A field, reduced to what decides where its bytes sit and how they are
/// read, plus the plugin key it exposes them under.
#[derive(Deserialize, Debug)]
pub struct ReleasedField {
    pub name: String,
    pub kind: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub element: Option<String>,
    pub count: Option<u32>,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub size: Option<u32>,
    /// The plugin key this field carried, where it carried one.
    pub plugin_key: Option<ReleasedPluginKey>,
}

impl ReleasedField {
    /// This field as the layout walk sees it.
    pub fn shape(&self) -> FieldShape<'_> {
        FieldShape {
            name: &self.name,
            kind: &self.kind,
            type_: self.type_.as_deref(),
            element: self.element.as_deref(),
            count: self.count,
            rows: self.rows,
            cols: self.cols,
            size: self.size,
        }
    }
}

impl NamedSizes for Released {
    fn enum_size(&self, name: &str) -> Option<usize> {
        self.enums
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.size as usize)
    }

    fn alias_size(&self, name: &str) -> Option<usize> {
        self.type_aliases
            .iter()
            .find(|a| a.name == name)
            .map(|a| prim_size(&a.underlying))
    }
}

impl Released {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Parse a released schema held in memory.
    ///
    /// Split out from [`Released::load`] so the comparison can be exercised
    /// against a pair of schemas built in a test, with no file to write.
    pub fn parse(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(toml::from_str(content)?)
    }

    /// The release this copy describes, where it says.
    ///
    /// A copy taken before the key existed says nothing, and the caller reads
    /// that as a release earlier than the one the key arrived in.
    /// Whether the crate has yet to release, in which case the file states
    /// that and describes nothing.
    pub fn unreleased(&self) -> bool {
        self.schema.unreleased
    }

    pub fn firmware_release(&self) -> Option<&str> {
        self.schema.firmware_release.as_deref()
    }

    /// The value of a named constant, where the file declares one.
    pub fn constant(&self, name: &str) -> Option<&ConstantValue> {
        self.constants
            .iter()
            .find(|c| c.name == name)
            .map(|c| &c.value)
    }

    /// The constants the copy put in the plugin API.
    ///
    /// A constant the copy declares without `ora_api` is not one of them: a
    /// plugin could not see it then, so this release is where it arrives.
    pub fn ora_constants(&self) -> impl Iterator<Item = &ReleasedConstant> {
        self.constants.iter().filter(|c| c.ora_api)
    }

    /// Every plugin key the copy carried.
    ///
    /// Only `[[structs]]` fields are walked, because that is where the working
    /// schema's own `plugin_keys` looks and a key found anywhere else would
    /// never be generated.
    pub fn plugin_keys(&self) -> impl Iterator<Item = &ReleasedPluginKey> {
        self.structs
            .iter()
            .flat_map(|s| s.fields.iter())
            .filter_map(|f| f.plugin_key.as_ref())
    }
}
