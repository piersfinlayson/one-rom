// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom hardware`.
//!
//! Each command:
//! - stops a One ROM that's running or in limp mode
//! - reads or writes its OTP through [`LocalOtpAccess`]
//! - reboots that One ROM into running mode
//!
//! Tests drive the OTP part against onerom-app's `MemoryOtp`.

use std::fmt::Display;
use std::io::{IsTerminal, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use onerom_app::{
    LocalFetch, LocalOtpAccess, Plan, Request, RequestDate, Signer, SignerTable, StepKind, Verdict,
    prepare, read_chip_id, read_commissioning, verify_instance,
};
use onerom_cli::otp::{PicobootOtp, escape_controls, format_date};
use onerom_cli::signing::{KeyFile, SigningServer};
use onerom_cli::{CliFetch, DeviceState, Error, Options};
use onerom_metadata::otp::{AreaIssue, CommissioningInstance, format_chip_id};
use serde::Serialize;

use crate::args::hardware::{HardwareCommissionArgs, HardwareValidateArgs, today};
use crate::commissioning::{instance_values, issue_text, signer_name, unknown_keys};
use crate::program::{reboot_to_running, reboot_to_stopped};
use crate::utils::check_device;

/// The states a One ROM is stopped from before its OTP is read or written.
/// OTP is reached through the bootloader.
const STOP_FROM: [DeviceState; 2] = [DeviceState::Running, DeviceState::Limp];

// ---------------------------------------------------------------------------
// commission
// ---------------------------------------------------------------------------

/// The source of a commissioning instance's signature.
enum Signing {
    Server(SigningServer),
    File(KeyFile),
}

pub async fn cmd_commission(
    options: &mut Options,
    args: &HardwareCommissionArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();

    if let Some(firmware) = device.firmware_board()
        && firmware != args.board
    {
        if args.force {
            eprintln!(
                "Warning: the firmware's board type '{}' doesn't match '{}' (continuing due to --force)",
                firmware.name(),
                args.board.name()
            );
        } else {
            return Err(Error::BoardMismatch {
                firmware: firmware.name().to_string(),
                expected: args.board.name().to_string(),
            });
        }
    }

    // Asked for before the One ROM is stopped so a missing PIN leaves it as it
    // was.
    let signing = signing(args)?;

    let stopped = reboot_to_stopped(options, &STOP_FROM).await?;
    let result = commission(options, args, &signing).await;
    restart(options, stopped, result).await
}

/// The signature's source that `args` asks for. It asks the user for a PIN
/// where `--pin` doesn't provide one.
fn signing(args: &HardwareCommissionArgs) -> Result<Signing, Error> {
    if let Some(path) = &args.key {
        return Ok(Signing::File(KeyFile::read(path)?));
    }
    // clap requires --signer or --key.
    let url = args.signer.as_deref().unwrap_or_default();
    let pin = match &args.pin {
        Some(pin) => pin.clone(),
        None => read_pin(url)?,
    };
    if pin.is_empty() {
        return Err(Error::InvalidArgument(
            "--pin".to_string(),
            "The PIN is empty".to_string(),
        ));
    }
    Ok(Signing::Server(SigningServer::new(url, &pin)?))
}

/// Commissions the stopped One ROM.
async fn commission(
    options: &Options,
    args: &HardwareCommissionArgs,
    signing: &Signing,
) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    println!("{device}");

    let (table, _) = signer_table().await;
    let public_key = match signing {
        Signing::Server(server) => server.public_key().await?,
        Signing::File(key) => key.public_key(),
    };
    let signer = signing_key(&table, &public_key)?;
    let mut otp = PicobootOtp::open(device).await?;
    commission_otp(
        &mut otp,
        args,
        signing,
        (signer, &table),
        options.yes,
        &mut std::io::stdout(),
    )
    .await
}

