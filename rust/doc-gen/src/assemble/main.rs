// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM document assembler.
//!
//! A PDF may be more than one of the markdown files in `docs/`. The CLI manual
//! is the overview followed by the manual, because a reader holding one PDF
//! should not have to fetch another to learn what a One ROM is - and the
//! overview is a document in its own right as well, so the words are written
//! once and published twice.
//!
//! `docs/pdf/docs.toml` says which files, in which order:
//!
//! ```toml
//! [[documents.members]]
//! source = "docs/OVERVIEW.md"
//!
//! [[documents.members]]
//! source = "docs/CLI-MANUAL.md"
//! ```
//!
//! This writes one assembled markdown file per such document, into the build
//! directory, named for the document's slug. `docs/pdf/render.py` then renders
//! that file exactly as it renders a single-source one.
//!
//! It has a second job, on the other side of the same rule. A markdown file in
//! `docs/` may name another one in a fragment marker, and the assembled text is
//! written into the committed file between the markers, so a reader on GitHub
//! meets a whole document:
//!
//! ```text
//! <!--[fragment:docs/fragments/recovery-steps.md]-->
//! ### Putting the device in the bootloader
//! <!--[/]-->
//! ```
//!
//! That is the only writing any tool here does to `docs/`, and it reaches the
//! region between the markers alone. Prose outside one is never touched.
//!
//! Run with:
//!
//! ```text
//! cargo run -p doc-gen --bin doc-assemble -- --out-dir docs/pdf/build
//! cargo run -p doc-gen --bin doc-assemble -- --fragments docs
//! ```
//!
//! `ci/build-docs.sh` runs the first before the renderer. The two never call
//! each other - the script puts them in order. `ci/rust-tests.sh` runs the
//! second, and fails where a region it rewrote differs from what is committed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use doc_gen::assembly::{self, Member};
use doc_gen::{documents, fragment, repo_root};

const USAGE: &str = "\
One ROM document assembler.

  doc-assemble --out-dir <dir> [--config <path>] [--source <name>]
  doc-assemble --fragments <path>...

With --out-dir, writes one assembled markdown file per document that has
members, named <slug>.md there.  A document with a single source is left alone
- the renderer reads it directly.

  --config    the document set, default docs/pdf/docs.toml
  --out-dir   where the assembled files are written
  --source    assemble only documents on this version source, e.g. cli

With --fragments, fills in the fragment regions of the markdown files named -
each path being a file or a directory to walk - writing each back where its
region moved.  Prints
the path of every file that carries a region, so a caller can check them
against what is committed.

Paths are relative to the repository root.
";

/// The set assembled when no config is named.
const DEFAULT_CONFIG: &str = "docs/pdf/docs.toml";

