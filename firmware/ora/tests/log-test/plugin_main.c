// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Exercises One ROM's plugin logging API (ORA_ID_LOG_*).
//
// One source, flashed as both the system and the user plugin, so two instances
// run on different cores and contend for the same log channel.  Neither
// instance is told which role it plays: both attempt the write claim and take
// their role from the result, so either outcome of the race is a valid run.
//
// Flashed on its own it finds no second instance, runs the checks a single
// instance can run, and reports that the claim exclusion was not exercised.
//
// The verdict is a repeating count of status LED flashes, so no debug probe is
// needed to read it.  A one-line summary is also left unread in the log
// channel, for a probe attached after the run.  See README.md.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.
#if defined(PLUGIN_TYPE_NUM) && (PLUGIN_TYPE_NUM == ORA_PLUGIN_TYPE_SYSTEM)
ORA_DEFINE_SYSTEM_PLUGIN(log_test_main, 0, 1, 0, 0, 0, 7, 2);
#else // User plugin
ORA_DEFINE_USER_PLUGIN(log_test_main, 0, 1, 0, 0, 0, 7, 2);
#endif // Plugin type check

// ---------------------------------------------------------------------------
// Verdict codes
//
// The status LED repeats this many short flashes, then pauses.  1 and 2 are
// passes, and everything above names the check that failed.  README.md carries
// the same table, with every cause each code covers.
//
// A steady lit LED, with no flashing at all, means the logging API could not
// be looked up.
// ---------------------------------------------------------------------------

#define LT_PASS             1u  // Two instances, all checks passed
#define LT_SOLO             2u  // Ran alone - claim exclusion not exercised
#define LT_F_RACE           3u  // A write claim was neither granted nor refused
#define LT_F_OPEN_READ      4u  // A read claim was neither granted nor refused
#define LT_F_UNCLAIMED_WR   5u  // A write without the write claim was allowed
#define LT_F_UNCLAIMED_RD   6u  // A read without the read claim was allowed
#define LT_F_WRITE          7u  // A write with the claim held did not store
#define LT_F_PAYLOAD        8u  // The payload did not read back
#define LT_F_CLOSE_WRITE    9u  // A write claim outlived its close
#define LT_F_QUERY         10u  // Query failed, or its invariant did not hold
#define LT_F_SURVIVE       11u  // Unread bytes did not survive the close
#define LT_F_FULL          12u  // The channel's full behaviour was wrong
#define LT_F_VERDICT       13u  // The other instance never sent its verdict
#define LT_F_CLOSE_READ    14u  // A read claim outlived its close
#define LT_F_NOT_ALONE     15u  // A second plugin is installed but never read
#define LT_F_BOTH_WRITERS  16u  // Both instances were granted the write claim
#define LT_F_BOTH_READERS  17u  // Neither instance was granted the write claim
#define LT_CODE_MAX        LT_F_BOTH_READERS

// Which side of the claim race this instance ended up on.
#define LT_ROLE_UNRESOLVED  0u
#define LT_ROLE_WRITER      1u
#define LT_ROLE_READER      2u

// ---------------------------------------------------------------------------
// Tunables
// ---------------------------------------------------------------------------

#define LT_CH               ORA_LOG_CHANNEL_0

// Bytes moved per read.  Small, because every buffer here lives on a 512 byte
// stack that the ORA and ring frames of each API call are pushed onto too -
// ora_log_read plus onerom_rtt_read is another 72 bytes below every read here.
// It is independent of the payload length, which reads back in two goes.
#define LT_CHUNK            16u

// A poll round is one API call plus LT_ROUND_LOOPS of delay.  The delay loop
// is three cycles on Cortex-M33, so a round is roughly 0.2ms at 150MHz and the
// budgets below are roughly 0.5s, 2s, 4s, 0.2s and 9s.  Clock dependent, and
// deliberately generous.
//
// Each budget is set against a different event, which is why they are not one
// number:
//
// - LT_ROUNDS_LONG covers waits within a run, which only have to outlast the
//   skew between the two cores starting - microseconds.
// - LT_ROUNDS_SEND covers taking the write claim to hand a verdict over.  The
//   other instance can hold that claim for the whole of its own run, so this
//   is measured against a run, not against a core start.
// - LT_ROUNDS_SETTLE is how long the collecting instance holds off writing its
//   summary after taking a frame, so that the sending instance is certain to
//   observe the channel empty and know its frame was taken.  It is also the
//   per-attempt wait on the sending side.
// - LT_ROUNDS_COLLECT is how long an instance waits for a frame.  It must
//   outlast the sending side, so that a slow sender is waited for rather than
//   reported missing.
//
//   Not the sender's whole elapsed time, which can exceed it: a sender can
//   spend LT_ROUNDS_SEND on its first attempt alone.  But it can only spend
//   that while the other instance holds the write claim, and the collector does
//   not start waiting until it has released that claim - so what has to fit
//   inside LT_ROUNDS_COLLECT is the sender's total from the claim coming free,
//   which is at most LT_SEND_ATTEMPTS attempts of (claim + taken + refill).
//
// LT_ROUNDS_SEND stays strictly smaller than LT_ROUNDS_COLLECT too: equal
// budgets let both expire together and turn one stuck claim into two timeouts
// and no report.
#define LT_ROUND_LOOPS      10000u
#define LT_ROUNDS_PEER      2500u
#define LT_ROUNDS_LONG      10000u
#define LT_ROUNDS_SEND      20000u
#define LT_ROUNDS_SETTLE    1000u
#define LT_SEND_ATTEMPTS    6u
#define LT_ROUNDS_COLLECT   45000u
STATIC_ASSERT(LT_ROUNDS_SEND < LT_ROUNDS_COLLECT,
              "a sender must give up before the collector waiting for it does");