/// The key in `table` whose public key is `public_key`. Refuses a key that
/// isn't in the table or is retired.
fn signing_key<'a>(table: &'a SignerTable, public_key: &[u8; 32]) -> Result<&'a Signer, Error> {
    let signer = table
        .find(public_key)
        .ok_or_else(|| Error::SigningKeyUnknown(hex::encode(public_key)))?;
    if signer.is_retired() {
        return Err(Error::SigningKeyRetired {
            id: signer.id(),
            name: escape_controls(signer.name()),
        });
    }
    Ok(signer)
}

/// Commissions the board `otp` reaches as `args` asks.
///
/// `signing` signs the instance and `signer`'s key in `table` checks the
/// signature. `yes` writes without asking.
async fn commission_otp<O: LocalOtpAccess>(
    otp: &mut O,
    args: &HardwareCommissionArgs,
    signing: &Signing,
    (signer, table): (&Signer, &SignerTable),
    yes: bool,
    out: &mut impl Write,
) -> Result<(), Error> {
    line(
        out,
        format!("Signing key: {}", signer_name(signer.id(), table)),
    )?;
    let request = Request {
        board: args.board,
        size: args.size,
        manufacturer: args.manufacturer.clone(),
        date: match &args.date {
            Some(date) => RequestDate::Given(date.clone()),
            None => RequestDate::Today(today()),
        },
        signer: signer.id(),
        force: args.force,
    };
    let prepared = prepare(otp, &request).await?;
    if prepared.date_from_current_instance() {
        line(
            out,
            "The current commissioning instance matches this request apart from its date. This run finishes it with that date.",
        )?;
    }
    line(
        out,
        format!(
            "Commissioning {} by {} on {} as size {}",
            args.board.name(),
            escape_controls(&args.manufacturer),
            format_date(prepared.date()),
            args.size
        ),
    )?;

    let signature = match signing {
        Signing::Server(server) => {
            server
                .sign(
                    prepared.chip_id(),
                    args.board,
                    &args.manufacturer,
                    prepared.date(),
                )
                .await?
        }
        Signing::File(key) => key.sign(prepared.message()),
    };
    check_signature(signer, prepared.message(), &signature)?;
    let plan = prepared.plan(&signature)?;

    let rows = print_plan(&plan, out)?;
    if rows == 0 {
        return line(
            out,
            "Every row already holds its value so there isn't anything to write.",
        );
    }
    line(out, "OTP can't be erased.")?;
    if yes {
        line(out, "Auto-accepted (--yes)")?;
    } else if !ask_to_write(rows, out)? {
        return line(out, "Aborted");
    }

    line(out, "Writing OTP - DO NOT DISCONNECT")?;
    let mut shown = Ok(());
    plan.execute(otp, |step| {
        let state = if step.writes.iter().all(|write| write.holds) {
            "already written"
        } else {
            "written"
        };
        if shown.is_ok() {
            shown = line(out, format!("{}: {state}", step_name(step.kind)));
        }
    })
    .await?;
    shown?;
    print_next_reset(&plan, out)
}

/// Refuses a signature that doesn't verify with `signer`'s key.
fn check_signature(signer: &Signer, message: &[u8], signature: &[u8; 64]) -> Result<(), Error> {
    if signer.verify(message, signature) {
        Ok(())
    } else {
        Err(Error::BadSignature {
            id: signer.id(),
            name: escape_controls(signer.name()),
        })
    }
}

/// Prints every row of `plan` and returns how many need writing.
fn print_plan(plan: &Plan, out: &mut impl Write) -> Result<usize, Error> {
    line(out, "Rows:")?;
    let mut rows = 0;
    for step in plan.steps() {
        line(out, format!("  {}", step_name(step.kind)))?;
        for write in &step.writes {
            let note = if write.holds {
                "  (already holds it)"
            } else {
                rows += 1;
                ""
            };
            line(
                out,
                format!("    {:#05x}  {}{note}", write.row, write.value),
            )?;
        }
    }
    Ok(rows)
}

