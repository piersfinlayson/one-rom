// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the firmware's millisecond time base.
//!
//! On a device the count comes from TIMER0's free-running microsecond counter.
//! There is no TIMER0 in this process, so the firmware reads its two halves
//! from a harness-owned script instead (see `stub_timer_raw_hi` and siblings in
//! `firmware/test/stub_rp235x.c`). The code that assembles the halves is the
//! same in both builds, so it is the real thing under test here. That is what
//! makes the interesting cases reachable: the microsecond-to-millisecond
//! conversion, the 49.7 day wrap, and a counter that moves mid-read would
//! otherwise need a test to wait hours or days of wall time.

use onerom_fw_emulator::Emulator;

/// Verify that the uptime reads as milliseconds, advances, and wraps where the
/// API says it does.
pub fn test_plugin_uptime_ms(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();

    // Milliseconds, not microseconds, and truncated rather than rounded.
    let conversions: &[(u64, u32)] = &[
        (0, 0),
        (999, 0),
        (1_000, 1),
        (1_999, 1),
        (1_500_000, 1_500),
        (60_000_000, 60_000),
    ];
    for (us, expected_ms) in conversions {
        emu.set_timer_us(*us);
        let got = emu.get_plugin_uptime_ms();
        if got != *expected_ms {
            errors.push(format!(
                "{} us: got {} ms, expected {}",
                us, got, expected_ms
            ));
        }
    }

    // Monotonic across a run of advances, and advancing by the amount asked
    // for.
    emu.set_timer_us(0);
    let mut previous = emu.get_plugin_uptime_ms();
    let mut expected = 0u32;
    for step in 1..=10u32 {
        emu.advance_timer_us(u64::from(step) * 1_000);
        expected += step;
        let now = emu.get_plugin_uptime_ms();
        if now < previous {
            errors.push(format!("went backwards: {} ms after {} ms", now, previous));
        }
        if now != expected {
            errors.push(format!(
                "after {} advances: got {} ms, expected {}",
                step, now, expected
            ));
        }
        previous = now;
    }
    println!("  uptime after 10 advances: {} ms", previous);

    // The wrap is at 2^32 milliseconds - 49.7 days - which is what reading the
    // full 64-bit microsecond count buys. A 32-bit microsecond read would have
    // wrapped at about 71 minutes, and the 4294967295 ms case below is well
    // past that.
    let wrap_us = u64::from(u32::MAX) * 1_000;
    let wraps: &[(u64, u32)] = &[
        (wrap_us, u32::MAX),
        (wrap_us + 1_000, 0),
        (wrap_us + 2_000, 1),
    ];
    for (us, expected_ms) in wraps {
        emu.set_timer_us(*us);
        let got = emu.get_plugin_uptime_ms();
        if got != *expected_ms {
            errors.push(format!(
                "wrap at {} us: got {} ms, expected {}",
                us, got, expected_ms
            ));
        }
    }
    println!("  wraps at {} ms", u32::MAX);

    // Leave the clock where the run found it, so a later test in this process
    // does not inherit a clock parked past the wrap.
    emu.set_timer_us(0);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Verify that the two halves of the raw counter are assembled into one
/// consistent value.
///
/// The firmware reads the high half, the low half, then the high half again,
/// and retries while the high half moved. Getting that wrong is invisible on a
/// device for 71 minutes at a time and then corrupts a timestamp by over an
/// hour, so the sequence is scripted here instead of waited for.
///
/// Each scripted entry is the counter's value as of one half-read. The values
/// are chosen so that a missing retry, a low half read outside the two high
/// reads, and a single fix-up instead of a loop each produce a *different*
/// millisecond result rather than one that merely rounds to the same answer.
pub fn test_plugin_uptime_raw_read(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();

    // Straddling a high-half increment: low half at its maximum, then rolled
    // over into the next high half.
    const BEFORE: u64 = (1u64 << 32) | 0xFFFF_FFFF;
    const AFTER: u64 = (2u64 << 32) | 50_000;
    const LATER: u64 = (2u64 << 32) | 100_000;
    const LATEST: u64 = (3u64 << 32) | 200_000;

    // The two ways the low half ends up outside the pair of high reads, each
    // leaving its own signature: read after both high reads at the top, which
    // pairs the old high half with a new low one, or read after the second high
    // read inside the retry, which pairs the new high half with a later low one.
    const LO_OUTSIDE_AT_TOP: u64 =
        (BEFORE & 0xFFFF_FFFF_0000_0000) | (AFTER & 0x0000_0000_FFFF_FFFF);
    const LO_OUTSIDE_IN_RETRY: u64 = LATER;

    // A counter that does not move needs no retry.
    emu.set_timer_raw_script(&[AFTER]);
    let got = emu.get_plugin_uptime_ms();
    if got != (AFTER / 1_000) as u32 {
        errors.push(format!(
            "steady counter: got {} ms, expected {}",
            got,
            AFTER / 1_000
        ));
    }

    // The high half moves between the two reads of it. The retry must re-read
    // the low half and report the new pair, not the stale one.
    //
    // Without the retry the result is BEFORE. With the low half read outside
    // the two high reads it is one of the two signatures above.
    emu.set_timer_raw_script(&[BEFORE, BEFORE, AFTER, AFTER, LATER]);
    let got = emu.get_plugin_uptime_ms();
    let expected = (AFTER / 1_000) as u32;
    if got != expected {
        let hint = if got == (BEFORE / 1_000) as u32 {
            " - looks like the retry is missing"
        } else if got == (LO_OUTSIDE_AT_TOP / 1_000) as u32
            || got == (LO_OUTSIDE_IN_RETRY / 1_000) as u32
        {
            " - looks like the low half is read outside the two high reads"
        } else {
            ""
        };
        errors.push(format!(
            "counter moved mid-read: got {} ms, expected {}{}",
            got, expected, hint
        ));
    }

    // The high half moves twice, so the retry has to loop rather than fix up
    // once. A single fix-up reports AFTER.
    emu.set_timer_raw_script(&[BEFORE, BEFORE, AFTER, AFTER, LATEST, LATEST, LATEST]);
    let got = emu.get_plugin_uptime_ms();
    let expected = (LATEST / 1_000) as u32;
    if got != expected {
        let hint = if got == (AFTER / 1_000) as u32 {
            " - looks like a single fix-up rather than a loop"
        } else {
            ""
        };
        errors.push(format!(
            "counter moved twice mid-read: got {} ms, expected {}{}",
            got, expected, hint
        ));
    }
    println!("  retry assembles a consistent pair across a high-half change");

    // Leave the clock where the run found it.
    emu.set_timer_us(0);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
