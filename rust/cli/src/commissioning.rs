// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Text describing a board's OTP for the commands that show it.
//!
//! A string from OTP is printed with its control characters escaped because a
//! board's OTP can hold any bytes.

use onerom_app::{
    BoardSize, EccRow, FLASH_DEVINFO_CS0_SIZE_SHIFT, FLASH_DEVINFO_CS1_GPIO,
    FLASH_DEVINFO_CS1_SIZE_SHIFT, FLASH_DEVINFO_D8H_ERASE_SUPPORTED, FLASH_DEVINFO_SIZE_BITS,
    LOCK1_READ_ONLY, OtpReport, PageLock, SignerTable,
};
use onerom_cli::otp::{Commissioning, escape_controls, format_date};
use onerom_config::hw::Board;
use onerom_metadata::otp::pico_otp::whitelabel::{
    OTP_ROW_USB_BOOT_FLAGS, OTP_ROW_USB_BOOT_FLAGS_R1, OTP_ROW_USB_BOOT_FLAGS_R2,
    OTP_ROW_USB_WHITE_LABEL_DATA,
};
use onerom_metadata::otp::{AreaIssue, CommissioningArea, CommissioningInstance, StoreEntry};
use onerom_metadata::{
    OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_BOOT_FLAGS0_R1_ROW, OTP_BOOT_FLAGS0_R2_ROW,
    OTP_BOOT_FLAGS0_ROW, OTP_COMMISSIONING_SIG_LEN, OTP_FLASH_DEVINFO_ROW,
    OTP_FLASH_PARTITION_SLOT_SIZE_ROW, OTP_PAGE_ROWS, OneromOtpEntry,
};

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

/// A label and its value, shown as a line.
pub type Labelled = (String, String);

/// `instance`'s board type, manufacturer and date with their labels. A value
/// the instance doesn't have is left out.
pub fn instance_values(instance: &CommissioningInstance) -> Vec<Labelled> {
    [
        ("Board type:", instance.board().map(escape_controls)),
        (
            "Manufacturer:",
            instance.manufacturer().map(escape_controls),
        ),
        ("Date:", instance.date().map(format_date)),
    ]
    .into_iter()
    .filter_map(|(label, value)| Some((label.to_string(), value?)))
    .collect()
}

/// `values` a line each, indented two spaces, with the values lined up past
/// the longest label.
pub fn labelled_lines(values: &[Labelled]) -> Vec<String> {
    let width = values
        .iter()
        .map(|(label, _)| label.len() + 1)
        .max()
        .unwrap_or_default();
    values
        .iter()
        .map(|(label, value)| format!("  {label:width$}{value}"))
        .collect()
}

/// `instance`'s state, shown beside its row. `current` is whether it's the
/// instance in use.
pub fn instance_state(instance: &CommissioningInstance, current: bool) -> &'static str {
    if current {
        "current"
    } else if !instance.is_complete() {
        "incomplete"
    } else if !instance.is_valid() {
        "missing values"
    } else {
        "replaced"
    }
}

/// Signing key `id` as `NAME (ID)`. `signer ID` where `table` doesn't have
/// it.
pub fn signer_name(id: u16, table: &SignerTable) -> String {
    match table.get(id) {
        Some(signer) => format!("{} ({id})", escape_controls(signer.name())),
        None => format!("signer {id}"),
    }
}

/// An entry the parser skipped because this build doesn't know its key.
pub struct UnknownKey {
    /// The entry's key row.
    pub row: u16,
    /// The key.
    pub key: u32,
    /// The value's length in bytes.
    pub len: usize,
}

/// The entries in `entries` with a key this build doesn't know.
pub fn unknown_keys(entries: &[StoreEntry]) -> impl Iterator<Item = UnknownKey> + '_ {
    entries.iter().filter_map(|entry| match entry {
        StoreEntry::Entry {
            row,
            entry: OneromOtpEntry::Unknown { key, params },
        } => Some(UnknownKey {
            row: *row,
            key: *key,
            len: params.len(),
        }),
        StoreEntry::Entry { .. } | StoreEntry::Deleted { .. } | StoreEntry::Unreadable { .. } => {
            None
        }
    })
}

impl UnknownKey {
    /// The entry as `key KEY (N bytes) at row ROW`.
    pub fn text(&self) -> String {
        format!(
            "key {} ({} bytes) at row {:#05x}",
            self.key, self.len, self.row
        )
    }
}

/// The text for an issue the parser found in an area.
pub fn issue_text(issue: &AreaIssue) -> String {
    match issue {
        AreaIssue::UnknownVersion { row, version } => {
            format!("unknown version {version} at row {row:#05x}")
        }
        AreaIssue::LostPlace { row } => format!("data at row {row:#05x} doesn't parse"),
    }
}

/// The `Commissioned:` value for an area holding an unknown version. `None`
/// for any other area.
///
/// The parser stops at an unknown version so the area doesn't have a current
/// instance. Newer tooling probably wrote it. The board may be commissioned.
pub fn unknown_version(area: &CommissioningArea) -> Option<String> {
    area.issues().iter().find_map(|issue| match issue {
        AreaIssue::UnknownVersion { row, version } => {
            Some(format!("unknown (version {version} at row {row:#05x})"))
        }
        AreaIssue::LostPlace { .. } => None,
    })
}

