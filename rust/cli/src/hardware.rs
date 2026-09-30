// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom hardware`.
//!
//! Each command apart from `sign`:
//! - stops a One ROM that's running or in limp mode
//! - reads or writes its OTP through [`LocalOtpAccess`]
//! - reboots that One ROM into running mode
//!
//! `sign` doesn't use a One ROM. Tests drive the OTP part against
//! onerom-app's `MemoryOtp`.

use std::fmt::Display;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use onerom_app::{
    BoardSize, CommissionError, LocalFetch, LocalOtpAccess, Plan, Request, RequestDate, RowValue,
    Signer, SignerTable, Step, StepKind, Verdict, plan_size, prepare, read_board_size,
    read_chip_id, read_commissioning, verify_instance,
};
use onerom_cli::error::NEWER_DATA;
use onerom_cli::otp::{PicobootOtp, board_size_text, escape_controls, format_date};
use onerom_cli::signing::{KeyFile, SigningServer};
use onerom_cli::{CliFetch, DeviceState, Error, Options};
use onerom_config::hw::Board;
use onerom_metadata::MaybeKnown;
use onerom_metadata::otp::{
    AreaIssue, BuildError, CommissioningInstance, CommissioningValues, format_chip_id, white_label,
};
use serde::Serialize;

use crate::args::hardware::{
    HardwareCommissionArgs, HardwareRequestSignatureArgs, HardwareSetSizeArgs, HardwareSignArgs,
    HardwareValidateArgs, today,
};
use crate::commissioning::{
    Labelled, instance_state, instance_values, issue_text, labelled_lines, signer_name,
    unknown_keys, unknown_version, white_label_values,
};
use crate::inspect::write_otp;
use crate::program::{reboot_stopped, reboot_stopped_and_select, reboot_to_stopped, restart};
use crate::signing_request::{self, SigningRequest};
use crate::utils::check_device;

/// The states a One ROM is stopped from before its OTP is read or written.
/// OTP is reached through the bootloader.
const STOP_FROM: [DeviceState; 2] = [DeviceState::Running, DeviceState::Limp];

/// onerom-app's error carrying the CLI's fetch error.
type AppError = onerom_app::Error<onerom_fw::Error>;

// ---------------------------------------------------------------------------
// commission
// ---------------------------------------------------------------------------

/// The source of a commissioning instance's signature.
pub(crate) enum Signing {
    Server(SigningServer),
    File(KeyFile),
    /// A signature `--signature` provides, from the key whose ID is
    /// `key_id`.
    Given {
        key_id: u16,
        signature: [u8; 64],
    },
}

/// What a commissioning instance's signature covers.
pub(crate) struct ToSign<'a> {
    pub(crate) chip_id: [u16; 4],
    pub(crate) board: Board,
    pub(crate) manufacturer: &'a str,
    pub(crate) date: &'a str,
    /// The message the values and `chip_id` make.
    pub(crate) message: &'a [u8],
}

/// Signs a commissioning instance.
///
/// Each command asks twice. The first signature isn't recorded and is
/// checked before the user is asked. The second is recorded, and is only
/// asked for once they agree. `hardware sign --dry-run` asks only for the
/// first.
pub(crate) trait SignatureSource {
    /// The signature over `to_sign`, not recorded.
    async fn dry_run(&self, to_sign: &ToSign<'_>) -> Result<[u8; 64], Error>;

    /// The signature over `to_sign`, recorded. `None` from a source that
    /// doesn't keep a record.
    async fn record(&self, to_sign: &ToSign<'_>) -> Result<Option<[u8; 64]>, Error>;
}

impl SignatureSource for Signing {
    async fn dry_run(&self, to_sign: &ToSign<'_>) -> Result<[u8; 64], Error> {
        match self {
            Self::Server(server) => {
                server
                    .sign_dry_run(
                        to_sign.chip_id,
                        to_sign.board,
                        to_sign.manufacturer,
                        to_sign.date,
                    )
                    .await
            }
            Self::File(key) => Ok(key.sign(to_sign.message)),
            Self::Given { signature, .. } => Ok(*signature),
        }
    }

    async fn record(&self, to_sign: &ToSign<'_>) -> Result<Option<[u8; 64]>, Error> {
        match self {
            Self::Server(server) => server
                .sign(
                    to_sign.chip_id,
                    to_sign.board,
                    to_sign.manufacturer,
                    to_sign.date,
                )
                .await
                .map(Some),
            Self::File(_) | Self::Given { .. } => Ok(None),
        }
    }
}

pub async fn cmd_commission(
    options: &mut Options,
    args: &HardwareCommissionArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();
    check_firmware(
        OtpCommand::Commission,
        device.firmware_board(),
        args.board,
        args.force,
    )?;

    // The PIN and the signing key are checked before the One ROM is stopped so
    // a missing PIN or a refused key leaves it as it was.
    let signing = signing(args)?;
    let (table, keys) = signer_table().await;
    let signer = signer_for(&table, &signing, &args.manufacturer).await?;

    let stopped = reboot_to_stopped(options, &STOP_FROM).await?;
    let result = commission(options, args, &signing, (signer, &table)).await;
    if matches!(result, Ok(true)) && (args.validate || args.inspect_otp) {
        let result = check_commissioned(options, args, (&table, keys)).await;
        return restart(options, stopped, result).await;
    }
    reboot_after_writing(options, stopped, result).await
}

/// Reboots the One ROM into stopped mode so the settings commissioning wrote
/// take effect. Then runs `--validate` and `--inspect-otp` where `args` sets
/// them. `table` is the signing key table commissioning used, from the source
/// `keys`.
async fn check_commissioned(
    options: &mut Options,
    args: &HardwareCommissionArgs,
    (table, keys): (&SignerTable, SigningKeys),
) -> Result<(), Error> {
    reboot_stopped_and_select(options).await?;
    let device = options.device.as_ref().unwrap();
    let mut otp = PicobootOtp::open(device).await?;
    let built_in = SignerTable::built_in();
    check_commissioned_otp(
        &mut otp,
        device,
        args.validate.then_some(((table, keys), &CliFetch)),
        args.inspect_otp.then_some(&built_in),
        options.verbose,
        &mut std::io::stdout(),
    )
    .await
}

/// Runs `--validate` and then `--inspect-otp` on the board `otp` reaches. Each
/// prints a blank line, then what `hardware validate` or `inspect otp` prints
/// with `device` as the board's device line.
///
/// `validate` contains:
/// - the signing key table and its source `keys`
/// - `fetch`, which fetches a retired key's record file
///
/// `inspect` is the table `inspect otp` takes key names from. A step that is
/// `None` is skipped. A failed validation ends the run before `--inspect-otp`.
pub(crate) async fn check_commissioned_otp<
    O: LocalOtpAccess,
    F: LocalFetch<Error = onerom_fw::Error>,
>(
    otp: &mut O,
    device: &impl Display,
    validate: Option<((&SignerTable, SigningKeys), &F)>,
    inspect: Option<&SignerTable>,
    verbose: bool,
    out: &mut impl Write,
) -> Result<(), Error> {
    if let Some((keys, fetch)) = validate {
        line(out, "")?;
        line(out, device)?;
        validate_otp(otp, keys, fetch, (false, verbose), out).await?;
    }
    if let Some(table) = inspect {
        line(out, "")?;
        write_otp(otp, device, table, verbose, out).await?;
    }
    Ok(())
}

/// The signature's source that `args` asks for. It asks the user for the PIN
/// of a signing server's key or an encrypted key file where `--pin` doesn't
/// provide one.
pub(crate) fn signing(args: &HardwareCommissionArgs) -> Result<Signing, Error> {
    if let Some(signature) = args.signature {
        // clap requires --key-id with --signature.
        return Ok(Signing::Given {
            key_id: args.key_id.unwrap_or_default(),
            signature,
        });
    }
    key_source(
        (args.signer.as_deref(), args.key_id),
        args.key.as_deref(),
        args.pin.as_deref(),
        &mut std::io::stdout(),
    )
}

/// The signing server's key or the key file that `--signer` and `--key-id`
/// or `--key` identify. `pin` is `--pin`. Where it doesn't provide the PIN of
/// the server's key or of an encrypted key file, the terminal is asked for it
/// and `prompt` shows the question.
pub(crate) fn key_source(
    (signer, key_id): (Option<&str>, Option<u16>),
    key: Option<&Path>,
    pin: Option<&str>,
    prompt: &mut impl Write,
) -> Result<Signing, Error> {
    if let Some(path) = key {
        let name = path.display().to_string();
        let pin = match pin {
            Some(pin) => Some(pin.to_string()),
            None if KeyFile::is_encrypted(path)? => Some(read_pin(
                &name,
                Error::KeyFileEncrypted(name.clone()),
                prompt,
            )?),
            None => None,
        };
        return Ok(Signing::File(KeyFile::read(path, pin.as_deref())?));
    }
    // clap requires --signer where --key isn't given, and --key-id with
    // --signer.
    let address = signer.unwrap_or_default();
    let id = key_id.unwrap_or_default();
    let pin = match pin {
        Some(pin) => pin.to_string(),
        None => read_pin(&format!("signing key #{id}"), Error::NoPin, prompt)?,
    };
    Ok(Signing::Server(SigningServer::new(address, id, &pin)?))
}

/// Commissions the stopped One ROM, signing with `signer`'s key in `table`.
/// Returns whether it wrote to OTP.
async fn commission(
    options: &Options,
    args: &HardwareCommissionArgs,
    signing: &Signing,
    (signer, table): (&Signer, &SignerTable),
) -> Result<bool, Error> {
    let device = options.device.as_ref().unwrap();
    println!("{device}");

    let mut otp = PicobootOtp::open(device).await?;
    commission_otp(
        &mut otp,
        args,
        signing,
        (signer, table),
        options,
        &mut std::io::stdout(),
        &mut std::io::stdin().lock(),
    )
    .await
}

/// The key in `table` that `signing` signs `manufacturer` with. It refuses a
/// key the table doesn't contain, has retired or doesn't allow to sign
/// `manufacturer`. For a signing server it then fetches the key's public key
/// and refuses one that isn't the table's.
pub(crate) async fn signer_for<'a>(
    table: &'a SignerTable,
    signing: &Signing,
    manufacturer: &str,
) -> Result<&'a Signer, Error> {
    let signer = match signing {
        Signing::File(key) => signing_key(table, &key.public_key())?,
        Signing::Server(server) => signer_with_id(table, server.id())?,
        Signing::Given { key_id, .. } => signer_with_id(table, *key_id)?,
    };
    check_key_allows(signer, manufacturer)?;
    if let Signing::Server(server) = signing {
        let public_key = server.public_key().await?;
        check_server_key(table, server.id(), &public_key, server.address())?;
    }
    Ok(signer)
}

/// The key in `table` whose public key is `public_key`. Refuses a key that
/// isn't in the table or is retired.
pub(crate) fn signing_key<'a>(
    table: &'a SignerTable,
    public_key: &[u8; 32],
) -> Result<&'a Signer, Error> {
    let signer = table
        .find(public_key)
        .ok_or_else(|| Error::SigningKeyUnknown(hex::encode(public_key)))?;
    current(signer)
}

/// The key in `table` whose ID is `id`. Refuses a key that isn't in the table
/// or is retired.
pub(crate) fn signer_with_id(table: &SignerTable, id: u16) -> Result<&Signer, Error> {
    let signer = table.get(id).ok_or(Error::SigningKeyIdUnknown(id))?;
    current(signer)
}

/// `signer`, refused where it's retired.
fn current(signer: &Signer) -> Result<&Signer, Error> {
    if signer.is_retired() {
        return Err(Error::SigningKeyRetired {
            id: signer.id(),
            name: escape_controls(signer.name()),
        });
    }
    Ok(signer)
}

/// Refuses `manufacturer` where `signer`'s key doesn't allow it.
pub(crate) fn check_key_allows(signer: &Signer, manufacturer: &str) -> Result<(), Error> {
    if signer.allows(manufacturer) {
        Ok(())
    } else {
        Err(Error::ManufacturerNotAllowed {
            id: signer.id(),
            name: escape_controls(signer.name()),
            manufacturer: manufacturer.to_string(),
        })
    }
}

/// Refuses `public_key`, the public key of key `id` on the signing server at
/// `address`, where it isn't key `id` in `table`.
pub(crate) fn check_server_key(
    table: &SignerTable,
    id: u16,
    public_key: &[u8; 32],
    address: &str,
) -> Result<(), Error> {
    if table.find(public_key).map(Signer::id) == Some(id) {
        Ok(())
    } else {
        Err(Error::SigningServerKeyMismatch {
            url: address.to_string(),
            id,
        })
    }
}

