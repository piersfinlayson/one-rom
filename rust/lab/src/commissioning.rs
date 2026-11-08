// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - checking a One ROM's commissioning signature
//!
//! RBCP returns the commissioning instance's OTP rows, so Lab checks the
//! signature as `onerom hardware validate` does.

use alloc::vec::Vec;

use onerom_app::{LocalFetch, SignerTable, Verdict, verify_instance};
use onerom_metadata::otp::CommissioningInstance;

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
pub async fn check(
    instance: &CommissioningInstance,
    chip_id: [u16; 4],
    table: &SignerTable,
) -> Check {
    match verify_instance(instance, chip_id, table, &NoFetch).await {
        Ok(verdict) => Check::Verdict(verdict),
        // Only a retired key's record file is fetched.
        Err(_) => Check::Retired,
    }
}
