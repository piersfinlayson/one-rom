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

/// `instance`'s values as `BOARD by MANUFACTURER on DATE`. A value the
/// instance doesn't have is left out.
pub fn instance_values(instance: &CommissioningInstance) -> String {
    let values: Vec<String> = [
        instance.board().map(escape_controls),
        instance
            .manufacturer()
            .map(|name| format!("by {}", escape_controls(name))),
        instance
            .date()
            .map(|date| format!("on {}", format_date(date))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if values.is_empty() {
        "values missing".to_string()
    } else {
        values.join(" ")
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

// ---------------------------------------------------------------------------
// scan and inspect info
// ---------------------------------------------------------------------------

/// The lines `scan` and `inspect info` show beneath a device's line.
/// `firmware` is the board the device's firmware is for.
///
/// - The current instance's values.
/// - With `verbose`, its signer and row.
/// - With `verbose`, a line saying there isn't a current instance or that OTP
///   couldn't be read.
/// - Each unknown version and skipped key.
/// - A board that differs from the firmware's.
///
/// They don't say whether a signature is valid.
pub fn device_lines(
    firmware: Option<Board>,
    commissioning: &Commissioning,
    verbose: bool,
    table: &SignerTable,
) -> Vec<String> {
    let area = match commissioning {
        Commissioning::Read(area) => area,
        Commissioning::Unreadable if verbose => return vec!["OTP not readable".to_string()],
        Commissioning::Unreadable | Commissioning::NotRead => return Vec::new(),
    };
    let mut lines = Vec::new();
    match area.current() {
        Some(current) => {
            let mut line = format!("Commissioned: {}", instance_values(current));
            if verbose && let Some(signer) = current.signer() {
                line.push_str(&format!(
                    ", signer {}, row {:#05x}",
                    signer_name(signer, table),
                    current.first_row()
                ));
            }
            lines.push(line);
            if let (Some(commissioned), Some(firmware)) = (current.board(), firmware)
                && Board::try_from_str(commissioned) != Some(firmware)
            {
                lines.push(format!(
                    "Commissioned as {} but the firmware is for {}",
                    escape_controls(commissioned),
                    firmware.name()
                ));
            }
        }
        None if verbose => lines.push("Not commissioned".to_string()),
        None => {}
    }
    lines.extend(newer_data(area));
    lines.extend(skipped_keys(area));
    lines
}

/// A line for each unknown version in `area`.
fn newer_data(area: &CommissioningArea) -> impl Iterator<Item = String> + '_ {
    area.issues().iter().filter_map(|issue| match issue {
        AreaIssue::UnknownVersion { .. } => Some(format!("Commissioning: {}", issue_text(issue))),
        AreaIssue::LostPlace { .. } => None,
    })
}

/// A line for each entry in `area` whose key this build doesn't know.
fn skipped_keys(area: &CommissioningArea) -> impl Iterator<Item = String> + '_ {
    area.instances()
        .iter()
        .flat_map(|instance| unknown_keys(instance.entries()))
        .map(|unknown| format!("Skipped unknown {}", unknown.text()))
}

// ---------------------------------------------------------------------------
// inspect otp
// ---------------------------------------------------------------------------

/// The lines `inspect otp` shows for `report`.
///
/// These are always shown:
/// - unknown keys
/// - an unknown general store version
/// - pico-otp's reason it can't decode the white label and its warnings
///
/// `verbose` adds:
/// - every instance and entry
/// - the issues
/// - the raw rows
/// - the lock words
pub fn report_lines(report: &OtpReport, verbose: bool, table: &SignerTable) -> Vec<String> {
    let area = &report.commissioning;
    let mut lines = vec![
        format!("Size:          {}", size_text(report)),
        format!("Commissioning: {}", commissioning_text(report, table)),
    ];
    if !verbose {
        let instances = area.instances().iter();
        let general_store = report.general_store.iter();
        lines.extend(
            instances
                .flat_map(|instance| unknown_keys(instance.entries()))
                .chain(general_store.flat_map(|store| unknown_keys(store.entries())))
                .map(|unknown| format!("Skipped:       unknown {}", unknown.text())),
        );
    }
    lines.push(format!("White label:   {}", white_label_text(report)));
    lines.extend(
        report
            .white_label_warnings
            .iter()
            .map(|warning| format!("               Warning: {}", escape_controls(warning))),
    );
    lines.push(format!("General store: {}", general_store_text(report)));
    if verbose {
        lines.extend(verbose_lines(report));
    }
    lines
}

/// The board size and the FLASH_DEVINFO it comes from.
fn size_text(report: &OtpReport) -> String {
    let enabled = majority(report.boot_flags0) & OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE != 0;
    match report.size {
        Some(BoardSize::M) => "M".to_string(),
        Some(BoardSize::L) => format!("L ({})", devinfo_text(report.flash_devinfo, enabled)),
        None => format!(
            "neither M nor L ({})",
            devinfo_text(report.flash_devinfo, enabled)
        ),
    }
}

/// FLASH_DEVINFO's fields and whether BOOT_FLAGS0 enables it.
fn devinfo_text(devinfo: EccRow, enabled: bool) -> String {
    let state = if enabled { "enabled" } else { "not enabled" };
    let Some(value) = devinfo.value else {
        return if devinfo.raw.count_ones() <= 1 {
            format!("FLASH_DEVINFO unwritten, {state}")
        } else {
            format!(
                "FLASH_DEVINFO raw {:#08x} isn't a valid ECC value, {state}",
                devinfo.raw
            )
        };
    };
    let cs0 = flash_size((value >> FLASH_DEVINFO_CS0_SIZE_SHIFT) & FLASH_DEVINFO_SIZE_BITS);
    let cs1 = flash_size((value >> FLASH_DEVINFO_CS1_SIZE_SHIFT) & FLASH_DEVINFO_SIZE_BITS);
    let gpio = value & FLASH_DEVINFO_CS1_GPIO;
    let erase = if value & FLASH_DEVINFO_D8H_ERASE_SUPPORTED != 0 {
        ", D8h erase"
    } else {
        ""
    };
    format!("FLASH_DEVINFO {value:#06x} {state}: CS0 {cs0}, CS1 {cs1} on GPIO{gpio}{erase}")
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

/// The current instance with its signer and locks or the reason there isn't
/// one.
fn commissioning_text(report: &OtpReport, table: &SignerTable) -> String {
    let area = &report.commissioning;
    if let Some(issue) = area
        .issues()
        .iter()
        .find(|issue| matches!(issue, AreaIssue::UnknownVersion { .. }))
    {
        return issue_text(issue);
    }
    let Some(current) = area.current() else {
        return "not commissioned".to_string();
    };
    let signer = current
        .signer()
        .map(|id| format!(", signer {}", signer_name(id, table)))
        .unwrap_or_default();
    format!(
        "{}{signer}, row {:#05x}, {}",
        instance_values(current),
        current.first_row(),
        locks_text(current, &report.locks)
    )
}

/// Whether each page of `instance` is locked.
fn locks_text(instance: &CommissioningInstance, locks: &[PageLock]) -> String {
    let first = instance.first_row() / OTP_PAGE_ROWS;
    let last = last_row(instance) / OTP_PAGE_ROWS;
    (first..=last)
        .map(|page| {
            let state = match locks.iter().find(|lock| lock.page == page).map(|l| l.raw) {
                Some(LOCK1_READ_ONLY) => "locked".to_string(),
                Some(0) => "not locked".to_string(),
                Some(raw) => format!("lock word {raw:#08x}"),
                None => "lock not read".to_string(),
            };
            format!("page {page} {state}")
        })
        .collect::<Vec<_>>()
        .join(", ")
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

/// The seven bootloader USB strings One ROM sets. Each is a section and a
/// field of picotool's JSON.
const WHITE_LABEL_STRINGS: [(&str, &str); 7] = [
    ("device", "manufacturer"),
    ("device", "product"),
    ("volume", "label"),
    ("volume", "redirect_url"),
    ("volume", "redirect_name"),
    ("volume", "model"),
    ("volume", "board_id"),
];

/// The bootloader USB strings One ROM sets. They're in OTP.md's order. Where
/// pico-otp can't decode them the text carries pico-otp's reason.
fn white_label_text(report: &OtpReport) -> String {
    let Some(json) = &report.white_label else {
        return match &report.white_label_error {
            Some(error) => format!("can't be decoded: {}", escape_controls(error)),
            // The white label is unwritten so read_report didn't decode it.
            None => "not set".to_string(),
        };
    };
    let strings = WHITE_LABEL_STRINGS.map(|(section, field)| {
        json.get(section)
            .and_then(|section| section.get(field))
            .and_then(serde_json::Value::as_str)
    });
    if strings.iter().all(Option::is_none) {
        return "not set".to_string();
    }
    strings
        .map(|string| string.map_or("-".to_string(), escape_controls))
        .join(" / ")
}

/// The general store's entry count and issues. `not started` where it hasn't
/// been started.
fn general_store_text(report: &OtpReport) -> String {
    let Some(store) = &report.general_store else {
        return "not started".to_string();
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
    lines.push("Rows:".to_string());
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
    lines.extend(
        rows.iter()
            .map(|(name, row, raw)| format!("  {row:#05x} {name}: {raw:#08x}")),
    );
    lines.push("Lock words:".to_string());
    lines.extend(
        report
            .locks
            .iter()
            .map(|lock| format!("  Page {}: {:#08x}", lock.page, lock.raw)),
    );
    lines
}

/// `instance`'s state. `current` is whether it's the current instance.
fn instance_state(instance: &CommissioningInstance, current: bool) -> &'static str {
    if current {
        "current"
    } else if !instance.is_complete() {
        "incomplete"
    } else if !instance.is_valid() {
        "missing values"
    } else {
        "earlier"
    }
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

    use crate::test_board::{blank_board, commissioned_board, table};

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
        assert_eq!(
            device_lines(Some(board("fire-24-f")), &commissioning, false, &table),
            ["Commissioned: fire-24-f by piers.rocks on 2026-01-01"]
        );
        assert_eq!(
            device_lines(Some(board("fire-24-f")), &commissioning, true, &table),
            [
                "Commissioned: fire-24-f by piers.rocks on 2026-01-01, signer test signer (1), row 0x0c0"
            ]
        );
        // A blank board doesn't have firmware to compare with.
        assert_eq!(
            device_lines(None, &commissioning, false, &table),
            ["Commissioned: fire-24-f by piers.rocks on 2026-01-01"]
        );
    }

    #[tokio::test]
    async fn firmware_for_another_board_is_always_shown() {
        let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
        let commissioning = area(&mut otp).await;
        assert_eq!(
            device_lines(
                Some(board("fire-24-e")),
                &commissioning,
                false,
                &table(None)
            ),
            [
                "Commissioned: fire-24-f by piers.rocks on 2026-01-01",
                "Commissioned as fire-24-f but the firmware is for fire-24-e",
            ]
        );
    }

    #[tokio::test]
    async fn verbose_says_where_there_isnt_commissioning() {
        let table = table(None);
        let mut otp = blank_board();
        let commissioning = area(&mut otp).await;
        assert!(device_lines(None, &commissioning, false, &table).is_empty());
        assert_eq!(
            device_lines(None, &commissioning, true, &table),
            ["Not commissioned"]
        );
        assert!(device_lines(None, &Commissioning::Unreadable, false, &table).is_empty());
        assert_eq!(
            device_lines(None, &Commissioning::Unreadable, true, &table),
            ["OTP not readable"]
        );
        assert!(device_lines(None, &Commissioning::NotRead, true, &table).is_empty());
    }

    #[test]
    fn newer_data_and_skipped_keys_are_always_shown() {
        let table = table(None);
        let newer = area_of(&[OTP_STORE_MAGIC, 2]);
        assert_eq!(
            device_lines(None, &newer, false, &table),
            ["Commissioning: unknown version 2 at row 0x0c0"]
        );
        // An instance holding key 9. It ends before its signature.
        let skipped = area_of(&[OTP_STORE_MAGIC, 1, 9, 4, 0x0201, 0x0403, 0, 0]);
        assert_eq!(
            device_lines(None, &skipped, false, &table),
            ["Skipped unknown key 9 (4 bytes) at row 0x0c2"]
        );
    }

    #[test]
    fn otp_strings_are_shown_with_control_characters_escaped() {
        let values =
            CommissioningValues::new(board("fire-24-f"), "bad\u{1b}[2Jname", "20260101", 1)
                .unwrap();
        let signature = crate::test_board::key()
            .sign(&values.message(crate::test_board::CHIP_ID))
            .to_bytes();
        let instance =
            NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW).unwrap();
        let mut rows = vec![0; 64];
        for write in instance.writes(&signature) {
            rows[usize::from(write.row - OTP_COMMISSIONING_AREA_FIRST_ROW)] = write.value;
        }
        assert_eq!(
            device_lines(None, &area_of(&rows), false, &table(None)),
            ["Commissioned: fire-24-f by bad\\u{1b}[2Jname on 2026-01-01"]
        );
    }

    #[tokio::test]
    async fn an_l_boards_otp_is_shown() {
        let mut otp = commissioned_board("fire-40-a", BoardSize::L).await;
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, false, &table(None));
        println!("{}", lines.join("\n"));
        assert_eq!(
            lines,
            [
                "Size:          L (FLASH_DEVINFO 0x99af enabled: CS0 2MB, CS1 2MB on GPIO47, D8h erase)",
                "Commissioning: fire-40-a by piers.rocks on 2026-01-01, signer test signer (1), row 0x0c0, page 3 locked",
                "White label:   piers.rocks / One ROM Bootloader / ONEROM / https://onerom.org / onerom.org / One ROM / fire-40-a",
                "General store: not started",
            ]
        );
    }

    #[tokio::test]
    async fn a_blank_boards_otp_is_shown() {
        let mut otp = blank_board();
        let report = read_report(&mut otp).await.unwrap();
        assert_eq!(
            report_lines(&report, false, &table(None)),
            [
                "Size:          M",
                "Commissioning: not commissioned",
                "White label:   not set",
                "General store: not started",
            ]
        );

        // --json shows that the white label isn't decoded and doesn't have an
        // error or warnings.
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["white_label"], serde_json::Value::Null);
        assert_eq!(json["white_label_error"], serde_json::Value::Null);
        assert_eq!(json["white_label_warnings"], serde_json::json!([]));
    }

    /// The lines showing `report`'s white label warnings.
    fn warning_lines(report: &OtpReport) -> Vec<String> {
        report
            .white_label_warnings
            .iter()
            .map(|warning| format!("               Warning: {warning}"))
            .collect()
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
        assert!(!report.white_label_warnings.is_empty());
        let lines = report_lines(&report, false, &table(None));
        println!("{}", lines.join("\n"));
        let white_label = lines
            .iter()
            .position(|line| line == "White label:   not set")
            .unwrap_or_else(|| panic!("{lines:?}"));
        let warnings = warning_lines(&report);
        assert_eq!(
            lines[white_label + 1..white_label + 1 + warnings.len()],
            warnings
        );
        assert!(
            warnings
                .iter()
                .any(|line| line.contains("WHITE_LABEL_ADDR_VALID")),
            "{warnings:?}"
        );

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
        assert!(
            lines.contains(
                &"White label:   can't be decoded: Invalid white label data: row\\u{7}".to_string()
            ),
            "{lines:?}"
        );
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
        for expected in [
            "Commissioning instances:",
            "  Instance at row 0x0c0 (current)",
            "    Row 0x0c2: COMMISSIONING_BOARD fire-40-a",
            "    Row 0x0c9: COMMISSIONING_MANUFACTURER piers.rocks",
            "    Row 0x0d1: COMMISSIONING_DATE 20260101",
            "    Row 0x0d7: COMMISSIONING_SIGNER 1",
            "Rows:",
            "  0x048 BOOT_FLAGS0: 0x000020",
            "  0x054 FLASH_DEVINFO: 0x3a99af",
            "  0x059 USB_BOOT_FLAGS: 0x40f130",
            "Lock words:",
            "  Page 3: 0x151515",
            "  Page 4: 0x000000",
        ] {
            assert!(lines.iter().any(|line| line == expected), "{expected}");
        }
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("    Row 0x0da: COMMISSIONING_SIG ")),
            "{lines:?}"
        );
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
        assert!(
            lines.contains(&"Skipped:       unknown key 9 (2 bytes) at row 0x4c2".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"General store: 1 entry".to_string()),
            "{lines:?}"
        );

        let mut otp = blank_board();
        put(&mut otp, OTP_GENERAL_STORE_FIRST_ROW, &[OTP_STORE_MAGIC, 2]);
        put(
            &mut otp,
            OTP_COMMISSIONING_AREA_FIRST_ROW,
            &[OTP_STORE_MAGIC, 3],
        );
        let report = read_report(&mut otp).await.unwrap();
        let lines = report_lines(&report, false, &table(None));
        assert!(
            lines.contains(&"Commissioning: unknown version 3 at row 0x0c0".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"General store: unknown version 2".to_string()),
            "{lines:?}"
        );
    }

    #[test]
    fn a_flash_size_is_4kb_shifted_left() {
        assert_eq!(flash_size(0), "none");
        assert_eq!(flash_size(1), "8KB");
        assert_eq!(flash_size(8), "1MB");
        assert_eq!(flash_size(9), "2MB");
        assert_eq!(flash_size(12), "16MB");
    }

    #[test]
    fn flash_devinfo_says_what_it_holds() {
        let row = |raw| EccRow {
            raw,
            value: Some(raw as u16),
        };
        assert_eq!(
            devinfo_text(row(0x99af), false),
            "FLASH_DEVINFO 0x99af not enabled: CS0 2MB, CS1 2MB on GPIO47, D8h erase"
        );
        assert_eq!(
            devinfo_text(row(0x0900), true),
            "FLASH_DEVINFO 0x0900 enabled: CS0 2MB, CS1 none on GPIO0"
        );
        let unwritten = EccRow {
            raw: 0,
            value: None,
        };
        assert_eq!(
            devinfo_text(unwritten, true),
            "FLASH_DEVINFO unwritten, enabled"
        );
        let damaged = EccRow {
            raw: 0x3a99ae,
            value: None,
        };
        assert_eq!(
            devinfo_text(damaged, false),
            "FLASH_DEVINFO raw 0x3a99ae isn't a valid ECC value, not enabled"
        );
    }
}
