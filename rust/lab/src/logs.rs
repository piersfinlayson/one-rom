// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

// Not called when USB is enabled, but required to bring in _SEGGER_RTT
#[allow(dead_code)]
pub fn init_rtt() {
    rtt_target::rtt_init_log!(
        log::LevelFilter::Debug,
        rtt_target::ChannelMode::NoBlockSkip,
        8192
    );
}