// One attempt after the write claim is free costs at most LT_ROUNDS_SETTLE to
// take the claim, LT_ROUNDS_SETTLE waiting for the frame to be taken, and
// three times that waiting for the answer.
STATIC_ASSERT(LT_ROUNDS_COLLECT > (LT_SEND_ATTEMPTS * 5u * LT_ROUNDS_SETTLE),
              "a collector must outlast every send attempt after the claim frees");

// Status LED timings, in delay loops - roughly 200ms and 1.5s at 200MHz.
#define LT_FLASH_LOOPS      13000000u
#define LT_GAP_LOOPS        100000000u

// The payload, written by the claim holder and read back by the other side.
//
// It starts with a marker the firmware's own text logging cannot emit, so the
// reader can pick the payload out of boot log still sitting in the channel.
// 0x02 appears nowhere else in the payload, so no proper prefix of it is also
// a suffix - it has no border - and restarting the match at the byte that
// failed can never skip a payload that starts there.
//
// The ramp catches a channel that delivers the right bytes in the wrong order,
// which a repeated constant would not.
static const uint8_t lt_payload[] = {
    0x02, 'L', 'O', 'G', 'T', 'E', 'S', 'T',
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
    0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
};
#define LT_PAYLOAD_LEN      ((uint32_t)sizeof(lt_payload))

// The verdict frame, sent by the user instance and collected by the system
// instance: marker, the sender's role, then its verdict as two ASCII digits.
//
// The role is in the frame because "we were both granted the write claim" is
// the single most important thing this plugin can find, and neither instance
// can see it alone - each knows only its own side of the race.  Without the
// role it shows up as some downstream data failure, and points a reader at the
// ring rather than at the claim.
#define LT_VERDICT_MARKER   0x02
#define LT_TAG_WRITER       'W'
#define LT_TAG_READER       'R'
#define LT_TAG_UNRESOLVED   'U'
#define LT_VERDICT_LEN      4u
STATIC_ASSERT(LT_CHUNK >= LT_VERDICT_LEN,
              "a read buffer must hold a whole verdict frame");

// ---------------------------------------------------------------------------
// The API functions this plugin uses
//
// Kept on the stack, as ora/plugin.h recommends.  The user plugin's static RAM
// and its stack share 1KB, so a plugin that puts nothing in static RAM leaves
// the whole 512 byte stack half for the stack.
// ---------------------------------------------------------------------------

typedef struct {
    ora_log_open_write_fn_t  open_write;
    ora_log_write_fn_t       write;
    ora_log_close_write_fn_t close_write;
    ora_log_open_read_fn_t   open_read;
    ora_log_read_fn_t        read;
    ora_log_close_read_fn_t  close_read;
    ora_log_query_fn_t       query;
    ora_set_status_led_fn_t  set_led;
    ora_get_flash_slot_count_fn_t slot_count;
    const char              *name;
    uint8_t                  is_system;
    uint8_t                  role;

    // How many plugin slots are installed, from the firmware's own metadata.
    //
    // This is the ground truth the rest of the plugin is measured against, and
    // the reason it is here: everything else this instance can observe about a
    // second one runs through the claim mechanism, which is the thing under
    // test.  An instrument whose only evidence depends on what it is testing
    // reports "I am alone" for a broken claim, which reads as "you forgot to
    // flash the second plugin" and sends a reader in entirely the wrong
    // direction.  ora_get_flash_slot_count touches none of that.
    //
    // It counts what is installed, not what is running, so a second plugin
    // that is not this one - or one the firmware refused to launch - also
    // shows up here as 2.  That is still the right answer to the only question
    // asked of it: whether concluding "alone" is permitted.
    uint8_t                  plugin_slots;

    // Set once this instance has proof a second one is running: a claim
    // refused as ORA_RESULT_LOG_CHANNEL_IN_USE, or a verdict frame arriving.
    //
    // A refusal is only proof while this instance holds no claim of that kind.
    // ora_log_claim tests whether a channel is claimed at all, not whether
    // someone else holds it, so re-opening a claim already held returns
    // ORA_RESULT_LOG_CHANNEL_IN_USE against oneself.  Every site that credits a
    // refusal here has closed, or never took, the claim it is asking for.
    //
    // A channel that empties is not proof either way: a debug probe drains the
    // same channel through the same read position, so inferring a second
    // instance from a drain turns an attached probe into a phantom one.
    uint8_t                  peer_seen;
} lt_api_t;

// ---------------------------------------------------------------------------
// Timing and parking
// ---------------------------------------------------------------------------

// Three cycles per iteration on Cortex-M33 - one for SUBS, two for the taken
// BNE.  Nothing here touches SRAM, so the delay does not contend with the ROM
// serving DMA.
static void lt_delay(uint32_t loops) {
    if (loops == 0u) {
        return;
    }
    __asm volatile (
        "1: subs %0, %0, #1 \n"
        "   bne  1b         \n"
        : "+r" (loops) :: "cc"
    );
}

// Where an instance ends up when it has nothing left to report.
//
// A plugin has no scheduler to return to - returning from the entry point
// makes the firmware log that the plugin exited unexpectedly - so the core
// parks here instead.  WFE holds the core in a low power state until an event
// arrives, touching neither SRAM nor the log, which leaves the device quiet
// enough to observe.
static void lt_park(void) __attribute__((noreturn));
static void lt_park(void) {
    while (1) {
        __asm volatile ("wfe");
    }
}

