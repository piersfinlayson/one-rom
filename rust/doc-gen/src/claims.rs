// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A claim one file makes about what another file says.
//!
//! The rest of this crate compares a value a *document* states against the
//! source that owns it. A claim is the same idea with no document in it: two
//! files in the tree that have to say the same thing, where neither can read
//! the other.
//!
//! They live here because a crate's build script cannot reach the repository
//! root - a published tarball has to build from `CARGO_MANIFEST_DIR` alone -
//! while this tool starts at the root and writes nothing.

use std::path::Path;

use crate::repo_root;

/// The repo-root Makefile, which owns the firmware version.
const MAKEFILE: &str = "Makefile";

/// The metadata schema, which states the release it describes.
const SCHEMA: &str = "rust/metadata/metadata_schema.toml";

/// The firmware parser, which states the newest firmware it reads.
const PARSER: &str = "rust/fw-parser/src/lib.rs";

/// The v2 builder, which states the newest firmware it will build.
const BUILDER: &str = "rust/gen/src/v2/builder.rs";

/// One claim that does not hold, ready to print.
pub struct Problem {
    /// The file making the claim, relative to the repository root.
    pub file: String,
    pub detail: String,
}

/// Check every cross-file claim, reporting all of them rather than the first.
pub fn check() -> Vec<Problem> {
    let root = repo_root();
    let makefile = root.join(MAKEFILE);
    [
        (SCHEMA, firmware_release(&root.join(SCHEMA), &makefile)),
        (PARSER, max_version(&root.join(PARSER), &makefile)),
        (BUILDER, max_fw_version(&root.join(BUILDER), &makefile)),
    ]
    .into_iter()
    .filter_map(|(file, outcome)| {
        outcome.err().map(|detail| Problem {
            file: file.to_string(),
            detail,
        })
    })
    .collect()
}

/// `[schema] firmware_release` against the Makefile's version.
///
/// Every `first_release` in the schema is written against the release it
/// describes, so a schema left on the previous one dates the whole file, and
/// nothing in the file itself can tell.
///
/// Both files are named rather than found, so a test can hand it a pair that
/// disagree.
fn firmware_release(schema: &Path, makefile: &Path) -> Result<(), String> {
    let stated = schema_release(schema)?;
    let built = makefile_version(makefile)?;

    if stated == built {
        Ok(())
    } else {
        Err(format!(
            "[schema] firmware_release says '{stated}', {MAKEFILE} builds {built}"
        ))
    }
}

/// The parser's `MAX_VERSION_MAJOR` and `MAX_VERSION_MINOR` against the
/// Makefile's version.
///
/// The ceiling moves with the branch version - left behind, the parser
/// refuses the firmware built beside it.  `MAX_VERSION_PATCH` is 999,
/// standing for any patch of that minor, so nothing in the Makefile answers
/// to it.
fn max_version(parser: &Path, makefile: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(parser)
        .map_err(|e| format!("could not read {}: {e}", parser.display()))?;
    let stated = ["MAX_VERSION_MAJOR", "MAX_VERSION_MINOR"]
        .into_iter()
        .map(|name| rust_const(&text, PARSER, name))
        .collect::<Result<Vec<String>, String>>()?
        .join(".");

    let built = makefile_parts(makefile)?[..2].join(".");

    if stated == built {
        Ok(())
    } else {
        Err(format!(
            "MAX_VERSION_MAJOR and MAX_VERSION_MINOR say '{stated}', {MAKEFILE} builds {built}"
        ))
    }
}

/// The newest firmware the v2 builder will build, against what is being built.
///
/// A second ceiling, separate from the parser's: it gates building firmware
/// rather than reading it.  It was once left behind when the parser's moved,
/// which broke every firmware build on the branch.
fn max_fw_version(builder: &Path, makefile: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(builder)
        .map_err(|e| format!("could not read {}: {e}", builder.display()))?;
    let stated = version_new_parts(&rust_const(&text, BUILDER, "MAX_FW_VERSION")?)?[..2].join(".");
    let built = makefile_parts(makefile)?[..2].join(".");

    if stated == built {
        Ok(())
    } else {
        Err(format!(
            "MAX_FW_VERSION says '{stated}', {MAKEFILE} builds {built}"
        ))
    }
}

