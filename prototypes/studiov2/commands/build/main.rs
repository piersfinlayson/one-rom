// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reads the One ROM CLI's clap argument definitions and writes them out as
//! the plain data `src/lib.rs` describes.
//!
//! It reads the Rust source with `syn` rather than running the CLI or scraping
//! `--help`.  The types are the only place an option's shape is written down,
//! and `--help` has already thrown them away by the time it is printed.
//!
//! One thing an option's help needs is not in the argument definitions at all:
//! a period or a hold the firmware chose, which the source quotes by name.
//! `constants` resolves those against the metadata schema, so what a pane shows
//! is a number a user can read.  All of it happens here at build time, and the
//! crate itself keeps no dependency, which is what keeps a `no_std` consumer
//! possible.

mod attrs;
mod constants;
mod emit;
mod index;
mod source;
mod values;
mod walk;

use std::path::{Path, PathBuf};

/// The CLI's argument definitions, relative to this crate.
const ARGS: &str = "../../../rust/cli/src/args";

/// Where `LogLevel` is declared - a `ValueEnum` a global option takes.
const LOG_LEVEL: &str = "../../../rust/cli/src/lib/mod.rs";

/// Where `FileFormat` is declared - the value set behind `image convert`.
const FILE_FORMAT: &str = "../../../rust/gen/src/image.rs";

fn main() {
    for path in [ARGS, LOG_LEVEL, FILE_FORMAT] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=build");
    println!("cargo:rerun-if-changed=sources.rs");

    let index = index::Index::read_dir(Path::new(ARGS));
    let values = values::ValueSets::read(&index, Path::new(LOG_LEVEL), Path::new(FILE_FORMAT));
    let mut description = walk::walk(&index, &values);
    source::apply(&mut description);

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("description.rs"), emit::emit(&description))
        .expect("could not write the description");
}
