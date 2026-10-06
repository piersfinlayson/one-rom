// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The server's clones of the public and private record repositories.
//! - The public record has key n's lines in `signatures/n.txt`.
//! - The private record has key n's lines in `signatures/n.jsonl`.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use onerom_config::hw::Board;
use onerom_metadata::otp::{RecordLine, format_chip_id, parse_record};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::timeout;

/// The longest a git command may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// The server's clone of the public record repository.
pub struct PublicRecord(Repository);

/// The server's clone of the private record repository.
pub struct PrivateRecord(Repository);

/// A line of the private record. It records one request the server signed.
///
/// It displays as the line without its newline, which is a JSON object with
/// these fields in this order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateLine {
    pub request: RequestType,
    /// The chip's CHIPID, written as [`format_chip_id`] writes it.
    #[serde(with = "chip_id_text")]
    pub chip_id: [u16; 4],
    /// The board, written as its canonical name.
    pub board: Board,
    pub manufacturer: String,
    /// The UTC commissioning date, `YYYYMMDD`.
    pub date: String,
    /// The signature, written as 128 lowercase hex digits.
    #[serde(with = "signature_hex")]
    pub signature: [u8; 64],
}

impl fmt::Display for PrivateLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&serde_json::to_string(self).map_err(|_| fmt::Error)?)
    }
}

/// The type of request a [`PrivateLine`] records. A live line is added before
/// the public line so it doesn't mean the signature was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestType {
    Live,
    DryRun,
}

/// What [`PublicRecord::add`] or [`PrivateRecord::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// The line was committed and pushed.
    New,
    /// The remote already had the line.
    AlreadyThere,
}

/// Why a record couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("can't run git: {0}")]
    Spawn(io::Error),
    #[error("git {command} failed: {message}")]
    Git { command: String, message: String },
    #[error("git {command} didn't finish within {} seconds", GIT_TIMEOUT.as_secs())]
    Timeout { command: String },
    #[error("can't read or write {path}: {source}")]
    File { path: PathBuf, source: io::Error },
    #[error("line {line} of {file} isn't a record line")]
    Malformed { file: String, line: usize },
}

impl PublicRecord {
    /// Opens the clone in `dir`. It must be a git work tree whose branch has an
    /// upstream to push to.
    pub async fn open(dir: PathBuf) -> Result<Self, Error> {
        Repository::open(dir).await.map(Self)
    }

    /// Adds `signature`'s line to key `id`'s file and pushes it. A line the
    /// remote already has isn't added again. Returns once the remote has the
    /// line.
    pub async fn add(&self, id: u16, signature: &[u8; 64]) -> Result<Added, Error> {
        self.0
            .add(
                &format!("signatures/{id}.txt"),
                &RecordLine::new(signature),
                &format!("Key {id}"),
            )
            .await
    }
}

impl PrivateRecord {
    /// Opens the clone in `dir`. It must be a git work tree whose branch has an
    /// upstream to push to.
    pub async fn open(dir: PathBuf) -> Result<Self, Error> {
        Repository::open(dir).await.map(Self)
    }

    /// Adds `line` to key `id`'s file and pushes it. A line the remote already
    /// has isn't added again. Returns once the remote has the line.
    pub async fn add(&self, id: u16, line: &PrivateLine) -> Result<Added, Error> {
        self.0
            .add(
                &format!("signatures/{id}.jsonl"),
                line,
                &format!("Key {id}: {}", format_chip_id(line.chip_id)),
            )
            .await
    }
}

/// A line of a record file.
trait Line: fmt::Display + PartialEq + Sized {
    /// The lines of `text`, or the number of the first line that isn't one.
    /// Blank lines are skipped.
    fn parse_file(text: &str) -> Result<Vec<Self>, usize>;
}

impl Line for RecordLine {
    fn parse_file(text: &str) -> Result<Vec<Self>, usize> {
        parse_record(text).map_err(|error| error.line)
    }
}

impl Line for PrivateLine {
    fn parse_file(text: &str) -> Result<Vec<Self>, usize> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(i, line)| serde_json::from_str(line).map_err(|_| i + 1))
            .collect()
    }
}

/// A clone of a record repository.
struct Repository {
    dir: PathBuf,
}

impl Repository {
    async fn open(dir: PathBuf) -> Result<Self, Error> {
        let repository = Self { dir };
        repository
            .git(&["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .await?;
        Ok(repository)
    }

    /// Adds `line` to `file`, commits it with `message` and pushes it. A line
    /// the remote already has isn't added again. Returns once the remote has
    /// the line.
    ///
    /// The clone is reset to the remote first so a line that a failed request
    /// left behind isn't taken as recorded.
    async fn add<L: Line>(&self, file: &str, line: &L, message: &str) -> Result<Added, Error> {
        self.git(&["fetch", "--quiet"]).await?;
        self.git(&["reset", "--quiet", "--hard", "@{upstream}"])
            .await?;
        self.git(&["clean", "--quiet", "--force", "-d", "-x"])
            .await?;

        let path = self.dir.join(file);
        let file_error = |source| Error::File {
            path: path.clone(),
            source,
        };
        let mut text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(file_error(error)),
        };
        let lines = L::parse_file(&text).map_err(|number| Error::Malformed {
            file: file.into(),
            line: number,
        })?;
        if lines.contains(line) {
            return Ok(Added::AlreadyThere);
        }

        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("{line}\n"));
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(file_error)?;
        }
        tokio::fs::write(&path, text).await.map_err(file_error)?;
        self.git(&["add", "--", file]).await?;
        self.git(&["commit", "--quiet", "--message", message])
            .await?;
        self.git(&["push", "--quiet"]).await?;
        Ok(Added::New)
    }

    /// Runs git in the clone.
    async fn git(&self, args: &[&str]) -> Result<(), Error> {
        let command = args.first().copied().unwrap_or_default();
        let run = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = timeout(GIT_TIMEOUT, run)
            .await
            .map_err(|_| Error::Timeout {
                command: command.into(),
            })?
            .map_err(Error::Spawn)?;
        if !output.status.success() {
            return Err(Error::Git {
                command: command.into(),
                message: String::from_utf8_lossy(&output.stderr)
                    .trim()
                    .replace('\n', " "),
            });
        }
        Ok(())
    }
}

/// Serde for a CHIPID as [`format_chip_id`] writes it.
mod chip_id_text {
    use onerom_metadata::otp::{format_chip_id, parse_chip_id};
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S: Serializer>(chip_id: &[u16; 4], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format_chip_id(*chip_id))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u16; 4], D::Error> {
        parse_chip_id(&String::deserialize(deserializer)?)
            .ok_or_else(|| de::Error::custom("isn't 16 uppercase hex digits"))
    }
}

/// Serde for a signature as 128 lowercase hex digits.
mod signature_hex {
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S: Serializer>(
        signature: &[u8; 64],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let text: String = signature.iter().map(|byte| format!("{byte:02x}")).collect();
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 64], D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() != 128 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(de::Error::custom("isn't 128 lowercase hex digits"));
        }
        let mut signature = [0; 64];
        for (i, byte) in signature.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).map_err(de::Error::custom)?;
        }
        Ok(signature)
    }
}
