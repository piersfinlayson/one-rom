// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The server's clone of the record repository. Key n's lines are recorded in
//! `signatures/n.txt`.

use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use onerom_metadata::otp::{RecordLine, format_chip_id, parse_record};
use tokio::process::Command;
use tokio::time::timeout;

/// The longest a git command may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// The server's clone of the record repository.
pub struct Record {
    dir: PathBuf,
}

/// What [`Record::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// The line was committed and pushed.
    New,
    /// The remote already had the line.
    AlreadyThere,
}

/// Why the record couldn't be written.
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

impl Record {
    /// Opens the clone in `dir`. It must be a git work tree whose branch has an
    /// upstream to push to.
    pub async fn open(dir: PathBuf) -> Result<Self, Error> {
        let record = Self { dir };
        record
            .git(&["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .await?;
        Ok(record)
    }

    /// Records `signature` for the chip whose CHIPID is `chip_id` in key `id`'s
    /// file and pushes it. A line the remote already has isn't added again.
    /// Returns once the remote has the line.
    ///
    /// The clone is reset to the remote first so a line that a failed request
    /// left behind isn't taken as recorded.
    pub async fn add(
        &self,
        id: u16,
        chip_id: [u16; 4],
        signature: &[u8; 64],
    ) -> Result<Added, Error> {
        self.git(&["fetch", "--quiet"]).await?;
        self.git(&["reset", "--quiet", "--hard", "@{upstream}"])
            .await?;
        self.git(&["clean", "--quiet", "--force", "-d", "-x"])
            .await?;

        let file = format!("signatures/{id}.txt");
        let path = self.dir.join(&file);
        let file_error = |source| Error::File {
            path: path.clone(),
            source,
        };
        let mut text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(file_error(error)),
        };
        let lines = parse_record(&text).map_err(|error| Error::Malformed {
            file: file.clone(),
            line: error.line,
        })?;
        let line = RecordLine::new(chip_id, signature);
        if lines.contains(&line) {
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
        self.git(&["add", "--", &file]).await?;
        self.git(&[
            "commit",
            "--quiet",
            "--message",
            &format!("Key {id}: {}", format_chip_id(chip_id)),
        ])
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
