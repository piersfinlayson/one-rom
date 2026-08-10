// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom self` - report on and download One ROM CLI releases.
//!
//! Both commands read the CLI's own release manifest; see
//! [`onerom_cli::release`] for what that manifest is and how it is verified.
//! Neither command installs anything: `download` writes the published artifact
//! and prints the install step for it, leaving a package-managed install
//! (the Linux `.deb`) to the package manager that owns it.

use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_cli::release::{
    Artifact, DOWNLOAD_PAGE, Release, Releases, Status, TARGET_ALL, current_target,
    require_current_target,
};
use onerom_cli::{Error, Options};

use crate::args::self_cmd::{SelfCheckArgs, SelfDownloadArgs};

/// The version of this binary, which is what a check compares.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

// ------------------------------- self check -------------------------------

pub async fn cmd_check(options: &Options, _args: &SelfCheckArgs) -> Result<(), Error> {
    let releases = Releases::from_network().await?;
    let target = require_current_target(&releases)?;

    if options.verbose {
        println!("Manifest: {}", Releases::manifest_url());
        println!("Platform: {target}");
    }

    println!("One ROM CLI v{CURRENT}");

    match releases.status(CURRENT, target)? {
        Status::UpToDate => println!("This is the latest release."),
        Status::Available(latest) => {
            println!("A newer release is available: v{latest}");
            println!();
            println!("Download it with:");
            println!("  onerom self download");
            println!("or from {DOWNLOAD_PAGE}");
        }
        Status::Ahead(latest) => {
            println!("This build is newer than the latest release (v{latest}).");
        }
    }

    Ok(())
}

// ------------------------------ self download ------------------------------

pub async fn cmd_download(options: &Options, args: &SelfDownloadArgs) -> Result<(), Error> {
    let releases = Releases::from_network().await?;

    let all = args.target.as_deref() == Some(TARGET_ALL);
    if all && args.output.is_some() {
        return Err(Error::InvalidArgument(
            "--output".to_string(),
            "--target all downloads several files.  Use --path to name a directory.".to_string(),
        ));
    }

    // With no single platform in view, "latest" is the newest release across
    // all of them; otherwise it is the newest for the platform being fetched.
    let target = if all {
        None
    } else {
        Some(match args.target.as_deref() {
            Some(target) => target,
            None => require_current_target(&releases)?,
        })
    };

    let release = releases.resolve(args.version.as_deref(), target)?;

    let artifacts: Vec<&Artifact> = match target {
        Some(target) => vec![
            release
                .artifact(target)
                .ok_or_else(|| missing_artifact(&releases, release, target))?,
        ],
        None => release.platforms.iter().collect(),
    };

    // Resolve and check every output path before downloading anything, so a
    // file in the way - or a directory that is not there - fails before the
    // network work rather than part-way through it. With --target all that is
    // the difference between one clear error and a half-populated directory.
    let outputs: Vec<PathBuf> = artifacts
        .iter()
        .map(|artifact| resolve_output(args, artifact))
        .collect();
    for out in &outputs {
        check_output_path(out, args.force)?;
    }

    for (artifact, out) in artifacts.iter().zip(outputs.iter()) {
        download_one(options, release, artifact, out).await?;
    }

    Ok(())
}

/// Download, verify and write one artifact, reporting what was done.
async fn download_one(
    options: &Options,
    release: &Release,
    artifact: &Artifact,
    out: &Path,
) -> Result<(), Error> {
    if options.verbose {
        println!("Downloading {} ...", release.url(artifact));
    } else {
        println!(
            "Downloading One ROM CLI v{} for {} ...",
            release.version, artifact.target
        );
    }

    let data = release.download(artifact).await?;

    std::fs::write(out, &data).map_err(|e| Error::io(out, e))?;

    if options.verbose {
        println!(
            "Written {} bytes to {} (sha256 verified)",
            data.len(),
            out.display()
        );
    } else {
        println!("Downloaded to {}", out.display());
    }

    if let Some(hint) = install_hint(artifact, out) {
        println!("{hint}");
    }

    Ok(())
}

/// Check a download can be written where it was asked to go.
///
/// The directory is not created: `--path` naming somewhere that does not exist
/// is more often a typo than an instruction to build a tree.
fn check_output_path(out: &Path, force: bool) -> Result<(), Error> {
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        return Err(Error::OutputDirMissing(parent.display().to_string()));
    }

    if out.exists() && !force {
        return Err(Error::OutputExists(out.display().to_string()));
    }

    Ok(())
}

/// Why a release has no artifact for `target`.
///
/// A target the manifest has never published is a mistyped `--target`; one it
/// publishes but this release lacks is a real gap in that release, and sending
/// the user to the platform list would not help them. The manifest keeps
/// "latest" per platform precisely because a release need not carry every one.
fn missing_artifact(releases: &Releases, release: &Release, target: &str) -> Error {
    if releases.latest(target).is_some() {
        Error::CliTargetNotInRelease(
            release.version.clone(),
            target.to_string(),
            release.targets_str(),
        )
    } else {
        Error::CliTargetUnknown(target.to_string(), releases.targets_str())
    }
}

/// Where one artifact is written.
///
/// `--output` names the file outright, `--path` the directory holding it; with
/// neither, the published filename lands in the current directory. The
/// published filename already carries the version and architecture, so it is
/// the default in both directory cases.
fn resolve_output(args: &SelfDownloadArgs, artifact: &Artifact) -> PathBuf {
    if let Some(output) = &args.output {
        PathBuf::from(output)
    } else if let Some(path) = &args.path {
        PathBuf::from(path).join(&artifact.filename)
    } else {
        PathBuf::from(&artifact.filename)
    }
}