/// Commissions the board `otp` reaches as `args` asks.
///
/// `signing` signs the instance and `signer`'s key in `table` checks the
/// signature. With `--yes` it writes without asking. Otherwise `input` holds
/// the user's answer. `--verbose` lists every row and the bootloader USB
/// info it writes. Returns whether it wrote to OTP.
pub(crate) async fn commission_otp<O: LocalOtpAccess, S: SignatureSource>(
    otp: &mut O,
    args: &HardwareCommissionArgs,
    signing: &S,
    (signer, table): (&Signer, &SignerTable),
    options: &Options,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<bool, Error> {
    let size = args
        .board_size()
        .map_err(|e| Error::InvalidArgument("--size".to_string(), e.to_string()))?;
    let request = Request {
        board: args.board,
        size,
        manufacturer: args.manufacturer.clone(),
        date: match &args.date {
            Some(date) => RequestDate::Given(date.clone()),
            None => RequestDate::Today(today()),
        },
        signer: signer.id(),
        force: args.force,
    };
    let prepared = match prepare(otp, &request).await {
        Ok(prepared) => prepared,
        // The refusal shows the instance's values, with the signing key's
        // name from the table.
        Err(CommissionError::AlreadyCommissioned {
            board,
            manufacturer,
            date,
            signer,
            ..
        }) => {
            let values: Vec<Labelled> = [
                ("Board type:", escape_controls(&board)),
                ("Manufacturer:", escape_controls(&manufacturer)),
                ("Date:", format_date(&date)),
                ("Signing key:", signer_name(signer, table)),
            ]
            .into_iter()
            .map(|(label, value)| (label.to_string(), value))
            .collect();
            let instance: Vec<String> = labelled_lines(&values)
                .into_iter()
                .map(|line| format!("  {line}"))
                .collect();
            return Err(Error::AlreadyCommissioned {
                instance: instance.join("\n"),
            });
        }
        Err(e) => return Err(e.into()),
    };
    if let Some(replaced) = prepared.replaced_instance() {
        let mut values = instance_values(replaced);
        if let Some(signer) = replaced.signer() {
            values.push(("Signing key:".to_string(), signer_name(signer, table)));
        }
        line(out, "WARNING: Re-commissioning this One ROM.")?;
        line(out, "Commissioned: yes")?;
        for text in labelled_lines(&values) {
            line(out, text)?;
        }
    }
    let mut shown_date = format_date(prepared.date());
    if prepared.date_from_current_instance() {
        shown_date.push_str(" (from the commissioning instance)");
    }
    let values: Vec<Labelled> = [
        ("Board type:", args.board.name().to_string()),
        ("Board size:", size.to_string()),
        ("Manufacturer:", escape_controls(&args.manufacturer)),
        ("Date:", shown_date),
        ("Signing key:", signer_name(signer.id(), table)),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_string(), value))
    .collect();
    line(out, "Commissioning:")?;
    for text in labelled_lines(&values) {
        line(out, text)?;
    }

    // Planning consumes `prepared`, and the recorded signature needs these.
    let date = prepared.date().to_string();
    let message = prepared.message().to_vec();
    let to_sign = ToSign {
        chip_id: prepared.chip_id(),
        board: args.board,
        manufacturer: &args.manufacturer,
        date: &date,
        message: &message,
    };
    let signature = signing.dry_run(&to_sign).await?;
    check_signature(signer, &message, &signature)?;
    let plan = prepared.plan(&signature)?;

    // The strings are the same on every One ROM apart from the board's name,
    // so only --verbose shows them.
    if options.verbose && writes_white_label_strings(&plan) {
        line(out, "Bootloader USB info:")?;
        for text in labelled_lines(&white_label_strings(args.board)) {
            line(out, text)?;
        }
    }
    let rows = print_plan(&plan, options.verbose, out)?;
    if rows == 0 {
        line(out, "Nothing to write")?;
        return Ok(false);
    }
    if args.dry_run {
        line(out, "Dry run - nothing written")?;
        return Ok(false);
    }
    if !agree_to_write(rows, options, out, input)? {
        return Ok(false);
    }

    // Ed25519 makes one signature from a key and a message, so the recorded
    // signature is the one shown unless the source changed its key.
    if let Some(recorded) = signing.record(&to_sign).await?
        && recorded != signature
    {
        return Err(Error::RecordedSignatureDiffers);
    }

    write_plan(&plan, otp, out, Error::CommissionFailed).await?;
    line(out, "Commissioning complete")?;
    Ok(true)
}

/// Whether `plan` writes any of the bootloader USB strings' rows.
fn writes_white_label_strings(plan: &Plan) -> bool {
    plan.steps().iter().any(|step| {
        matches!(step.kind, StepKind::WhiteLabelStrings)
            && step.writes.iter().any(|write| !write.holds)
    })
}