/// The name of the part of OTP a step of `kind` writes.
fn step_name(kind: StepKind) -> String {
    match kind {
        StepKind::FlashDevinfo => "FLASH_DEVINFO".to_string(),
        StepKind::Instance => "Commissioning instance".to_string(),
        StepKind::Lock { page } => format!("Lock for page {page}"),
        StepKind::WhiteLabelStrings => "Bootloader USB strings".to_string(),
        StepKind::WhiteLabelTable => "White label table".to_string(),
        StepKind::WhiteLabelAddr => "USB_WHITE_LABEL_ADDR".to_string(),
        StepKind::UsbBootFlags => "USB_BOOT_FLAGS and its copies".to_string(),
        StepKind::BootFlags0 => "FLASH_DEVINFO_ENABLE in BOOT_FLAGS0 and its copies".to_string(),
    }
}

/// Asks the user whether to write `rows` rows.
fn ask_to_write(rows: usize, out: &mut impl Write) -> Result<bool, Error> {
    write!(out, "Write {rows} rows? (y/N): ").map_err(|e| Error::io("stdout", e))?;
    out.flush().map_err(|e| Error::io("stdout", e))?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| Error::io("stdin", e))?;

    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Prints the parts of OTP `plan` wrote that take effect only at a reset.
fn print_next_reset(plan: &Plan, out: &mut impl Write) -> Result<(), Error> {
    // Whether a step whose kind `part` matches wrote a row.
    let wrote = |part: fn(StepKind) -> bool| {
        plan.steps()
            .iter()
            .any(|step| part(step.kind) && step.writes.iter().any(|write| !write.holds))
    };
    let changes: Vec<&str> = [
        (
            wrote(|kind| {
                matches!(
                    kind,
                    StepKind::WhiteLabelStrings
                        | StepKind::WhiteLabelTable
                        | StepKind::WhiteLabelAddr
                        | StepKind::UsbBootFlags
                )
            }),
            "the bootloader USB strings",
        ),
        (
            wrote(|kind| matches!(kind, StepKind::Lock { .. })),
            "the page locks",
        ),
        (
            wrote(|kind| matches!(kind, StepKind::BootFlags0)),
            "FLASH_DEVINFO_ENABLE",
        ),
    ]
    .into_iter()
    .filter_map(|(wrote, change)| wrote.then_some(change))
    .collect();
    if !changes.is_empty() {
        line(out, "These take effect from the One ROM's next reset:")?;
        for change in changes {
            line(out, format!("- {change}"))?;
        }
    }
    Ok(())
}

/// Asks the terminal for the PIN of the key at `url` without echoing it.
fn read_pin(url: &str) -> Result<String, Error> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::NoPin);
    }
    print!("PIN for {url}: ");
    std::io::stdout()
        .flush()
        .map_err(|e| Error::io("stdout", e))?;

    terminal::enable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    let pin = read_hidden_line();
    // The terminal is restored whatever happened.
    let restored = terminal::disable_raw_mode();
    println!();
    restored.map_err(|e| Error::io("terminal", e))?;
    pin?.ok_or_else(|| Error::Aborted("The PIN wasn't entered".to_string()))
}

/// Reads a line from a terminal in raw mode. `None` where the user pressed
/// Escape or Ctrl-C.
#[allow(clippy::wildcard_enum_match_arm)]
fn read_hidden_line() -> Result<Option<String>, Error> {
    let mut line = String::new();
    loop {
        let Event::Key(key) = event::read().map_err(|e| Error::io("terminal", e))? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Enter => return Ok(Some(line)),
            KeyCode::Esc => return Ok(None),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(None);
            }
            KeyCode::Char(c) => line.push(c),
            KeyCode::Backspace => {
                line.pop();
            }
            // Every other key is ignored.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

pub async fn cmd_validate(options: &mut Options, args: &HardwareValidateArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let stopped = reboot_to_stopped(options, &STOP_FROM).await?;
    let result = validate(options, args).await;
    restart(options, stopped, result).await
}

/// The source of the table of signing keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SigningKeys {
    BuiltIn,
    Downloaded,
}

