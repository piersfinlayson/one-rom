// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - checking a One ROM's commissioning signature
//!
//! RBCP returns the values, not the signed rows. Lab rebuilds the rows as
//! `onerom hardware commission` writes them.

use alloc::vec;
use alloc::vec::Vec;

use onerom_app::{LocalFetch, SignerTable, Verdict, verify_instance};
use onerom_config::hw::BOARDS;
use onerom_metadata::OTP_COMMISSIONING_AREA_FIRST_ROW;
use onerom_metadata::otp::{CommissioningArea, CommissioningValues, NewCommissioningInstance};

use crate::rbcp::Values;

pub enum Check {
    Verdict(Verdict),
    /// Verified with a retired key. Lab can't download the key's record file.
    Retired,
}

/// Lab doesn't have a network.
struct NoFetch;

impl LocalFetch for NoFetch {
    type Error = ();

    async fn fetch(&self, _source: &str) -> Result<Vec<u8>, ()> {
        Err(())
    }
}

/// `chip_id` is rows `0x000`–`0x003`, row `0x000` first.
pub async fn check(values: &Values, chip_id: [u16; 4], table: &SignerTable) -> Check {
    let (Some(board), Some(manufacturer), Some(date), Some(signer), Some(signature)) = (
        values.board.as_deref(),
        values.manufacturer.as_deref(),
        values.date.as_deref(),
        values.signer,
        values.signature.as_ref(),
    ) else {
        return Check::Verdict(Verdict::Invalid);
    };
    // An unknown board, or values `hardware commission` can't write.
    let Some(rows) = rows(board, manufacturer, date, signer, signature) else {
        return Check::Verdict(Verdict::BadSignature);
    };
    let area = CommissioningArea::parse(&rows);
    let Some(instance) = area.current() else {
        return Check::Verdict(Verdict::BadSignature);
    };
    match verify_instance(instance, chip_id, table, &NoFetch).await {
        Ok(verdict) => Check::Verdict(verdict),
        // Only a retired key's record file is fetched.
        Err(_) => Check::Retired,
    }
}

/// The commissioning area's rows as `hardware commission` writes the values.
fn rows(
    board: &str,
    manufacturer: &str,
    date: &str,
    signer: u16,
    signature: &[u8; 64],
) -> Option<Vec<u16>> {
    let board = *BOARDS.iter().find(|b| b.name() == board)?;
    let values = CommissioningValues::new(board, manufacturer, date, signer).ok()?;
    let instance = NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW).ok()?;
    let writes = instance.writes(signature);
    let len = writes
        .iter()
        .map(|w| w.row - OTP_COMMISSIONING_AREA_FIRST_ROW + 1)
        .max()?;
    let mut rows = vec![0; usize::from(len)];
    for w in writes {
        rows[usize::from(w.row - OTP_COMMISSIONING_AREA_FIRST_ROW)] = w.value;
    }
    Some(rows)
}