/// A path as given, or as it reads from the repository root.
fn from_root(path: &str) -> PathBuf {
    let given = Path::new(path);
    if given.is_absolute() {
        given.to_path_buf()
    } else {
        repo_root().join(given)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    // Every path after the flag, not just the first - a host can live outside
    // docs/, so a run needs to name more than one place.
    let values = |name: &str| -> Vec<String> {
        args.iter()
            .position(|a| a == name)
            .map(|i| {
                args[i + 1..]
                    .iter()
                    .take_while(|a| !a.starts_with("--"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    // One job per run.  The two write to different places for different
    // reasons, and a run that did both would have to say which of them failed.
    if flag("--fragments") && flag("--out-dir") {
        eprintln!("error: --fragments and --out-dir are separate jobs, so run one at a time\n");
        print!("{USAGE}");
        return ExitCode::FAILURE;
    }

    if flag("--fragments") {
        let paths = values("--fragments");
        if paths.is_empty() {
            eprintln!("error: --fragments needs a path\n");
            print!("{USAGE}");
            return ExitCode::FAILURE;
        };
        return match fragments(&paths) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let Some(out_dir) = value("--out-dir") else {
        eprintln!("error: --out-dir needs a directory, or --fragments a path\n");
        print!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let config = value("--config").unwrap_or_else(|| DEFAULT_CONFIG.to_string());

    match run(&config, &out_dir, value("--source").as_deref()) {
        Ok(written) => {
            if written == 0 {
                println!("No document has members, so nothing was assembled.");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Fill in the fragment regions of one file, or of every markdown file under a
/// directory.
///
/// Prints one repository-relative path per line, for every file that carries a
/// region - rewritten or already current. That list is what `ci/rust-tests.sh`
/// holds a stale region against, so it has to name a file whose text did not
/// move as well as one whose did. Everything else goes to stderr, so the list
/// reads on its own.
fn fragments(paths: &[String]) -> Result<(), String> {
    let root = repo_root();
    let mut hosts = 0;
    let mut rewritten = 0;

    let mut found = Vec::new();
    for path in paths {
        found.extend(documents(&from_root(path))?);
    }
    found.sort();
    found.dedup();

    for document in found {
        let relative = document
            .strip_prefix(&root)
            .unwrap_or(&document)
            .to_string_lossy()
            .to_string();
        let Some(changed) = fragment::fill_file(&root, &relative)? else {
            continue;
        };
        hosts += 1;
        if changed {
            rewritten += 1;
            eprintln!("filled in {relative}");
        }
        println!("{relative}");
    }

    eprintln!("{hosts} document(s) carry a fragment region, {rewritten} rewritten.");
    Ok(())
}

/// Assemble every document in the set that has members.
fn run(config: &str, out_dir: &str, source: Option<&str>) -> Result<usize, String> {
    let config_path = from_root(config);
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("could not read {}: {e}", config_path.display()))?;
    let set: toml::Value = toml::from_str(&text)
        .map_err(|e| format!("could not parse {}: {e}", config_path.display()))?;

    let documents = set
        .get("documents")
        .and_then(|d| d.as_array())
        .ok_or_else(|| format!("{} has no [[documents]]", config_path.display()))?;

    let out_dir = from_root(out_dir);
    let mut written = 0;

    for document in documents {
        let slug = document
            .get("slug")
            .and_then(|s| s.as_str())
            .ok_or_else(|| format!("{}: a document has no slug", config_path.display()))?;

        let members = document.get("members").and_then(|m| m.as_array());
        let has_source = document.get("source").is_some();

        match (members, has_source) {
            (Some(_), true) => {
                return Err(format!(
                    "{slug}: has both source and members.\n  A document is one \
                     file or a list of them, not both - drop whichever is not \
                     what you meant."
                ));
            }
            (None, false) => {
                return Err(format!("{slug}: has neither source nor members"));
            }
            (None, true) => continue,
            (Some(members), false) => {
                if let Some(source) = source
                    && document.get("version_source").and_then(|s| s.as_str()) != Some(source)
                {
                    continue;
                }
                let repo_url = set
                    .get("project")
                    .and_then(|p| p.get("repo_url"))
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| {
                        format!(
                            "{slug}: has members, so [project] needs repo_url.\n  A link \
                             out of the set becomes a URL at the repository, and \
                             nothing else in [project] composes into one."
                        )
                    })?;

                assemble_document(slug, members, repo_url, &out_dir)?;
                written += 1;
            }
        }
    }

    Ok(written)
}

/// Read one document's members, assemble them, and write the result.
fn assemble_document(
    slug: &str,
    members: &[toml::Value],
    repo_url: &str,
    out_dir: &Path,
) -> Result<(), String> {
    let paths = member_paths(slug, members)?;

    let mut read = Vec::with_capacity(paths.len());
    for path in &paths {
        let full = repo_root().join(path);
        let text = std::fs::read_to_string(&full)
            .map_err(|e| format!("{slug}: could not read {}: {e}", full.display()))?;
        read.push(Member {
            path: path.clone(),
            text,
        });
    }

    let assembled = assembly::assemble(&read, repo_url).map_err(|e| format!("{slug}: {e}"))?;

    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("could not create {}: {e}", out_dir.display()))?;
    let out = out_dir.join(format!("{slug}.md"));
    std::fs::write(&out, assembled)
        .map_err(|e| format!("could not write {}: {e}", out.display()))?;

    println!("assembled {slug} from {}", paths.join(", "));
    Ok(())
}

/// Every member's source, in the order the document lists them.
fn member_paths(slug: &str, members: &[toml::Value]) -> Result<Vec<String>, String> {
    if members.is_empty() {
        return Err(format!("{slug}: has an empty members list"));
    }
    members
        .iter()
        .map(|member| {
            member
                .get("source")
                .and_then(|s| s.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{slug}: a member has no source"))
        })
        .collect()
}