// ---------------------------------------------------------------------------
// Claims
//
// Every claim attempt goes through these, so that a refusal is recorded as
// proof of a second instance wherever it happens, rather than only where the
// caller happened to look for one.
// ---------------------------------------------------------------------------

static ora_result_t lt_open_write(lt_api_t *api) {
    ora_result_t result = api->open_write(LT_CH, api->name);

    if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
        api->peer_seen = 1u;
    }

    return result;
}

static ora_result_t lt_open_read(lt_api_t *api) {
    ora_result_t result = api->open_read(LT_CH);

    if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
        api->peer_seen = 1u;
    }

    return result;
}

// Takes the write claim, polling until the other instance has released it.
static uint8_t lt_take_write(lt_api_t *api, uint32_t rounds) {
    while (rounds-- > 0u) {
        ora_result_t result = lt_open_write(api);

        if (result == ORA_RESULT_OK) {
            return 1u;
        }
        if (result != ORA_RESULT_LOG_CHANNEL_IN_USE) {
            return 0u;
        }
        lt_delay(LT_ROUND_LOOPS);
    }

    return 0u;
}

// Takes the read claim, polling until the other instance has released it.
//
// A poll rather than a single attempt because the instance holding the write
// claim briefly takes and releases this one while it waits for a reader to
// appear.  A single attempt could land in that window and fail a run that is
// behaving correctly.
//
// @p may_credit says whether a refusal here is evidence of a second instance.
// It is only so where this instance provably holds no read claim: a close that
// failed to release leaves the claim held, and ora_log_claim tests whether a
// channel is claimed at all rather than by whom, so it would then refuse this
// instance against itself.
static uint8_t lt_take_read(lt_api_t *api, uint32_t rounds, uint8_t may_credit) {
    while (rounds-- > 0u) {
        ora_result_t result = may_credit ? lt_open_read(api)
                                         : api->open_read(LT_CH);

        if (result == ORA_RESULT_OK) {
            return 1u;
        }
        if (result != ORA_RESULT_LOG_CHANNEL_IN_USE) {
            return 0u;
        }
        lt_delay(LT_ROUND_LOOPS);
    }

    return 0u;
}

// ---------------------------------------------------------------------------
// Writing text to the channel
//
// ora_log_write takes bytes rather than a format string, so reporting needs no
// scratch buffer and almost no stack.  That matters here: ora_log's formatter
// runs on the calling plugin's stack, and the user plugin has 512 bytes of it.
//
// Every call needs the write claim.  A refused write is ignored - the LED is
// the primary report, and the summary is a convenience for a probe.
//
// Lengths come from the literal rather than from a run time scan.  A scan
// would be a hand written strlen, and GCC recognises those and emits a call to
// the library one, which a plugin has no libc to link against.
// ---------------------------------------------------------------------------

#define LT_SAY(api, lit) \
    (api)->write(LT_CH, (lit), (uint32_t)(sizeof(lit) - 1u))

// Two digits at most - every verdict code is below LT_CODE_MAX.  A single
// digit value is written from digits[1], the units: writing one byte from
// digits[0] emits the tens digit instead, which prints every code below 10 as
// "0".
static void lt_say_num(const lt_api_t *api, uint32_t value) {
    char digits[2];

    digits[0] = (char)('0' + ((value / 10u) % 10u));
    digits[1] = (char)('0' + (value % 10u));
    if (value >= 10u) {
        api->write(LT_CH, digits, 2u);
    } else {
        api->write(LT_CH, &digits[1], 1u);
    }
}

// ---------------------------------------------------------------------------
// Channel helpers
// ---------------------------------------------------------------------------

// Reads and discards everything currently in the channel.  Needs the read
// claim.  Bounded, so a channel being refilled faster than it drains cannot
// trap the caller here.
static void lt_drain(const lt_api_t *api) {
    uint8_t buf[LT_CHUNK];
    uint32_t rounds = LT_ROUNDS_LONG;

    while (rounds-- > 0u) {
        uint32_t copied = 0u;

        if (api->read(LT_CH, buf, LT_CHUNK, &copied) != ORA_RESULT_OK) {
            return;
        }
        if (copied == 0u) {
            return;
        }
    }
}

// Empties the channel after a failure, best effort.
//
// A run that stops part way through can leave the channel full - the check
// that fills it deliberately does - and a full channel drops the verdict frame
// and the summary that follow.  Without this, every such failure would be
// reported as the frame going missing rather than as itself.
//
// Skipped silently if the read claim is refused, which means the other
// instance is still running and will drain the channel itself.
//
// Deliberately the raw call rather than the wrapper: this is cleanup, not
// evidence.  A refusal here says nothing reliable about a second instance,
// because this instance may still hold the claim itself after a failed close.
static void lt_clear_channel(const lt_api_t *api) {
    if (api->open_read(LT_CH) == ORA_RESULT_OK) {
        lt_drain(api);
        api->close_read(LT_CH);
    }
}

// Checks the invariant ora_log_query documents: for a channel that exists,
// size is free plus pending plus the one byte the ring always holds back.  The
// three come from one snapshot, so this holds however busy the channel is.
static uint8_t lt_query_ok(const lt_api_t *api, uint32_t *size_out,
                           uint32_t *pending_out) {
    uint32_t size = 0u, freed = 0u, pending = 0u;

    if (api->query(LT_CH, &size, &freed, &pending) != ORA_RESULT_OK) {
        return 0u;
    }
    if ((size == 0u) || (size != (freed + pending + 1u))) {
        return 0u;
    }
    if (size_out != NULL) {
        *size_out = size;
    }
    if (pending_out != NULL) {
        *pending_out = pending;
    }

    return 1u;
}

