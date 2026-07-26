// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Helper for writing generated Rust source files.
//!
//! The `chip` and `hw` build steps emit Rust modules directly into the crate's
//! git-ignored `src/` tree.  Those files must match `cargo fmt` output or the
//! tree drifts out of format on every rebuild, so this helper formats the
//! generated code with `rustfmt` before it lands.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// Write `code` to `path` as rustfmt-formatted Rust, touching the file only if
/// its contents actually change, and never exposing a partially written file to
/// a concurrent reader.
///
/// The generated modules live in the shared source tree, and concurrent builds
/// can regenerate them while another process is compiling `onerom-config` - the
/// firmware emulator's build script, for instance, spawns a nested `cargo` that
/// rebuilds this crate at the same time as the outer build.  Rewriting `path`
/// in place would let that reader observe a half-written or half-formatted file.
/// To stay safe:
///
/// * We format a per-process temporary file and compare it to the current
///   contents of `path`.  If they match, `path` is left completely untouched,
///   so there is no mtime churn (which would trigger rebuilds) - this holds
///   whenever the generator output is stable for the same inputs.
/// * Otherwise we atomically `rename` the temporary file over `path`, so a
///   reader always sees a complete file, never a torn one.
///
/// If `rustfmt` is unavailable or fails - e.g. a minimal environment without the
/// component - the unformatted (but valid) code is written instead.
pub fn write_rust(path: &Path, code: &str) {
    // The temp lives beside `path` (so the rename is atomic) and must itself end
    // in `.rs`, since rustfmt only formats `.rs` files.  A leading dot and the
    // pid keep it hidden and unique per process.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("generated");
    let tmp = path.with_file_name(format!(".{stem}.{}.tmp.rs", std::process::id()));

    fs::write(&tmp, code).unwrap_or_else(|e| panic!("Failed to write {}: {}", tmp.display(), e));

    // Format the temporary file in place; on failure it keeps the unformatted
    // code, which is still valid Rust.
    let _ = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(&tmp)
        .stderr(Stdio::null())
        .status();

    let formatted =
        fs::read(&tmp).unwrap_or_else(|e| panic!("Failed to read {}: {}", tmp.display(), e));

    // Unchanged?  Drop the temp and leave `path` (and its mtime) alone.
    if fs::read(path).ok().as_deref() == Some(formatted.as_slice()) {
        let _ = fs::remove_file(&tmp);
        return;
    }

    fs::rename(&tmp, path).unwrap_or_else(|e| {
        panic!(
            "Failed to rename {} to {}: {}",
            tmp.display(),
            path.display(),
            e
        )
    });
}
