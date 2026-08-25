// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What the One ROM documentation tools are built from.
//!
//! The crate's two binaries share this library rather than each carrying its
//! own copy of the rules:
//!
//! - `doc-gen` checks the values a document states against the sources that own
//!   them. It writes nothing.
//! - `doc-assemble` fills in the fragment regions of the documents in `docs/`,
//!   and joins whole documents into the single markdown file a PDF is rendered
//!   from.
//!
//! [`marker`] owns the syntax both binaries read, and [`assembly`] owns how a
//! heading's level is worked out when one document's text lands inside another.
//! A fragment region and a PDF member shift by the same rule, computed by the
//! same code, so the two cannot drift apart.

pub mod assembly;
pub mod format;
pub mod fragment;
pub mod marker;
pub mod source;

use std::path::{Path, PathBuf};

/// Levels up from `CARGO_MANIFEST_DIR` to the repository root.
/// `CARGO_MANIFEST_DIR` = `<repo>/rust/doc-gen`, so two pops reach `<repo>`.
const LEVELS_UP_TO_REPO_ROOT: usize = 2;

/// The repository root, so a path a tool is given reads the same from anywhere.
pub fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..LEVELS_UP_TO_REPO_ROOT {
        path.pop();
    }
    path
}

/// The markdown files under `path`, or `path` itself when it names a file.
///
/// `docs/wip` is skipped.  It holds proposals, which describe mechanisms that
/// do not exist and quote the marker syntax as an example - so there is nothing
/// in the sources to check a value against, and nothing on disk for a fragment
/// marker there to name.  A scratch document under it is reached by naming it
/// directly.
pub fn documents(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(format!(
            "{} is neither a file nor a directory",
            path.display()
        ));
    }

    let mut found = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("could not read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("could not read {}: {e}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                // wip holds proposals, describing mechanisms that do not
                // exist, and build holds what this tool wrote - neither is a
                // document a reader is served or an author edits.
                if path
                    .file_name()
                    .is_some_and(|name| name == "wip" || name == "build")
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}