/// The bootloader USB IDs and strings `hardware commission` writes for
/// `board`, a label and value each.
fn white_label_strings(board: Board) -> Vec<Labelled> {
    // A test converts every board's white label.
    let json = white_label(board)
        .to_json()
        .expect("pico-otp refuses a board's white label");
    white_label_values(&json)
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

/// Prints a line for each part of OTP `plan` writes and, with `verbose`, every
/// row. Returns how many rows need writing.
fn print_plan(plan: &Plan, verbose: bool, out: &mut impl Write) -> Result<usize, Error> {
    let parts: Vec<Part> = plan.steps().iter().map(Part::new).collect();
    print_parts(out, "Already present:", &parts, |part| part.present)?;
    print_parts(out, "To write:", &parts, |part| part.to_write)?;
    if verbose {
        print_rows(plan, out)?;
    }
    Ok(parts.iter().map(|part| part.to_write).sum())
}

/// A part of OTP a plan writes, such as the commissioning instance.
struct Part {
    label: String,
    /// Its rows already holding their value.
    present: usize,
    /// Its rows to write.
    to_write: usize,
}

impl Part {
    fn new(step: &Step) -> Self {
        let label = match (step.kind, step.writes.iter().map(|write| write.row).min()) {
            (StepKind::Instance, Some(first)) => {
                format!("{} at row {first:#05x}", step_name(step.kind))
            }
            (kind, _) => step_name(kind),
        };
        let present = step.writes.iter().filter(|write| write.holds).count();
        Self {
            label,
            present,
            to_write: step.writes.len() - present,
        }
    }
}

/// Prints `heading`, a line for each of `parts` with rows that `rows` counts
/// and their total. Nothing where none has any.
fn print_parts(
    out: &mut impl Write,
    heading: &str,
    parts: &[Part],
    rows: fn(&Part) -> usize,
) -> Result<(), Error> {
    let listed: Vec<&Part> = parts.iter().filter(|part| rows(part) > 0).collect();
    if listed.is_empty() {
        return Ok(());
    }
    // Both lists share their columns. A total is the widest count.
    let width = parts
        .iter()
        .map(|part| part.label.len())
        .chain([TOTAL.len()])
        .max()
        .unwrap_or_default();
    let present: usize = parts.iter().map(|part| part.present).sum();
    let to_write: usize = parts.iter().map(|part| part.to_write).sum();
    let digits = present.max(to_write).to_string().len();
    let count = |label: &str, n: usize| {
        let unit = if n == 1 { "row" } else { "rows" };
        format!("  {label:width$}  {n:>digits$} {unit}")
    };
    line(out, heading)?;
    for part in &listed {
        line(out, count(&part.label, rows(part)))?;
    }
    line(
        out,
        count(TOTAL, listed.iter().map(|part| rows(part)).sum()),
    )
}

/// The label of a list's total.
const TOTAL: &str = "Total";

/// Prints every row of `plan`, each part's in row order. A part writes an
/// entry's key row after its value, so that isn't the order it writes them.
fn print_rows(plan: &Plan, out: &mut impl Write) -> Result<(), Error> {
    line(out, "Rows:")?;
    for step in plan.steps() {
        line(out, format!("  {}", step_name(step.kind)))?;
        let mut writes: Vec<_> = step.writes.iter().collect();
        writes.sort_by_key(|write| write.row);
        for write in writes {
            let value = match write.value {
                RowValue::Ecc(value) => format!("{value:#06x} with ECC"),
                RowValue::Raw(value) => format!("{value:#08x} raw"),
            };
            let note = if write.holds {
                "  (already present)"
            } else {
                ""
            };
            line(out, format!("    {:#05x}  {value}{note}", write.row))?;
        }
    }
    Ok(())
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

/// Warns that OTP writes can't be undone and asks the user whether to write
/// `rows` rows. With `--yes` it doesn't ask. `input` contains their answer.
/// Returns whether to write.
fn agree_to_write(
    rows: usize,
    options: &Options,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<bool, Error> {
    line(out, "WARNING: OTP writes cannot be undone.")?;
    agree(&format!("Write {rows} rows?"), options, out, input)
}

/// Asks the user `question`. With `--yes` it doesn't ask. `input` contains
/// their answer. Returns whether they agreed.
fn agree(
    question: &str,
    options: &Options,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<bool, Error> {
    if options.yes {
        line(out, "Auto-accepted (--yes)")?;
        Ok(true)
    } else if ask(question, out, input)? {
        Ok(true)
    } else {
        line(out, "Aborted")?;
        Ok(false)
    }
}

/// Writes `plan` to `otp`, a line for each step as it ends. `failed` makes the
/// error for a failure while writing.
async fn write_plan<O: LocalOtpAccess>(
    plan: &Plan,
    otp: &mut O,
    out: &mut impl Write,
    failed: fn(CommissionError) -> Error,
) -> Result<(), Error> {
    line(out, "Writing OTP - DO NOT DISCONNECT")?;
    let mut shown = Ok(());
    plan.execute(otp, |step| {
        let state = if step.writes.iter().all(|write| write.holds) {
            "already present"
        } else {
            "written"
        };
        if shown.is_ok() {
            shown = line(out, format!("{}: {state}", step_name(step.kind)));
        }
    })
    .await
    // Execute writes as it goes so a failure can leave OTP part written.
    .map_err(failed)?;
    shown
}

/// Asks the user `question`, which a y or n answers. `input` contains their
/// answer. Returns whether it's yes.
fn ask(question: &str, out: &mut impl Write, input: &mut impl BufRead) -> Result<bool, Error> {
    write!(out, "{question} (y/N): ").map_err(|e| Error::io("stdout", e))?;
    out.flush().map_err(|e| Error::io("stdout", e))?;

    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .map_err(|e| Error::io("stdin", e))?;

    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Asks the terminal for the PIN of `key`, such as `signing key #2` or a key
/// file, without echoing it. `prompt` shows the question. `no_terminal` is the
/// error where there isn't a terminal to ask. An empty PIN counts as not
/// entered.
fn read_pin(key: &str, no_terminal: Error, prompt: &mut impl Write) -> Result<String, Error> {
    if !std::io::stdin().is_terminal() {
        return Err(no_terminal);
    }
    write!(prompt, "PIN for {key}: ").map_err(|e| Error::io("stdout", e))?;
    prompt.flush().map_err(|e| Error::io("stdout", e))?;

    terminal::enable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    let pin = read_hidden_line();
    // The terminal is restored whatever happened.
    let restored = terminal::disable_raw_mode();
    let ended = writeln!(prompt).map_err(|e| Error::io("stdout", e));
    restored.map_err(|e| Error::io("terminal", e))?;
    ended?;
    pin?.filter(|pin| !pin.is_empty())
        .ok_or_else(|| Error::Aborted("The PIN wasn't entered".to_string()))
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
// set-size
// ---------------------------------------------------------------------------

pub async fn cmd_set_size(options: &mut Options, args: &HardwareSetSizeArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();
    check_firmware(
        OtpCommand::SetSize,
        device.firmware_board(),
        args.board,
        args.force,
    )?;

    let stopped = reboot_to_stopped(options, &STOP_FROM).await?;
    let result = set_size(options, args).await;
    reboot_after_writing(options, stopped, result).await
}

/// Sets the stopped One ROM's size. Returns whether it wrote to OTP.
async fn set_size(options: &Options, args: &HardwareSetSizeArgs) -> Result<bool, Error> {
    let device = options.device.as_ref().unwrap();
    println!("{device}");
    let mut otp = PicobootOtp::open(device).await?;
    set_size_otp(
        &mut otp,
        args,
        options,
        &mut std::io::stdout(),
        &mut std::io::stdin().lock(),
    )
    .await
}

/// Sets the size of the board `otp` reaches as `args` asks.
///
/// With `--yes` it writes without asking. Otherwise `input` contains the
/// user's answer. `--verbose` lists every row. Returns whether it wrote to
/// OTP.
pub(crate) async fn set_size_otp<O: LocalOtpAccess>(
    otp: &mut O,
    args: &HardwareSetSizeArgs,
    options: &Options,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<bool, Error> {
    let plan = plan_size(otp, args.board, args.size)
        .await
        .map_err(Error::SetSize)?;
    let values: Vec<Labelled> = [
        ("Board type:", args.board.name().to_string()),
        ("Board size:", args.size.to_string()),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_string(), value))
    .collect();
    line(out, "Setting board size:")?;
    for text in labelled_lines(&values) {
        line(out, text)?;
    }

    let rows = print_plan(&plan, options.verbose, out)?;
    if rows == 0 {
        line(
            out,
            format!("Nothing to write - already size {}", args.size),
        )?;
        return Ok(false);
    }
    if args.dry_run {
        line(out, "Dry run - nothing written")?;
        return Ok(false);
    }
    if !agree_to_write(rows, options, out, input)? {
        return Ok(false);
    }
    write_plan(&plan, otp, out, Error::SetSizeFailed).await?;
    line(out, "Board size set")?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// sign
// ---------------------------------------------------------------------------

pub async fn cmd_sign(options: &Options, args: &HardwareSignArgs) -> Result<(), Error> {
    // With --json stdout carries only the result, for a script to read. The
    // values, the questions and the answers go to stderr.
    let mut ui: Box<dyn Write> = if args.json {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::stdout())
    };
    // Asked for before anything slow.
    let signing = key_source(
        (args.signer.as_deref(), args.key_id),
        args.key.as_deref(),
        args.pin.as_deref(),
        &mut ui,
    )?;
    let (table, _) = signer_table().await;
    let signer = signer_for(&table, &signing, &args.manufacturer).await?;
    sign_values(
        args,
        &signing,
        (signer, &table),
        options,
        &mut ui,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    )
    .await
}

/// `hardware sign --json`'s output.
#[derive(Serialize)]
struct Signed<'a> {
    /// CHIPID as the bootloader's USB serial number shows it.
    chip_id: String,
    board: &'a str,
    size: BoardSize,
    manufacturer: &'a str,
    /// The commissioning date as `YYYYMMDD`.
    date: &'a str,
    key_id: u16,
    /// The signature in lowercase hex.
    signature: String,
    /// The `hardware commission` command that writes the signed instance.
    /// Left out after `--dry-run`, whose signature isn't recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
}

/// Signs the commissioning instance `args` describes with `signing`.
///
/// It checks the signature `signing` makes without recording against
/// `signer`'s key in `table`. It then asks the user, and a source that keeps
/// a record records the signature once they agree. `--dry-run` doesn't ask
/// or record.
///
/// `ui` shows the values and the question. `input` contains the user's
/// answer, and with `--yes` it doesn't ask. `out` shows the signature and
/// the `hardware commission` command that writes the instance, as JSON with
/// `--json`.
pub(crate) async fn sign_values<S: SignatureSource>(
    args: &HardwareSignArgs,
    signing: &S,
    (signer, table): (&Signer, &SignerTable),
    options: &Options,
    ui: &mut impl Write,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<(), Error> {
    let size = args
        .board_size()
        .map_err(|e| Error::InvalidArgument("--size".to_string(), e.to_string()))?;
    let date = args.date.clone().unwrap_or_else(today);
    let values = CommissioningValues::new(args.board, &args.manufacturer, &date, signer.id())
        .map_err(values_error)?;
    let message = values.message(args.chip_id);

    let shown: Vec<Labelled> = [
        ("Chip ID:", format_chip_id(args.chip_id)),
        ("Board type:", args.board.name().to_string()),
        ("Board size:", size.to_string()),
        ("Manufacturer:", escape_controls(&args.manufacturer)),
        ("Date:", format_date(&date)),
        ("Signing key:", signer_name(signer.id(), table)),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_string(), value))
    .collect();
    line(ui, "Signing:")?;
    for text in labelled_lines(&shown) {
        line(ui, text)?;
    }

    let to_sign = ToSign {
        chip_id: args.chip_id,
        board: args.board,
        manufacturer: &args.manufacturer,
        date: &date,
        message: &message,
    };
    let signature = signing.dry_run(&to_sign).await?;
    check_signature(signer, &message, &signature)?;
    if !args.dry_run {
        if !agree("Sign?", options, ui, input)? {
            return Ok(());
        }
        // Ed25519 makes one signature from a key and a message, so the
        // recorded signature is the one checked unless the source changed
        // its key.
        if let Some(recorded) = signing.record(&to_sign).await?
            && recorded != signature
        {
            return Err(Error::RecordedSignatureDiffers);
        }
    }

    // A dry run's signature isn't recorded, so it doesn't provide a command to
    // use it.
    let command = (!args.dry_run).then(|| {
        commission_command(
            (args.board, size),
            &args.manufacturer,
            &date,
            signer.id(),
            &signature,
        )
    });
    if args.json {
        let signed = Signed {
            chip_id: format_chip_id(args.chip_id),
            board: args.board.name(),
            size,
            manufacturer: &args.manufacturer,
            date: &date,
            key_id: signer.id(),
            signature: hex::encode(signature),
            command,
        };
        let json =
            serde_json::to_string_pretty(&signed).map_err(|e| Error::Other(e.to_string()))?;
        line(out, json)?;
    } else {
        line(out, format!("Signature: {}", hex::encode(signature)))?;
        match command {
            Some(command) => {
                line(out, "Command to commission the One ROM:")?;
                line(out, format!("  {command}"))?;
            }
            None => line(out, "Dry run - signature not recorded")?,
        }
    }
    Ok(())
}

/// The error for values `CommissioningValues::new` refused with `error`.
fn values_error(error: BuildError) -> Error {
    match error {
        BuildError::DoesNotFit => Error::InvalidArgument(
            "--manufacturer".to_string(),
            "The manufacturer's name is too long to fit in OTP.".to_string(),
        ),
        // clap refuses a manufacturer check_manufacturer() refuses and a bad
        // date, and every key's ID is 1 or more. Only an instance has a first
        // row.
        BuildError::EmptyManufacturer
        | BuildError::BadManufacturer
        | BuildError::BadDate
        | BuildError::BadSigner
        | BuildError::BadFirstRow => Error::Commission(CommissionError::Build(error)),
    }
}

/// The `hardware commission` command that writes the instance of `board` and
/// its size by `manufacturer` on `date`, with `signature` from key `key_id`.
/// Each word a shell would change is quoted.
fn commission_command(
    (board, size): (Board, BoardSize),
    manufacturer: &str,
    date: &str,
    key_id: u16,
    signature: &[u8; 64],
) -> String {
    let (size, key_id, signature) = (size.to_string(), key_id.to_string(), hex::encode(signature));
    [
        "onerom",
        "hardware",
        "commission",
        "--board",
        board.name(),
        "--size",
        &size,
        "--manufacturer",
        manufacturer,
        "--date",
        date,
        "--key-id",
        &key_id,
        "--signature",
        &signature,
    ]
    .iter()
    .map(|word| shell_word(word))
    .collect::<Vec<_>>()
    .join(" ")
}

/// `word` as a shell reads it back. A word containing only characters a shell
/// leaves alone is unchanged. Any other is in single quotes.
pub(crate) fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/:=@%+,".contains(&b));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

// ---------------------------------------------------------------------------
// request-signature
// ---------------------------------------------------------------------------

pub async fn cmd_request_signature(
    options: &mut Options,
    args: &HardwareRequestSignatureArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();
    // request-signature doesn't have --force.
    check_firmware(
        OtpCommand::RequestSignature,
        device.firmware_board(),
        args.board,
        false,
    )?;

    let stopped = reboot_to_stopped(options, &STOP_FROM).await?;
    let result = request_signature(options, args).await;
    restart(options, stopped, result).await
}

/// Checks the stopped One ROM and shows its signing request.
async fn request_signature(
    options: &Options,
    args: &HardwareRequestSignatureArgs,
) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    println!("{device}");
    let mut otp = PicobootOtp::open(device).await?;
    request_signature_otp(&mut otp, args, &mut std::io::stdout()).await
}

/// Checks that `hardware commission` would commission the board `otp` reaches
/// with a community signing request's values. Then shows the request and the
/// link that raises it. It refuses what `hardware commission` refuses before
/// it asks for a signature.
pub(crate) async fn request_signature_otp<O: LocalOtpAccess>(
    otp: &mut O,
    args: &HardwareRequestSignatureArgs,
    out: &mut impl Write,
) -> Result<(), Error> {
    let size = args
        .board_size()
        .map_err(|e| Error::InvalidArgument("--size".to_string(), e.to_string()))?;
    // The signature covers the date it's made on, which is given to
    // hardware commission. A board commissioned with these values on
    // another date is refused, as that run would be.
    let request = Request {
        board: args.board,
        size,
        manufacturer: signing_request::MANUFACTURER.to_string(),
        date: RequestDate::Given(today()),
        signer: signing_request::KEY_ID,
        force: false,
    };
    let prepared = prepare(otp, &request)
        .await
        .map_err(Error::RequestSignature)?;

    let request = SigningRequest {
        chip_id: prepared.chip_id(),
        board: args.board,
        size,
    };
    // The link fills in the same values.
    let labels = ["Chip ID:", "Board type:", "Board size:", "Manufacturer:"];
    let values: Vec<Labelled> = labels
        .into_iter()
        .zip(request.fields())
        .map(|(label, (_, value))| (label.to_string(), value))
        .collect();
    line(out, "Signing request:")?;
    for text in labelled_lines(&values) {
        line(out, text)?;
    }
    line(
        out,
        "Open this link to raise a signing request for this One ROM on GitHub:",
    )?;
    line(out, format!("  {}", request.link()))
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
#[serde(rename_all = "snake_case")]
pub(crate) enum SigningKeys {
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
    valid: bool,
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
    /// The reason the check failed, as the text output's error line shows it.
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
        (args.json, options.verbose),
        &mut std::io::stdout(),
    )
    .await
}

/// Checks the commissioning of the board `otp` reaches against `table`.
/// `fetch` fetches a retired key's record file. `json` shows the result as
/// JSON. Otherwise the board size comes first, beneath the device line and
/// its board type. `verbose` adds the table's source, the row of the
/// instance in use and every other instance.
pub(crate) async fn validate_otp<O: LocalOtpAccess, F: LocalFetch<Error = onerom_fw::Error>>(
    otp: &mut O,
    (table, keys): (&SignerTable, SigningKeys),
    fetch: &F,
    (json, verbose): (bool, bool),
    out: &mut impl Write,
) -> Result<(), Error> {
    let chip_id = read_chip_id(otp).await?;
    let area = read_commissioning(otp).await?;

    let current = area.current().map(CommissioningInstance::first_row);
    let mut checks = Vec::new();
    for instance in area.instances() {
        let verdict = verify_instance(instance, chip_id, table, fetch).await;
        checks.push((instance, verdict));
    }

    let newer = area.issues().iter().find_map(|issue| match *issue {
        AreaIssue::UnknownVersion { row, version } => Some((row, version)),
        AreaIssue::LostPlace { .. } => None,
    });
    let outcome = match checks
        .iter()
        .find(|(instance, _)| Some(instance.first_row()) == current)
    {
        // The parser stops at an unknown version so there isn't a current
        // instance.
        None => Err(match newer {
            Some((row, version)) => format!(
                "The commissioning instance at row {row:#05x} has unknown version {version}.\n  {NEWER_DATA}"
            ),
            None => "OTP doesn't contain a valid commissioning instance.".to_string(),
        }),
        Some((_, Ok(verdict))) if verdict.is_accepted() => Ok(()),
        Some((_, Ok(verdict))) => Err(sentence(verdict_text(*verdict))),
        Some((_, Err(onerom_app::Error::Fetch { .. }))) => {
            Err("The retired key's record couldn't be downloaded.".to_string())
        }
        Some((_, Err(onerom_app::Error::Plugin(_) | onerom_app::Error::Signer(_)))) => {
            Err("The retired key's record couldn't be read.".to_string())
        }
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
            valid: outcome.is_ok(),
        };
        let json =
            serde_json::to_string_pretty(&validation).map_err(|e| Error::Other(e.to_string()))?;
        line(out, json)?;
    } else {
        let size = read_board_size(otp).await?;
        let size = board_size_text(MaybeKnown::Known(size));
        line(out, format!("Board size: {size}"))?;
        if verbose {
            let keys = match keys {
                SigningKeys::BuiltIn => "built-in",
                SigningKeys::Downloaded => "downloaded",
            };
            line(out, format!("Signing key table: {keys}"))?;
        }
        let (in_use, others): (Vec<_>, Vec<_>) = checks
            .iter()
            .partition(|(instance, _)| Some(instance.first_row()) == current);
        match in_use.first() {
            Some((instance, verdict)) => {
                line(out, "Commissioned: yes")?;
                let mut values = checked_values(instance, verdict, table);
                if verbose {
                    let row = format!("{:#05x}", instance.first_row());
                    values.push(("Row:".to_string(), row));
                }
                for text in labelled_lines(&values) {
                    line(out, text)?;
                }
            }
            None => match unknown_version(&area) {
                Some(unknown) => line(out, format!("Commissioned: {unknown}"))?,
                None => line(out, "Commissioned: no")?,
            },
        }
        if verbose && !others.is_empty() {
            let heading = if in_use.is_empty() {
                "Commissioning instances:"
            } else {
                "Other commissioning instances:"
            };
            line(out, heading)?;
            for (instance, verdict) in others {
                let state = instance_state(instance, false);
                let row = instance.first_row();
                line(out, format!("  Instance at row {row:#05x} ({state})"))?;
                for text in labelled_lines(&checked_values(instance, verdict, table)) {
                    line(out, format!("  {text}"))?;
                }
            }
        }
        // The Commissioned line shows an unknown version.
        for issue in area.issues() {
            if let AreaIssue::LostPlace { .. } = issue {
                line(out, format!("Commissioning area: {}", issue_text(issue)))?;
            }
        }
        if outcome.is_ok() {
            line(out, "Commissioning information valid")?;
        }
    }
    outcome.map_err(Error::NotValidated)
}

/// `instance`'s check as JSON.
fn instance_check<'a>(
    instance: &'a CommissioningInstance,
    verdict: &Result<Verdict, AppError>,
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
        error: verdict.as_ref().err().map(error_line),
        unknown_keys: unknown_keys(instance.entries())
            .map(|unknown| UnknownKeyJson {
                row: unknown.row,
                key: unknown.key,
                len: unknown.len,
            })
            .collect(),
    }
}

/// `instance`'s values, the check of its signature and the keys skipped in
/// it, a line each.
fn checked_values(
    instance: &CommissioningInstance,
    verdict: &Result<Verdict, AppError>,
    table: &SignerTable,
) -> Vec<Labelled> {
    let mut values = instance_values(instance);
    values.extend(check_values(instance, verdict, table));
    values.extend(unknown_keys(instance.entries()).map(|unknown| {
        (
            "Skipped:".to_string(),
            format!("unknown {}", unknown.text()),
        )
    }));
    values
}

/// The check of `instance`'s signature, a line each. An incomplete instance
/// or one missing values doesn't have one, and its state says so.
fn check_values(
    instance: &CommissioningInstance,
    verdict: &Result<Verdict, AppError>,
    table: &SignerTable,
) -> Vec<Labelled> {
    let signer = || {
        let name = match instance.signer() {
            Some(id) => signer_name(id, table),
            None => "unknown".to_string(),
        };
        ("Signing key:".to_string(), name)
    };
    let signature = |text: &str| ("Signature:".to_string(), text.to_string());
    match verdict {
        Ok(Verdict::Incomplete | Verdict::Invalid) => Vec::new(),
        Ok(verdict) => vec![signer(), signature(verdict_text(*verdict))],
        Err(e) => vec![
            signer(),
            signature("not checked"),
            ("Error:".to_string(), error_line(e)),
        ],
    }
}

/// `verdict` as a phrase about an instance's signature.
fn verdict_text(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Verified => "verified",
        Verdict::Recorded => "verified and recorded before the key was retired",
        Verdict::NotRecorded | Verdict::RecordChanged => "invalid due to retired key",
        Verdict::ManufacturerNotAllowed => {
            "invalid because the signing key cannot sign this manufacturer"
        }
        Verdict::BadSignature => "invalid",
        Verdict::UnknownSigner => "invalid due to unknown signing key",
        Verdict::Incomplete => "incomplete",
        Verdict::Invalid => "missing values",
    }
}

/// `phrase` as a sentence, with a capital letter and a full stop.
fn sentence(phrase: &str) -> String {
    let mut chars = phrase.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// Why a retired key's record couldn't be used, on one line.
fn error_line(e: &AppError) -> String {
    match e {
        onerom_app::Error::Fetch {
            source,
            error: onerom_fw::Error::Http { status, .. },
        } => format!("couldn't download {source} (HTTP {status})"),
        onerom_app::Error::Fetch { source, error } => {
            format!("couldn't download {source}: {}", fetch_reason(error))
        }
        onerom_app::Error::Plugin(_) | onerom_app::Error::Signer(_) => one_line(&e.to_string()),
    }
}

/// Why a fetch failed, on one line. For a network error it's the root cause.
/// reqwest's own text only says the request failed and repeats the URL.
fn fetch_reason(error: &onerom_fw::Error) -> String {
    let text = if let onerom_fw::Error::Network { error, .. } = error {
        let mut cause: &dyn std::error::Error = error;
        while let Some(source) = cause.source() {
            cause = source;
        }
        cause.to_string()
    } else {
        error.to_string()
    };
    one_line(&text)
}

/// `text` with its lines joined by spaces.
fn one_line(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ")
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
            eprintln!("{}", built_in_warning(&e));
            (SignerTable::built_in(), SigningKeys::BuiltIn)
        }
    }
}