// Reads the payload out of the channel, ignoring anything ahead of it.  Used
// where the channel may still hold boot log the reader has never seen.
static uint8_t lt_await_payload(const lt_api_t *api, uint32_t rounds) {
    uint8_t buf[LT_CHUNK];
    uint32_t matched = 0u;

    while (rounds-- > 0u) {
        uint32_t copied = 0u;

        if (api->read(LT_CH, buf, LT_CHUNK, &copied) != ORA_RESULT_OK) {
            return 0u;
        }
        for (uint32_t i = 0u; i < copied; i++) {
            if (buf[i] == lt_payload[matched]) {
                if (++matched == LT_PAYLOAD_LEN) {
                    return 1u;
                }
            } else {
                matched = (buf[i] == lt_payload[0]) ? 1u : 0u;
            }
        }
        if (copied == 0u) {
            lt_delay(LT_ROUND_LOOPS);
        }
    }

    return 0u;
}

// Reads exactly the payload and nothing else.  Used where the channel is known
// to hold the payload alone, so any extra or reordered byte is a failure.
static uint8_t lt_read_payload(const lt_api_t *api, uint32_t rounds) {
    uint8_t buf[LT_CHUNK];
    uint32_t got = 0u;

    while (rounds-- > 0u) {
        uint32_t copied = 0u;
        uint32_t want = LT_PAYLOAD_LEN - got;

        if (want > LT_CHUNK) {
            want = LT_CHUNK;
        }
        if (api->read(LT_CH, buf, want, &copied) != ORA_RESULT_OK) {
            return 0u;
        }
        for (uint32_t i = 0u; i < copied; i++) {
            if (buf[i] != lt_payload[got + i]) {
                return 0u;
            }
        }
        got += copied;
        if (got == LT_PAYLOAD_LEN) {
            return 1u;
        }
        if (copied == 0u) {
            lt_delay(LT_ROUND_LOOPS);
        }
    }

    return 0u;
}

// Fills the channel until a write is refused.  Needs the write claim, and
// needs nothing to be draining.
//
// Positive on both sides: at least one write must be stored before the channel
// reports itself full, and query must then agree there is no room for another
// payload.  The write budget comes from the channel's own reported size, so a
// channel that never refuses is caught rather than looped on.
//
// The payload doubles as the filler, so this needs no buffer of its own.
static uint8_t lt_check_full(const lt_api_t *api) {
    uint32_t stored = 0u;
    uint32_t size = 0u, freed = 0u;
    uint32_t writes;

    if (!lt_query_ok(api, &size, NULL)) {
        return 0u;
    }
    writes = (size / LT_PAYLOAD_LEN) + 2u;

    while (writes-- > 0u) {
        ora_result_t result = api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN);

        if (result == ORA_RESULT_OK) {
            stored++;
            continue;
        }
        if ((result != ORA_RESULT_LOG_FULL) || (stored == 0u)) {
            return 0u;
        }
        if (api->query(LT_CH, NULL, &freed, NULL) != ORA_RESULT_OK) {
            return 0u;
        }

        return (freed < LT_PAYLOAD_LEN) ? 1u : 0u;
    }

    // The channel accepted more than it says it can hold.
    return 0u;
}

// ---------------------------------------------------------------------------
// The checks each side of the race runs
// ---------------------------------------------------------------------------

