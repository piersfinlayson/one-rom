// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Crate versions, for the documents that state which release they describe.
//!
//! A document is versioned as the thing it documents rather than as the
//! repository, so its own version statement has to move when that crate's does.
//! Naming the crate here makes that a checked fact rather than a step to
//! remember at release.

use std::path::PathBuf;

/// Which crate each version name reads from, relative to the repository root.
const CRATES: &[(&str, &str)] = &[("cli", "rust/cli/Cargo.toml")];

/// Levels up from `CARGO_MANIFEST_DIR` to the repository root.
/// `CARGO_MANIFEST_DIR` = `<repo>/rust/doc-gen`, so two pops reach `<repo>`.
const LEVELS_UP_TO_REPO_ROOT: usize = 2;

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..LEVELS_UP_TO_REPO_ROOT {
        path.pop();
    }
    path
}

/// The version of the crate `name` stands for, e.g. `0.4.0`.
pub fn resolve(name: &str) -> Result<String, String> {
    let known: Vec<&str> = CRATES.iter().map(|(name, _)| *name).collect();
    let (_, manifest) = CRATES
        .iter()
        .find(|(declared, _)| *declared == name)
        .ok_or_else(|| {
            format!(
                "unknown version '{name}' - known versions are {}",
                known.join(", ")
            )
        })?;

    let path = repo_root().join(manifest);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let manifest: toml::Value =
        toml::from_str(&text).map_err(|e| format!("could not parse {}: {e}", path.display()))?;

    manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{} states no package version", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_version_is_read_from_its_manifest() {
        let version = resolve("cli").unwrap();
        // Three dot-separated numbers, whatever they currently are.
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "{version}");
        assert!(
            parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())),
            "{version}"
        );
    }

    #[test]
    fn an_unknown_name_lists_the_ones_there_are() {
        let err = resolve("studio").unwrap_err();
        assert!(err.contains("studio"), "{err}");
        assert!(err.contains("cli"), "{err}");
    }
}