/// `hardware validate --json`'s output.
#[derive(Serialize)]
struct Validation<'a> {
    /// CHIPID as the bootloader's USB serial number shows it.
    chip_id: String,
    signing_keys: SigningKeys,
    instances: Vec<InstanceCheck<'a>>,
    issues: &'a [AreaIssue],
    validates: bool,
}

/// One instance's check.
#[derive(Serialize)]
struct InstanceCheck<'a> {
    row: u16,
    current: bool,
    board: Option<&'a str>,
    manufacturer: Option<&'a str>,
    date: Option<&'a str>,
    signer: Option<u16>,
    signer_name: Option<&'a str>,
    /// `None` where the check failed.
    verdict: Option<Verdict>,
    /// The reason the check failed.
    error: Option<String>,
    unknown_keys: Vec<UnknownKeyJson>,
}

/// An entry with a key this build doesn't know.
#[derive(Serialize)]
struct UnknownKeyJson {
    row: u16,
    key: u32,
    len: usize,
}

/// Checks the stopped One ROM's commissioning.
async fn validate(options: &Options, args: &HardwareValidateArgs) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if !args.json {
        println!("{device}");
    }
    let (table, keys) = signer_table().await;
    let mut otp = PicobootOtp::open(device).await?;
    validate_otp(
        &mut otp,
        (&table, keys),
        &CliFetch,
        args.json,
        &mut std::io::stdout(),
    )
    .await
}

/// Checks the commissioning of the board `otp` reaches against `table`.
/// `fetch` fetches a retired key's record file. `json` shows the result as
/// JSON.
async fn validate_otp<O: LocalOtpAccess, F: LocalFetch>(
    otp: &mut O,
    (table, keys): (&SignerTable, SigningKeys),
    fetch: &F,
    json: bool,
    out: &mut impl Write,
) -> Result<(), Error>
where
    F::Error: Display,
{
    let chip_id = read_chip_id(otp).await?;
    let area = read_commissioning(otp).await?;

    let current = area.current().map(CommissioningInstance::first_row);
    let mut checks = Vec::new();
    for instance in area.instances() {
        let verdict = verify_instance(instance, chip_id, table, fetch)
            .await
            .map_err(|e| e.to_string());
        checks.push((instance, verdict));
    }

    let outcome = match checks
        .iter()
        .find(|(instance, _)| Some(instance.first_row()) == current)
    {
        None => Err("OTP doesn't hold a current commissioning instance.".to_string()),
        Some((_, Ok(verdict))) if verdict.is_accepted() => Ok(()),
        Some((instance, Ok(verdict))) => Err(format!(
            "The current instance at row {:#05x}: {}",
            instance.first_row(),
            verdict_text(*verdict)
        )),
        Some((_, Err(e))) => Err(format!(
            "The current instance's signature couldn't be checked.\n  {e}"
        )),
    };

    if json {
        let validation = Validation {
            chip_id: format_chip_id(chip_id),
            signing_keys: keys,
            instances: checks
                .iter()
                .map(|(instance, verdict)| instance_check(instance, verdict, current, table))
                .collect(),
            issues: area.issues(),
            validates: outcome.is_ok(),
        };
        let json =
            serde_json::to_string_pretty(&validation).map_err(|e| Error::Other(e.to_string()))?;
        line(out, json)?;
    } else {
        let keys = match keys {
            SigningKeys::BuiltIn => "built-in",
            SigningKeys::Downloaded => "downloaded",
        };
        line(out, format!("Signing keys: {keys}"))?;
        if checks.is_empty() {
            line(out, "OTP doesn't hold a commissioning instance")?;
        }
        for (instance, verdict) in &checks {
            let current = if Some(instance.first_row()) == current {
                " (current)"
            } else {
                ""
            };
            line(
                out,
                format!(
                    "Instance at row {:#05x}{current}: {}",
                    instance.first_row(),
                    instance_values(instance)
                ),
            )?;
            line(out, format!("  {}", check_text(instance, verdict, table)))?;
            for unknown in unknown_keys(instance.entries()) {
                line(out, format!("  Skipped unknown {}", unknown.text()))?;
            }
        }
        for issue in area.issues() {
            line(out, format!("Commissioning area: {}", issue_text(issue)))?;
        }
        if outcome.is_ok() {
            line(out, "This One ROM's commissioning validates.")?;
        }
    }
    outcome.map_err(Error::NotValidated)
}