// Runs alone.  Entered holding the write claim, and only where no claim has
// ever been refused and the firmware reports a single plugin slot.  A timeout
// on its own is never enough to conclude this: a slow second instance, or one
// the claim was wrongly granted to, looks identical from here and would then
// be trampled by the checks below.
//
// Covers everything the two instance run covers except the claim exclusion
// itself, which one instance cannot produce.  Any claim refused from here
// means the conclusion was wrong, and says so rather than reporting a pass.
static uint32_t lt_run_solo(lt_api_t *api) {
    uint8_t buf[1];
    uint32_t copied = 0u;
    uint32_t pending = 0u;
    ora_result_t result;

    result = lt_open_read(api);
    if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
        return LT_F_NOT_ALONE;
    }
    if (result != ORA_RESULT_OK) {
        return LT_F_OPEN_READ;
    }

    // Clear whatever the channel already held.
    lt_drain(api);

    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) != ORA_RESULT_OK) {
        return LT_F_WRITE;
    }
    if (!lt_read_payload(api, LT_ROUNDS_PEER)) {
        return LT_F_PAYLOAD;
    }

    // Leave a payload unread across the close of both claims.
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) != ORA_RESULT_OK) {
        return LT_F_WRITE;
    }
    if (api->close_write(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_WRITE;
    }
    if (api->close_read(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_READ;
    }

    // Holding no claim at all: query must still answer, and both claimed
    // operations must be refused.
    if (!lt_query_ok(api, NULL, &pending)) {
        return LT_F_QUERY;
    }
    if (pending != LT_PAYLOAD_LEN) {
        return LT_F_SURVIVE;
    }
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_WR;
    }
    if (api->read(LT_CH, buf, sizeof(buf), &copied) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_RD;
    }

    // Take both claims back, and confirm the bytes survived the close.
    result = lt_open_read(api);
    if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
        return LT_F_NOT_ALONE;
    }
    if (result != ORA_RESULT_OK) {
        return LT_F_OPEN_READ;
    }
    if (!lt_read_payload(api, LT_ROUNDS_PEER)) {
        return LT_F_SURVIVE;
    }
    result = lt_open_write(api);
    if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
        return LT_F_NOT_ALONE;
    }
    if (result != ORA_RESULT_OK) {
        return LT_F_RACE;
    }
    if (!lt_check_full(api)) {
        return LT_F_FULL;
    }

    lt_drain(api);
    if (api->close_read(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_READ;
    }
    if (api->close_write(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_WRITE;
    }

    // Only one plugin slot is installed, so there was nothing to contend with.
    return (api->plugin_slots > 1u) ? LT_F_NOT_ALONE : LT_SOLO;
}

// Won the write claim.  Entered holding it.
//
// Hands off to lt_run_solo if no second instance ever claims the channel for
// reading.
static uint32_t lt_run_writer(lt_api_t *api) {
    uint8_t buf[1];
    uint32_t copied = 0u;
    uint32_t pending = 0u;
    uint32_t rounds;
    uint8_t written = 0u;
    uint8_t drained = 0u;

    // A write claim is not a read claim.
    if (api->read(LT_CH, buf, sizeof(buf), &copied) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_RD;
    }

    // Wait for the other instance to claim the channel for reading, and write
    // nothing until it has.  Two reasons, both of which rule out the simpler
    // "wait for the channel to empty":
    //
    // Being refused this claim is evidence of a second instance that nothing
    // else can fake - and valid evidence here, because this instance holds
    // only the write claim and so cannot be refusing itself (see peer_seen).
    // A channel that empties is not evidence: a debug probe drains it too.
    //
    // It also orders the payload behind core 0 finishing its plugin launch.
    // launch_plugins_inner starts core 1 and then logs about the user plugin,
    // so core 0 firmware logging overlaps a system plugin that is already
    // running, and a write from the other core corrupts rather than
    // interleaves (see ora_log_open_write_fn_t).  Once the other instance
    // holds the read claim, core 0 is executing that instance rather than the
    // firmware, and the window has closed.
    //
    // The claim is given straight back on each round it is granted, so that
    // the other instance is not refused it when it does arrive.
    for (rounds = LT_ROUNDS_LONG; rounds > 0u; rounds--) {
        ora_result_t result = lt_open_read(api);

        if (result == ORA_RESULT_LOG_CHANNEL_IN_USE) {
            break;
        }
        if (result != ORA_RESULT_OK) {
            return LT_F_OPEN_READ;
        }
        if (api->close_read(LT_CH) != ORA_RESULT_OK) {
            return LT_F_CLOSE_READ;
        }
        lt_delay(LT_ROUND_LOOPS);
    }
    if (!api->peer_seen) {
        // Nothing ever refused this instance a claim.  Whether that means it is
        // alone is not a question the log API can answer - if the claim is
        // broken, both instances get here - so it is answered from the
        // firmware's slot metadata instead.
        if (api->plugin_slots > 1u) {
            return LT_F_NOT_ALONE;
        }
        return lt_run_solo(api);
    }

    // Boot log may still fill the channel, in which case this write is refused
    // until the other instance has drained enough of it.
    for (rounds = LT_ROUNDS_LONG; rounds > 0u; rounds--) {
        ora_result_t result = api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN);

        if (result == ORA_RESULT_OK) {
            written = 1u;
            break;
        }
        if (result != ORA_RESULT_LOG_FULL) {
            return LT_F_WRITE;
        }
        lt_delay(LT_ROUND_LOOPS);
    }
    if (!written) {
        return LT_F_WRITE;
    }

    // The channel emptying is the other instance having consumed the payload.
    for (rounds = LT_ROUNDS_LONG; rounds > 0u; rounds--) {
        if (!lt_query_ok(api, NULL, &pending)) {
            return LT_F_QUERY;
        }
        if (pending == 0u) {
            drained = 1u;
            break;
        }
        lt_delay(LT_ROUND_LOOPS);
    }
    if (!drained) {
        return LT_F_PAYLOAD;
    }

    // Write a payload the other instance has not seen, then release the claim
    // with it still unread.
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) != ORA_RESULT_OK) {
        return LT_F_WRITE;
    }
    if (api->close_write(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_WRITE;
    }

    // Holding no claim at all.  The other instance may already be reading
    // those bytes back, so only the invariant is asserted here - checking the
    // bytes themselves is its job, not this one's.
    if (!lt_query_ok(api, NULL, NULL)) {
        return LT_F_QUERY;
    }
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_WR;
    }

    return LT_PASS;
}

// Lost the write claim.  Entered holding nothing.
static uint32_t lt_run_reader(lt_api_t *api) {
    uint32_t pending = 0u;

    // The other instance holds the write claim, so this one may not write.
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_WR;
    }

    // Read and write claims are independent, so this must be granted while the
    // other instance holds the channel for writing.
    if (!lt_take_read(api, LT_ROUNDS_LONG, 1u)) {
        return LT_F_OPEN_READ;
    }
    if (api->write(LT_CH, lt_payload, LT_PAYLOAD_LEN) !=
        ORA_RESULT_INVALID_ARG) {
        return LT_F_UNCLAIMED_WR;
    }

    if (!lt_await_payload(api, LT_ROUNDS_LONG)) {
        return LT_F_PAYLOAD;
    }

    // The write claim becoming grantable is the other instance's close.
    if (!lt_take_write(api, LT_ROUNDS_LONG)) {
        return LT_F_CLOSE_WRITE;
    }

    // Nothing has written since, so the channel holds the closing instance's
    // last payload and nothing else.
    if (!lt_query_ok(api, NULL, &pending)) {
        return LT_F_QUERY;
    }
    if (pending != LT_PAYLOAD_LEN) {
        return LT_F_SURVIVE;
    }
    if (!lt_read_payload(api, LT_ROUNDS_PEER)) {
        return LT_F_SURVIVE;
    }

    if (!lt_check_full(api)) {
        return LT_F_FULL;
    }

    lt_drain(api);
    if (api->close_read(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_READ;
    }
    if (api->close_write(LT_CH) != ORA_RESULT_OK) {
        return LT_F_CLOSE_WRITE;
    }

    return LT_PASS;
}

