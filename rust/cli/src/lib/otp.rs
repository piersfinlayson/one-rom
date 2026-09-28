// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! An RP2350's OTP reached over PICOBOOT and the commissioning it holds.

use log::debug;
use onerom_app::{OtpAccess, OtpError, read_commissioning};
use onerom_metadata::otp::{CommissioningArea, CommissioningInstance};
use onerom_metadata::{MaybeKnown, OTP_PAGE_ROWS, OneromBoardSize};
use picoboot::cmd::PicobootStatus;
use picoboot::{Connection, Picoboot};

use crate::usb::get_picoboot;
use crate::{Device, Error};

/// Rows in OTP.
const OTP_ROWS: u32 = 4096;

/// Page 61's first row. Pages 61–63 are read a row at a time.
const PAGE_61_FIRST_ROW: u16 = 61 * OTP_PAGE_ROWS;

/// An RP2350's OTP reached over PICOBOOT.
///
/// Access permissions are set per page so a read command stays within a page.
/// Pages 61–63 have special permissions so they're read a row at a time.
/// picotool's `otp dump` reads the same way.
pub struct PicobootOtp {
    picoboot: Picoboot,
}

impl PicobootOtp {
    /// Opens `device`'s PICOBOOT interface.
    pub async fn open(device: &Device) -> Result<Self, Error> {
        let mut picoboot = get_picoboot(device, false).await?;
        // A previous operation can leave an endpoint halted.
        picoboot
            .connect()
            .await
            .map_err(|e| Error::Usb(e.to_string()))?
            .reset_interface()
            .await
            .map_err(|e| Error::Usb(e.to_string()))?;
        Ok(Self { picoboot })
    }

    async fn connection(&mut self) -> Result<&mut Connection, OtpError> {
        self.picoboot
            .connect()
            .await
            .map_err(|e| OtpError::Transport(e.to_string()))
    }
}

impl OtpAccess for PicobootOtp {
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError> {
        let commands = read_commands(row, count)?;
        let conn = self.connection().await?;
        let mut rows = Vec::with_capacity(usize::from(count));
        for (row, count) in commands {
            match conn.otp_read_ecc(row, count).await {
                Ok(read) => rows.extend(read),
                Err(e) => return Err(failure(conn, e).await),
            }
        }
        Ok(rows)
    }

    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError> {
        let commands = read_commands(row, count)?;
        let conn = self.connection().await?;
        let mut rows = Vec::with_capacity(usize::from(count));
        for (row, count) in commands {
            match conn.otp_read_raw(row, count).await {
                Ok(read) => rows.extend(read),
                Err(e) => return Err(failure(conn, e).await),
            }
        }
        Ok(rows)
    }

    async fn write_ecc(&mut self, row: u16, value: u16) -> Result<(), OtpError> {
        let conn = self.connection().await?;
        match conn.otp_write_ecc(row, &[value]).await {
            Ok(()) => Ok(()),
            Err(e) => Err(failure(conn, e).await),
        }
    }

    async fn write_raw(&mut self, row: u16, value: u32) -> Result<(), OtpError> {
        let conn = self.connection().await?;
        match conn.otp_write_raw(row, &[value]).await {
            Ok(()) => Ok(()),
            Err(e) => Err(failure(conn, e).await),
        }
    }
}

/// The first row and row count of each command that reads `count` rows from
/// `row`. Refuses rows past the end of OTP.
fn read_commands(row: u16, count: u16) -> Result<Vec<(u16, u16)>, OtpError> {
    let end = u32::from(row) + u32::from(count);
    if end > OTP_ROWS {
        return Err(OtpError::Transport(format!(
            "{count} rows from row {row:#05x} run past the end of OTP"
        )));
    }
    let page_rows = u32::from(OTP_PAGE_ROWS);
    let mut commands = Vec::new();
    let mut first = u32::from(row);
    while first < end {
        let limit = if first >= u32::from(PAGE_61_FIRST_ROW) {
            first + 1
        } else {
            (first / page_rows + 1) * page_rows
        };
        let next = limit.min(end);
        // Both fit in a u16 because `end` is at most 4096.
        commands.push((first as u16, (next - first) as u16));
        first = next;
    }
    Ok(commands)
}