/// How to install what was just downloaded, where that can be said usefully.
///
/// Only offered for an artifact this machine can actually run: a cross-platform
/// download - one of `--target all`, or a `--target` naming another platform -
/// is being fetched to carry elsewhere, so instructions for the local machine
/// would be wrong.
fn install_hint(artifact: &Artifact, out: &Path) -> Option<String> {
    if current_target() != Some(artifact.target.as_str()) {
        return None;
    }

    let out = out.display();
    if artifact.filename.ends_with(".deb") {
        Some(format!("Install it with:\n  sudo dpkg -i {out}"))
    } else if artifact.filename.ends_with(".zip") {
        Some(format!(
            "Unzip {out} and put the onerom executable on your PATH."
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(target: &str, filename: &str) -> Artifact {
        Artifact {
            target: target.to_string(),
            filename: filename.to_string(),
            sha256: String::new(),
        }
    }

    fn args() -> SelfDownloadArgs {
        SelfDownloadArgs {
            version: None,
            target: None,
            output: None,
            path: None,
            force: false,
        }
    }

    /// The published filename carries the version and architecture, so it is
    /// what both directory forms write - only --output renames the file.
    #[test]
    fn output_resolution_follows_output_then_path_then_cwd() {
        let artifact = artifact("x86_64-unknown-linux-gnu", "onerom-cli_0.3.0-1_amd64.deb");

        let default = resolve_output(&args(), &artifact);
        assert_eq!(default, PathBuf::from("onerom-cli_0.3.0-1_amd64.deb"));

        let in_dir = resolve_output(
            &SelfDownloadArgs {
                path: Some("dist".to_string()),
                ..args()
            },
            &artifact,
        );
        assert_eq!(
            in_dir,
            PathBuf::from("dist").join("onerom-cli_0.3.0-1_amd64.deb")
        );

        let named = resolve_output(
            &SelfDownloadArgs {
                output: Some("cli.deb".to_string()),
                ..args()
            },
            &artifact,
        );
        assert_eq!(named, PathBuf::from("cli.deb"));
    }

    /// Both refusals happen before any download, so `--target all` cannot
    /// half-populate a directory before hitting the file that was in the way.
    #[test]
    fn an_output_path_is_checked_before_anything_is_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("onerom-cli.zip");
        std::fs::write(&existing, b"already here").unwrap();

        // A free path in an existing directory is fine.
        assert!(check_output_path(&dir.path().join("new.zip"), false).is_ok());
        // As is a bare filename, whose parent is the current directory.
        assert!(check_output_path(Path::new("new.zip"), false).is_ok());

        let err = check_output_path(&existing, false).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert!(check_output_path(&existing, true).is_ok());

        // A directory that is not there is a typo, not an instruction to
        // create one.
        let missing = dir.path().join("nope").join("onerom-cli.zip");
        let err = check_output_path(&missing, true).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        assert!(!dir.path().join("nope").exists());
    }

    /// A release missing a platform the manifest publishes is a gap in that
    /// release, not a mistyped --target, and the two say different things.
    #[test]
    fn a_missing_artifact_distinguishes_a_gap_from_a_typo() {
        // One release, carrying only Linux, in a manifest that publishes macOS
        // too - the shape `latest` being per-platform allows for.
        let json = r#"{
          "version": 1,
          "latest": {
            "x86_64-unknown-linux-gnu": "0.3.0",
            "universal-apple-darwin": "0.2.0"
          },
          "releases": [
            {
              "version": "0.3.0",
              "path": "v0.3.0",
              "platforms": [
                {
                  "target": "x86_64-unknown-linux-gnu",
                  "filename": "onerom-cli_0.3.0-1_amd64.deb",
                  "sha256": "00"
                }
              ]
            }
          ]
        }"#;
        let releases = Releases::from_json("test", json).unwrap();
        let release = releases.release("0.3.0").unwrap();

        // Published, but not in this release: name the release and what it has.
        let msg = missing_artifact(&releases, release, "universal-apple-darwin").to_string();
        assert!(msg.contains("v0.3.0"), "{msg}");
        assert!(msg.contains("universal-apple-darwin"), "{msg}");
        assert!(msg.contains("x86_64-unknown-linux-gnu"), "{msg}");

        // Never published: point at the platform list instead.
        let msg = missing_artifact(&releases, release, "mips-unknown-linux-gnu").to_string();
        assert!(msg.contains("Unknown --target"), "{msg}");
        assert!(!msg.contains("v0.3.0"), "{msg}");
    }

    /// A hint is given for the running platform's artifact, and matches the
    /// artifact's own file type rather than the platform's usual one.
    #[test]
    fn install_hint_is_offered_only_for_this_platform() {
        let out = Path::new("onerom-cli.deb");

        let Some(target) = current_target() else {
            return;
        };

        let deb = artifact(target, "onerom-cli_0.3.0-1_amd64.deb");
        let hint = install_hint(&deb, out).expect("local artifact gets a hint");
        assert!(hint.contains("dpkg -i onerom-cli.deb"), "{hint}");

        let zip = artifact(target, "onerom-cli-mac-0.3.0.zip");
        let hint = install_hint(&zip, out).expect("local artifact gets a hint");
        assert!(hint.contains("PATH"), "{hint}");

        // Another platform's artifact is being carried elsewhere, so local
        // install instructions would be wrong.
        let other = artifact("some-other-platform", "onerom-cli_0.3.0-1_amd64.deb");
        assert_eq!(install_hint(&other, out), None);

        // An unrecognised file type gets no hint rather than a guessed one.
        let odd = artifact(target, "onerom-cli-0.3.0.tar.xz");
        assert_eq!(install_hint(&odd, out), None);
    }
}