// ---------------------------------------------------------------------------
// Reporting
//
// There is one status LED, two instances, and no lock on ora_set_status_led,
// so two patterns driven at once are unreadable and one of them could be read
// as a pass.  The system instance is the only one that ever drives it while a
// second instance is known to be up.  The user instance hands its verdict over
// through the channel and parks.
// ---------------------------------------------------------------------------

// Hands this instance's verdict and role to the other one, re-sending until it
// is taken, and reports whether it was.
//
// The frame can be lost rather than simply delayed: an instance that fails
// empties the channel on its way out, and a full channel drops a write whole.
// Re-sending covers both.
//
// Acknowledgement is two edges, not one.  The channel emptying says only that
// something consumed the frame, and plenty of things consume this channel that
// have never heard of a verdict frame - a debug probe, and the USB system
// plugin draining to CDC.  Taking an empty channel as an acknowledgement lets
// this instance park on a paired verdict it agreed with nobody, which is a
// pass it did not earn.  So it then waits for the channel to refill: only a
// collecting instance writes back, and its summary is that write.  The
// collector holds off for LT_ROUNDS_SETTLE first, so the empty channel is
// always observable in between.
//
// The first attempt is the patient one: the other instance can hold the write
// claim for the whole of its own run.
static uint8_t lt_hand_over(lt_api_t *api, uint32_t verdict) {
    uint8_t frame[LT_VERDICT_LEN];
    uint32_t attempt;
    uint8_t taken;

    frame[0] = LT_VERDICT_MARKER;
    if (api->role == LT_ROLE_WRITER) {
        frame[1] = LT_TAG_WRITER;
    } else if (api->role == LT_ROLE_READER) {
        frame[1] = LT_TAG_READER;
    } else {
        frame[1] = LT_TAG_UNRESOLVED;
    }
    frame[2] = (uint8_t)('0' + ((verdict / 10u) % 10u));
    frame[3] = (uint8_t)('0' + (verdict % 10u));

    for (attempt = 0u; attempt < LT_SEND_ATTEMPTS; attempt++) {
        uint32_t rounds;

        if (!lt_take_write(api, (attempt == 0u) ? LT_ROUNDS_SEND
                                                : LT_ROUNDS_SETTLE)) {
            continue;
        }
        if (api->write(LT_CH, frame, LT_VERDICT_LEN) != ORA_RESULT_OK) {
            api->close_write(LT_CH);
            lt_delay(LT_ROUND_LOOPS);
            continue;
        }
        if (api->close_write(LT_CH) != ORA_RESULT_OK) {
            return 0u;
        }

        taken = 0u;
        for (rounds = LT_ROUNDS_SETTLE; rounds > 0u; rounds--) {
            uint32_t pending = 0u;

            if (api->query(LT_CH, NULL, NULL, &pending) != ORA_RESULT_OK) {
                return 0u;
            }
            if (pending == 0u) {
                taken = 1u;
                break;
            }
            lt_delay(LT_ROUND_LOOPS);
        }
        if (!taken) {
            continue;
        }

        // Consumed by something.  Only a collecting instance answers.
        for (rounds = LT_ROUNDS_SETTLE * 3u; rounds > 0u; rounds--) {
            uint32_t pending = 0u;

            if (api->query(LT_CH, NULL, NULL, &pending) != ORA_RESULT_OK) {
                return 0u;
            }
            if (pending != 0u) {
                api->peer_seen = 1u;
                return 1u;
            }
            lt_delay(LT_ROUND_LOOPS);
        }

        // Drained, with no answer.  It may have been eaten rather than
        // collected - a failing instance empties the channel on its way out -
        // so send it again before concluding anything.
    }

    // Every attempt was consumed without an answer.  Whatever is on the other
    // core, it is not this plugin.
    return 0u;
}

