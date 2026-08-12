// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the plugin logging API (`ORA_ID_LOG_*`).
//!
//! # What is being tested, and what is not
//!
//! The ring itself is covered by the host C tests in `firmware/test/rtt/`,
//! which drive `onerom_rtt_read`/`_write`/`_query` directly and mutation-check
//! their own coverage. What only the emulator can reach is the layer above:
//! the claim tables, the caller identity the claims are keyed on, and whether
//! each documented return code is the one actually produced.
//!
//! # Standing in for two plugins
//!
//! On a device the calling plugin is derived from `SIO_CPUID`, because core 1
//! runs the system plugin and core 0 the user plugin, so a plugin cannot claim
//! to be the other one. There is no such register on the host, so the harness
//! says instead, through `set_calling_plugin`. Every test that matters here is
//! about one plugin being kept out of another's claim, so switching identity
//! between calls is the whole point rather than a convenience.
//!
//! Claims persist for the life of the emulator instance, so each test releases
//! what it took. A test that leaves a claim behind fails the next one, which is
//! a nuisance rather than a hazard, but it also makes an accidental leak
//! visible instead of silent.

use onerom_fw_emulator::{Emulator, OraResult, ffi};

const CH0: u32 = 0;
/// Declared by `ONEROM_RTT_MAX_UP_BUFFERS` but never given a buffer, so every
/// call in the family must reject it. This is the "declared in a newer header,
/// absent on this firmware" case a plugin has to cope with.
const CH_ABSENT: u32 = 1;
/// Past `ONEROM_RTT_MAX_UP_BUFFERS` entirely.
const CH_OUT_OF_RANGE: u32 = 99;

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

fn check(what: &str, got: OraResult, want: OraResult) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

/// A claim excludes the other plugin, and only the other plugin.
///
/// Arm as the system plugin, then stimulate as the user plugin: every write
/// path must refuse it. The discriminating half is that the *same* plugin is
/// still admitted, so the test cannot pass by refusing everyone.
pub fn test_write_claim_excludes_other_plugin(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims channel 0",
        emu.log_open_write(CH0, c"system"),
        OraResult::Ok,
    )?;

    // A second claim by the same plugin is still a second claim.
    check(
        "system re-claims its own channel",
        emu.log_open_write(CH0, c"system-again"),
        OraResult::LogChannelInUse,
    )?;

    emu.set_calling_plugin(USER);
    check(
        "user claims a taken channel",
        emu.log_open_write(CH0, c"user"),
        OraResult::LogChannelInUse,
    )?;
    check(
        "user writes a channel it does not hold",
        emu.log_write(CH0, b"nope"),
        OraResult::InvalidArg,
    )?;
    check(
        "user closes a claim it does not hold",
        emu.log_close_write(CH0),
        OraResult::InvalidArg,
    )?;

    // Discriminate: the holder is unaffected by any of that.
    emu.set_calling_plugin(SYSTEM);
    check(
        "holder writes after the other plugin was refused",
        emu.log_write(CH0, b"ok"),
        OraResult::Ok,
    )?;
    check(
        "holder closes its own claim",
        emu.log_close_write(CH0),
        OraResult::Ok,
    )?;

    // And once released, the other plugin can take it.
    emu.set_calling_plugin(USER);
    check(
        "user claims a released channel",
        emu.log_open_write(CH0, c"user"),
        OraResult::Ok,
    )?;
    check(
        "user releases it again",
        emu.log_close_write(CH0),
        OraResult::Ok,
    )?;

    Ok(())
}

/// Read and write claims are independent, and the bytes survive the crossing.
///
/// One plugin writes, the other reads, which is the arrangement P4 uses: the
/// USB plugin drains a channel the other plugin is writing.
pub fn test_read_and_write_claims_are_independent(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims for writing",
        emu.log_open_write(CH0, c"system"),
        OraResult::Ok,
    )?;

    emu.set_calling_plugin(USER);
    check(
        "user claims the same channel for reading",
        emu.log_open_read(CH0),
        OraResult::Ok,
    )?;

    // Drain whatever boot logging left behind, so the comparison below is
    // against this test's own bytes.
    let (r, _) = emu.log_read(CH0, 4096);
    check("user drains the channel", r, OraResult::Ok)?;

    emu.set_calling_plugin(SYSTEM);
    check(
        "system writes",
        emu.log_write(CH0, b"hello from the writer"),
        OraResult::Ok,
    )?;

    emu.set_calling_plugin(USER);
    let (r, got) = emu.log_read(CH0, 4096);
    check("user reads", r, OraResult::Ok)?;
    if got != b"hello from the writer" {
        return Err(format!(
            "reader got {:?}, want {:?}",
            String::from_utf8_lossy(&got),
            "hello from the writer"
        ));
    }

    // Closing the write claim leaves the read claim alone, and vice versa.
    emu.set_calling_plugin(SYSTEM);
    check(
        "system closes its write claim",
        emu.log_close_write(CH0),
        OraResult::Ok,
    )?;

    emu.set_calling_plugin(USER);
    check(
        "read claim survives the writer closing",
        emu.log_read(CH0, 16).0,
        OraResult::Ok,
    )?;
    check(
        "user closes its read claim",
        emu.log_close_read(CH0),
        OraResult::Ok,
    )?;
    check(
        "reading after closing the read claim",
        emu.log_read(CH0, 16).0,
        OraResult::InvalidArg,
    )?;

    Ok(())
}