// ---------------------------------------------------------------------------
// scan and inspect info
// ---------------------------------------------------------------------------

/// The lines `scan` and `inspect info` show beneath a device's line.
/// `firmware` is the board the device's firmware is for.
///
/// With `details`:
/// - `Commissioned: yes` and beneath it the current instance's values
/// - with `verbose`, its signer
/// - with `verbose`, a line saying there isn't a current instance or that OTP
///   couldn't be read
///
/// Always, where they apply:
/// - `Commissioned: unknown` for an area holding an unknown version
/// - each skipped key
/// - a board that differs from the firmware's
///
/// `scan` asks for `details` only with `--verbose`. The lines don't say
/// whether a signature is valid.
pub fn device_lines(
    firmware: Option<Board>,
    commissioning: &Commissioning,
    (details, verbose): (bool, bool),
    table: &SignerTable,
) -> Vec<String> {
    let area = match commissioning {
        Commissioning::Read(area) => area,
        Commissioning::Unreadable if details && verbose => {
            return vec!["Commissioned: unknown (OTP not readable)".to_string()];
        }
        Commissioning::LabRunning if details && verbose => {
            return vec![
                "Commissioned: unknown (OTP not readable while One ROM Lab is running)".to_string(),
            ];
        }
        Commissioning::Unreadable | Commissioning::LabRunning | Commissioning::NotRead => {
            return Vec::new();
        }
    };
    let mut lines = Vec::new();
    // An area holding an unknown version doesn't have a current instance.
    match (unknown_version(area), area.current()) {
        (Some(unknown), _) => lines.push(format!("Commissioned: {unknown}")),
        (None, Some(current)) => {
            if details {
                let mut values = instance_values(current);
                if verbose && let Some(signer) = current.signer() {
                    values.push(("Signing key:".to_string(), signer_name(signer, table)));
                }
                lines.push("Commissioned: yes".to_string());
                lines.extend(labelled_lines(&values));
            }
            if let (Some(commissioned), Some(firmware)) = (current.board(), firmware)
                && Board::try_from_str(commissioned) != Some(firmware)
            {
                lines.push(format!(
                    "Board type mismatch: firmware type {}, commissioned type {}",
                    firmware.name(),
                    escape_controls(commissioned)
                ));
            }
        }
        (None, None) if details && verbose => lines.push("Commissioned: no".to_string()),
        (None, None) => {}
    }
    lines.extend(skipped_keys(area));
    lines
}

/// A line for each entry in `area` whose key this build doesn't know.
fn skipped_keys(area: &CommissioningArea) -> impl Iterator<Item = String> + '_ {
    area.instances()
        .iter()
        .flat_map(|instance| unknown_keys(instance.entries()))
        .map(|unknown| format!("Skipped: unknown {}", unknown.text()))
}

// ---------------------------------------------------------------------------
// inspect otp
// ---------------------------------------------------------------------------

/// The lines `inspect otp` shows for `report`.
///
/// These are always shown:
/// - unknown keys
/// - a general store, which only a newer tool starts
/// - pico-otp's reason it can't decode the white label and its warnings
///
/// `verbose` adds:
/// - the FLASH_DEVINFO fields beneath the board size
/// - the current instance's row and page locks
/// - every instance and entry
/// - the issues
/// - the raw rows
/// - the lock words
pub fn report_lines(report: &OtpReport, verbose: bool, table: &SignerTable) -> Vec<String> {
    let area = &report.commissioning;
    let mut lines = size_lines(report, verbose);
    lines.extend(commissioning_lines(report, table, verbose));
    if !verbose {
        let instances = area.instances().iter();
        let general_store = report.general_store.iter();
        lines.extend(
            instances
                .flat_map(|instance| unknown_keys(instance.entries()))
                .chain(general_store.flat_map(|store| unknown_keys(store.entries())))
                .map(|unknown| labelled("Skipped:", format!("unknown {}", unknown.text()))),
        );
    }
    lines.extend(white_label_lines(report));
    lines.extend(
        report
            .white_label_warnings
            .iter()
            .map(|warning| format!("  Warning: {}", escape_controls(warning))),
    );
    // Nothing uses the general store yet. A started one was written by a newer
    // tool, so it's shown.
    if verbose || report.general_store.is_some() {
        lines.push(labelled("General store:", general_store_text(report)));
    }
    if verbose {
        lines.extend(verbose_lines(report));
    }
    lines
}

/// `value` after `label`. It isn't padded, as the lines beneath each heading
/// line up among themselves.
fn labelled(label: &str, value: impl std::fmt::Display) -> String {
    format!("{label} {value}")
}

/// The board size. With `verbose`, the FLASH_DEVINFO it comes from beneath
/// it.
fn size_lines(report: &OtpReport, verbose: bool) -> Vec<String> {
    let size = match report.size {
        Some(BoardSize::M) => "M",
        Some(BoardSize::L) => "L",
        None => "neither M nor L",
    };
    let mut lines = vec![labelled("Board size:", size)];
    if verbose {
        let enabled = majority(report.boot_flags0) & OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE != 0;
        lines.extend(labelled_lines(&devinfo_values(
            report.flash_devinfo,
            enabled,
        )));
    }
    lines
}