// Collects the other instance's frame.  Returns its verdict code, or 0 - a
// value no code takes - if none arrived or what arrived was not a frame this
// build understands.  @p role_out receives the sender's role.
//
// The digits are range checked rather than trusted.  A byte outside '0' to '9'
// accumulated as one would give a value far outside the code range, which then
// wins every comparison against a real verdict and drives the LED for a count
// nobody could read.
static uint32_t lt_collect_verdict(lt_api_t *api, uint8_t *role_out) {
    uint8_t buf[LT_CHUNK];
    uint32_t rounds;
    uint32_t matched = 0u;
    uint32_t verdict = 0u;
    uint8_t role = LT_ROLE_UNRESOLVED;

    *role_out = LT_ROLE_UNRESOLVED;

    if (!lt_take_read(api, LT_ROUNDS_COLLECT, 0u)) {
        return 0u;
    }

    for (rounds = LT_ROUNDS_COLLECT;
         (rounds > 0u) && (matched < LT_VERDICT_LEN); rounds--) {
        uint32_t copied = 0u;

        if (api->read(LT_CH, buf, LT_CHUNK, &copied) != ORA_RESULT_OK) {
            break;
        }
        for (uint32_t i = 0u; (i < copied) && (matched < LT_VERDICT_LEN); i++) {
            uint8_t byte = buf[i];

            if ((matched >= 2u) && (byte >= '0') && (byte <= '9')) {
                verdict = (verdict * 10u) + (uint32_t)(byte - '0');
                matched++;
            } else if (matched == 1u) {
                if (byte == LT_TAG_WRITER) {
                    role = LT_ROLE_WRITER;
                    matched = 2u;
                } else if (byte == LT_TAG_READER) {
                    role = LT_ROLE_READER;
                    matched = 2u;
                } else if (byte == LT_TAG_UNRESOLVED) {
                    role = LT_ROLE_UNRESOLVED;
                    matched = 2u;
                } else {
                    matched = (byte == LT_VERDICT_MARKER) ? 1u : 0u;
                }
            } else {
                matched = (byte == LT_VERDICT_MARKER) ? 1u : 0u;
                verdict = 0u;
            }
        }
        if (copied == 0u) {
            lt_delay(LT_ROUND_LOOPS);
        }
    }

    api->close_read(LT_CH);

    if ((matched != LT_VERDICT_LEN) || (verdict < LT_PASS) ||
        (verdict > LT_CODE_MAX)) {
        return 0u;
    }
    api->peer_seen = 1u;
    *role_out = role;

    return verdict;
}

// The run's verdict, from this instance's, the other one's, and the other
// one's role.
//
// Order matters, and it is ordered by how close each answer is to a cause:
//
//  - A correct run has exactly one instance on each side of the race.  Both
//    reporting the same side is the most specific thing this plugin can find,
//    and neither could see it alone: each knows only its own result.  Both
//    granted the write claim is the exclusion failing open, both refused it is
//    the claim table left non-zero - and that one otherwise surfaces as check
//    4 on one instance and check 8 on the other, neither of which points at
//    the claim.
//  - "I never met the other instance" is a symptom.  If the other instance
//    reported an actual check failure, that is the cause and this is its
//    consequence.
//  - Where both failed a check, the lower code wins.  The codes run roughly in
//    the order the checks do, so the lower one is the earlier failure, and the
//    later one is more likely to be fallout from it.  Neither instance can
//    tell which is which on its own, so the summary names both.
static uint32_t lt_combine(const lt_api_t *api, uint32_t own, uint32_t peer,
                           uint8_t peer_role) {
    if (peer != 0u) {
        if ((api->role == LT_ROLE_WRITER) && (peer_role == LT_ROLE_WRITER)) {
            return LT_F_BOTH_WRITERS;
        }
        if ((api->role == LT_ROLE_READER) && (peer_role == LT_ROLE_READER)) {
            return LT_F_BOTH_READERS;
        }
    }
    if ((own == LT_SOLO) || (own == LT_F_NOT_ALONE)) {
        if (peer > LT_SOLO) {
            return peer;
        }
        if (own == LT_SOLO) {
            return (api->peer_seen || (api->plugin_slots > 1u))
                   ? LT_F_NOT_ALONE : LT_SOLO;
        }
        return LT_F_NOT_ALONE;
    }
    if (own > LT_SOLO) {
        return ((peer > LT_SOLO) && (peer < own)) ? peer : own;
    }
    if (peer == 0u) {
        return api->peer_seen ? LT_F_VERDICT : own;
    }
    if (peer == LT_SOLO) {
        return LT_F_NOT_ALONE;
    }

    return peer;
}

// Leaves a one-line summary in the channel, unread, then blinks the verdict
// for as long as the device is powered.
//
// The summary is for a probe attached after the run.  A probe attached during
// it would take bytes the reading instance is waiting for, because a probe and
// a plugin reader share one read position.
//
// The LED can only show one number, so the summary always carries both sides:
// @p own is this instance's own verdict and @p peer the other one's, where one
// arrived.  @p verdict is what the two combine to, and is what the LED shows.
//
// Printing both unconditionally matters more than it looks.  Whenever the
// other instance's code wins, this instance's is otherwise recorded nowhere -
// and the rule that picks between two failures is a heuristic, not a proof.
// The lower code is usually the earlier check and so the likelier cause, but
// not always: a close_read that fails to release gives this instance check 14
// while the claim it is still holding gives the other one check 4, and 4 wins.
// The summary is where that is recoverable.
static void lt_report(lt_api_t *api, uint32_t verdict, uint32_t own,
                      uint32_t peer) __attribute__((noreturn));