/// `instance`'s check as JSON.
fn instance_check<'a>(
    instance: &'a CommissioningInstance,
    verdict: &Result<Verdict, String>,
    current: Option<u16>,
    table: &'a SignerTable,
) -> InstanceCheck<'a> {
    let signer = instance.signer();
    InstanceCheck {
        row: instance.first_row(),
        current: Some(instance.first_row()) == current,
        board: instance.board(),
        manufacturer: instance.manufacturer(),
        date: instance.date(),
        signer,
        signer_name: signer.and_then(|id| table.get(id)).map(Signer::name),
        verdict: verdict.as_ref().ok().copied(),
        error: verdict.as_ref().err().cloned(),
        unknown_keys: unknown_keys(instance.entries())
            .map(|unknown| UnknownKeyJson {
                row: unknown.row,
                key: unknown.key,
                len: unknown.len,
            })
            .collect(),
    }
}

/// The line describing the check of `instance`'s signature.
fn check_text(
    instance: &CommissioningInstance,
    verdict: &Result<Verdict, String>,
    table: &SignerTable,
) -> String {
    let signer = || match instance.signer() {
        Some(id) => signer_name(id, table),
        None => "an unknown signer".to_string(),
    };
    match verdict {
        Ok(Verdict::Incomplete) => "Incomplete".to_string(),
        Ok(Verdict::Invalid) => "Missing values".to_string(),
        Ok(verdict) => format!("Signed by {}: {}", signer(), verdict_text(*verdict)),
        Err(e) => format!(
            "Signed by {}. The signature couldn't be checked.\n    {e}",
            signer()
        ),
    }
}

/// `verdict` as a phrase about an instance's signature.
fn verdict_text(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Verified => "verified",
        Verdict::Recorded => "verified and recorded before the key was retired",
        Verdict::NotRecorded => "not recorded before the key was retired",
        Verdict::RecordChanged => "the key's record has changed since it was retired",
        Verdict::BadSignature => "the signature doesn't verify",
        Verdict::UnknownSigner => "not in the table of signing keys",
        Verdict::Incomplete => "incomplete",
        Verdict::Invalid => "missing values",
    }
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// The current table of signing keys. Where it can't be downloaded, the table
/// built into this CLI.
async fn signer_table() -> (SignerTable, SigningKeys) {
    match SignerTable::download(&CliFetch).await {
        Ok(table) => (table, SigningKeys::Downloaded),
        Err(e) => {
            eprintln!(
                "Warning: couldn't download the signing keys so the ones built into this CLI are used.\n  {e}"
            );
            (SignerTable::built_in(), SigningKeys::BuiltIn)
        }
    }
}

/// Reboots a One ROM this command `stopped` back into running mode and
/// returns `result`. A failed reboot is the error where `result` is `Ok`.
async fn restart(options: &Options, stopped: bool, result: Result<(), Error>) -> Result<(), Error> {
    if !stopped {
        return result;
    }
    match (result, reboot_to_running(options).await) {
        (Ok(()), rebooted) => rebooted,
        (Err(e), Ok(())) => Err(e),
        (Err(e), Err(reboot)) => {
            eprintln!("Warning: couldn't reboot the One ROM into running mode.\n  {reboot}");
            Err(e)
        }
    }
}