/// FLASH_DEVINFO's fields and whether BOOT_FLAGS0 enables it, a line each.
fn devinfo_values(devinfo: EccRow, enabled: bool) -> Vec<Labelled> {
    let yes_no = |yes| if yes { "yes" } else { "no" }.to_string();
    let enable = ("FLASH_DEVINFO_ENABLE:".to_string(), yes_no(enabled));
    // An unwritten row reads as 0 with ECC, which would decode as two chip
    // selects without chips. A row can leave the factory with one bit set.
    let unwritten = devinfo.raw.count_ones() <= 1;
    let Some(value) = devinfo.value.filter(|_| !unwritten) else {
        let raw = if unwritten {
            "unwritten".to_string()
        } else {
            format!("invalid value {:#08x}", devinfo.raw)
        };
        return vec![("FLASH_DEVINFO:".to_string(), raw), enable];
    };
    let size = |shift| flash_size((value >> shift) & FLASH_DEVINFO_SIZE_BITS);
    vec![
        ("FLASH_DEVINFO:".to_string(), format!("{value:#06x}")),
        enable,
        ("CS0 size:".to_string(), size(FLASH_DEVINFO_CS0_SIZE_SHIFT)),
        ("CS1 size:".to_string(), size(FLASH_DEVINFO_CS1_SIZE_SHIFT)),
        (
            "CS1 GPIO:".to_string(),
            (value & FLASH_DEVINFO_CS1_GPIO).to_string(),
        ),
        (
            "D8h erase:".to_string(),
            yes_no(value & FLASH_DEVINFO_D8H_ERASE_SUPPORTED != 0),
        ),
    ]
}

/// A FLASH_DEVINFO size field. The bootrom reads a size `n` other than 0 as
/// 4KB shifted left `n` times.
fn flash_size(size: u16) -> String {
    if size == 0 {
        return "none".to_string();
    }
    let bytes = 4096_u64 << size;
    if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else {
        format!("{}KB", bytes / 1024)
    }
}

/// Each bit's majority across three copies of a row.
fn majority([a, b, c]: [u32; 3]) -> u32 {
    (a & b) | (a & c) | (b & c)
}

/// `Commissioned: yes` and beneath it the current instance with its signer.
/// With `verbose`, its row and whether its pages are locked too. `no` or the
/// reason there isn't one otherwise.
fn commissioning_lines(report: &OtpReport, table: &SignerTable, verbose: bool) -> Vec<String> {
    let area = &report.commissioning;
    if let Some(unknown) = unknown_version(area) {
        return vec![labelled("Commissioned:", unknown)];
    }
    let Some(current) = area.current() else {
        return vec![labelled("Commissioned:", "no")];
    };
    let mut values = instance_values(current);
    if let Some(signer) = current.signer() {
        values.push(("Signing key:".to_string(), signer_name(signer, table)));
    }
    if verbose {
        values.push(("Row:".to_string(), format!("{:#05x}", current.first_row())));
        values.extend(page_locks(current, &report.locks));
    }
    let mut lines = vec![labelled("Commissioned:", "yes")];
    lines.extend(labelled_lines(&values));
    lines
}

/// Whether each page of `instance` is locked, a line each.
fn page_locks(instance: &CommissioningInstance, locks: &[PageLock]) -> Vec<Labelled> {
    let first = instance.first_row() / OTP_PAGE_ROWS;
    let last = last_row(instance) / OTP_PAGE_ROWS;
    (first..=last)
        .map(|page| {
            let state = match locks.iter().find(|lock| lock.page == page).map(|l| l.raw) {
                Some(LOCK1_READ_ONLY) => "locked".to_string(),
                Some(0) => "unlocked".to_string(),
                Some(raw) => format!("lock word {raw:#08x}"),
                None => "lock not read".to_string(),
            };
            (format!("Page {page}:"), state)
        })
        .collect()
}

/// A complete instance's last row. Its signature's value ends it.
fn last_row(instance: &CommissioningInstance) -> u16 {
    let signature = instance
        .entries()
        .iter()
        .rev()
        .find_map(|entry| match entry {
            StoreEntry::Entry {
                row,
                entry: OneromOtpEntry::OtpKeyCommissioningSig { .. },
            } => Some(*row),
            StoreEntry::Entry { .. }
            | StoreEntry::Deleted { .. }
            | StoreEntry::Unreadable { .. } => None,
        });
    // A key row and a length row followed by two bytes of the value per row.
    signature.map_or(instance.first_row(), |row| {
        row + 1 + OTP_COMMISSIONING_SIG_LEN.div_ceil(2) as u16
    })
}

/// The bootloader USB IDs One ROM sets, with a label each. Each is a field of
/// the device section of picotool's JSON.
const WHITE_LABEL_IDS: [(&str, &str); 2] = [("USB VID:", "vid"), ("USB PID:", "pid")];

/// The seven bootloader USB strings One ROM sets, with OTP.md's names for
/// them. Each is a section and a field of picotool's JSON.
const WHITE_LABEL_STRINGS: [(&str, &str, &str); 7] = [
    ("USB manufacturer:", "device", "manufacturer"),
    ("USB product:", "device", "product"),
    ("Volume label:", "volume", "label"),
    ("INDEX.HTM link:", "volume", "redirect_url"),
    ("INDEX.HTM link name:", "volume", "redirect_name"),
    ("INFO_UF2.TXT model:", "volume", "model"),
    ("INFO_UF2.TXT board ID:", "volume", "board_id"),
];