/// The error for command failure `e`.
///
/// A device's refusal arrives as a failed transfer and its reason is in the
/// device's command status. Resetting the interface clears the status so it's
/// read first.
async fn failure(conn: &mut Connection, e: picoboot::Error) -> OtpError {
    let status = conn
        .get_command_status()
        .await
        .inspect_err(|e| debug!("Couldn't read the OTP command's status: {e}"))
        .ok()
        .map(|status| status.get_status_code());
    debug!("OTP command failed: {e} (status {status:?})");
    // The next command needs a clean endpoint. If the reset fails, that
    // command fails too.
    conn.reset_interface().await.ok();
    otp_error(status, e.to_string())
}

/// The [`OtpError`] for a failed command. `status` is the device's command
/// status and `detail` the transport's error.
fn otp_error(status: Option<PicobootStatus>, detail: String) -> OtpError {
    match status {
        Some(PicobootStatus::NotPermitted) => OtpError::NotPermitted,
        Some(PicobootStatus::UnsupportedModification) => OtpError::UnsupportedModification,
        Some(PicobootStatus::Ok) | None => OtpError::Transport(detail),
        Some(status) => OtpError::Transport(format!("{detail} (device status {status:?})")),
    }
}

/// A device's commissioning area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Commissioning {
    /// The area hasn't been read.
    NotRead,
    /// The area couldn't be read.
    Unreadable,
    /// The area as read.
    Read(CommissioningArea),
}

impl Commissioning {
    /// Reads `device`'s commissioning area. A failure is logged at debug level.
    pub async fn read(device: &Device) -> Self {
        let area = match PicobootOtp::open(device).await {
            Ok(mut otp) => read_commissioning(&mut otp).await.map_err(Error::from),
            Err(e) => Err(e),
        };
        match area {
            Ok(area) => Self::Read(area),
            Err(e) => {
                debug!("Couldn't read the commissioning area of {device}: {e}");
                Self::Unreadable
            }
        }
    }

    /// The current commissioning instance.
    pub(crate) fn current(&self) -> Option<&CommissioningInstance> {
        match self {
            Self::Read(area) => area.current(),
            Self::NotRead | Self::Unreadable => None,
        }
    }
}

/// Reads the board size `device`'s OTP configures. A failure is logged at debug
/// level.
pub(crate) async fn read_board_size(device: &Device) -> Option<OneromBoardSize> {
    let size = match PicobootOtp::open(device).await {
        Ok(mut otp) => onerom_app::read_board_size(&mut otp)
            .await
            .map_err(Error::from),
        Err(e) => Err(e),
    };
    match size {
        Ok(size) => Some(size),
        Err(e) => {
            debug!("Couldn't read the board size of {device}: {e}");
            None
        }
    }
}

/// The value a `Board size:` line shows for `size`.
pub fn board_size_text(size: MaybeKnown<OneromBoardSize>) -> String {
    match size {
        MaybeKnown::Known(OneromBoardSize::BoardSizeM) => "M".to_string(),
        MaybeKnown::Known(OneromBoardSize::BoardSizeL) => "L".to_string(),
        MaybeKnown::Known(OneromBoardSize::BoardSizeOther) => "neither M nor L".to_string(),
        // Firmware before 0.8.0 doesn't record a size.
        MaybeKnown::Known(OneromBoardSize::BoardSizeUnknown) => "unknown".to_string(),
        // A size newer firmware recorded, with its number.
        MaybeKnown::Unknown(_) => size.to_string(),
    }
}