/// The warning that the built-in table of signing keys is used because
/// downloading the latest failed with `e`. A `Signer` error means the latest
/// was downloaded and refused.
pub(crate) fn built_in_warning(e: &AppError) -> String {
    let not_downloaded =
        "Warning: using the built-in signing keys because the latest couldn't be downloaded.";
    let (first, reason) = match e {
        onerom_app::Error::Fetch {
            source,
            error: onerom_fw::Error::Network { .. },
        } => (not_downloaded, format!("Couldn't reach {source}.")),
        onerom_app::Error::Fetch {
            source,
            error: onerom_fw::Error::Http { status, .. },
        } => (not_downloaded, format!("{source} returned HTTP {status}.")),
        onerom_app::Error::Fetch { .. } | onerom_app::Error::Plugin(_) => {
            (not_downloaded, one_line(&e.to_string()))
        }
        onerom_app::Error::Signer(_) => (
            "Warning: using the built-in signing keys because the latest is invalid.",
            sentence(&one_line(&e.to_string())),
        ),
    };
    format!("{first}\n  {reason}")
}

/// A command writing or checking OTP. Each has its own text for firmware for
/// another board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OtpCommand {
    Commission,
    SetSize,
    /// `hardware request-signature`, which doesn't have `--force`.
    RequestSignature,
}

/// Refuses firmware for a board other than `board` unless `force`, which warns
/// instead. `firmware` is the board the One ROM's firmware is for.
pub(crate) fn check_firmware(
    command: OtpCommand,
    firmware: Option<Board>,
    board: Board,
    force: bool,
) -> Result<(), Error> {
    let Some(firmware) = firmware.filter(|&firmware| firmware != board) else {
        return Ok(());
    };
    if force {
        eprintln!("{}", firmware_warning(command, firmware, board));
        return Ok(());
    }
    let (firmware, board) = (firmware.name().to_string(), board.name().to_string());
    Err(match command {
        OtpCommand::Commission => Error::FirmwareForAnotherBoard { firmware, board },
        OtpCommand::SetSize => Error::SetSizeFirmwareForAnotherBoard { firmware, board },
        OtpCommand::RequestSignature => {
            Error::RequestSignatureFirmwareForAnotherBoard { firmware, board }
        }
    })
}

/// The warning that `--force` let `command` go ahead with firmware for
/// `firmware` on a One ROM given as `board`.
pub(crate) fn firmware_warning(command: OtpCommand, firmware: Board, board: Board) -> String {
    let given = match command {
        OtpCommand::Commission => "the commissioned board type",
        // request-signature is never forced.
        OtpCommand::SetSize | OtpCommand::RequestSignature => "board type",
    };
    format!(
        "Warning: firmware board type '{}' does not match {given} '{}' (continuing due to --force)",
        firmware.name(),
        board.name()
    )
}

/// Reboots the One ROM after a command that can write OTP and returns the
/// command's `result`. `stopped` is whether this command stopped the One ROM.
/// `result` is `Ok(true)` where the command wrote.
///
/// The bootloader USB strings, the page locks and FLASH_DEVINFO_ENABLE apply
/// from the next reboot. A One ROM this command stopped is rebooted into
/// running mode whatever happened. One that was already stopped is rebooted
/// into the bootloader again after a run that wrote.
async fn reboot_after_writing(
    options: &Options,
    stopped: bool,
    result: Result<bool, Error>,
) -> Result<(), Error> {
    match result {
        Ok(true) if !stopped => reboot_stopped(options).await,
        result => restart(options, stopped, result.map(|_| ())).await,
    }
}