/// The arguments of a `FirmwareVersion::new(major, minor, ...)` call.
fn version_new_parts(value: &str) -> Result<Vec<String>, String> {
    let args = value
        .split_once('(')
        .and_then(|(_, rest)| rest.rsplit_once(')'))
        .map(|(args, _)| args)
        .ok_or_else(|| format!("'{value}' is not a FirmwareVersion::new(...) call"))?;

    let parts: Vec<String> = args.split(',').map(|a| a.trim().to_string()).collect();
    if parts.len() < 2 {
        return Err(format!("'{value}' names fewer than two version parts"));
    }
    Ok(parts)
}

/// The release the schema says it describes.
fn schema_release(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let schema: toml::Value =
        toml::from_str(&text).map_err(|e| format!("could not parse {}: {e}", path.display()))?;

    schema
        .get("schema")
        .and_then(|schema| schema.get("firmware_release"))
        .and_then(|release| release.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{} states no [schema] firmware_release", path.display()))
}

/// The version the Makefile builds, as `major.minor.patch`.
fn makefile_version(path: &Path) -> Result<String, String> {
    Ok(makefile_parts(path)?.join("."))
}

/// The version the Makefile builds, as its major, minor and patch parts.
///
/// A claim comparing fewer than three of them takes the parts it wants from
/// here, so both read the same three assignments.
fn makefile_parts(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;

    ["VERSION_MAJOR", "VERSION_MINOR", "VERSION_PATCH"]
        .into_iter()
        .map(|name| assignment(&text, name))
        .collect()
}

/// The value of a `NAME := value` assignment in a Makefile.
///
/// The three versions are plain literals at the top of the file, so this reads
/// them as text.  Asking make would need a make, and would run whatever else
/// the file does.
fn assignment(text: &str, name: &str) -> Result<String, String> {
    text.lines()
        .filter_map(|line| line.strip_prefix(name))
        .find_map(|rest| rest.trim_start().strip_prefix(":="))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{MAKEFILE} has no '{name} := ' assignment"))
}

/// The value of a `pub const NAME: TYPE = value;` declaration in Rust source.
///
/// Read as text for the same reason the Makefile is: asking the compiler what
/// the constant holds would mean building the crate being checked.
fn rust_const(text: &str, file: &str, name: &str) -> Result<String, String> {
    let declaration = format!("pub const {name}:");
    text.lines()
        .filter_map(|line| line.trim_start().strip_prefix(&declaration))
        .find_map(|rest| rest.split_once('='))
        .and_then(|(_, value)| value.split(';').next())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{file} declares no '{name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_claim_in_the_tree_holds() {
        let problems = check();
        let detail: Vec<&str> = problems.iter().map(|p| p.detail.as_str()).collect();
        assert!(problems.is_empty(), "{detail:?}");
    }

    /// A pair on disk that disagree, so the rule is exercised rather than
    /// only the tree that happens to satisfy it.
    fn pair(release: &str, major: &str, minor: &str, patch: &str) -> Result<(), String> {
        let dir = tempfile::TempDir::new().unwrap();
        let schema = dir.path().join("schema.toml");
        let makefile = dir.path().join("Makefile");
        std::fs::write(
            &schema,
            format!("[schema]\nfirmware_release = \"{release}\"\n"),
        )
        .unwrap();
        std::fs::write(
            &makefile,
            format!(
                "VERSION_MAJOR := {major}\nVERSION_MINOR := {minor}\nVERSION_PATCH := {patch}\n"
            ),
        )
        .unwrap();
        firmware_release(&schema, &makefile)
    }

    #[test]
    fn a_schema_on_the_version_the_makefile_builds_passes() {
        pair("0.8.0", "0", "8", "0").unwrap();
    }

    #[test]
    fn a_schema_left_on_the_previous_release_is_reported() {
        let err = pair("0.7.2", "0", "8", "0").unwrap_err();
        assert!(err.contains("0.7.2"), "{err}");
        assert!(err.contains("0.8.0"), "{err}");
    }

    /// A parser and a Makefile on disk that can be made to disagree.
    fn ceiling(major: &str, minor: &str, version: &str) -> Result<(), String> {
        let dir = tempfile::TempDir::new().unwrap();
        let parser = dir.path().join("lib.rs");
        let makefile = dir.path().join("Makefile");
        std::fs::write(
            &parser,
            format!(
                "pub const MAX_VERSION_MAJOR: u16 = {major};\n\
                 pub const MAX_VERSION_MINOR: u16 = {minor};\n\
                 pub const MAX_VERSION_PATCH: u16 = 999;\n"
            ),
        )
        .unwrap();
        std::fs::write(&makefile, makefile_text(version)).unwrap();
        max_version(&parser, &makefile)
    }

    /// A Makefile building the named release.
    fn makefile_text(version: &str) -> String {
        let parts: Vec<&str> = version.split('.').collect();
        format!(
            "VERSION_MAJOR := {}\nVERSION_MINOR := {}\nVERSION_PATCH := {}\n",
            parts[0], parts[1], parts[2]
        )
    }

    #[test]
    fn a_ceiling_on_the_version_the_makefile_builds_passes() {
        ceiling("0", "8", "0.8.0").unwrap();
    }

    /// The patch is 999, standing for any patch of that minor, so a Makefile
    /// on a later patch of the same minor says nothing about the ceiling.
    #[test]
    fn a_ceiling_on_a_later_patch_of_the_same_minor_passes() {
        ceiling("0", "8", "0.8.3").unwrap();
    }

    #[test]
    fn a_ceiling_left_on_the_previous_release_is_reported() {
        let err = ceiling("0", "7", "0.8.0").unwrap_err();
        assert!(err.contains("0.7"), "{err}");
        assert!(err.contains("0.8"), "{err}");
    }

    #[test]
    fn an_assignment_is_read_from_its_line() {
        let text = "VERSION_MAJOR := 0\nVERSION_MINOR := 8\nVERSION_PATCH := 0\n";
        assert_eq!(assignment(text, "VERSION_MINOR").unwrap(), "8");
    }

    #[test]
    fn an_assignment_that_is_not_there_says_which_one() {
        let err = assignment("VERSION_MAJOR := 0\n", "VERSION_PATCH").unwrap_err();
        assert!(err.contains("VERSION_PATCH"), "{err}");
    }

    #[test]
    fn a_constant_is_read_from_its_declaration() {
        let text = "pub const MAX_VERSION_MINOR: u16 = 8;\n";
        assert_eq!(rust_const(text, PARSER, "MAX_VERSION_MINOR").unwrap(), "8");
    }

    #[test]
    fn a_constant_that_is_not_there_says_which_one() {
        let err = rust_const(
            "pub const MAX_VERSION_MAJOR: u16 = 0;\n",
            PARSER,
            "MAX_VERSION_MINOR",
        )
        .unwrap_err();
        assert!(err.contains("MAX_VERSION_MINOR"), "{err}");
        assert!(err.contains(PARSER), "{err}");
    }

    #[test]
    fn a_firmware_version_call_gives_up_its_parts() {
        let parts = version_new_parts("FirmwareVersion::new(0, 8, 999, 999)").unwrap();
        assert_eq!(parts[..2].join("."), "0.8");
    }

    #[test]
    fn something_that_is_not_a_version_call_says_so() {
        let err = version_new_parts("42").unwrap_err();
        assert!(err.contains("FirmwareVersion::new"), "{err}");
    }
}