/// `text` with each control character escaped. OTP strings are printed this
/// way because a board's OTP can hold any bytes.
pub fn escape_controls(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// A `YYYYMMDD` commissioning date as `YYYY-MM-DD`. Other text is returned
/// with its control characters escaped.
pub fn format_date(date: &str) -> String {
    match (date.len(), date.bytes().all(|b| b.is_ascii_digit())) {
        (8, true) => format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]),
        _ => escape_controls(date),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_stays_within_a_page() {
        assert_eq!(read_commands(0x0c0, 64).unwrap(), [(0x0c0, 64)]);
        assert_eq!(read_commands(0x048, 21).unwrap(), [(0x048, 21)]);
        assert_eq!(
            read_commands(0x0f0, 0x60).unwrap(),
            [(0x0f0, 0x10), (0x100, 0x40), (0x140, 0x10)]
        );
        assert_eq!(
            read_commands(0xec0, 128).unwrap(),
            [(0xec0, 64), (0xf00, 64)]
        );
    }

    #[test]
    fn pages_61_to_63_are_read_a_row_at_a_time() {
        assert_eq!(
            read_commands(0xf3e, 4).unwrap(),
            [(0xf3e, 2), (0xf40, 1), (0xf41, 1)]
        );
        assert_eq!(
            read_commands(0xf87, 31).unwrap(),
            (0xf87..0xfa6).map(|row| (row, 1)).collect::<Vec<_>>()
        );
        assert_eq!(read_commands(0xfff, 1).unwrap(), [(0xfff, 1)]);
    }

    #[test]
    fn a_read_past_the_end_of_otp_is_refused() {
        assert!(read_commands(0xfff, 2).is_err());
        assert!(read_commands(u16::MAX, u16::MAX).is_err());
        assert_eq!(read_commands(0x100, 0).unwrap(), []);
    }

    #[test]
    fn a_refusal_keeps_the_devices_reason() {
        assert_eq!(
            otp_error(Some(PicobootStatus::NotPermitted), "stall".into()),
            OtpError::NotPermitted
        );
        assert_eq!(
            otp_error(
                Some(PicobootStatus::UnsupportedModification),
                "stall".into()
            ),
            OtpError::UnsupportedModification
        );
        assert_eq!(
            otp_error(None, "timed out".into()),
            OtpError::Transport("timed out".into())
        );
        assert_eq!(
            otp_error(Some(PicobootStatus::Ok), "timed out".into()),
            OtpError::Transport("timed out".into())
        );
        let OtpError::Transport(detail) =
            otp_error(Some(PicobootStatus::InvalidAddress), "stall".into())
        else {
            panic!("an unexpected status isn't a transport failure");
        };
        assert!(detail.contains("stall"), "{detail}");
        assert!(detail.contains("InvalidAddress"), "{detail}");
    }

    #[test]
    fn control_characters_are_escaped() {
        assert_eq!(escape_controls("piers.rocks"), "piers.rocks");
        assert_eq!(escape_controls("a\u{1b}[2Jb\n"), "a\\u{1b}[2Jb\\n");
        assert_eq!(escape_controls("Café"), "Café");
    }

    #[test]
    fn a_date_is_shown_with_dashes() {
        assert_eq!(format_date("20260926"), "2026-09-26");
        assert_eq!(format_date("2026-9-26"), "2026-9-26");
        assert_eq!(format_date("2026092\u{7}"), "2026092\\u{7}");
    }

    #[test]
    fn a_board_size_shows_as_its_letter_or_says_it_is_neither() {
        use OneromBoardSize::{BoardSizeL, BoardSizeM, BoardSizeOther, BoardSizeUnknown};
        assert_eq!(board_size_text(MaybeKnown::Known(BoardSizeM)), "M");
        assert_eq!(board_size_text(MaybeKnown::Known(BoardSizeL)), "L");
        // A size from newer firmware keeps its number.
        assert!(board_size_text(MaybeKnown::Unknown(3)).contains("0x03"));
        // Neither M nor L, and not recorded, are told apart from M, from L and
        // from each other.
        let other = board_size_text(MaybeKnown::Known(BoardSizeOther));
        let unrecorded = board_size_text(MaybeKnown::Known(BoardSizeUnknown));
        for text in [&other, &unrecorded] {
            assert!(text != "M" && text != "L", "{text}");
        }
        assert_ne!(other, unrecorded);
    }
}
