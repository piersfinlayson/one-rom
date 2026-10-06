// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::{args, utils::check_device};
use onerom_cli::{Error, Options};

pub async fn cmd_slot(options: &Options, args: &args::update::UpdateSlotArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("update flash".to_string()))
}

pub async fn cmd_commit(
    options: &Options,
    args: &args::update::UpdateCommitArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("update commit".to_string()))
}