static void lt_report(lt_api_t *api, uint32_t verdict, uint32_t own,
                      uint32_t peer) {
    if (lt_take_write(api, LT_ROUNDS_PEER)) {
        LT_SAY(api, "\r\nlog-test: ");
        if (verdict == LT_PASS) {
            LT_SAY(api, "PASS");
        } else if (verdict == LT_SOLO) {
            LT_SAY(api, "SOLO, ran alone - claim exclusion not exercised");
        } else {
            LT_SAY(api, "FAIL at check ");
            lt_say_num(api, verdict);
        }
        LT_SAY(api, " - reported by the ");
        if (api->is_system) {
            LT_SAY(api, "system");
        } else {
            LT_SAY(api, "user");
        }
        LT_SAY(api, " instance, which ");
        if (api->role == LT_ROLE_WRITER) {
            LT_SAY(api, "won the write claim");
        } else if (api->role == LT_ROLE_READER) {
            LT_SAY(api, "lost the write claim and read instead");
        } else {
            LT_SAY(api, "never resolved the write claim");
        }
        LT_SAY(api, " (own ");
        lt_say_num(api, own);
        if (peer != 0u) {
            LT_SAY(api, ", peer ");
            lt_say_num(api, peer);
        } else {
            LT_SAY(api, ", peer none");
        }
        LT_SAY(api, ")\r\n");
        api->close_write(LT_CH);
    }

    while (1) {
        for (uint32_t i = 0u; i < verdict; i++) {
            api->set_led(1u);
            lt_delay(LT_FLASH_LOOPS);
            api->set_led(0u);
            lt_delay(LT_FLASH_LOOPS);
        }
        lt_delay(LT_GAP_LOOPS);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

void log_test_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    lt_api_t api;
    uint32_t verdict;
    uint32_t peer = 0u;
    uint8_t peer_role = LT_ROLE_UNRESOLVED;

    (void)entry_args;

    api.open_write  = ora_lookup_fn(ORA_ID_LOG_OPEN_WRITE);
    api.write       = ora_lookup_fn(ORA_ID_LOG_WRITE);
    api.close_write = ora_lookup_fn(ORA_ID_LOG_CLOSE_WRITE);
    api.open_read   = ora_lookup_fn(ORA_ID_LOG_OPEN_READ);
    api.read        = ora_lookup_fn(ORA_ID_LOG_READ);
    api.close_read  = ora_lookup_fn(ORA_ID_LOG_CLOSE_READ);
    api.query       = ora_lookup_fn(ORA_ID_LOG_QUERY);
    api.set_led     = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    api.slot_count  = ora_lookup_fn(ORA_ID_GET_FLASH_SLOT_COUNT);
    api.is_system   = (plugin_type == ORA_PLUGIN_TYPE_SYSTEM) ? 1u : 0u;
    api.role        = LT_ROLE_UNRESOLVED;
    api.peer_seen   = 0u;
    api.plugin_slots = 0u;

    // ora_lookup_fn returns NULL for an identifier the running firmware does
    // not implement.  min_fw_version keeps this plugin off firmware older than
    // the logging API, so none of these can be NULL here - but a plugin that
    // wants to run on both an old and a new firmware has to check, and this is
    // where it would.  Neither the log nor a flash count is available if the
    // lookups failed, so the report is the LED held steadily on.
    if ((api.open_write == NULL) || (api.write == NULL) ||
        (api.close_write == NULL) || (api.open_read == NULL) ||
        (api.read == NULL) || (api.close_read == NULL) ||
        (api.query == NULL) || (api.slot_count == NULL) ||
        (api.set_led == NULL)) {
        if (api.set_led != NULL) {
            api.set_led(1u);
        }
        lt_park();
    }

    // Ground truth, taken before anything is claimed.
    api.plugin_slots = api.slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS);

    // Named per instance, so a probe front end shows which one holds the
    // claim.  The name is not copied, so it has to outlive the claim - a
    // string literal does.
    api.name = api.is_system ? "log-test-sys" : "log-test-usr";

    // Both instances race for the same claim, and neither is told the outcome
    // in advance.  Which core arrives first is not ordered, and is part of what
    // this checks.
    switch (lt_open_write(&api)) {
        case ORA_RESULT_OK:
            api.role = LT_ROLE_WRITER;
            verdict = lt_run_writer(&api);
            break;
        case ORA_RESULT_LOG_CHANNEL_IN_USE:
            api.role = LT_ROLE_READER;
            verdict = lt_run_reader(&api);
            break;
        default:
            verdict = LT_F_RACE;
            break;
    }

    // A failing instance may still hold claims, and may have left the channel
    // full.  Release and empty it, so the exchange below reports the failure
    // rather than the consequences of it.
    if (verdict > LT_SOLO) {
        api.close_write(LT_CH);
        api.close_read(LT_CH);
        lt_clear_channel(&api);
    }

    // The system instance collects and reports, whatever its own verdict, so
    // that the other instance's frame is always consumed and the LED has one
    // owner.  Skipped where the firmware says there is no second plugin at
    // all, which is the whole of the wait on a run with one instance.
    if (api.is_system) {
        if (api.plugin_slots > 1u) {
            peer = lt_collect_verdict(&api, &peer_role);
            if (peer != 0u) {
                // Hold off writing the summary, so the sending instance is
                // certain to see the channel empty and know it was taken.
                for (uint32_t i = 0u; i < LT_ROUNDS_SETTLE; i++) {
                    lt_delay(LT_ROUND_LOOPS);
                }
            }
        }
        lt_report(&api, lt_combine(&api, verdict, peer, peer_role), verdict,
                  peer);
    }

    // User instance.  Hand the verdict over and go quiet: the system instance
    // owns the LED, and two patterns on one LED are unreadable.
    //
    // Reporting here is not a fallback for "the system instance might be slow".
    // A collecting instance answers within a fraction of a second of taking a
    // frame, so this is only reached once several frames have gone unanswered,
    // whatever the elapsed time.  Whatever is on the other core is then not
    // this plugin, the paired run never happened, and it must not be reported
    // as though it had - nor left as a dark LED, which is the one outcome this
    // instrument cannot express.
    if (!lt_hand_over(&api, verdict)) {
        lt_report(&api, (verdict > LT_SOLO) ? verdict : LT_F_VERDICT, verdict,
                  0u);
    }
    lt_park();
}