/// Writes `text` and a newline to `out`.
fn line(out: &mut impl Write, text: impl Display) -> Result<(), Error> {
    writeln!(out, "{text}").map_err(|e| Error::io("stdout", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser;
    use ed25519_dalek::Signer as _;

    use crate::args::CommandTrait;
    use onerom_app::{Interruption, MemoryOtp};
    use onerom_metadata::otp::{CommissioningValues, RecordLine};
    use serde_json::json;
    use sha2::{Digest, Sha256};

    use crate::test_board::{
        self, CHIP_ID, Files, SIGNER_NAME, blank_board, holds, in_turn, key_file, shows, table,
    };

    /// The URL of the retired key's record in [`retired_table`].
    const RECORD_URL: &str = "https://example.invalid/signatures/1.txt";

    /// The parts a first run on a fire-24-f writes, in order, with each
    /// part's rows.
    const FIRST_RUN: [(StepKind, usize); 6] = [
        (StepKind::Instance, 60),
        (StepKind::Lock { page: 3 }, 1),
        (StepKind::WhiteLabelStrings, 41),
        (StepKind::WhiteLabelTable, 9),
        (StepKind::WhiteLabelAddr, 1),
        (StepKind::UsbBootFlags, 3),
    ];

    /// The rows a first run on a fire-24-f writes.
    fn first_run_rows() -> usize {
        FIRST_RUN.iter().map(|(_, rows)| rows).sum()
    }

    /// The part a line of a list of parts shows and the part's row count.
    /// `None` for any other line.
    fn part(line: &str) -> Option<(StepKind, usize)> {
        let kind = [
            StepKind::FlashDevinfo,
            StepKind::Instance,
            StepKind::Lock { page: 3 },
            StepKind::WhiteLabelStrings,
            StepKind::WhiteLabelTable,
            StepKind::WhiteLabelAddr,
            StepKind::UsbBootFlags,
            StepKind::BootFlags0,
        ]
        .into_iter()
        .find(|kind| holds(line, &[&step_name(*kind)]))?;
        // The count is the decimal number on the line besides the part's name.
        line.replacen(&step_name(kind), "", 1)
            .split(|c: char| !c.is_alphanumeric())
            .find_map(|word| word.parse().ok())
            .map(|rows| (kind, rows))
    }

    /// The lists of parts `out` shows. A list is a run of lines that each
    /// show a part and its row count.
    fn part_lists(out: &str) -> Vec<Vec<(StepKind, usize)>> {
        let parts: Vec<Option<(StepKind, usize)>> = out.lines().map(part).collect();
        parts
            .split(Option::is_none)
            .filter(|list| !list.is_empty())
            .map(|list| list.iter().flatten().copied().collect())
            .collect()
    }

    /// Whether `out` lists `rows` beneath the name of `kind`'s part, a line
    /// each. Each row is its number and its value.
    fn lists_rows(out: &str, kind: StepKind, rows: &[&[&str]]) -> bool {
        let name = step_name(kind);
        let name = [name.as_str()];
        let run: Vec<&[&str]> = [&name[..]]
            .into_iter()
            .chain(rows.iter().copied())
            .collect();
        in_turn(&out.lines().collect::<Vec<_>>(), &run)
    }

    /// Whether `out` shows the values of a request [`test_board::args`] makes
    /// for an M fire-24-f, a line each.
    fn shows_request(out: &str) -> bool {
        let lines: Vec<&str> = out.lines().collect();
        in_turn(
            &lines,
            &[&["fire-24-f"], &["M"], &["piers.rocks"], &["2026-01-01"]],
        )
    }

    /// Options with `--yes` and `--verbose` as given.
    fn options(yes: bool, verbose: bool) -> Options {
        Options {
            verbose,
            log_level: onerom_cli::LogLevel::Warn,
            yes,
            unrecognised: false,
            device: None,
            vid_pid: Vec::new(),
        }
    }

    /// [`commission_with`] with `--yes` and `--verbose`.
    async fn commission(
        otp: &mut MemoryOtp,
        args: &HardwareCommissionArgs,
    ) -> Result<String, (Error, String)> {
        commission_with(otp, args, &options(true, true)).await
    }

    /// Commissions `otp` as `args` and `options` ask with the test key. Returns
    /// the output.
    async fn commission_with(
        otp: &mut MemoryOtp,
        args: &HardwareCommissionArgs,
        options: &Options,
    ) -> Result<String, (Error, String)> {
        let table = table(None);
        let signing = Signing::File(KeyFile::read(args.key.as_ref().unwrap(), None).unwrap());
        let Signing::File(key) = &signing else {
            unreachable!()
        };
        let signer = signing_key(&table, &key.public_key()).unwrap();
        let mut out = Vec::new();
        let result = commission_otp(
            otp,
            args,
            &signing,
            (signer, &table),
            options,
            &mut out,
            &mut std::io::empty(),
        )
        .await;
        let out = String::from_utf8(out).unwrap();
        match result {
            Ok(_) => Ok(out),
            Err(e) => Err((e, out)),
        }
    }

    /// A signing server holding the test key. It lists the requests it gets.
    #[derive(Default)]
    struct Server {
        requests: std::cell::RefCell<Vec<&'static str>>,
        /// Records a signature other than the dry run's.
        changes_signature: bool,
        /// Signs the dry run with a key other than the test key.
        other_key: bool,
    }

    impl SignatureSource for Server {
        async fn dry_run(&self, to_sign: &ToSign<'_>) -> Result<[u8; 64], Error> {
            self.requests.borrow_mut().push("dry run");
            let key = if self.other_key {
                ed25519_dalek::SigningKey::from_bytes(&[2; 32])
            } else {
                test_board::key()
            };
            Ok(key.sign(to_sign.message).to_bytes())
        }

        async fn record(&self, to_sign: &ToSign<'_>) -> Result<Option<[u8; 64]>, Error> {
            self.requests.borrow_mut().push("record");
            let message = if self.changes_signature {
                b"another message".as_slice()
            } else {
                to_sign.message
            };
            Ok(Some(test_board::key().sign(message).to_bytes()))
        }
    }

    /// Commissions `otp` as `args` asks with `server` signing. `answer` is the
    /// user's answer, where there's a question. Returns the result, the
    /// output and whether the user was asked.
    async fn commission_with_server(
        otp: &mut MemoryOtp,
        args: &HardwareCommissionArgs,
        server: &Server,
        answer: Option<&str>,
    ) -> (Result<bool, Error>, String, bool) {
        let table = table(None);
        let signer = table.get(1).unwrap();
        let typed = answer.unwrap_or_default().as_bytes();
        let mut input = typed;
        let mut out = Vec::new();
        let result = commission_otp(
            otp,
            args,
            server,
            (signer, &table),
            &options(answer.is_none(), true),
            &mut out,
            &mut input,
        )
        .await;
        // The answer is read only where the user is asked.
        let asked = input.len() < typed.len();
        (result, String::from_utf8(out).unwrap(), asked)
    }

    /// The server records a signature only once the user agrees to the rows
    /// it's shown.
    #[tokio::test]
    async fn a_signature_is_recorded_only_once_the_user_agrees() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);

        let mut otp = blank_board();
        let server = Server::default();
        let (result, out, asked) =
            commission_with_server(&mut otp, &args, &server, Some("n\n")).await;
        // Whether the run wrote, which decides whether a One ROM that was
        // already stopped is rebooted.
        assert!(!result.unwrap(), "{out}");
        assert!(asked, "{out}");
        assert_eq!(*server.requests.borrow(), ["dry run"]);
        assert_eq!(otp.write_count(), 0);

        let server = Server::default();
        let (result, out, asked) =
            commission_with_server(&mut otp, &args, &server, Some("y\n")).await;
        assert!(result.unwrap(), "{out}");
        assert!(asked, "{out}");
        assert_eq!(*server.requests.borrow(), ["dry run", "record"]);
        assert_eq!(otp.write_count(), first_run_rows());
    }

    #[tokio::test]
    async fn a_dry_run_shows_every_row_and_doesnt_record_ask_or_write() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        args.dry_run = true;
        let mut otp = blank_board();
        let server = Server::default();
        let (result, out, asked) =
            commission_with_server(&mut otp, &args, &server, Some("y\n")).await;
        assert!(!result.unwrap(), "{out}");
        println!("{out}");

        // The signature's first value row follows its key and length rows.
        let values =
            CommissioningValues::new(args.board, "piers.rocks", test_board::DATE, 1).unwrap();
        let signature = test_board::key().sign(&values.message(CHIP_ID)).to_bytes();
        let first = u16::from_le_bytes([signature[0], signature[1]]);
        assert!(
            shows(out.lines(), &["0x0dc", &format!("{first:#06x}")]),
            "{out}"
        );
        // The total of the rows to write.
        assert!(
            shows(out.lines(), &[&first_run_rows().to_string()]),
            "{out}"
        );
        assert!(!asked, "{out}");
        assert_eq!(*server.requests.borrow(), ["dry run"]);
        assert_eq!(otp.write_count(), 0);
    }

    #[tokio::test]
    async fn a_recorded_signature_other_than_the_one_shown_is_refused() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        let server = Server {
            changes_signature: true,
            ..Server::default()
        };
        let (result, ..) = commission_with_server(&mut otp, &args, &server, None).await;
        let error = result.unwrap_err();
        assert!(matches!(error, Error::RecordedSignatureDiffers), "{error}");
        assert_eq!(otp.write_count(), 0);
    }

    /// A board commissioned as the run asks already holds every row, so
    /// nothing is written and the server isn't asked to record the signature.
    #[tokio::test]
    async fn nothing_is_recorded_for_a_board_commissioned_as_asked() {
        let mut otp = commissioned_board().await;
        let writes = otp.write_count();
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let server = Server::default();
        let (result, out, _) = commission_with_server(&mut otp, &args, &server, None).await;
        assert!(!result.unwrap(), "{out}");
        assert_eq!(otp.write_count(), writes, "{out}");
        assert_eq!(*server.requests.borrow(), ["dry run"]);
    }

    /// Validates `otp` against `table`. Returns the output.
    async fn validate(
        otp: &mut MemoryOtp,
        table: &SignerTable,
        files: &Files,
        json: bool,
    ) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let result = validate_otp(
            otp,
            (table, SigningKeys::Downloaded),
            files,
            (json, false),
            &mut out,
        )
        .await;
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
        // The signing key and the request.
        assert!(shows(out.lines(), &[SIGNER_NAME, "1"]), "{out}");
        assert!(shows_request(&out), "{out}");
        // The instance's first rows in row order, the board's key before its
        // length, and its page's lock word.
        let instance = [
            ["0x0c0", "0x524f"].as_slice(),
            &["0x0c1", "0x0001"],
            &["0x0c2", "0x0001"],
            &["0x0c3", "0x0009"],
        ];
        assert!(lists_rows(&out, StepKind::Instance, &instance), "{out}");
        let lock = [["0xf87", "0x151515"].as_slice()];
        assert!(lists_rows(&out, StepKind::Lock { page: 3 }, &lock), "{out}");
        // Every row is written, the lock word and USB_BOOT_FLAGS among them.
        assert_eq!(otp.write_count(), first_run_rows());
        assert_eq!(otp.rows()[0xf87], 0x151515);
        assert_eq!(otp.rows()[0x059..=0x05b], [0x40f133; 3]);
        // A run on an M board doesn't write FLASH_DEVINFO.
        assert!(!out.contains(&step_name(StepKind::FlashDevinfo)), "{out}");

        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        println!("{out}");
        result.unwrap();
        // The board size first, then the instance's values and its signer a
        // line each. Its row is --verbose information.
        let lines: Vec<&str> = out.lines().collect();
        assert!(holds(lines[0], &["M"]), "{out}");
        let instance: [&[&str]; 4] = [
            &["fire-24-f"],
            &["piers.rocks"],
            &["2026-01-01"],
            &[SIGNER_NAME, "1"],
        ];
        assert!(in_turn(&lines, &instance), "{out}");
        assert!(!shows(&lines, &["0x0c0"]), "{out}");
    }

    /// --verbose adds the row of the instance in use and each other instance
    /// with its row and state.
    #[tokio::test]
    async fn verbose_validate_shows_every_instance() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();
        args.date = Some("20260102".to_string());
        args.force = true;
        commission(&mut otp, &args).await.unwrap();

        let mut out = Vec::new();
        let files = Files(Vec::new());
        let keys = (&table(None), SigningKeys::Downloaded);
        validate_otp(&mut otp, keys, &files, (false, true), &mut out)
            .await
            .unwrap();
        let out = String::from_utf8(out).unwrap();
        println!("{out}");
        let lines: Vec<&str> = out.lines().collect();
        // The instance in use shows its row. The replaced one follows with its
        // row, its state and its own date.
        assert!(shows(&lines, &["0x100"]), "{out}");
        let earlier = lines
            .iter()
            .position(|line| holds(line, &["0x0c0", "replaced"]))
            .unwrap_or_else(|| panic!("{out}"));
        assert!(shows(&lines[earlier..], &["2026-01-01"]), "{out}");
        assert!(!shows(&lines[..earlier], &["2026-01-01"]), "{out}");
    }

    /// The device line each check after commissioning shows.
    const DEVICE: &str = "One ROM Fire 24 F";

    /// Runs `--validate` and `--inspect-otp` on `otp` where `steps` sets each,
    /// with the test table. Returns the result and the output.
    async fn after_commissioning(
        otp: &mut MemoryOtp,
        (validate, inspect): (bool, bool),
    ) -> (Result<(), Error>, String) {
        let table = table(None);
        let files = Files(Vec::new());
        let mut out = Vec::new();
        let result = check_commissioned_otp(
            otp,
            &DEVICE,
            validate.then_some(((&table, SigningKeys::Downloaded), &files)),
            inspect.then_some(&table),
            false,
            &mut out,
        )
        .await;
        (result, String::from_utf8(out).unwrap())
    }

    /// What `inspect otp` prints for `otp` beneath [`DEVICE`], with the test
    /// table.
    async fn inspected(otp: &mut MemoryOtp) -> String {
        let mut out = Vec::new();
        write_otp(otp, &DEVICE, &table(None), false, &mut out)
            .await
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    /// Each check after commissioning prints a blank line, then what its
    /// command prints. Validation comes first.
    #[tokio::test]
    async fn the_checks_after_commissioning_validate_then_inspect_otp() {
        let mut otp = commissioned_board().await;
        let (result, validated) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        result.unwrap();
        let inspected = inspected(&mut otp).await;
        for (steps, expected) in [
            ((true, false), format!("\n{DEVICE}\n{validated}")),
            ((false, true), format!("\n{inspected}")),
            (
                (true, true),
                format!("\n{DEVICE}\n{validated}\n{inspected}"),
            ),
        ] {
            let (result, out) = after_commissioning(&mut otp, steps).await;
            result.unwrap();
            assert_eq!(out, expected, "{steps:?}");
        }
    }

    /// A failed validation ends the checks before `--inspect-otp`, with
    /// validation's error.
    #[tokio::test]
    async fn a_failed_validation_ends_the_checks_after_commissioning() {
        let mut otp = blank_board();
        let (refused, validated) =
            validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        let (result, out) = after_commissioning(&mut otp, (true, true)).await;
        assert_eq!(
            result.unwrap_err().to_string(),
            refused.unwrap_err().to_string()
        );
        assert_eq!(out, format!("\n{DEVICE}\n{validated}"));
    }

    /// Without --verbose a run shows a line for each part of OTP rather than
    /// each row.
    #[tokio::test]
    async fn a_run_shows_a_line_for_each_part() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        let out = commission_with(&mut otp, &args, &options(true, false))
            .await
            .unwrap();
        println!("{out}");
        // A first run lists only the parts to write.
        assert_eq!(part_lists(&out), [FIRST_RUN], "{out}");
        // The instance's line holds its first row. Its second row isn't shown.
        let instance = step_name(StepKind::Instance);
        assert!(shows(out.lines(), &[&instance, "0x0c0", "60"]), "{out}");
        assert!(!shows(out.lines(), &["0x0c1"]), "{out}");
    }

    /// --verbose shows the bootloader USB info a run writes, a line each in
    /// turn. A run that doesn't write them doesn't show them.
    #[tokio::test]
    async fn verbose_shows_the_strings_a_run_writes() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let strings = white_label_strings(args.board);
        let values: Vec<[&str; 1]> = strings.iter().map(|(_, value)| [value.as_str()]).collect();
        let values: Vec<&[&str]> = values.iter().map(|value| value.as_slice()).collect();
        let shows_strings = |out: &str| in_turn(&out.lines().collect::<Vec<_>>(), &values);

        let out = commission_with(&mut blank_board(), &args, &options(true, false))
            .await
            .unwrap();
        assert!(!shows_strings(&out), "{out}");

        let mut otp = blank_board();
        let out = commission_with(&mut otp, &args, &options(true, true))
            .await
            .unwrap();
        assert!(shows_strings(&out), "{out}");

        // A second run finds them present.
        let out = commission_with(&mut otp, &args, &options(true, true))
            .await
            .unwrap();
        assert!(!shows_strings(&out), "{out}");
    }

    /// Every board's bootloader USB strings can be shown, with its name as the
    /// INFO_UF2.TXT board ID.
    #[test]
    fn every_boards_strings_can_be_shown() {
        for board in onerom_config::hw::BOARDS {
            let strings = white_label_strings(board);
            let board_id = strings.last().map(|(_, value)| value.as_str());
            assert_eq!(board_id, Some(board.name()), "{board}");
        }
    }

    /// A run after one that stopped part way lists the rows already present
    /// and the rows it writes.
    #[tokio::test]
    async fn a_run_after_one_that_stopped_lists_both() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        // The instance, its page's lock and 8 of the strings' 41 rows land.
        otp.interrupt(70, Interruption::NotLanded);
        let (error, _) = commission_with(&mut otp, &args, &options(true, false))
            .await
            .unwrap_err();
        // Running it again completes it.
        assert!(
            matches!(
                error,
                Error::CommissionFailed(CommissionError::Otp {
                    error: onerom_app::OtpError::Transport(_),
                    ..
                })
            ),
            "{error}"
        );

        let out = commission_with(&mut otp, &args, &options(true, false))
            .await
            .unwrap();
        // The rows already present, then the rows to write.
        assert_eq!(
            part_lists(&out),
            [
                vec![
                    (StepKind::Instance, 60),
                    (StepKind::Lock { page: 3 }, 1),
                    (StepKind::WhiteLabelStrings, 8),
                ],
                vec![
                    (StepKind::WhiteLabelStrings, 33),
                    (StepKind::WhiteLabelTable, 9),
                    (StepKind::WhiteLabelAddr, 1),
                    (StepKind::UsbBootFlags, 3),
                ],
            ],
            "{out}"
        );
        // Each list's total.
        assert!(shows(out.lines(), &["69"]), "{out}");
        assert!(shows(out.lines(), &["46"]), "{out}");
    }

    /// A row that doesn't contain its value once written fails the run.
    #[tokio::test]
    async fn a_row_failing_to_verify_fails_the_run() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        otp.corrupt(5, 0x000100);
        let (error, _) = commission(&mut otp, &args).await.unwrap_err();
        assert!(
            matches!(
                error,
                Error::CommissionFailed(CommissionError::ReadBack { .. })
            ),
            "{error}"
        );
    }

    /// A commissioning area holding an unknown version doesn't validate. The
    /// reason holds the version and its row. One line of the output shows the
    /// version's row.
    #[tokio::test]
    async fn an_area_from_a_newer_version_doesnt_validate() {
        use onerom_metadata::OTP_STORE_MAGIC;
        use onerom_metadata::otp::pico_otp::ecc_encode;
        let mut otp = blank_board();
        otp.set_raw(0x0c0, ecc_encode(OTP_STORE_MAGIC));
        otp.set_raw(0x0c1, ecc_encode(2));
        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        let error = result.unwrap_err();
        let values = ["2", "0x0c0"];
        assert!(
            matches!(&error, Error::NotValidated(reason) if holds(reason, &values)),
            "{error}"
        );
        let lines = out.lines().filter(|line| holds(line, &["0x0c0"]));
        assert_eq!(lines.count(), 1, "{out}");

        let (_, out) = validate(&mut otp, &table(None), &Files(Vec::new()), true).await;
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        let issue = json!({ "issue": "unknown_version", "row": 0x0c0, "version": 2 });
        assert_eq!(json["issues"], json!([issue]));
        assert_eq!(json["valid"], false);
    }

    #[tokio::test]
    async fn an_l_board_gets_its_second_flash_chip() {
        let (_dir, path) = key_file();
        let mut otp = blank_board();
        let out = commission(&mut otp, &test_board::args("fire-40-a", BoardSize::L, path))
            .await
            .unwrap();
        println!("{out}");
        // The rows of FLASH_DEVINFO and of FLASH_DEVINFO_ENABLE in BOOT_FLAGS0.
        let devinfo = [["0x054", "0x99af"].as_slice()];
        assert!(lists_rows(&out, StepKind::FlashDevinfo, &devinfo), "{out}");
        let boot_flags0 = [["0x048", "0x000020"].as_slice()];
        assert!(
            lists_rows(&out, StepKind::BootFlags0, &boot_flags0),
            "{out}"
        );
        // Both are written.
        assert_eq!(otp.rows()[0x054], 0x3a99af);
        assert_eq!(otp.rows()[0x048..=0x04a], [0x000020; 3]);
    }

    #[tokio::test]
    async fn a_second_run_writes_nothing() {
        let (_dir, path) = key_file();
        let args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        let first = commission(&mut otp, &args).await.unwrap();
        let writes = otp.write_count();

        let out = commission(&mut otp, &args).await.unwrap();
        assert_eq!(otp.write_count(), writes);
        // One list holding every part with all its rows.
        assert_eq!(part_lists(&out), [FIRST_RUN], "{out}");
        // A row's line marks it as present, so it differs from the first run's.
        let row = |out: &str| {
            out.lines()
                .find(|line| holds(line, &["0x0c0", "0x524f"]))
                .map(str::to_string)
        };
        assert_ne!(row(&out), row(&first), "{out}");
    }

    /// A size that doesn't suit the board is refused before anything is
    /// written, even where the CLI's check after parsing hasn't run.
    #[tokio::test]
    async fn a_size_that_doesnt_suit_the_board_is_refused() {
        let (_dir, path) = key_file();
        for (board, size) in [("fire-40-a", None), ("fire-24-f", Some(BoardSize::L))] {
            let mut args = test_board::args(board, BoardSize::M, path.clone());
            args.size = size;
            let mut otp = blank_board();
            let (error, out) = commission(&mut otp, &args).await.unwrap_err();
            assert!(matches!(error, Error::InvalidArgument(..)), "{error}");
            assert_eq!(out, "", "{board}");
            assert_eq!(otp.write_count(), 0, "{board}");
        }
    }

    /// A run without --date finishes an instance that differs only in its date.
    #[tokio::test]
    async fn a_run_without_a_date_takes_the_current_instances_date() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();
        let dated = commission(&mut otp, &args).await.unwrap();

        args.date = None;
        let out = commission(&mut otp, &args).await.unwrap();
        // The request's values hold the current instance's date.
        assert!(shows_request(&out), "{out}");
        // One line differs from a run given that date. It holds the date and
        // says where the date came from.
        let differs: Vec<&str> = out
            .lines()
            .filter(|line| !dated.lines().any(|dated| dated == *line))
            .collect();
        assert_eq!(differs.len(), 1, "{out}");
        assert!(holds(differs[0], &["2026-01-01"]), "{out}");
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
        // The refusal shows the instance in use with its signing key by
        // name, a line each.
        assert!(
            matches!(&error, Error::AlreadyCommissioned { .. }),
            "{error}"
        );
        let text = error.to_string();
        let lines: Vec<&str> = text.lines().collect();
        let instance: [&[&str]; 4] = [
            &["fire-24-f"],
            &["piers.rocks"],
            &["2026-01-01"],
            &[SIGNER_NAME, "1"],
        ];
        assert!(in_turn(&lines, &instance), "{text}");
        assert_eq!(otp.rows(), rows);
    }

    /// --force with another date re-commissions. The instance it replaces
    /// shows before the new values, and the new instance goes on the next
    /// page.
    #[tokio::test]
    async fn force_shows_the_instance_it_replaces() {
        let (_dir, path) = key_file();
        let mut args = test_board::args("fire-24-f", BoardSize::M, path);
        let mut otp = blank_board();
        commission(&mut otp, &args).await.unwrap();
        args.date = Some("20260102".to_string());
        args.force = true;
        let out = commission(&mut otp, &args).await.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let date = |date| lines.iter().position(|line| holds(line, &[date]));
        let (replaced, new) = (date("2026-01-01"), date("2026-01-02"));
        assert!(replaced.is_some() && replaced < new, "{out}");
        let instance = step_name(StepKind::Instance);
        assert!(shows(&lines, &[&instance, "0x100"]), "{out}");
    }

    /// Options for a run that didn't find a device.
    fn no_device() -> Options {
        options(false, false)
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
                "valid": true,
            })
        );
    }

    #[test]
    fn json_shows_the_source_of_the_signing_keys() {
        let value = |keys| serde_json::to_value(keys).unwrap();
        assert_eq!(value(SigningKeys::BuiltIn), json!("built_in"));
        assert_eq!(value(SigningKeys::Downloaded), json!("downloaded"));
    }

    /// A refused table's warning has another first line from a failed
    /// download's. Its reason identifies the signer and the field.
    #[test]
    fn a_refused_table_is_told_apart_from_a_failed_download() {
        let url = "https://images.onerom.org/signers.json";
        let failed = AppError::fetch(
            url,
            onerom_fw::Error::Http {
                url: url.to_string(),
                status: 404,
            },
        );
        let refused = AppError::Signer(onerom_app::SignerError::BadField {
            id: 300,
            field: "manufacturers",
        });
        let failed = built_in_warning(&failed);
        let refused = built_in_warning(&refused);
        assert_ne!(failed.lines().next(), refused.lines().next());
        let reason = refused.lines().nth(1).unwrap();
        assert!(holds(reason, &["300", "manufacturers"]), "{refused}");
    }

    /// The verdict `--json` shows on `otp`'s first instance checked against
    /// `table`.
    async fn verdict(otp: &mut MemoryOtp, table: &SignerTable, files: &Files) -> serde_json::Value {
        let (_, out) = validate(otp, table, files, true).await;
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        json["instances"][0]["verdict"].clone()
    }

    #[tokio::test]
    async fn a_board_without_commissioning_doesnt_validate() {
        let mut otp = blank_board();
        let (result, _) = validate(&mut otp, &table(None), &Files(Vec::new()), false).await;
        let error = result.unwrap_err();
        assert!(matches!(error, Error::NotValidated(_)), "{error}");

        let (result, out) = validate(&mut otp, &table(None), &Files(Vec::new()), true).await;
        assert!(matches!(result, Err(Error::NotValidated(_))));
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["instances"], json!([]));
        assert_eq!(json["valid"], false);
    }

    #[tokio::test]
    async fn an_unknown_signer_doesnt_validate() {
        let mut otp = commissioned_board().await;
        let empty = SignerTable::parse(br#"{"version": 1, "signers": []}"#).unwrap();
        let (result, out) = validate(&mut otp, &empty, &Files(Vec::new()), false).await;
        // The signer is shown by its ID.
        assert!(shows(out.lines(), &["1"]), "{out}");
        // The reason is the verdict's text.
        let error = result.unwrap_err();
        let expected = sentence(verdict_text(Verdict::UnknownSigner));
        assert!(
            matches!(&error, Error::NotValidated(reason) if *reason == expected),
            "{error}"
        );
        let verdict = verdict(&mut otp, &empty, &Files(Vec::new())).await;
        assert_eq!(verdict, "unknown_signer");
    }

    /// The board is commissioned by piers.rocks and signed by key 1.
    #[tokio::test]
    async fn a_manufacturer_the_key_doesnt_allow_doesnt_validate() {
        let mut otp = commissioned_board().await;
        let files = Files(Vec::new());
        let refusing = test_board::table_allowing(&["onerom.org"]);
        let (result, out) = validate(&mut otp, &refusing, &files, false).await;
        // The reason is the verdict's text.
        let error = result.unwrap_err();
        let expected = sentence(verdict_text(Verdict::ManufacturerNotAllowed));
        assert!(
            matches!(&error, Error::NotValidated(reason) if *reason == expected),
            "{error}\n{out}"
        );

        let (result, out) = validate(&mut otp, &refusing, &files, true).await;
        assert!(matches!(result, Err(Error::NotValidated(_))));
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["instances"][0]["verdict"], "manufacturer_not_allowed");
        assert_eq!(json["instances"][0]["error"], serde_json::Value::Null);
        assert_eq!(json["valid"], false);

        let allowing = test_board::table_allowing(&["piers.rocks"]);
        let (result, out) = validate(&mut otp, &allowing, &files, false).await;
        result.unwrap_or_else(|e| panic!("{e}\n{out}"));
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
        format!("{}\n", RecordLine::new(signature))
    }

    #[tokio::test]
    async fn a_retired_keys_recorded_signature_validates() {
        let mut otp = commissioned_board().await;
        let record = record_line(&mut otp).await.into_bytes();
        let files = Files(vec![(RECORD_URL.to_string(), record.clone())]);
        let retired = retired_table(&record);
        let (result, out) = validate(&mut otp, &retired, &files, false).await;
        result.unwrap();
        assert!(shows(out.lines(), &[SIGNER_NAME, "1"]), "{out}");
        assert_eq!(verdict(&mut otp, &retired, &files).await, "recorded");
    }

    /// A record that can't be downloaded shows on one line, with its URL and
    /// any HTTP status. onerom-fw's text for either error is two lines.
    #[tokio::test]
    async fn a_download_failure_shows_on_one_line() {
        let mut otp = commissioned_board().await;
        let area = read_commissioning(&mut otp).await.unwrap();
        let instance = area.current().unwrap();
        let http = onerom_fw::Error::Http {
            url: RECORD_URL.to_string(),
            status: 404,
        };
        // reqwest's error for a URL it can't parse stands in for a network
        // error.
        let network = reqwest::Client::new().get("").build().unwrap_err();
        let network = onerom_fw::Error::network(RECORD_URL.to_string(), network);
        for (error, shown) in [(http, &[RECORD_URL, "404"][..]), (network, &[RECORD_URL])] {
            let verdict: Result<Verdict, AppError> = Err(AppError::fetch(RECORD_URL, error));
            let values = check_values(instance, &verdict, &table(None));
            let lines: Vec<&str> = values
                .iter()
                .map(|(_, value)| value.as_str())
                .filter(|value| holds(value, &[RECORD_URL]))
                .collect();
            assert_eq!(lines.len(), 1, "{values:?}");
            assert!(!lines[0].contains('\n'), "{values:?}");
            assert!(holds(lines[0], shown), "{values:?}");
        }
    }

    #[tokio::test]
    async fn a_retired_keys_unrecorded_signature_doesnt_validate() {
        let mut otp = commissioned_board().await;

        // Without a record.
        let retired = table(Some(json!({})));
        let files = Files(Vec::new());
        let (result, _) = validate(&mut otp, &retired, &files, false).await;
        assert!(matches!(result, Err(Error::NotValidated(_))));
        assert_eq!(verdict(&mut otp, &retired, &files).await, "not_recorded");

        // A record that doesn't list it.
        let record = b"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\n";
        let retired = retired_table(record);
        let files = Files(vec![(RECORD_URL.to_string(), record.to_vec())]);
        let (result, _) = validate(&mut otp, &retired, &files, false).await;
        assert!(matches!(result, Err(Error::NotValidated(_))));
        assert_eq!(verdict(&mut otp, &retired, &files).await, "not_recorded");

        // A record changed since the key was retired.
        let line = record_line(&mut otp).await;
        let files = Files(vec![(RECORD_URL.to_string(), line.into_bytes())]);
        let (result, _) = validate(&mut otp, &retired, &files, false).await;
        assert!(matches!(result, Err(Error::NotValidated(_))));
        assert_eq!(verdict(&mut otp, &retired, &files).await, "record_changed");

        // A record that can't be fetched. A line of the output holds its URL
        // and the server's status. The reason differs from the reason for a
        // record that can't be read.
        let files = Files(Vec::new());
        let (result, out) = validate(&mut otp, &retired, &files, false).await;
        assert!(shows(out.lines(), &[RECORD_URL, "404"]), "{out}");
        let Err(Error::NotValidated(not_fetched)) = result else {
            panic!("{out}");
        };
        let malformed = b"a line that isn't a record line\n";
        let files = Files(vec![(RECORD_URL.to_string(), malformed.to_vec())]);
        let unreadable = retired_table(malformed);
        let (result, out) = validate(&mut otp, &unreadable, &files, false).await;
        let Err(Error::NotValidated(not_read)) = result else {
            panic!("{out}");
        };
        assert_ne!(not_fetched, not_read);
        let verdict = verdict(&mut otp, &unreadable, &files).await;
        assert_eq!(verdict, serde_json::Value::Null);

        // --json's error is the text of the line holding the URL and status.
        let files = Files(Vec::new());
        let (_, text) = validate(&mut otp, &retired, &files, false).await;
        let (_, out) = validate(&mut otp, &retired, &files, true).await;
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        let instance = &json["instances"][0];
        assert_eq!(instance["verdict"], serde_json::Value::Null);
        let error = instance["error"].as_str().unwrap();
        assert!(holds(error, &[RECORD_URL, "404"]), "{json}");
        assert!(text.lines().any(|line| line.ends_with(error)), "{text}");
    }

    // -----------------------------------------------------------------------
    // set-size
    // -----------------------------------------------------------------------

    /// `hardware set-size` for `board` and `size` without `--force` or
    /// `--dry-run`.
    fn size_args(board: &str, size: BoardSize) -> HardwareSetSizeArgs {
        HardwareSetSizeArgs {
            board: Board::try_from_str(board).unwrap(),
            size,
            force: false,
            dry_run: false,
        }
    }

    /// Sets `otp`'s size as `args` asks, with `--verbose`. `answer` is the
    /// user's answer where there's a question. `None` runs with `--yes`.
    /// Returns the result, the output and whether the user was asked.
    async fn set_size_with(
        otp: &mut MemoryOtp,
        args: &HardwareSetSizeArgs,
        answer: Option<&str>,
    ) -> (Result<bool, Error>, String, bool) {
        let typed = answer.unwrap_or_default().as_bytes();
        let mut input = typed;
        let mut out = Vec::new();
        let options = options(answer.is_none(), true);
        let result = set_size_otp(otp, args, &options, &mut out, &mut input).await;
        // The answer is read only where the user is asked.
        let asked = input.len() < typed.len();
        (result, String::from_utf8(out).unwrap(), asked)
    }

    /// A board set to `size` as a fire-40-a.
    async fn sized_board(size: BoardSize) -> MemoryOtp {
        let mut otp = blank_board();
        let (result, out, _) = set_size_with(&mut otp, &size_args("fire-40-a", size), None).await;
        result.unwrap_or_else(|e| panic!("{e}\n{out}"));
        otp
    }

    /// --yes writes without asking. The output shows the board and its size,
    /// then the parts to write and their rows.
    #[tokio::test]
    async fn l_is_set_with_yes_and_says_so() {
        let mut otp = blank_board();
        let args = size_args("fire-40-a", BoardSize::L);
        let (result, out, asked) = set_size_with(&mut otp, &args, None).await;
        println!("{out}");
        assert!(result.unwrap(), "{out}");
        assert!(!asked, "{out}");
        let lines: Vec<&str> = out.lines().collect();
        assert!(in_turn(&lines, &[&["fire-40-a"], &["L"]]), "{out}");
        assert_eq!(
            part_lists(&out),
            [vec![(StepKind::FlashDevinfo, 1), (StepKind::BootFlags0, 3)]],
            "{out}"
        );
        let devinfo = [["0x054", "0x99af"].as_slice()];
        assert!(lists_rows(&out, StepKind::FlashDevinfo, &devinfo), "{out}");
        // A line for each step once it's written.
        for kind in [StepKind::FlashDevinfo, StepKind::BootFlags0] {
            let written = lines
                .iter()
                .rev()
                .take(3)
                .filter(|line| holds(line, &[&step_name(kind)]))
                .count();
            assert_eq!(written, 1, "{out}");
        }
        assert_eq!(otp.write_count(), 4);
        assert_eq!(otp.rows()[0x054], 0x3a99af);
        assert_eq!(otp.rows()[0x048..=0x04a], [0x20; 3]);
    }

    /// The size is written only once the user agrees.
    #[tokio::test]
    async fn a_size_is_set_only_once_the_user_agrees() {
        let mut otp = blank_board();
        let args = size_args("fire-40-a", BoardSize::L);
        let (result, out, asked) = set_size_with(&mut otp, &args, Some("n\n")).await;
        assert!(!result.unwrap(), "{out}");
        assert!(asked, "{out}");
        assert_eq!(otp.write_count(), 0);

        let (result, out, asked) = set_size_with(&mut otp, &args, Some("y\n")).await;
        assert!(result.unwrap(), "{out}");
        assert!(asked, "{out}");
        assert_eq!(otp.write_count(), 4);
    }

    #[tokio::test]
    async fn a_set_size_dry_run_doesnt_ask_or_write() {
        let mut otp = blank_board();
        let mut args = size_args("fire-40-a", BoardSize::L);
        args.dry_run = true;
        let (result, out, asked) = set_size_with(&mut otp, &args, Some("y\n")).await;
        assert!(!result.unwrap(), "{out}");
        assert!(!asked, "{out}");
        assert_eq!(otp.write_count(), 0);
        // The rows it would write.
        assert!(shows(out.lines(), &["0x054", "0x99af"]), "{out}");
    }

    /// A board already the size asked for, M or otherwise, has nothing to
    /// write. The last line shows the size.
    #[tokio::test]
    async fn a_board_already_its_size_has_nothing_to_write() {
        for &size in BoardSize::supported_values() {
            let mut otp = sized_board(size).await;
            let writes = otp.write_count();
            let args = size_args("fire-40-a", size);
            let (result, out, asked) = set_size_with(&mut otp, &args, Some("y\n")).await;
            assert!(!result.unwrap(), "{out}");
            assert!(!asked, "{out}");
            assert_eq!(otp.write_count(), writes, "{out}");
            let last = out.lines().last().unwrap_or_default();
            assert!(holds(last, &[&size.to_string()]), "{out}");
        }
    }

    /// A refusal before anything is written prints nothing.
    #[tokio::test]
    async fn a_refused_size_is_a_set_size_error() {
        let mut otp = sized_board(BoardSize::L).await;
        let writes = otp.write_count();
        let args = size_args("fire-40-a", BoardSize::M);
        let (result, out, _) = set_size_with(&mut otp, &args, None).await;
        let error = result.unwrap_err();
        let already = CommissionError::SizeAlreadySet {
            size: onerom_metadata::OneromBoardSize::BoardSizeL,
            requested: BoardSize::M,
        };
        assert!(
            matches!(&error, Error::SetSize(refused) if *refused == already),
            "{error}"
        );
        assert_eq!(out, "");
        assert_eq!(otp.write_count(), writes);

        let mut otp = commissioned_board().await;
        let (result, _, _) = set_size_with(&mut otp, &args, None).await;
        let error = result.unwrap_err();
        let another = CommissionError::CommissionedAsAnotherBoard {
            board: "fire-24-f".to_string(),
            requested: args.board,
        };
        assert!(
            matches!(&error, Error::SetSize(refused) if *refused == another),
            "{error}"
        );
    }

    /// A failure once writing has begun is a failure part way. Only a lost
    /// connection is completed by running again.
    #[tokio::test]
    async fn a_failure_while_writing_the_size_fails_part_way() {
        let args = size_args("fire-40-a", BoardSize::L);
        let mut lost = blank_board();
        // FLASH_DEVINFO lands. The first BOOT_FLAGS0 copy doesn't.
        lost.interrupt(2, Interruption::NotLanded);
        let mut wrong = blank_board();
        wrong.corrupt(1, 0x000100);
        for mut otp in [lost, wrong] {
            let (result, out, _) = set_size_with(&mut otp, &args, None).await;
            let error = result.unwrap_err();
            assert!(matches!(error, Error::SetSizeFailed(_)), "{error}\n{out}");
        }
    }

    #[tokio::test]
    async fn set_size_needs_a_device() {
        let args = size_args("fire-40-a", BoardSize::L);
        let error = cmd_set_size(&mut no_device(), &args).await.unwrap_err();
        assert!(matches!(error, Error::NoDevice), "{error}");
    }

    // -----------------------------------------------------------------------
    // --key-id, --signature and a signing server's key
    // -----------------------------------------------------------------------

    /// [`test_board::args`] signed with `--key-id 1 --signature SIGNATURE` in
    /// place of `--key`.
    fn signature_args(board: &str, size: BoardSize, signature: [u8; 64]) -> HardwareCommissionArgs {
        let mut args = test_board::args(board, size, std::path::PathBuf::new());
        args.key = None;
        args.key_id = Some(1);
        args.signature = Some(signature);
        args
    }

    /// The test key's signature over the instance `args` asks for, signed by
    /// signer `id`, on a board whose CHIPID is [`CHIP_ID`].
    fn signature_for(args: &HardwareCommissionArgs, id: u16) -> [u8; 64] {
        let date = args.date.as_deref().unwrap();
        let values = CommissioningValues::new(args.board, &args.manufacturer, date, id).unwrap();
        test_board::key().sign(&values.message(CHIP_ID)).to_bytes()
    }

    /// Commissions `otp` as `args` asks, with `--yes`, the signature from
    /// `--signature` and the test table. Returns the result and the output.
    async fn commission_signed(
        otp: &mut MemoryOtp,
        args: &HardwareCommissionArgs,
        table: &SignerTable,
    ) -> (Result<bool, Error>, String) {
        let signing = signing(args).unwrap();
        let signer = match signer_for(table, &signing, &args.manufacturer).await {
            Ok(signer) => signer,
            Err(e) => return (Err(e), String::new()),
        };
        let mut out = Vec::new();
        let result = commission_otp(
            otp,
            args,
            &signing,
            (signer, table),
            &options(true, false),
            &mut out,
            &mut std::io::empty(),
        )
        .await;
        (result, String::from_utf8(out).unwrap())
    }

    /// A signature `--signature` provides is written without a second
    /// signing, and the board validates.
    #[tokio::test]
    async fn a_given_signature_is_written_and_validates() {
        let mut args = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        args.signature = Some(signature_for(&args, 1));
        let mut otp = blank_board();
        let table = table(None);
        let (result, out) = commission_signed(&mut otp, &args, &table).await;
        assert!(result.unwrap(), "{out}");
        assert_eq!(otp.write_count(), first_run_rows());
        let (result, out) = validate(&mut otp, &table, &Files(Vec::new()), false).await;
        result.unwrap_or_else(|e| panic!("{e}\n{out}"));
        // A second run finds every row present.
        let (result, out) = commission_signed(&mut otp, &args, &table).await;
        assert!(!result.unwrap(), "{out}");
        assert_eq!(otp.write_count(), first_run_rows());
    }

    #[tokio::test]
    async fn a_given_signature_from_another_key_is_refused_before_writing() {
        let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let mut args = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        let date = args.date.clone().unwrap();
        let values = CommissioningValues::new(args.board, "piers.rocks", &date, 1).unwrap();
        args.signature = Some(other.sign(&values.message(CHIP_ID)).to_bytes());
        let mut otp = blank_board();
        let (result, out) = commission_signed(&mut otp, &args, &table(None)).await;
        let error = result.unwrap_err();
        assert!(
            matches!(error, Error::BadSignature { id: 1, .. }),
            "{error}\n{out}"
        );
        assert_eq!(otp.write_count(), 0);

        // The key's signature over another date is refused too.
        let mut args = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        args.signature = Some(signature_for(&args, 1));
        args.date = Some("20260102".to_string());
        let (result, _) = commission_signed(&mut otp, &args, &table(None)).await;
        assert!(matches!(result, Err(Error::BadSignature { .. })));
        assert_eq!(otp.write_count(), 0);
    }

    #[tokio::test]
    async fn a_key_id_the_table_doesnt_contain_is_refused() {
        let mut args = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        args.key_id = Some(7);
        args.signature = Some(signature_for(&args, 7));
        let mut otp = blank_board();
        let (result, _) = commission_signed(&mut otp, &args, &table(None)).await;
        assert!(
            matches!(result, Err(Error::SigningKeyIdUnknown(7))),
            "{result:?}"
        );
        assert_eq!(otp.write_count(), 0);
    }

    #[tokio::test]
    async fn a_retired_key_id_is_refused() {
        let mut args = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        args.signature = Some(signature_for(&args, 1));
        let mut otp = blank_board();
        let retired = table(Some(json!({})));
        let (result, _) = commission_signed(&mut otp, &args, &retired).await;
        assert!(
            matches!(result, Err(Error::SigningKeyRetired { id: 1, .. })),
            "{result:?}"
        );
        assert_eq!(otp.write_count(), 0);
    }

    /// Whether `result` is the refusal of key 1 signing piers.rocks.
    fn piers_rocks_not_allowed<T>(result: &Result<T, Error>) -> bool {
        matches!(
            result,
            Err(Error::ManufacturerNotAllowed { id: 1, manufacturer, .. })
                if manufacturer == "piers.rocks"
        )
    }

    /// Each signature source is refused a manufacturer its key doesn't allow
    /// before the board is written. A signing server's key is refused before
    /// its public key is fetched.
    #[tokio::test]
    async fn commission_refuses_a_manufacturer_the_key_doesnt_allow() {
        let (_dir, path) = key_file();
        let key_file = test_board::args("fire-24-f", BoardSize::M, path);
        let mut server = test_board::args("fire-24-f", BoardSize::M, std::path::PathBuf::new());
        server.key = None;
        server.signer = Some("https://example.invalid".to_string());
        server.key_id = Some(1);
        server.pin = Some("1234".to_string());
        let mut signature = signature_args("fire-24-f", BoardSize::M, [0; 64]);
        signature.signature = Some(signature_for(&signature, 1));

        let refusing = test_board::table_allowing(&["onerom.org"]);
        for args in [&key_file, &server, &signature] {
            let signing = signing(args).unwrap();
            let result = signer_for(&refusing, &signing, &args.manufacturer).await;
            assert!(piers_rocks_not_allowed(&result), "{:?}", result.err());
        }
        let mut otp = blank_board();
        let (result, _) = commission_signed(&mut otp, &signature, &refusing).await;
        assert!(piers_rocks_not_allowed(&result), "{result:?}");
        assert_eq!(otp.write_count(), 0);

        let allowing = test_board::table_allowing(&["onerom.org", "piers.rocks"]);
        for args in [&key_file, &signature] {
            let signing = signing(args).unwrap();
            let signer = signer_for(&allowing, &signing, &args.manufacturer).await;
            assert_eq!(signer.unwrap().id(), 1);
        }
    }

    /// A signing server's key is refused where its public key isn't the
    /// table's key with the same ID.
    #[test]
    fn a_signing_servers_key_must_be_the_tables() {
        let table = table(None);
        let address = "https://example.invalid/";
        let key = test_board::key().verifying_key().to_bytes();
        assert!(check_server_key(&table, 1, &key, address).is_ok());
        let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32])
            .verifying_key()
            .to_bytes();
        for (id, public_key) in [(1, other), (2, key)] {
            let error = check_server_key(&table, id, &public_key, address).unwrap_err();
            assert!(
                matches!(
                    &error,
                    Error::SigningServerKeyMismatch { url, id: i } if url == address && *i == id
                ),
                "{error}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // sign
    // -----------------------------------------------------------------------

    /// `words` as a shell splits them. It reads the single quotes
    /// [`shell_word`] writes.
    fn shell_split(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word: Option<String> = None;
        let mut chars = line.chars();
        let mut quoted = false;
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    quoted = !quoted;
                    word.get_or_insert_default();
                }
                '\\' if !quoted => word.get_or_insert_default().extend(chars.next()),
                ' ' if !quoted => words.extend(word.take()),
                c => word.get_or_insert_default().push(c),
            }
        }
        words.extend(word);
        words
    }

    /// `hardware sign` with `words` after `onerom hardware sign`, parsed and
    /// checked as the CLI does.
    fn sign_args(words: &[&str]) -> HardwareSignArgs {
        let line = ["onerom", "hardware", "sign"].iter().chain(words);
        let cli = crate::args::Cli::try_parse_from(line).unwrap();
        cli.command.check_args().unwrap();
        let crate::args::Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let crate::args::hardware::HardwareCommands::Sign(args) = hardware.command else {
            panic!("not hardware sign");
        };
        args
    }

    /// The values the tests sign, for the board in the test's CHIPID.
    fn to_sign(board: &str, manufacturer: &str) -> Vec<String> {
        [
            "--chip-id",
            &format_chip_id(CHIP_ID),
            "--board",
            board,
            "--manufacturer",
            manufacturer,
            "--date",
            test_board::DATE,
        ]
        .map(str::to_string)
        .to_vec()
    }

    /// Signs as `args` asks with `signing`, signer 1 in the test table.
    /// `answer` is the user's answer, where there's a question. `None` runs
    /// with `--yes`. Returns the result, what the user sees before signing
    /// and the output.
    async fn sign_with<S: SignatureSource>(
        args: &HardwareSignArgs,
        signing: &S,
        answer: Option<&str>,
    ) -> (Result<(), Error>, String, String) {
        let table = table(None);
        let (mut ui, mut out) = (Vec::new(), Vec::new());
        let mut input = answer.unwrap_or_default().as_bytes();
        let result = sign_values(
            args,
            signing,
            (table.get(1).unwrap(), &table),
            &options(answer.is_none(), false),
            &mut ui,
            &mut input,
            &mut out,
        )
        .await;
        let text = |bytes| String::from_utf8(bytes).unwrap();
        (result, text(ui), text(out))
    }

    /// Signs `words` with the test key file and `--yes`. Returns what the
    /// user sees before signing and the output.
    async fn sign_with_key_file(words: &[&str]) -> (String, String) {
        let (_dir, path) = key_file();
        let key = path.to_str().unwrap();
        let words: Vec<&str> = words.iter().copied().chain(["--key", key]).collect();
        let args = sign_args(&words);
        let signing = Signing::File(KeyFile::read(&path, None).unwrap());
        let (result, ui, out) = sign_with(&args, &signing, None).await;
        result.unwrap_or_else(|e| panic!("{e}\n{ui}{out}"));
        (ui, out)
    }

    /// The command line a sign's output shows, parsed and checked.
    fn command_in(out: &str) -> HardwareCommissionArgs {
        let command = out
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("onerom hardware commission "))
            .unwrap_or_else(|| panic!("{out}"));
        let cli = crate::args::Cli::try_parse_from(shell_split(command)).unwrap();
        cli.command.check_args().unwrap();
        let crate::args::Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let crate::args::hardware::HardwareCommands::Commission(args) = hardware.command else {
            panic!("not hardware commission");
        };
        args
    }

    /// A key file signs the values, and the command shown commissions a
    /// board with them.
    #[tokio::test]
    async fn a_key_file_signs_and_the_command_commissions() {
        let words = to_sign("fire-40-a", "piers.rocks");
        let words: Vec<&str> = words
            .iter()
            .map(String::as_str)
            .chain(["--size", "L"])
            .collect();
        let (ui, out) = sign_with_key_file(&words).await;
        println!("{ui}{out}");
        // The values, a line each, then the answer without a question.
        let lines: Vec<&str> = ui.lines().collect();
        let values: [&[&str]; 6] = [
            &["DE3F9C232F655B6B"],
            &["fire-40-a"],
            &["L"],
            &["piers.rocks"],
            &["2026-01-01"],
            &[SIGNER_NAME, "1"],
        ];
        assert!(in_turn(&lines, &values), "{ui}");
        assert!(!ui.contains("(y/N)"), "{ui}");

        // The signature verifies with the test key.
        let values = CommissioningValues::new(
            Board::try_from_str("fire-40-a").unwrap(),
            "piers.rocks",
            test_board::DATE,
            1,
        )
        .unwrap();
        let expected = test_board::key().sign(&values.message(CHIP_ID)).to_bytes();
        assert!(shows(out.lines(), &[&hex::encode(expected)]), "{out}");

        // The command carries the values, the key's ID and the signature.
        let args = command_in(&out);
        assert_eq!(args.board, Board::try_from_str("fire-40-a").unwrap());
        assert_eq!(args.size, Some(BoardSize::L));
        assert_eq!(args.manufacturer, "piers.rocks");
        assert_eq!(args.date.as_deref(), Some(test_board::DATE));
        assert_eq!(args.key_id, Some(1));
        assert_eq!(args.signature, Some(expected));
        assert!(args.signer.is_none() && args.key.is_none() && args.pin.is_none());

        // It commissions a board whose CHIPID was signed.
        let mut otp = blank_board();
        let (result, out) = commission_signed(&mut otp, &args, &table(None)).await;
        assert!(result.unwrap(), "{out}");
    }

    /// The command always carries the size, M where the board doesn't
    /// support external flash.
    #[tokio::test]
    async fn the_command_always_carries_the_size() {
        let words = to_sign("fire-24-f", "piers.rocks");
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let (_, out) = sign_with_key_file(&words).await;
        assert_eq!(command_in(&out).size, Some(BoardSize::M), "{out}");
    }

    /// A manufacturer a shell would split or change is quoted.
    #[tokio::test]
    async fn a_manufacturer_a_shell_changes_is_quoted() {
        for manufacturer in ["Piers's Boards & Co", "a b", "$HOME", "\"x\"", "\\"] {
            let words = to_sign("fire-24-f", manufacturer);
            let words: Vec<&str> = words.iter().map(String::as_str).collect();
            let (_, out) = sign_with_key_file(&words).await;
            assert_eq!(command_in(&out).manufacturer, manufacturer, "{out}");
        }
    }

    #[tokio::test]
    async fn json_carries_the_signature_and_the_command() {
        let words = to_sign("fire-24-f", "piers.rocks");
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let (_, text) = sign_with_key_file(&words).await;
        let json_words: Vec<&str> = words.iter().copied().chain(["--json"]).collect();
        let (ui, out) = sign_with_key_file(&json_words).await;
        println!("{out}");
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        let command = command_in(&text);
        let signature = hex::encode(command.signature.unwrap());
        assert_eq!(
            json,
            json!({
                "chip_id": "DE3F9C232F655B6B",
                "board": "fire-24-f",
                "size": "M",
                "manufacturer": "piers.rocks",
                "date": "20260101",
                "key_id": 1,
                "signature": signature,
                "command": json["command"],
            })
        );
        // The command is the one the text shows.
        let shown = json["command"].as_str().unwrap();
        assert!(shows(text.lines(), &[shown]), "{text}");
        // What the user sees before signing is the same with --json.
        let (text_ui, _) = sign_with_key_file(&words).await;
        assert_eq!(ui, text_ui);
    }

    /// `hardware sign` with a signing server holding key 1, plus `extra`.
    fn server_sign_args(extra: &[&str]) -> HardwareSignArgs {
        let words = to_sign("fire-24-f", "onerom.org");
        let server = ["--signer", "https://example.invalid", "--key-id", "1"];
        let words: Vec<&str> = words
            .iter()
            .map(String::as_str)
            .chain(server)
            .chain(extra.iter().copied())
            .collect();
        sign_args(&words)
    }

    /// A signing server's signature is checked before the user is asked, and
    /// recorded only once they agree.
    #[tokio::test]
    async fn sign_records_a_signature_only_once_the_user_agrees() {
        let args = server_sign_args(&[]);
        let signing = Server::default();
        let (result, ui, out) = sign_with(&args, &signing, Some("n\n")).await;
        result.unwrap();
        assert!(ui.contains("(y/N)"), "{ui}");
        assert_eq!(out, "");
        assert_eq!(*signing.requests.borrow(), ["dry run"]);

        let signing = Server::default();
        let (result, ui, out) = sign_with(&args, &signing, Some("y\n")).await;
        result.unwrap_or_else(|e| panic!("{e}\n{ui}{out}"));
        assert_eq!(*signing.requests.borrow(), ["dry run", "record"]);
        assert_eq!(command_in(&out).key_id, Some(1));
    }

    /// A dry run doesn't ask or record. It shows the signature a recorded run
    /// shows, without the command, as text and as JSON.
    #[tokio::test]
    async fn a_sign_dry_run_doesnt_ask_or_record() {
        let signing = Server::default();
        let (result, _, recorded) = sign_with(&server_sign_args(&[]), &signing, None).await;
        result.unwrap();
        let signature = hex::encode(command_in(&recorded).signature.unwrap());

        let signing = Server::default();
        let args = server_sign_args(&["--dry-run"]);
        let (result, ui, out) = sign_with(&args, &signing, Some("y\n")).await;
        result.unwrap_or_else(|e| panic!("{e}\n{ui}{out}"));
        assert!(!ui.contains("(y/N)"), "{ui}");
        assert_eq!(*signing.requests.borrow(), ["dry run"]);
        assert!(shows(out.lines(), &[&signature]), "{out}");
        assert!(!out.contains("onerom hardware commission"), "{out}");

        let args = server_sign_args(&["--dry-run", "--json"]);
        let (result, _, out) = sign_with(&args, &Server::default(), None).await;
        result.unwrap();
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["signature"], signature);
        assert!(json.get("command").is_none(), "{out}");
    }

    /// A signature that doesn't verify with the table's key is refused
    /// before the user is asked and before anything is recorded.
    #[tokio::test]
    async fn a_signature_that_doesnt_verify_is_refused_before_recording() {
        let signing = Server {
            other_key: true,
            ..Server::default()
        };
        let args = server_sign_args(&[]);
        let (result, ui, out) = sign_with(&args, &signing, Some("y\n")).await;
        assert!(matches!(result, Err(Error::BadSignature { id: 1, .. })));
        assert!(!ui.contains("(y/N)"), "{ui}");
        assert_eq!(out, "");
        assert_eq!(*signing.requests.borrow(), ["dry run"]);
    }

    /// A recorded signature other than the one checked is refused and isn't
    /// shown.
    #[tokio::test]
    async fn a_recorded_signature_other_than_the_one_checked_is_refused() {
        let signing = Server {
            changes_signature: true,
            ..Server::default()
        };
        let (result, _, out) = sign_with(&server_sign_args(&[]), &signing, None).await;
        assert!(matches!(result, Err(Error::RecordedSignatureDiffers)));
        assert_eq!(out, "");
        assert_eq!(*signing.requests.borrow(), ["dry run", "record"]);
    }

    /// `hardware sign` refuses a manufacturer the key doesn't allow, for a
    /// key file and a signing server. The server's key is refused before its
    /// public key is fetched and before it's asked to sign.
    #[tokio::test]
    async fn sign_refuses_a_manufacturer_the_key_doesnt_allow() {
        let (_dir, path) = key_file();
        let words = to_sign("fire-24-f", "piers.rocks");
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let key_file = [&words[..], &["--key", path.to_str().unwrap()]].concat();
        let server = [
            &words[..],
            &["--signer", "https://example.invalid", "--key-id", "1"],
            &["--pin", "1234"],
        ]
        .concat();
        let table = test_board::table_allowing(&["onerom.org"]);
        for words in [key_file, server] {
            let args = sign_args(&words);
            let signing = key_source(
                (args.signer.as_deref(), args.key_id),
                args.key.as_deref(),
                args.pin.as_deref(),
                &mut std::io::sink(),
            )
            .unwrap();
            let result = signer_for(&table, &signing, &args.manufacturer).await;
            assert!(piers_rocks_not_allowed(&result), "{:?}", result.err());
        }
    }

    #[test]
    fn a_word_a_shell_leaves_alone_isnt_quoted() {
        for word in ["onerom", "--board", "fire-24-f", "piers.rocks", "20260101"] {
            assert_eq!(shell_word(word), word);
        }
        for word in ["", "a b", "it's", "$x", "a;b", "*"] {
            assert_eq!(shell_split(&shell_word(word)), [word], "{word}");
        }
    }

    // -----------------------------------------------------------------------
    // request-signature
    // -----------------------------------------------------------------------

    /// `hardware request-signature` for `board` and `size`.
    fn request_args(board: &str, size: Option<BoardSize>) -> HardwareRequestSignatureArgs {
        HardwareRequestSignatureArgs {
            board: Board::try_from_str(board).unwrap(),
            size,
        }
    }

    /// Runs `hardware request-signature` on `otp`. Returns the result and the
    /// output.
    async fn request(
        otp: &mut MemoryOtp,
        args: &HardwareRequestSignatureArgs,
    ) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let result = request_signature_otp(otp, args, &mut out).await;
        (result, String::from_utf8(out).unwrap())
    }

    #[tokio::test]
    async fn a_request_links_the_form_filled_in_and_writes_nothing() {
        for (board, size, shown) in [
            ("fire-24-f", None, BoardSize::M),
            ("fire-40-a", Some(BoardSize::L), BoardSize::L),
            ("fire-40-a", Some(BoardSize::M), BoardSize::M),
        ] {
            let mut otp = blank_board();
            let (result, out) = request(&mut otp, &request_args(board, size)).await;
            println!("{out}");
            result.unwrap_or_else(|e| panic!("{e}"));
            let lines: Vec<&str> = out.lines().collect();
            let size = shown.to_string();
            let values: [&[&str]; 4] = [
                &["DE3F9C232F655B6B"],
                &[board],
                &[&size],
                &[signing_request::MANUFACTURER],
            ];
            assert!(in_turn(&lines, &values), "{out}");
            // signing_request's tests check what the link fills in.
            let request = SigningRequest {
                chip_id: CHIP_ID,
                board: Board::try_from_str(board).unwrap(),
                size: shown,
            };
            assert!(shows(out.lines(), &[&request.link()]), "{out}");
            assert_eq!(otp.write_count(), 0);
        }
    }

    /// A fire-24-f commissioned with a community signing request's values
    /// on `date`. The test key signs it as signer 2.
    async fn community_board(date: &str) -> MemoryOtp {
        let mut otp = blank_board();
        let request = Request {
            board: Board::try_from_str("fire-24-f").unwrap(),
            size: BoardSize::M,
            manufacturer: signing_request::MANUFACTURER.to_string(),
            date: RequestDate::Given(date.to_string()),
            signer: signing_request::KEY_ID,
            force: false,
        };
        let prepared = prepare(&mut otp, &request).await.unwrap();
        let signature = test_board::key().sign(prepared.message()).to_bytes();
        let plan = prepared.plan(&signature).unwrap();
        plan.execute(&mut otp, |_| {}).await.unwrap();
        otp
    }

    /// A board commissioned with a community signing request's values on
    /// another date is refused, as the member's run with the signature's
    /// date would be.
    #[tokio::test]
    async fn a_request_for_a_board_commissioned_on_another_date_is_refused() {
        let mut otp = community_board(test_board::DATE).await;
        let (result, out) = request(&mut otp, &request_args("fire-24-f", None)).await;
        let error = result.unwrap_err();
        assert!(
            matches!(
                &error,
                Error::RequestSignature(CommissionError::AlreadyCommissioned {
                    only_date_differs: true,
                    ..
                })
            ),
            "{error}"
        );
        assert_eq!(out, "");
    }

    /// What `hardware commission` checks before it asks for a community
    /// signature on `otp`, as `board` and `size`.
    async fn commission_refusal(
        otp: &mut MemoryOtp,
        board: &str,
        size: BoardSize,
    ) -> Option<CommissionError> {
        let request = Request {
            board: Board::try_from_str(board).unwrap(),
            size,
            manufacturer: signing_request::MANUFACTURER.to_string(),
            date: RequestDate::Given(today()),
            signer: signing_request::KEY_ID,
            force: false,
        };
        prepare(otp, &request).await.err()
    }

    /// A board `hardware commission` refuses is refused for the same reason,
    /// and the advice identifies only request-signature's own options.
    #[tokio::test]
    async fn a_request_is_refused_where_commission_would_be() {
        use onerom_metadata::otp::pico_otp::ecc_encode;
        let flash_devinfo = |otp: &mut MemoryOtp| otp.set_raw(0x054, ecc_encode(0x99af));
        let version_2 = |otp: &mut MemoryOtp| {
            otp.set_raw(0x0c0, ecc_encode(onerom_metadata::OTP_STORE_MAGIC));
            otp.set_raw(0x0c1, ecc_encode(2));
        };
        let page_59_locked = |otp: &mut MemoryOtp| {
            otp.set_raw(0xf81 + 2 * 59, 0x151515);
            otp.reset();
        };
        let white_label_addr = |otp: &mut MemoryOtp| otp.set_raw(0x05c, ecc_encode(0x0100));
        type Case = (MemoryOtp, &'static str, BoardSize, fn(&mut MemoryOtp));
        let cases: Vec<Case> = vec![
            (
                commissioned_board().await,
                "fire-24-f",
                BoardSize::M,
                |_| {},
            ),
            (
                community_board(test_board::DATE).await,
                "fire-24-f",
                BoardSize::M,
                |_| {},
            ),
            (blank_board(), "fire-24-f", BoardSize::M, version_2),
            (blank_board(), "fire-24-f", BoardSize::M, flash_devinfo),
            (blank_board(), "fire-40-a", BoardSize::M, flash_devinfo),
            (
                sized_board(BoardSize::L).await,
                "fire-40-a",
                BoardSize::M,
                |_| {},
            ),
            (blank_board(), "fire-24-f", BoardSize::M, page_59_locked),
            (blank_board(), "fire-24-f", BoardSize::M, white_label_addr),
        ];
        let command = {
            let mut onerom = <crate::args::Cli as clap::CommandFactory>::command();
            onerom.build();
            let hardware = onerom.find_subcommand("hardware").unwrap().clone();
            hardware
                .find_subcommand("request-signature")
                .unwrap()
                .clone()
        };
        let options: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_long())
            .map(|long| format!("--{long}"))
            .collect();
        for (mut otp, board, size, set_up) in cases {
            set_up(&mut otp);
            let writes = otp.write_count();
            let expected = commission_refusal(&mut otp, board, size).await.unwrap();
            let args = request_args(board, Some(size));
            let (result, out) = request(&mut otp, &args).await;
            let error = result.unwrap_err();
            assert!(
                matches!(&error, Error::RequestSignature(e) if *e == expected),
                "{error}"
            );
            assert_eq!(out, "");
            assert_eq!(otp.write_count(), writes);
            let text = error.to_string();
            println!("{text}\n");
            for word in text.split(|c: char| c.is_whitespace() || c == ',') {
                if word.starts_with("--") {
                    let option = word.trim_end_matches(|c: char| !c.is_alphanumeric());
                    assert!(options.iter().any(|o| o == option), "{text}");
                }
            }
        }
    }

    #[tokio::test]
    async fn request_signature_needs_a_device() {
        let args = request_args("fire-24-f", None);
        let error = cmd_request_signature(&mut no_device(), &args)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::NoDevice), "{error}");
    }

    /// Firmware for another board is refused unless --force is given.
    #[test]
    fn firmware_for_another_board_needs_force() {
        let board = |name| Board::try_from_str(name).unwrap();
        let (f, e) = (board("fire-24-f"), board("fire-24-e"));
        let commands = [
            OtpCommand::Commission,
            OtpCommand::SetSize,
            OtpCommand::RequestSignature,
        ];
        for command in commands {
            for force in [false, true] {
                assert!(check_firmware(command, None, f, force).is_ok());
                assert!(check_firmware(command, Some(f), f, force).is_ok());
            }
            assert!(check_firmware(command, Some(e), f, true).is_ok());
        }
        // Each command refuses with its own error.
        let error = check_firmware(OtpCommand::Commission, Some(e), f, false).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::FirmwareForAnotherBoard { firmware, board }
                    if firmware == "fire-24-e" && board == "fire-24-f"
            ),
            "{error}"
        );
        let error = check_firmware(OtpCommand::SetSize, Some(e), f, false).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::SetSizeFirmwareForAnotherBoard { firmware, board }
                    if firmware == "fire-24-e" && board == "fire-24-f"
            ),
            "{error}"
        );
        let error = check_firmware(OtpCommand::RequestSignature, Some(e), f, false).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::RequestSignatureFirmwareForAnotherBoard { firmware, board }
                    if firmware == "fire-24-e" && board == "fire-24-f"
            ),
            "{error}"
        );
        // request-signature doesn't have --force, so its refusal doesn't
        // advise an option.
        let text = error.to_string();
        assert!(!text.contains("--"), "{text}");
        assert!(holds(&text, &["fire-24-e", "fire-24-f"]), "{text}");
    }
}