/// Writes `text` and a newline to `out`.
fn line(out: &mut impl Write, text: impl Display) -> Result<(), Error> {
    writeln!(out, "{text}").map_err(|e| Error::io("stdout", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::Signer as _;
    use onerom_app::{BoardSize, CommissionError, MemoryOtp};
    use onerom_metadata::otp::RecordLine;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    use crate::test_board::{self, CHIP_ID, Files, blank_board, key_file, table};

    /// The URL of the retired key's record in [`retired_table`].
    const RECORD_URL: &str = "https://example.invalid/signatures/1.txt";

    /// Commissions `otp` as `args` asks with the test key. Returns the output.
    async fn commission(
        otp: &mut MemoryOtp,
        args: &HardwareCommissionArgs,
    ) -> Result<String, (Error, String)> {
        let table = table(None);
        let signing = Signing::File(KeyFile::read(args.key.as_ref().unwrap()).unwrap());
        let Signing::File(key) = &signing else {
            unreachable!()
        };
        let signer = signing_key(&table, &key.public_key()).unwrap();
        let mut out = Vec::new();
        let result = commission_otp(otp, args, &signing, (signer, &table), true, &mut out).await;
        let out = String::from_utf8(out).unwrap();
        match result {
            Ok(()) => Ok(out),
            Err(e) => Err((e, out)),
        }
    }

    /// Validates `otp` against `table`. Returns the output.
    async fn validate(
        otp: &mut MemoryOtp,
        table: &SignerTable,
        files: &Files,
        json: bool,
    ) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let result =
            validate_otp(otp, (table, SigningKeys::Downloaded), files, json, &mut out).await;
        (result, String::from_utf8(out).unwrap())
    }

    /// A board commissioned as fire-24-f by the test key.
    async fn commissioned_board() -> MemoryOtp {
        test_board::commissioned_board("fire-24-f", BoardSize::M).await
    }

    #[tokio::test]
    async fn an_m_board_is_commissioned_and_validates() {
        let (_dir, path) = key_file();
        let mut otp = blank_board();
        let out = commission(&mut otp, &test_board::args("fire-24-f", BoardSize::M, path))
            .await
            .unwrap();
        println!("{out}");
        for expected in [
            "Signing key: test signer (1)\n",
            "Commissioning fire-24-f by piers.rocks on 2026-01-01 as size M\n",
            "Rows:\n  Commissioning instance\n    0x0c0  0x524f with ECC\n    0x0c1  0x0001 with ECC\n",
            "  Lock for page 3\n    0xf87  0x151515 raw\n",
            "OTP can't be erased.\nAuto-accepted (--yes)\nWriting OTP - DO NOT DISCONNECT\n",
            "Commissioning instance: written\nLock for page 3: written\n",
            "USB_BOOT_FLAGS and its copies: written\n",
            "These take effect from the One ROM's next reset:\n- the bootloader USB strings\n- the page locks\n",
        ] {
            assert!(out.contains(expected), "{expected}\n---\n{out}");
        }
        assert!(!out.contains("FLASH_DEVINFO"), "{out}");

        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        println!("{out}");
        result.unwrap();
        assert_eq!(
            out,
            "Signing keys: downloaded\n\
             Instance at row 0x0c0 (current): fire-24-f by piers.rocks on 2026-01-01\n  \
             Signed by test signer (1): verified\n\
             This One ROM's commissioning validates.\n"
        );
    }

    #[tokio::test]
    async fn an_l_board_gets_its_second_flash_chip() {
        let (_dir, path) = key_file();
        let mut otp = blank_board();
        let out = commission(&mut otp, &test_board::args("fire-40-a", BoardSize::L, path))
            .await
            .unwrap();
        for expected in [
            "  FLASH_DEVINFO\n    0x054  0x99af with ECC\n",
            "  FLASH_DEVINFO_ENABLE in BOOT_FLAGS0 and its copies\n    0x048  0x000020 raw\n",
            "FLASH_DEVINFO: written\n",
            "- FLASH_DEVINFO_ENABLE\n",
        ] {
            assert!(out.contains(expected), "{expected}\n---\n{out}");
        }
    }

    #[tokio::test]
    async fn a_second_run_writes_nothing() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();
        let writes = otp.write_count();

        let out = commission(&mut otp, &args).await.unwrap();
        assert_eq!(otp.write_count(), writes);
        assert!(
            out.ends_with("Every row already holds its value so there isn't anything to write.\n"),
            "{out}"
        );
        assert!(
            out.contains("0x0c0  0x524f with ECC  (already holds it)"),
            "{out}"
        );
        assert!(!out.contains("OTP can't be erased"), "{out}");
    }

    /// A run without --date finishes an instance that differs only in its date.
    #[tokio::test]
    async fn a_run_without_a_date_takes_the_current_instances_date() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();

        args.date = None;
        let out = commission(&mut otp, &args).await.unwrap();
        assert!(
            out.contains(
                "The current commissioning instance matches this request apart from its date. This run finishes it with that date.\n\
                 Commissioning fire-24-f by piers.rocks on 2026-01-01 as size M\n"
            ),
            "{out}"
        );
    }

    #[tokio::test]
    async fn another_date_is_another_commissioning() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();
        let rows = otp.rows().to_vec();

        args.date = Some("20260102".to_string());
        let (error, _) = commission(&mut otp, &args).await.unwrap_err();
        assert!(
            matches!(
                error,
                Error::Commission(CommissionError::AlreadyCommissioned {
                    only_date_differs: true,
                    ..
                })
            ),
            "{error}"
        );
        let text = error.to_string();
        assert!(
            text.contains("already commissioned as fire-24-f by piers.rocks on 2026-01-01"),
            "{text}"
        );
        assert!(text.contains("Leave out --date"), "{text}");
        assert_eq!(otp.rows(), rows);
    }

    /// Options for a run that didn't find a device.
    fn no_device() -> Options {
        Options {
            verbose: false,
            log_level: onerom_cli::LogLevel::Warn,
            yes: false,
            unrecognised: false,
            device: None,
            vid_pid: Vec::new(),
        }
    }

    /// Each command checks for a device before it opens anything.
    #[tokio::test]
    async fn validate_and_commission_need_a_device() {
        for json in [false, true] {
            let error = cmd_validate(&mut no_device(), &HardwareValidateArgs { json })
                .await
                .unwrap_err();
            assert!(matches!(error, Error::NoDevice), "{error}");
        }
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let error = cmd_commission(&mut no_device(), &args).await.unwrap_err();
        assert!(matches!(error, Error::NoDevice), "{error}");
    }

    #[test]
    fn an_unknown_or_retired_key_is_refused() {
        let unknown = [9; 32];
        assert!(matches!(
            signing_key(&table(None), &unknown),
            Err(Error::SigningKeyUnknown(_))
        ));
        let public_key = test_board::key().verifying_key().to_bytes();
        assert!(signing_key(&table(None), &public_key).is_ok());
        let retired = table(Some(json!({})));
        assert!(matches!(
            signing_key(&retired, &public_key),
            Err(Error::SigningKeyRetired { id: 1, .. })
        ));
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let table = table(None);
        let signer = table.get(1).unwrap();
        let signature = test_board::key().sign(b"message").to_bytes();
        assert!(check_signature(signer, b"message", &signature).is_ok());
        let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32])
            .sign(b"message")
            .to_bytes();
        let error = check_signature(signer, b"message", &other).unwrap_err();
        assert!(
            matches!(error, Error::BadSignature { id: 1, .. }),
            "{error}"
        );
        assert!(check_signature(signer, b"other message", &signature).is_err());
    }

    #[tokio::test]
    async fn validate_shows_json() {
        let mut otp = commissioned_board().await;
        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), true).await;
        println!("{out}");
        result.unwrap();
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            json,
            json!({
                "chip_id": "DE3F9C232F655B6B",
                "signing_keys": "downloaded",
                "instances": [{
                    "row": 0x0c0,
                    "current": true,
                    "board": "fire-24-f",
                    "manufacturer": "piers.rocks",
                    "date": "20260101",
                    "signer": 1,
                    "signer_name": "test signer",
                    "verdict": "verified",
                    "error": null,
                    "unknown_keys": [],
                }],
                "issues": [],
                "validates": true,
            })
        );
    }

    #[tokio::test]
    async fn a_board_without_commissioning_doesnt_validate() {
        let mut otp = blank_board();
        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        assert_eq!(
            out,
            "Signing keys: downloaded\nOTP doesn't hold a commissioning instance\n"
        );
        let error = result.unwrap_err();
        assert!(matches!(error, Error::NotValidated(_)), "{error}");
        assert!(
            error
                .to_string()
                .contains("doesn't hold a current commissioning instance"),
            "{error}"
        );

        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), true).await;
        assert!(result.is_err());
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["validates"], false);
    }

    #[tokio::test]
    async fn an_unknown_signer_doesnt_validate() {
        let mut otp = commissioned_board().await;
        let empty = SignerTable::parse(br#"{"version": 1, "signers": []}"#).unwrap();
        let (result, out) = validate(&mut otp, &empty, &Files(Vec::new()), false).await;
        assert!(
            out.contains("  Signed by signer 1: not in the table of signing keys\n"),
            "{out}"
        );
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("The current instance at row 0x0c0: not in the table of signing keys"),
            "{error}"
        );
    }

    /// A table in which the test key is retired with a record at
    /// [`RECORD_URL`] whose SHA-256 is `record`'s.
    fn retired_table(record: &[u8]) -> SignerTable {
        table(Some(json!({
            "record": RECORD_URL,
            "sha256": hex::encode(Sha256::digest(record)),
        })))
    }

    /// The record line of `otp`'s current instance.
    async fn record_line(otp: &mut MemoryOtp) -> String {
        let area = read_commissioning(otp).await.unwrap();
        let signature = area.current().unwrap().signature().unwrap();
        format!("{}\n", RecordLine::new(CHIP_ID, signature))
    }

    #[tokio::test]
    async fn a_retired_keys_recorded_signature_validates() {
        let mut otp = commissioned_board().await;
        let record = record_line(&mut otp).await.into_bytes();
        let files = Files(vec![(RECORD_URL.to_string(), record.clone())]);
        let (result, out) = validate(&mut otp, &retired_table(&record), &files, false).await;
        result.unwrap();
        assert!(
            out.contains(
                "  Signed by test signer (1): verified and recorded before the key was retired\n"
            ),
            "{out}"
        );
    }

    #[tokio::test]
    async fn a_retired_keys_unrecorded_signature_doesnt_validate() {
        let mut otp = commissioned_board().await;

        // Without a record.
        let (result, out) =
            validate(&mut otp, &table(Some(json!({}))), &Files(Vec::new()), false).await;
        assert!(result.is_err());
        assert!(
            out.contains(": not recorded before the key was retired\n"),
            "{out}"
        );

        // A record that doesn't list it.
        let record =
            b"E126C9F97C10ADAC 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\n";
        let files = Files(vec![(RECORD_URL.to_string(), record.to_vec())]);
        let (result, out) = validate(&mut otp, &retired_table(record), &files, false).await;
        assert!(result.is_err());
        assert!(
            out.contains(": not recorded before the key was retired\n"),
            "{out}"
        );

        // A record changed since the key was retired.
        let line = record_line(&mut otp).await;
        let files = Files(vec![(RECORD_URL.to_string(), line.into_bytes())]);
        let (result, out) = validate(&mut otp, &retired_table(record), &files, false).await;
        assert!(result.is_err());
        assert!(
            out.contains(": the key's record has changed since it was retired\n"),
            "{out}"
        );

        // A record that can't be fetched.
        let (result, out) =
            validate(&mut otp, &retired_table(record), &Files(Vec::new()), false).await;
        let error = result.unwrap_err().to_string();
        assert!(out.contains("The signature couldn't be checked."), "{out}");
        assert!(error.contains("couldn't be checked"), "{error}");
        assert!(error.contains(RECORD_URL), "{error}");
    }
}