/// `set` and beneath it the bootloader USB IDs and strings One ROM sets.
/// `not set` or pico-otp's reason it can't decode them otherwise.
fn white_label_lines(report: &OtpReport) -> Vec<String> {
    const HEADING: &str = "Bootloader USB info:";
    let Some(json) = &report.white_label else {
        let value = match &report.white_label_error {
            Some(error) => format!("can't be decoded: {}", escape_controls(error)),
            // The white label is unwritten so read_report didn't decode it.
            None => "not set".to_string(),
        };
        return vec![labelled(HEADING, value)];
    };
    let ids = white_label_ids(json);
    let strings = white_label_strings(json);
    if ids.iter().chain(&strings).all(|(_, value)| value.is_none()) {
        return vec![labelled(HEADING, "not set")];
    }
    let mut lines = vec![labelled(HEADING, "set")];
    lines.extend(labelled_lines(&white_label_values(json)));
    lines
}

/// The bootloader USB IDs One ROM sets that picotool's JSON `json` contains.
/// `None` for an ID it doesn't contain.
fn white_label_ids(json: &serde_json::Value) -> [(&'static str, Option<&str>); 2] {
    WHITE_LABEL_IDS.map(|(label, field)| {
        let id = json
            .get("device")
            .and_then(|device| device.get(field))
            .and_then(serde_json::Value::as_str);
        (label, id)
    })
}

/// The bootloader USB strings One ROM sets that picotool's JSON `json`
/// contains, in OTP.md's order. `None` for a string it doesn't contain.
fn white_label_strings(json: &serde_json::Value) -> [(&'static str, Option<&str>); 7] {
    WHITE_LABEL_STRINGS.map(|(label, section, field)| {
        let string = json
            .get(section)
            .and_then(|section| section.get(field))
            .and_then(serde_json::Value::as_str);
        (label, string)
    })
}

/// The bootloader USB IDs and strings One ROM sets, from picotool's JSON
/// `json`, a label and value each. The IDs come first, then the strings in
/// OTP.md's order. `not set` for one it doesn't contain.
pub(crate) fn white_label_values(json: &serde_json::Value) -> Vec<Labelled> {
    let ids = white_label_ids(json).map(|(label, id)| (label, id.map(id_text)));
    let strings =
        white_label_strings(json).map(|(label, string)| (label, string.map(escape_controls)));
    ids.into_iter()
        .chain(strings)
        .map(|(label, value)| (label.to_string(), value.unwrap_or("not set".to_string())))
        .collect()
}

/// A USB ID from picotool's JSON, such as `0xf540`, in hex with upper case
/// digits. Other text is escaped.
fn id_text(id: &str) -> String {
    id.strip_prefix("0x")
        .and_then(|digits| u16::from_str_radix(digits, 16).ok())
        .map_or_else(|| escape_controls(id), |id| format!("{id:#06X}"))
}

/// The general store's entry count and issues. `empty` where it hasn't been
/// started.
fn general_store_text(report: &OtpReport) -> String {
    let Some(store) = &report.general_store else {
        return "empty".to_string();
    };
    let mut text = match store.entries().len() {
        1 => "1 entry".to_string(),
        n => format!("{n} entries"),
    };
    for issue in store.issues() {
        match issue {
            AreaIssue::UnknownVersion { version, .. } => {
                return format!("unknown version {version}");
            }
            AreaIssue::LostPlace { .. } => {
                text.push_str(&format!(", {}", issue_text(issue)));
            }
        }
    }
    text
}

/// The lines `--verbose` adds.
fn verbose_lines(report: &OtpReport) -> Vec<String> {
    let area = &report.commissioning;
    let mut lines = vec!["Commissioning instances:".to_string()];
    if area.instances().is_empty() {
        lines.push("  None".to_string());
    }
    for instance in area.instances() {
        let current = area
            .current()
            .is_some_and(|current| current.first_row() == instance.first_row());
        lines.push(format!(
            "  Instance at row {:#05x} ({})",
            instance.first_row(),
            instance_state(instance, current)
        ));
        lines.extend(
            instance
                .entries()
                .iter()
                .map(|entry| format!("    {}", entry_text(entry))),
        );
    }
    if !area.issues().is_empty() {
        lines.push("Commissioning issues:".to_string());
        lines.extend(area.issues().iter().map(|i| format!("  {}", issue_text(i))));
    }
    if let Some(store) = &report.general_store
        && !store.issues().is_empty()
    {
        lines.push("General store issues:".to_string());
        lines.extend(
            store
                .issues()
                .iter()
                .map(|i| format!("  {}", issue_text(i))),
        );
    }
    if let Some(store) = &report.general_store {
        lines.push("General store entries:".to_string());
        if store.entries().is_empty() {
            lines.push("  None".to_string());
        }
        lines.extend(
            store
                .entries()
                .iter()
                .map(|entry| format!("  {}", entry_text(entry))),
        );
    }
    lines.push("Bootloader settings:".to_string());
    let rows = [
        ("BOOT_FLAGS0", OTP_BOOT_FLAGS0_ROW, report.boot_flags0[0]),
        (
            "BOOT_FLAGS0_R1",
            OTP_BOOT_FLAGS0_R1_ROW,
            report.boot_flags0[1],
        ),
        (
            "BOOT_FLAGS0_R2",
            OTP_BOOT_FLAGS0_R2_ROW,
            report.boot_flags0[2],
        ),
        (
            "FLASH_DEVINFO",
            OTP_FLASH_DEVINFO_ROW,
            report.flash_devinfo.raw,
        ),
        (
            "FLASH_PARTITION_SLOT_SIZE",
            OTP_FLASH_PARTITION_SLOT_SIZE_ROW,
            report.flash_partition_slot_size.raw,
        ),
        (
            "USB_BOOT_FLAGS",
            OTP_ROW_USB_BOOT_FLAGS,
            report.usb_boot_flags[0],
        ),
        (
            "USB_BOOT_FLAGS_R1",
            OTP_ROW_USB_BOOT_FLAGS_R1,
            report.usb_boot_flags[1],
        ),
        (
            "USB_BOOT_FLAGS_R2",
            OTP_ROW_USB_BOOT_FLAGS_R2,
            report.usb_boot_flags[2],
        ),
        (
            "USB_WHITE_LABEL_ADDR",
            OTP_ROW_USB_WHITE_LABEL_DATA,
            report.usb_white_label_addr.raw,
        ),
    ];
    // The values line up after the longest name.
    let width = rows
        .iter()
        .map(|(name, ..)| name.len() + 1)
        .max()
        .unwrap_or(0);
    lines.extend(rows.iter().map(|(name, row, raw)| {
        let name = format!("{name}:");
        format!("  {row:#05x} {name:width$} {raw:#08x}")
    }));
    lines.push("Lock words:".to_string());
    lines.extend(report.locks.iter().map(|lock| {
        let state = match lock.raw {
            LOCK1_READ_ONLY => " (locked)",
            0 => " (unlocked)",
            _ => "",
        };
        format!("  Page {:>2}: {:#08x}{state}", lock.page, lock.raw)
    }));
    lines
}

/// One entry of a list as `Row ROW: ENTRY`. A known key has OTP.md's name.
fn entry_text(entry: &StoreEntry) -> String {
    match entry {
        StoreEntry::Entry { row, entry } => {
            let entry = match entry {
                OneromOtpEntry::OtpKeyCommissioningBoard { name } => {
                    format!("COMMISSIONING_BOARD {}", escape_controls(name))
                }
                OneromOtpEntry::OtpKeyCommissioningManufacturer { name } => {
                    format!("COMMISSIONING_MANUFACTURER {}", escape_controls(name))
                }
                OneromOtpEntry::OtpKeyCommissioningDate { date } => {
                    format!("COMMISSIONING_DATE {}", escape_controls(date))
                }
                OneromOtpEntry::OtpKeyCommissioningSigner { id } => {
                    format!("COMMISSIONING_SIGNER {id}")
                }
                OneromOtpEntry::OtpKeyCommissioningSig { signature } => {
                    format!("COMMISSIONING_SIG {}", hex::encode(signature))
                }
                OneromOtpEntry::Unknown { key, params } => {
                    format!("unknown key {key}, {} bytes", params.len())
                }
            };
            format!("Row {row:#05x}: {entry}")
        }
        StoreEntry::Deleted { row, len } => format!("Row {row:#05x}: deleted, {len} bytes"),
        StoreEntry::Unreadable { row, key, len } => {
            format!("Row {row:#05x}: unreadable key {key}, {len} bytes")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::Signer as _;
    use onerom_app::{MemoryOtp, read_commissioning, read_report};
    use onerom_metadata::otp::pico_otp::ecc_encode;
    use onerom_metadata::otp::{CommissioningValues, NewCommissioningInstance};
    use onerom_metadata::{
        OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_GENERAL_STORE_FIRST_ROW, OTP_STORE_MAGIC,
    };

    use crate::test_board::{
        SIGNER_NAME, blank_board, commissioned_board, holds, holds_in_order, in_turn, shows, table,
    };

    fn board(name: &str) -> Board {
        Board::try_from_str(name).unwrap()
    }

    async fn area(otp: &mut MemoryOtp) -> Commissioning {
        Commissioning::Read(read_commissioning(otp).await.unwrap())
    }

    /// The area holding `rows` from row 0x0c0.
    fn area_of(rows: &[u16]) -> Commissioning {
        Commissioning::Read(CommissioningArea::parse(rows))
    }

    /// Writes `values` with ECC from `row`.
    fn put(otp: &mut MemoryOtp, row: u16, values: &[u16]) {
        for (row, &value) in (row..).zip(values) {
            otp.set_raw(row, ecc_encode(value));
        }
    }

    #[tokio::test]
    async fn a_commissioned_board_is_shown_without_its_signature_checked() {
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        let commissioning = area(&mut otp).await;
        let table = table(None);
        // A heading, then the board type, manufacturer and date a line each.
        let values: [&[&str]; 3] = [&["fire-24-f"], &["piers.rocks"], &["2026-01-01"]];
        let lines = device_lines(
            Some(board("fire-24-f")),
            &commissioning,
            (true, false),
            &table,
        );
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(in_turn(&lines[1..], &values), "{lines:?}");
        assert!(!shows(&lines, &[SIGNER_NAME]), "{lines:?}");
        // --verbose adds the signer but not the instance's row, which is
        // inspect otp's.
        let verbose = device_lines(
            Some(board("fire-24-f")),
            &commissioning,
            (true, true),
            &table,
        );
        assert_eq!(verbose.len(), 5, "{verbose:?}");
        assert_eq!(verbose[..4], lines[..], "{verbose:?}");
        assert!(holds(&verbose[4], &[SIGNER_NAME, "1"]), "{verbose:?}");
        assert!(!shows(&verbose, &["0x0c0"]), "{verbose:?}");
        // A blank board doesn't have firmware to compare with.
        assert_eq!(
            device_lines(None, &commissioning, (true, false), &table),
            lines
        );
    }

    #[tokio::test]
    async fn firmware_for_another_board_is_always_shown() {
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        let commissioning = area(&mut otp).await;
        let table = table(None);
        let lines = device_lines(
            Some(board("fire-24-e")),
            &commissioning,
            (true, false),
            &table,
        );
        // The commissioned board's lines and a line holding both boards.
        let matching = device_lines(
            Some(board("fire-24-f")),
            &commissioning,
            (true, false),
            &table,
        );
        assert_eq!(lines.len(), matching.len() + 1, "{lines:?}");
        assert_eq!(lines[..matching.len()], matching[..]);
        let last = lines.last().unwrap();
        assert!(holds(last, &["fire-24-f", "fire-24-e"]), "{lines:?}");
    }

    /// scan shows a line per board, so without --verbose it asks for the
    /// warnings alone.
    #[tokio::test]
    async fn without_details_only_warnings_are_shown() {
        let table = table(None);
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        let commissioning = area(&mut otp).await;
        let quiet = (false, false);
        // A board matching its firmware doesn't have anything to show.
        let lines = device_lines(Some(board("fire-24-f")), &commissioning, quiet, &table);
        assert!(lines.is_empty(), "{lines:?}");
        // Firmware for another board is shown.
        let lines = device_lines(Some(board("fire-24-e")), &commissioning, quiet, &table);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(holds(&lines[0], &["fire-24-f", "fire-24-e"]), "{lines:?}");
        // So is data from a newer version.
        let newer = device_lines(None, &area_of(&[OTP_STORE_MAGIC, 2]), quiet, &table);
        assert_eq!(newer.len(), 1, "{newer:?}");
    }

    #[tokio::test]
    async fn verbose_says_where_there_isnt_commissioning() {
        let table = table(None);
        let mut otp = blank_board();
        let commissioning = area(&mut otp).await;
        assert!(device_lines(None, &commissioning, (true, false), &table).is_empty());
        let not_commissioned = device_lines(None, &commissioning, (true, true), &table);
        assert_eq!(not_commissioned.len(), 1, "{not_commissioned:?}");
        assert!(device_lines(None, &Commissioning::Unreadable, (true, false), &table).is_empty());
        let unreadable = device_lines(None, &Commissioning::Unreadable, (true, true), &table);
        assert_eq!(unreadable.len(), 1, "{unreadable:?}");
        // The two cases are told apart.
        assert_ne!(not_commissioned, unreadable);
        assert!(device_lines(None, &Commissioning::NotRead, (true, true), &table).is_empty());
    }

    /// A running One ROM Lab's OTP isn't read. --verbose says so, apart from
    /// OTP that couldn't be read for another reason.
    #[test]
    fn verbose_says_a_running_lab_wasnt_read() {
        let table = table(None);
        let lab = Commissioning::LabRunning;
        assert!(device_lines(None, &lab, (false, false), &table).is_empty());
        assert!(device_lines(None, &lab, (true, false), &table).is_empty());
        let lines = device_lines(None, &lab, (true, true), &table);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let unreadable = device_lines(None, &Commissioning::Unreadable, (true, true), &table);
        assert_ne!(lines, unreadable);
    }

    #[test]
    fn newer_data_and_skipped_keys_are_always_shown() {
        let table = table(None);
        // The version and the row it's at.
        let newer = device_lines(None, &area_of(&[OTP_STORE_MAGIC, 2]), (true, false), &table);
        assert_eq!(newer.len(), 1, "{newer:?}");
        assert!(holds(&newer[0], &["2", "0x0c0"]), "{newer:?}");
        // --verbose shows the same line in place of the line saying a board
        // isn't commissioned.
        let verbose = device_lines(None, &area_of(&[OTP_STORE_MAGIC, 2]), (true, true), &table);
        assert_eq!(verbose, newer);
        let blank = device_lines(None, &area_of(&[0]), (true, true), &table);
        assert_ne!(verbose, blank);
        // An instance holding key 9. It ends before its signature. The line
        // holds the key, its length in bytes and its row.
        let skipped = area_of(&[OTP_STORE_MAGIC, 1, 9, 4, 0x0201, 0x0403, 0, 0]);
        let skipped = device_lines(None, &skipped, (true, false), &table);
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert!(holds(&skipped[0], &["9", "4", "0x0c2"]), "{skipped:?}");
    }

    #[test]
    fn otp_strings_are_shown_with_control_characters_escaped() {
        // CommissioningValues refuses a control character. The instance is
        // built with ? in its place and the row holding it is then changed.
        let values =
            CommissioningValues::new(board("fire-24-f"), "bad?[2Jname", "20260101", 1).unwrap();
        let signature = crate::test_board::key()
            .sign(&values.message(crate::test_board::CHIP_ID))
            .to_bytes();
        let instance =
            NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW).unwrap();
        let mut rows = vec![0; 64];
        for write in instance.writes(&signature) {
            rows[usize::from(write.row - OTP_COMMISSIONING_AREA_FIRST_ROW)] = write.value;
        }
        let placeholder = u16::from_le_bytes(*b"d?");
        let row = rows.iter().position(|&row| row == placeholder).unwrap();
        rows[row] = u16::from_le_bytes([b'd', 0x1b]);
        let lines = device_lines(None, &area_of(&rows), (true, false), &table(None));
        assert!(shows(&lines, &["bad\\u{1b}[2Jname"]), "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.contains('\u{1b}')),
            "{lines:?}"
        );
    }

    #[tokio::test]
    async fn an_l_boards_otp_is_shown() {
        let mut otp = commissioned_board("fire-40-a", BoardSize::L).await;
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, false, &table(None));
        println!("{}", lines.join("\n"));
        // The size, then the instance's four lines and the VID, the PID and
        // the seven strings beneath their headings. FLASH_DEVINFO's fields,
        // the row and the page's lock are --verbose, and the general store
        // hasn't been started.
        assert_eq!(lines.len(), 1 + 5 + 10, "{lines:?}");
        assert!(holds(&lines[0], &["L"]), "{lines:?}");
        // The current instance's values and signer.
        let instance: [&[&str]; 4] = [
            &["fire-40-a"],
            &["piers.rocks"],
            &["2026-01-01"],
            &[SIGNER_NAME, "1"],
        ];
        assert!(in_turn(&lines, &instance), "{lines:?}");
        // The bootloader USB IDs, then the strings in OTP.md's order.
        let strings = [
            "0x1209",
            "0xF540",
            "piers.rocks",
            "One ROM Bootloader",
            "ONEROM",
            "https://onerom.org",
            "onerom.org",
            "One ROM",
            "fire-40-a",
        ];
        let strings: Vec<&[&str]> = strings.iter().map(std::slice::from_ref).collect();
        assert!(in_turn(&lines, &strings), "{lines:?}");
    }

    #[tokio::test]
    async fn a_blank_boards_otp_is_shown() {
        let mut otp = blank_board();
        let report = read_report(&mut otp).await.unwrap();
        // Size, commissioning and the bootloader USB info, a line each.
        let lines = report_lines(&report, false, &table(None));
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(holds(&lines[0], &["M"]), "{lines:?}");

        // --json shows that the white label isn't decoded and doesn't have an
        // error or warnings.
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["white_label"], serde_json::Value::Null);
        assert_eq!(json["white_label_error"], serde_json::Value::Null);
        assert_eq!(json["white_label_warnings"], serde_json::json!([]));
    }

    /// Sets USB_BOOT_FLAGS and its two copies to `flags`.
    fn set_usb_boot_flags(otp: &mut MemoryOtp, flags: u32) {
        for row in 0x059..=0x05b {
            otp.set_raw(row, flags);
        }
    }

    /// pico-otp warns about the white label written without USB_BOOT_FLAGS. An
    /// interrupted commission can leave it this way.
    #[tokio::test]
    async fn pico_otps_warnings_are_always_shown() {
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        set_usb_boot_flags(&mut otp, 0);
        let report = read_report(&mut otp).await.unwrap();
        let warnings = &report.white_label_warnings;
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("WHITE_LABEL_ADDR_VALID")),
            "{warnings:?}"
        );
        let lines = report_lines(&report, false, &table(None));
        println!("{}", lines.join("\n"));
        // A line for each warning follows the bootloader USB info and
        // ends the output.
        let last = &lines[lines.len() - warnings.len()..];
        for (line, warning) in last.iter().zip(warnings) {
            assert!(holds(line, &[&escape_controls(warning)]), "{lines:?}");
        }

        // --json shows them too.
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["white_label_error"], serde_json::Value::Null);
        assert_eq!(
            json["white_label_warnings"],
            serde_json::json!(report.white_label_warnings)
        );
    }

    /// pico-otp's non-strict decoding doesn't refuse the rows `read_report`
    /// passes it. It drops a bad field with a warning instead. So this test
    /// puts a refusal in the report.
    #[tokio::test]
    async fn pico_otps_refusal_is_shown() {
        let mut otp = blank_board();
        let mut report = read_report(&mut otp).await.unwrap();
        report.white_label = None;
        report.white_label_error = Some("Invalid white label data: row\u{7}".to_string());
        let lines = report_lines(&report, false, &table(None));
        // The bootloader USB info line, the third, holds pico-otp's reason
        // with its control characters escaped.
        assert!(
            holds(&lines[2], &["Invalid white label data: row\\u{7}"]),
            "{lines:?}"
        );
        assert!(!lines[2].contains('\u{7}'), "{lines:?}");
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json["white_label_error"],
            "Invalid white label data: row\u{7}"
        );
    }

    #[tokio::test]
    async fn verbose_shows_every_entry_row_and_lock() {
        let mut otp = commissioned_board("fire-40-a", BoardSize::L).await;
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, true, &table(None));
        println!("{}", lines.join("\n"));
        // FLASH_DEVINFO's fields beneath the board size, which BOOT_FLAGS0
        // enables.
        let devinfo = labelled_lines(&devinfo_values(report.flash_devinfo, true));
        assert_eq!(lines[1..1 + devinfo.len()], devinfo[..], "{lines:?}");
        // The current instance's signer, row and page lock.
        let instance: [&[&str]; 3] = [&[SIGNER_NAME, "1"], &["0x0c0"], &["3", "locked"]];
        assert!(in_turn(&lines, &instance), "{lines:?}");
        // The instance's line, which says it's current, and then its entries,
        // a line each.
        assert!(shows(&lines, &["0x0c0", "current"]), "{lines:?}");
        let signature = report.commissioning.current().unwrap().signature().unwrap();
        let signature = hex::encode(signature);
        assert!(
            in_turn(
                &lines,
                &[
                    &["0x0c0"],
                    &["0x0c2", "COMMISSIONING_BOARD", "fire-40-a"],
                    &["0x0c9", "COMMISSIONING_MANUFACTURER", "piers.rocks"],
                    &["0x0d1", "COMMISSIONING_DATE", "20260101"],
                    &["0x0d7", "COMMISSIONING_SIGNER", "1"],
                    &["0x0da", "COMMISSIONING_SIG", &signature],
                ]
            ),
            "{lines:?}"
        );
        // Rows and lock words.
        for values in [
            ["0x048", "BOOT_FLAGS0", "0x000020"].as_slice(),
            &["0x054", "FLASH_DEVINFO", "0x3a99af"],
            &["0x059", "USB_BOOT_FLAGS", "0x40f133"],
            &["3", "0x151515"],
            &["4", "0x000000"],
        ] {
            assert!(shows(&lines, values), "{values:?}\n{lines:?}");
        }
    }

    #[tokio::test]
    async fn unknown_keys_and_versions_are_always_shown() {
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        // A general store holding key 9.
        put(
            &mut otp,
            OTP_GENERAL_STORE_FIRST_ROW,
            &[OTP_STORE_MAGIC, 1, 9, 2, 0x0201],
        );
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, false, &table(None));
        // The key, its length in bytes and its row.
        assert!(shows(&lines, &["9", "2", "0x4c2"]), "{lines:?}");
        // The general store's line, the last, holds its entry count.
        assert!(holds(lines.last().unwrap(), &["1"]), "{lines:?}");

        let mut otp = blank_board();
        put(&mut otp, OTP_GENERAL_STORE_FIRST_ROW, &[OTP_STORE_MAGIC, 2]);
        put(
            &mut otp,
            OTP_COMMISSIONING_AREA_FIRST_ROW,
            &[OTP_STORE_MAGIC, 3],
        );
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, false, &table(None));
        // The commissioning area's line, the second, holds its version and
        // row. The general store's line, the last, holds its version.
        assert!(holds(&lines[1], &["3", "0x0c0"]), "{lines:?}");
        assert!(holds(lines.last().unwrap(), &["2"]), "{lines:?}");
    }

    #[test]
    fn a_flash_size_is_4kb_shifted_left() {
        assert_eq!(flash_size(1), "8KB");
        assert_eq!(flash_size(8), "1MB");
        assert_eq!(flash_size(9), "2MB");
        assert_eq!(flash_size(12), "16MB");
        // 0 is a chip select without a chip so it doesn't have a size.
        let none = flash_size(0);
        assert!(!none.contains(|c: char| c.is_ascii_digit()), "{none}");
    }

    #[test]
    fn flash_devinfo_says_what_it_holds() {
        let row = |raw| EccRow {
            raw,
            value: Some(raw as u16),
        };
        let text = |devinfo, enabled| labelled_lines(&devinfo_values(devinfo, enabled)).join("\n");
        // 2MB on each chip select, the second on GPIO47, with D8h erase, not
        // enabled.
        let l = text(row(0x99af), false);
        assert!(
            holds_in_order(&l, &["0x99af", "no", "2MB", "2MB", "47", "yes"]),
            "{l}"
        );
        // 2MB on chip select 0 alone, without D8h erase, enabled.
        let m = text(row(0x0900), true);
        assert!(
            holds_in_order(&m, &["0x0900", "yes", "2MB", &flash_size(0), "0", "no"]),
            "{m}"
        );
        // It says whether BOOT_FLAGS0 enables it.
        assert_ne!(m, text(row(0x0900), false));
        // An unwritten row's raw value isn't shown.
        let unwritten = EccRow {
            raw: 0,
            value: None,
        };
        let blank = text(unwritten, true);
        assert!(!holds(&blank, &["0x000000"]), "{blank}");
        assert_ne!(blank, text(unwritten, false));
        // An unwritten row reads as 0 with ECC. It's unwritten rather than
        // chip selects without chips.
        let read = EccRow {
            raw: 0,
            value: Some(0),
        };
        assert_eq!(text(read, true), blank);
        // A damaged row's raw value is shown.
        let damaged = EccRow {
            raw: 0x3a99ae,
            value: None,
        };
        let damaged = text(damaged, false);
        assert!(holds(&damaged, &["0x3a99ae"]), "{damaged}");
    }
}
