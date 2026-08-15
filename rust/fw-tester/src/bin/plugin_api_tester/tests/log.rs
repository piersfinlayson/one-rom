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

use onerom_fw_emulator::{Emulator, OraResult, build_options, ffi};
use onerom_gen::Config;

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
/// `NotSupported` from every call, and so is one past the end of the table.
///
/// This is the case a plugin built against a newer header hits on older
/// firmware. The code has to be `NotSupported` and not the `InvalidArg` an
/// unheld claim earns, because the two ask for different responses: fall back
/// to a channel this firmware has, against fix the call. `test_write_claim_
/// excludes_other_plugin` holds the other half of that pair, so swapping the
/// two codes fails one test or the other whichever way round it is done.
/// `query` answering rather than faulting is what lets a plugin detect the
/// version difference without side effects.
pub fn test_absent_channel_is_rejected(emu: &Emulator) -> Result<(), String> {
    emu.set_calling_plugin(SYSTEM);

    for (label, channel) in [("absent", CH_ABSENT), ("out of range", CH_OUT_OF_RANGE)] {
        check(
            &format!("open_write on an {label} channel"),
            emu.log_open_write(channel, c"x"),
            OraResult::NotSupported,
        )?;
        check(
            &format!("open_read on an {label} channel"),
            emu.log_open_read(channel),
            OraResult::NotSupported,
        )?;
        check(
            &format!("write to an {label} channel"),
            emu.log_write(channel, b"x"),
            OraResult::NotSupported,
        )?;
        check(
            &format!("read from an {label} channel"),
            emu.log_read(channel, 16).0,
            OraResult::NotSupported,
        )?;
        check(
            &format!("close_write on an {label} channel"),
            emu.log_close_write(channel),
            OraResult::NotSupported,
        )?;
        check(
            &format!("close_read on an {label} channel"),
            emu.log_close_read(channel),
            OraResult::NotSupported,
        )?;
        check(
            &format!("query on an {label} channel"),
            emu.log_query(channel).0,
            OraResult::NotSupported,
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

/// Each log category answers for the gates that actually apply to it.
///
/// Two of the three gates a category can carry are moved under it here. Boot
/// logging moves while the firmware runs, so the test flips it and checks that
/// the categories carrying it follow and that the rest hold still. The compile
/// options do not move within a run, so the other half of that coverage is a
/// second run of this suite against a library built with `TEST_LOGGING=0` -
/// which is what `ci/test-emu.sh` adds one of. Between them, a category wired
/// to the wrong gate, or to none, holds still when it should move or moves
/// when it should not.
///
/// The runtime flip is also what tells `DEBUG` from `PLUGIN_DEBUG`. One ROM's
/// own `DEBUG()` lines are boot messages and stop when boot logging does, while
/// a plugin's `ora_debug_log` has no runtime gate at all, so swapping the two
/// answers fails this test in the boot-logging-off phase.
///
/// The turbo boot half of `BOOT` is asserted from the config rather than
/// stimulated - turbo boot is fixed in flash metadata, so a run can only
/// observe whichever way the config under test was built.
pub fn test_log_categories(
    emu: &Emulator,
    config: &Config,
    log_enabled: bool,
) -> Result<(), String> {
    // (category, label, answer with boot logging on, answer with it off).
    //
    // BOOT and DEBUG are the two that carry the runtime gate. The rest are
    // settled by the build, and what settled them is `build_options`, taken
    // from the `TEST_LOGGING` this library's C was compiled with rather than
    // from the firmware's own account of it.
    let boot_on = if config.turbo_boot { 0 } else { 1 };
    let plugin = u32::from(build_options::PLUGIN_LOGGING);
    let debug_boot = if build_options::DEBUG_LOGGING {
        boot_on
    } else {
        0
    };
    let plugin_debug = u32::from(build_options::PLUGIN_LOGGING && build_options::DEBUG_LOGGING);
    let categories: &[(ffi::ora_log_category_t, &str, u32, u32)] = &[
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_BOOT,
            "BOOT",
            boot_on,
            0,
        ),
        // ora_log reaches the channel with no runtime test, so the compile
        // gate is the whole answer.
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_PLUGIN_INTERNAL,
            "PLUGIN_INTERNAL",
            plugin,
            plugin,
        ),
        // DEBUG() carries the boot gates and the compile gate on top.
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_DEBUG,
            "DEBUG",
            debug_boot,
            0,
        ),
        // Neither ERR() nor ora_err_log carries a gate of any kind, so this
        // one stays 1 in a build with every logging option off.
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_ERROR,
            "ERROR",
            1,
            1,
        ),
        // The firmware never gates what a plugin puts in its own channel, and
        // the ora_log_write family is not compiled out with PLUGIN_LOGGING.
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_PLUGIN_APPLICATION,
            "PLUGIN_APPLICATION",
            1,
            1,
        ),
        // ora_debug_log needs both compile options and has no runtime gate.
        (
            ffi::ora_log_category_t_ORA_LOG_CATEGORY_PLUGIN_DEBUG,
            "PLUGIN_DEBUG",
            plugin_debug,
            plugin_debug,
        ),
    ];

    for (boot_logging, phase) in [(true, "boot logging on"), (false, "boot logging off")] {
        Emulator::set_logging(boot_logging);

        for (category, label, want_on, want_off) in categories {
            let want = if boot_logging { *want_on } else { *want_off };
            let (result, value) = emu.log_category_enabled(*category);
            if !result.is_ok() {
                Emulator::set_logging(log_enabled);
                return Err(format!(
                    "{} ({}): expected OK, got {:?}",
                    label, phase, result
                ));
            }
            let value = match value {
                Some(v) => v,
                None => {
                    Emulator::set_logging(log_enabled);
                    return Err(format!("{} ({}): OK but no value", label, phase));
                }
            };
            if value != want {
                Emulator::set_logging(log_enabled);
                return Err(format!(
                    "{} ({}): got {}, expected {}",
                    label, phase, value, want
                ));
            }
            if boot_logging {
                println!("  {}: {}", label, value);
            }
        }
    }

    // Leave the harness's logging as the run was started with, so a later test
    // sees what it would have seen.
    Emulator::set_logging(log_enabled);

    // A category this firmware does not know - what a plugin built against a
    // newer header asks - is NotSupported, as is the sentinel. That is a
    // different answer from the InvalidArg a NULL out pointer earns below,
    // and the difference is the whole point: one says fall back, the other
    // says the call was wrong.
    const UNKNOWN_CATEGORY: ffi::ora_log_category_t = 99;
    for (category, label) in [
        (UNKNOWN_CATEGORY, "an unknown category"),
        (ffi::ora_log_category_t_ORA_LOG_CATEGORY_INVALID, "INVALID"),
    ] {
        let (result, value) = emu.log_category_enabled(category);
        check(label, result, OraResult::NotSupported)?;
        if value.is_some() {
            return Err(format!("{}: wrote a value on failure", label));
        }
    }

    // A NULL out pointer is refused rather than written through.
    check(
        "NULL out pointer",
        emu.log_category_enabled_null_out(ffi::ora_log_category_t_ORA_LOG_CATEGORY_ERROR),
        OraResult::InvalidArg,
    )?;

    Ok(())
}
