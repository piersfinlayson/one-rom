// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Helper for writing generated Rust source files.
//!
//! The `chip` and `hw` build steps emit Rust modules directly into the crate's
//! committed `src/` tree.  Those files are checked in, so they must match
//! `cargo fmt` output or the tree drifts out of format on every rebuild.  This
//! helper writes the generated code and then formats it in place with
//! `rustfmt`.

use std::path::Path;
use std::process::{Command, Stdio};

/// Write `code` to `path`, then format it in place with `rustfmt`.
///
/// Formatting in place (rather than piping through stdin/stdout) keeps this
/// simple and avoids pipe-buffer deadlocks on the large generated files.  If
/// `rustfmt` is unavailable or fails - e.g. a minimal build environment that
/// lacks the component - the unformatted code is left as written: the crate
/// still compiles, it just won't be fmt-clean until regenerated where
/// `rustfmt` is present.
pub fn write_rust(path: &Path, code: &str) {
    std::fs::write(path, code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", path.display(), e));

    let _ = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(path)
        .stderr(Stdio::null())
        .status();
}