/// Unread bytes survive the writer closing, which is what makes close a
/// release of the claim rather than a teardown of the channel.
pub fn test_close_write_leaves_unread_bytes(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(USER);
    emu.log_open_read(CH0);
    let (_, _) = emu.log_read(CH0, 4096);

    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims for writing",
        emu.log_open_write(CH0, c"system"),
        OraResult::Ok,
    )?;
    check(
        "system writes then abandons the channel",
        emu.log_write(CH0, b"still here"),
        OraResult::Ok,
    )?;
    check("system closes", emu.log_close_write(CH0), OraResult::Ok)?;

    emu.set_calling_plugin(USER);
    let (r, got) = emu.log_read(CH0, 4096);
    check("reader reads after the writer closed", r, OraResult::Ok)?;
    if got != b"still here" {
        return Err(format!(
            "after close, reader got {:?}, want \"still here\"",
            String::from_utf8_lossy(&got)
        ));
    }
    check(
        "reader releases its claim",
        emu.log_close_read(CH0),
        OraResult::Ok,
    )?;

    Ok(())
}

/// `ora_log_query` needs no claim, in either direction, and its documented
/// identity holds.
pub fn test_query_needs_no_claim(emu: &Emulator) -> Result<(), String> {
    // Nothing is claimed at this point, which is the condition under test: a
    // consumer must be able to ask whether there is anything to drain before
    // it decides to claim anything.
    emu.set_calling_plugin(USER);
    let (r, size, free, pending) = emu.log_query(CH0);
    check("query with no claim held", r, OraResult::Ok)?;
    if size == 0 {
        return Err("query reported size 0 for a channel that exists".to_string());
    }
    if size != free + pending + 1 {
        return Err(format!(
            "size {size} != free {free} + pending {pending} + 1"
        ));
    }

    // A write moves pending and free in step, still satisfying the identity.
    emu.set_calling_plugin(SYSTEM);
    emu.log_open_write(CH0, c"system");
    emu.log_write(CH0, b"0123456789");
    let (_, size2, free2, pending2) = emu.log_query(CH0);
    if pending2 != pending + 10 {
        return Err(format!(
            "pending {pending2} after a 10 byte write, want {}",
            pending + 10
        ));
    }
    if size2 != free2 + pending2 + 1 {
        return Err(format!(
            "after write: size {size2} != free {free2} + pending {pending2} + 1"
        ));
    }
    emu.log_close_write(CH0);

    // Leave the channel drained for whatever runs next.
    emu.set_calling_plugin(USER);
    emu.log_open_read(CH0);
    emu.log_read(CH0, 4096);
    emu.log_close_read(CH0);

    Ok(())
}

/// A channel the header declares but this firmware has no buffer for is
/// rejected by every call, and so is one past the end of the table.
///
/// This is the case a plugin built against a newer header hits on older
/// firmware. `query` answering rather than faulting is what lets it detect
/// that and fall back.
pub fn test_absent_channel_is_rejected(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(SYSTEM);

    for (label, channel) in [("absent", CH_ABSENT), ("out of range", CH_OUT_OF_RANGE)] {
        check(
            &format!("open_write on an {label} channel"),
            emu.log_open_write(channel, c"x"),
            OraResult::InvalidArg,
        )?;
        check(
            &format!("open_read on an {label} channel"),
            emu.log_open_read(channel),
            OraResult::InvalidArg,
        )?;
        check(
            &format!("write to an {label} channel"),
            emu.log_write(channel, b"x"),
            OraResult::InvalidArg,
        )?;
        check(
            &format!("read from an {label} channel"),
            emu.log_read(channel, 16).0,
            OraResult::InvalidArg,
        )?;
        check(
            &format!("close_write on an {label} channel"),
            emu.log_close_write(channel),
            OraResult::InvalidArg,
        )?;
        check(
            &format!("close_read on an {label} channel"),
            emu.log_close_read(channel),
            OraResult::InvalidArg,
        )?;
        check(
            &format!("query on an {label} channel"),
            emu.log_query(channel).0,
            OraResult::InvalidArg,
        )?;
    }

    Ok(())
}

/// The documented edge cases of `write` and `read`: a zero-length write is a
/// success rather than a drop, a full channel reports `LogFull`, and a
/// zero-length read is not the "channel empty" signal.
pub fn test_write_and_read_edges(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims for writing",
        emu.log_open_write(CH0, c"system"),
        OraResult::Ok,
    )?;

    check("zero length write", emu.log_write(CH0, b""), OraResult::Ok)?;

    // Fill the channel, then confirm the next record is refused rather than
    // truncated.
    let (_, _, free, _) = emu.log_query(CH0);
    let filler = vec![b'F'; free as usize];
    check(
        "write of exactly the free space",
        emu.log_write(CH0, &filler),
        OraResult::Ok,
    )?;
    check(
        "write into a full channel",
        emu.log_write(CH0, b"x"),
        OraResult::LogFull,
    )?;

    emu.set_calling_plugin(USER);
    check(
        "user claims for reading",
        emu.log_open_read(CH0),
        OraResult::Ok,
    )?;

    // A zero length read must not be mistaken for an empty channel: there are
    // `free` bytes waiting.
    let (r, got) = emu.log_read(CH0, 0);
    check("zero length read", r, OraResult::Ok)?;
    if !got.is_empty() {
        return Err(format!("zero length read returned {} bytes", got.len()));
    }
    let (_, _, _, pending) = emu.log_query(CH0);
    if pending == 0 {
        return Err("a zero length read consumed the channel".to_string());
    }

    // Drain and release.
    loop {
        let (r, got) = emu.log_read(CH0, 4096);
        check("draining", r, OraResult::Ok)?;
        if got.is_empty() {
            break;
        }
    }
    emu.log_close_read(CH0);
    emu.set_calling_plugin(SYSTEM);
    emu.log_close_write(CH0);

    Ok(())
}
