// Host side test of firmware/src/rtt.c's ring.
//
// Compiled with AddressSanitizer, so an out of bounds write into the static
// up buffer is detected precisely rather than inferred.

#include "include.h"
#include <stdio.h>
#include <stdlib.h>

static int failures = 0;
static int checks = 0;

#define CHECK(cond, ...)                                                      \
    do {                                                                      \
        checks++;                                                             \
        if (!(cond)) {                                                        \
            failures++;                                                       \
            printf("  FAIL %s:%d: ", __func__, __LINE__);                     \
            printf(__VA_ARGS__);                                              \
            printf("\n");                                                     \
        }                                                                     \
    } while (0)

static onerom_rtt_up_t *up0(void) { return &_SEGGER_RTT.up[0]; }

// Drain the ring the way a host does: advance RdOff, copying out.
static unsigned drain(char *out, unsigned max) {
    onerom_rtt_up_t *r = up0();
    unsigned n = 0;
    while (r->read_offset != r->write_offset && n < max) {
        out[n++] = r->buffer[r->read_offset];
        r->read_offset = (r->read_offset + 1u) % r->size;
    }
    return n;
}

// ---------------------------------------------------------------------------

static void test_lazy_init(void) {
    // Arm: control block is zeroed, as .bss would be at reset.
    CHECK(_SEGGER_RTT.id[0] == '\0', "CB not zeroed before first write");

    // Stimulate.
    unsigned w = onerom_rtt_write(0, "x", 1);

    // Fence: the block a probe looks for must be complete and correct.
    CHECK(w == 1, "first write returned %u, want 1", w);
    CHECK(memcmp(_SEGGER_RTT.id, "SEGGER RTT", 10) == 0,
          "id is not \"SEGGER RTT\"");
    CHECK(_SEGGER_RTT.max_up_buffers == 3, "max_up_buffers %d, want 3",
          _SEGGER_RTT.max_up_buffers);
    CHECK(_SEGGER_RTT.max_down_buffers == 3, "max_down_buffers %d, want 3",
          _SEGGER_RTT.max_down_buffers);
    CHECK(up0()->size == 3584, "up[0] size %u, want 3584",
          up0()->size);
    CHECK(up0()->buffer != NULL, "up[0] has no buffer");
    CHECK(up0()->flags == ONEROM_RTT_MODE_NO_BLOCK_SKIP, "up[0] not skip mode");
    CHECK(_SEGGER_RTT.down[0].size == 16, "down[0] size %u, want 16",
          _SEGGER_RTT.down[0].size);
    CHECK(up0()->write_offset == 1, "WrOff %u, want 1", up0()->write_offset);

    char buf[8];
    unsigned n = drain(buf, sizeof(buf));
    CHECK(n == 1 && buf[0] == 'x', "drained %u bytes, first '%c'", n, buf[0]);
}

// Plugin launch builds the control block up front, so that a plugin's first log
// finds it built however little the firmware has logged before that point.
static void test_plugins_init(void) {
    // Arm: the control block as .bss leaves it at reset.
    memset(&_SEGGER_RTT, 0, sizeof(_SEGGER_RTT));
    CHECK(_SEGGER_RTT.id[0] == '\0', "CB not zeroed before plugins init");

    // Stimulate: no write, just the plugin launch hook.
    onerom_rtt_plugins_init();

    // Fence: a probe scanning for the block now finds a complete one.
    CHECK(memcmp(_SEGGER_RTT.id, "SEGGER RTT", 10) == 0,
          "plugins init left no identifier");
    CHECK(_SEGGER_RTT.max_up_buffers == 3,
          "max_up_buffers %d after plugins init, want 3",
          _SEGGER_RTT.max_up_buffers);
    CHECK(up0()->buffer != NULL, "up[0] has no buffer after plugins init");
    CHECK(up0()->size == 3584, "up[0] size %u after plugins init, want 3584",
          up0()->size);
    CHECK(up0()->write_offset == 0u, "WrOff %u after plugins init, want 0",
          up0()->write_offset);

    // Discriminate: a second call must leave a block that is already in use
    // alone, rather than rebuilding it and throwing away pending records.
    CHECK(onerom_rtt_write(0, "kept", 4) == 4u,
          "write after plugins init refused");
    onerom_rtt_plugins_init();
    CHECK(up0()->write_offset == 4u, "second plugins init moved WrOff to %u",
          up0()->write_offset);
    CHECK(memcmp(up0()->buffer, "kept", 4) == 0,
          "second plugins init cleared the buffer");

    char got[8];
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == 4u,
          "record lost across a second plugins init");
}

static void test_records_are_whole_and_ordered(void) {
    char expect[512];
    unsigned e = 0;
    // Stimulate: many records of varying length.
    for (int i = 0; i < 40; i++) {
        char rec[32];
        int len = snprintf(rec, sizeof(rec), "record-%d;", i);
        unsigned w = onerom_rtt_write(0, rec, (unsigned)len);
        CHECK(w == (unsigned)len, "record %d short write %u/%d", i, w, len);
        memcpy(expect + e, rec, (size_t)len);
        e += (unsigned)len;
    }
    // Fence: what comes out is exactly what went in, in order.
    char got[512];
    unsigned n = drain(got, sizeof(got));
    CHECK(n == e, "drained %u, wrote %u", n, e);
    CHECK(memcmp(got, expect, e) == 0, "drained bytes differ from written");
}

static void test_wrap(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = r->size;

    // Arm: position the write pointer 4 bytes from the end, ring empty.
    r->write_offset = size - 4u;
    r->read_offset = size - 4u;

    // Stimulate: a 10 byte record must straddle the wrap.
    unsigned w = onerom_rtt_write(0, "ABCDEFGHIJ", 10);
    CHECK(w == 10, "wrap write returned %u, want 10", w);
    CHECK(r->write_offset == 6, "WrOff %u after wrap, want 6", r->write_offset);

    // Fence: both halves landed in the right places.
    CHECK(memcmp(r->buffer + size - 4u, "ABCD", 4) == 0, "tail half wrong");
    CHECK(memcmp(r->buffer, "EFGHIJ", 6) == 0, "head half wrong");

    char got[16];
    unsigned n = drain(got, sizeof(got));
    CHECK(n == 10 && memcmp(got, "ABCDEFGHIJ", 10) == 0,
          "drained %u bytes across wrap", n);
}

static void test_skip_is_all_or_nothing(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = r->size;

    // Arm: leave exactly 10 bytes of free space.  One byte is always held
    // back, so free space is size - 1 - WrOff + RdOff with RdOff == 0.
    r->read_offset = 0u;
    r->write_offset = size - 11u;
    memset(r->buffer, '.', size);

    // Discriminate: the record that exactly fits is taken...
    unsigned w = onerom_rtt_write(0, "0123456789", 10);
    CHECK(w == 10, "exactly-fitting record returned %u, want 10", w);

    // ...and now, with zero space, even one byte is refused.
    unsigned before = r->write_offset;
    w = onerom_rtt_write(0, "Z", 1);
    CHECK(w == 0, "write into full ring returned %u, want 0", w);
    CHECK(r->write_offset == before, "WrOff moved on a dropped record");

    // Arm again: 9 bytes free, offer 10.  Must be dropped whole - no partial
    // record, and not one byte of it in the buffer.
    r->read_offset = 0u;
    r->write_offset = size - 10u;
    memset(r->buffer, '.', size);
    before = r->write_offset;
    w = onerom_rtt_write(0, "0123456789", 10);
    CHECK(w == 0, "over-long record returned %u, want 0", w);
    CHECK(r->write_offset == before, "WrOff moved on a dropped record");
    int clean = 1;
    for (unsigned i = 0; i < size; i++) {
        if (r->buffer[i] != '.') { clean = 0; break; }
    }
    CHECK(clean, "dropped record wrote bytes into the buffer");
}

// The reason the RdOff clamp exists.
//
// RdOff is host written.  Without the clamp, a bogus value makes the free
// space calculation wrap around to a huge number, the "does it fit" test
// passes for a record larger than the whole buffer, and the second memcpy of
// the wrap path then writes past the end of the buffer.
static void test_rdoff_clamp(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = r->size;
    static char big[8192];
    memset(big, 'A', sizeof(big));

    r->write_offset = 0u;

    // Stimulate: a hostile host leaves RdOff far outside the buffer, and a
    // record larger than the buffer is offered.
    r->read_offset = 0xFFFFFF00u;
    unsigned w = onerom_rtt_write(0, big, (unsigned)sizeof(big));
    CHECK(w == 0, "oversized record accepted with wild RdOff (returned %u)", w);
    CHECK(r->write_offset < size, "WrOff %u outside buffer", r->write_offset);

    // Just past the end is the boundary case.
    r->write_offset = 0u;
    r->read_offset = size;
    w = onerom_rtt_write(0, big, (unsigned)sizeof(big));
    CHECK(w == 0, "oversized record accepted with RdOff == size (returned %u)",
          w);
    CHECK(r->write_offset < size, "WrOff %u outside buffer", r->write_offset);

    // A legal record still works with a wild RdOff - the writer stays usable.
    r->write_offset = 0u;
    r->read_offset = 0xDEADBEEFu;
    w = onerom_rtt_write(0, "ok", 2);
    CHECK(w == 2, "legal record refused with wild RdOff (returned %u)", w);
    CHECK(memcmp(r->buffer, "ok", 2) == 0, "legal record not stored");
}

// The free space calculation has two branches, and the tests above only reach
// one of them.  A read_offset above write_offset means the ring has wrapped and
// the writer is behind the reader, and free space is then the gap between the
// two rather than the room to the end plus the room at the front.
static void test_write_when_wrapped(void) {
    onerom_rtt_up_t *r = up0();
    static char filler[128];
    memset(filler, 'W', sizeof(filler));

    // Arm: a wrapped ring with 90 bytes between writer and reader, one of which
    // is always held back - so 89 bytes are usable.
    r->write_offset = 10u;
    r->read_offset = 100u;
    r->buffer[99] = '!';

    // Discriminate: one byte more than that is refused...
    CHECK(onerom_rtt_write(0, filler, 90u) == 0u,
          "wrapped ring accepted a record one byte too long");
    CHECK(r->write_offset == 10u, "WrOff moved on a dropped record");

    // ...and exactly that is taken, without touching the byte held back.
    CHECK(onerom_rtt_write(0, filler, 89u) == 89u,
          "wrapped ring refused a record that exactly fits");
    CHECK(r->write_offset == 99u, "WrOff %u after wrapped write, want 99",
          r->write_offset);
    CHECK(r->buffer[99] == '!', "wrapped write consumed the held back byte");

    // Cross-check against query, which derives free from the pending count and
    // so does not share the arithmetic under test.
    unsigned free_now = 99u;
    onerom_rtt_query(0, NULL, &free_now, NULL);
    CHECK(free_now == 0u, "free %u after filling the gap, want 0", free_now);
}

// write_offset is the firmware's own, so a value outside the buffer means
// something has already gone wrong.  The clamp is what stops that becoming a
// copy past the end of the buffer, and it has to repair the field as well as
// the local copy - a dropped record would otherwise leave the bad value in
// place for the next writer to trip over.
static void test_write_offset_clamp(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = r->size;
    static char big[4096];
    memset(big, 'B', sizeof(big));

    // Arm: a wild WrOff, and a record too big to fit even once it is clamped.
    r->read_offset = 0u;
    r->write_offset = size + 50u;

    // Fence: the record is dropped, and the field is repaired even though
    // nothing was written.
    CHECK(onerom_rtt_write(0, big, size) == 0u,
          "oversized record accepted with a wild WrOff");
    CHECK(r->write_offset == 0u, "WrOff %u left out of range, want 0",
          r->write_offset);

    // Arm again, this time with a record that fits.
    r->read_offset = 0u;
    r->write_offset = size + 50u;
    memset(r->buffer, '.', size);

    // Fence: the bytes land at the start of the buffer rather than 50 bytes
    // past its end, and the offset that publishes them follows from there.
    CHECK(onerom_rtt_write(0, "HELLO", 5) == 5u,
          "legal record refused with a wild WrOff");
    CHECK(r->write_offset == 5u, "WrOff %u after clamped write, want 5",
          r->write_offset);
    CHECK(memcmp(r->buffer, "HELLO", 5) == 0,
          "clamped write stored the record elsewhere");
}

static void test_inactive_channel_drops(void) {
    onerom_rtt_up_t *r = up0();
    char *saved = r->buffer;
    unsigned saved_size = r->size;

    // A retired channel, as switchover will leave up[0] later on.
    r->buffer = NULL;
    r->size = 0u;
    CHECK(onerom_rtt_write(0, "x", 1) == 0, "write to retired channel accepted");

    r->buffer = saved;
    r->size = saved_size;

    // Out of range channel index.
    CHECK(onerom_rtt_write(3, "x", 1) == 0, "write to channel 3 accepted");
    CHECK(onerom_rtt_write(99, "x", 1) == 0, "write to channel 99 accepted");
}

// ---------------------------------------------------------------------------
// onerom_rtt_read / onerom_rtt_query
// ---------------------------------------------------------------------------

// Put the ring in a known empty state at a chosen offset.
static void arm_at(unsigned offset) {
    onerom_rtt_up_t *r = up0();
    r->write_offset = offset;
    r->read_offset = offset;
}

static void test_read_round_trip(void) {
    char got[64];

    // Arm.
    arm_at(0u);
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == 0u,
          "read from an empty channel returned data");

    // Stimulate: three records, read back as one stream.
    onerom_rtt_write(0, "one", 3);
    onerom_rtt_write(0, "two", 3);
    onerom_rtt_write(0, "three", 5);

    // Fence: bytes and order preserved, channel then empty.
    unsigned n = onerom_rtt_read(0, got, sizeof(got));
    CHECK(n == 11u, "read %u bytes, want 11", n);
    CHECK(memcmp(got, "onetwothree", 11) == 0, "read returned wrong bytes");
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == 0u,
          "channel not empty after draining it");
}

static void test_read_respects_max_len(void) {
    char src[100], got[100];
    for (unsigned i = 0; i < sizeof(src); i++) {
        src[i] = (char)('a' + (i % 26));
    }

    // Arm.
    arm_at(0u);
    CHECK(onerom_rtt_write(0, src, sizeof(src)) == sizeof(src),
          "setup write rejected");

    // Stimulate: take less than is there.
    unsigned n = onerom_rtt_read(0, got, 30u);

    // Fence: exactly what was asked for, and the rest is still pending.
    CHECK(n == 30u, "short read returned %u, want 30", n);
    CHECK(memcmp(got, src, 30) == 0, "short read returned wrong bytes");

    unsigned pending = 0u;
    onerom_rtt_query(0, NULL, NULL, &pending);
    CHECK(pending == 70u, "pending %u after short read, want 70", pending);

    n = onerom_rtt_read(0, got, sizeof(got));
    CHECK(n == 70u, "second read returned %u, want 70", n);
    CHECK(memcmp(got, src + 30, 70) == 0, "second read returned wrong bytes");
}

static void test_read_serves_the_wrap(void) {
    char got[32];
    onerom_rtt_up_t *r = up0();

    // Arm: 4 bytes from the end, so a 10 byte record straddles the wrap.
    arm_at(r->size - 4u);
    CHECK(onerom_rtt_write(0, "ABCDEFGHIJ", 10) == 10u, "setup write rejected");

    // Stimulate: one read, with room for all of it.
    unsigned n = onerom_rtt_read(0, got, sizeof(got));

    // Fence: the wrap is served inside the call.  A reader that stopped at the
    // end of the buffer would return 4 here.
    CHECK(n == 10u, "wrapped read returned %u, want 10", n);
    CHECK(memcmp(got, "ABCDEFGHIJ", 10) == 0, "wrapped read returned wrong bytes");
    CHECK(r->read_offset == 6u, "RdOff %u after wrapped read, want 6",
          r->read_offset);
}

static void test_read_lands_exactly_on_end(void) {
    onerom_rtt_up_t *r = up0();
    char got[32];

    // Arm: the record ends exactly on the last byte of the buffer, so the
    // copy is one memcpy that finishes at the wrap point.
    arm_at(r->size - 6u);
    CHECK(onerom_rtt_write(0, "ABCDEF", 6) == 6u, "setup write rejected");

    // Stimulate.
    unsigned n = onerom_rtt_read(0, got, sizeof(got));

    // Fence: RdOff is the discriminator.  Taking the wrapping branch here, or
    // leaving RdOff at size rather than 0, both return the right six bytes and
    // then report the whole buffer as pending on the next call.
    CHECK(n == 6u, "end-exact read returned %u, want 6", n);
    CHECK(memcmp(got, "ABCDEF", 6) == 0, "end-exact read returned wrong bytes");
    CHECK(r->read_offset == 0u, "RdOff %u after end-exact read, want 0",
          r->read_offset);
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == 0u,
          "channel not empty after end-exact read");
}

static void test_read_frees_space_for_the_writer(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = r->size;
    char got[64];

    // Arm: fill the ring, one byte always held back.
    arm_at(0u);
    char *filler = malloc(size);
    memset(filler, 'F', size);
    CHECK(onerom_rtt_write(0, filler, size - 1u) == size - 1u,
          "fill write rejected");

    // Fence the arming: a further write must now be dropped.
    CHECK(onerom_rtt_write(0, "x", 1) == 0u, "write accepted into a full ring");

    // Stimulate: take 64 bytes out.
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == sizeof(got),
          "read from a full ring came up short");

    // Fence: the writer can now use exactly that space, and no more.
    unsigned free_now = 0u;
    onerom_rtt_query(0, NULL, &free_now, NULL);
    CHECK(free_now == sizeof(got), "free %u after reading 64, want 64", free_now);
    CHECK(onerom_rtt_write(0, filler, sizeof(got) + 1u) == 0u,
          "write of free+1 accepted");
    CHECK(onerom_rtt_write(0, filler, sizeof(got)) == sizeof(got),
          "write of exactly free rejected");

    free(filler);
}

static void test_query_arithmetic(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = 0u, avail = 0u, pending = 0u;

    // Empty.
    arm_at(0u);
    onerom_rtt_query(0, &size, &avail, &pending);
    CHECK(size == 3584u, "query size %u, want 3584", size);
    CHECK(pending == 0u, "query pending %u on an empty ring", pending);
    CHECK(avail == size - 1u, "query free %u on an empty ring, want %u",
          avail, size - 1u);

    // Un-wrapped, read_offset below write_offset.
    onerom_rtt_write(0, "0123456789", 10);
    onerom_rtt_query(0, &size, &avail, &pending);
    CHECK(pending == 10u, "query pending %u after a 10 byte write", pending);
    CHECK(size == avail + pending + 1u,
          "unwrapped: size %u != free %u + pending %u + 1", size, avail, pending);

    // Wrapped, read_offset above write_offset.  This is the only state that
    // reaches the second branch of the pending arithmetic, where an unsigned
    // slip would live.
    arm_at(r->size - 4u);
    CHECK(onerom_rtt_write(0, "ABCDEFGHIJ", 10) == 10u, "wrap setup rejected");
    onerom_rtt_query(0, &size, &avail, &pending);
    CHECK(pending == 10u, "wrapped: pending %u, want 10", pending);
    CHECK(size == avail + pending + 1u,
          "wrapped: size %u != free %u + pending %u + 1", size, avail, pending);

    // Exactly full.  One byte is always held back, so the most that can be
    // pending is size - 1 and free is then 0.
    arm_at(0u);
    char *filler = malloc(r->size);
    memset(filler, 'F', r->size);
    CHECK(onerom_rtt_write(0, filler, r->size - 1u) == r->size - 1u,
          "fill write rejected");
    onerom_rtt_query(0, &size, &avail, &pending);
    CHECK(pending == size - 1u, "full: pending %u, want %u", pending, size - 1u);
    CHECK(avail == 0u, "full: free %u, want 0", avail);
    CHECK(size == avail + pending + 1u,
          "full: size %u != free %u + pending %u + 1", size, avail, pending);
    free(filler);

    // Every output is optional, and this must not fault.
    onerom_rtt_query(0, NULL, NULL, NULL);
}

static void test_set_name(void) {
    onerom_rtt_up_t *r = up0();
    static const char plugin_name[] = "plugin";

    // Arm: capture the firmware's own name, so the test does not need the
    // static symbol it lives in.
    arm_at(0u);
    const char *firmware_name = r->name;
    CHECK(firmware_name != NULL, "channel has no name to start with");

    // Stimulate: a caller's name is stored, not copied.
    CHECK(onerom_rtt_set_name(0, plugin_name) == 1u, "set_name rejected");
    CHECK(r->name == plugin_name, "name was copied rather than stored");

    // Fence: NULL restores the firmware's name rather than storing NULL, which
    // a probe would otherwise follow.
    CHECK(onerom_rtt_set_name(0, NULL) == 1u, "set_name(NULL) rejected");
    CHECK(r->name == firmware_name, "NULL did not restore the firmware name");

    // Out of range channels are rejected and change nothing.
    CHECK(onerom_rtt_set_name(3, plugin_name) == 0u, "set_name on channel 3 accepted");
    CHECK(onerom_rtt_set_name(99, plugin_name) == 0u, "set_name on channel 99 accepted");
    CHECK(r->name == firmware_name, "a rejected set_name changed channel 0");
}

static void test_read_rdoff_clamp(void) {
    onerom_rtt_up_t *r = up0();
    char got[64];

    // Arm: a record in the ring, then a host writes nonsense into RdOff.
    arm_at(0u);
    onerom_rtt_write(0, "payload", 7);
    r->read_offset = 0xFFFFFFFFu;

    // Stimulate: the clamp puts the read back at offset 0, which is where the
    // record is, so the payload comes back intact.
    unsigned n = onerom_rtt_read(0, got, sizeof(got));

    // Fence on the bytes, not on the length.  Without the clamp the offset
    // arithmetic wraps to a plausible-looking length and copies from four
    // gigabytes past the buffer, which stays in range and does not fault - so
    // only the content shows it happened.
    CHECK(n == 7u, "clamped read returned %u, want 7", n);
    CHECK(memcmp(got, "payload", 7) == 0, "clamped read returned wrong bytes");
    CHECK(r->read_offset < r->size, "RdOff %u left out of range",
          r->read_offset);

    unsigned pending = 0u;
    arm_at(0u);
    onerom_rtt_write(0, "again", 5);
    onerom_rtt_query(0, NULL, NULL, &pending);
    CHECK(pending == 5u, "pending %u after recovery, want 5", pending);
}

// The reader clamps write_offset for the same reason the writer does, and the
// clamp is what keeps the pending count describing bytes that were really
// written.
static void test_read_wroff_clamp(void) {
    onerom_rtt_up_t *r = up0();
    char got[32];

    // Arm: four bytes at the very end of the buffer, so WrOff has legitimately
    // wrapped to zero - then put a wild value there.
    arm_at(r->size - 4u);
    CHECK(onerom_rtt_write(0, "WXYZ", 4) == 4u, "setup write rejected");
    CHECK(r->write_offset == 0u, "setup left WrOff %u, want 0",
          r->write_offset);
    r->write_offset = r->size + 6u;

    // Stimulate.
    unsigned n = onerom_rtt_read(0, got, sizeof(got));

    // Fence on the count and the bytes.  Without the clamp the arithmetic
    // reports ten bytes pending here - the four that exist, plus six of
    // whatever the front of the buffer happens to hold.
    CHECK(n == 4u, "read with a wild WrOff returned %u, want 4", n);
    CHECK(memcmp(got, "WXYZ", 4) == 0, "read with a wild WrOff returned wrong bytes");
    CHECK(r->read_offset == 0u, "RdOff %u after the clamped read, want 0",
          r->read_offset);
}

// query clamps both offsets, and reports what the clamp leaves rather than
// arithmetic on nonsense.  Both branches matter: an unclamped offset here
// underflows free to something near four gigabytes, and a caller that sizes a
// write to it walks off the end of the buffer.
static void test_query_offset_clamps(void) {
    onerom_rtt_up_t *r = up0();
    unsigned size = 0u, free_now = 0u, pending = 0u;

    // Arm: five real bytes in the ring, then a wild WrOff.
    arm_at(0u);
    CHECK(onerom_rtt_write(0, "12345", 5) == 5u, "setup write rejected");
    r->write_offset = r->size + 7u;

    // Fence: nothing is claimed to be pending, and the whole buffer bar the
    // held back byte is reported free.
    onerom_rtt_query(0, &size, &free_now, &pending);
    CHECK(size == r->size, "query size %u with a wild WrOff, want %u", size,
          r->size);
    CHECK(pending == 0u, "pending %u with a wild WrOff, want 0", pending);
    CHECK(free_now == size - 1u, "free %u with a wild WrOff, want %u", free_now,
          size - 1u);

    // Arm again: the same five bytes, and this time a wild RdOff.  The clamp
    // puts the reader back at zero, which is where the record starts, so all
    // five are still pending.
    arm_at(0u);
    CHECK(onerom_rtt_write(0, "12345", 5) == 5u, "setup write rejected");
    r->read_offset = r->size + 9u;

    size = free_now = pending = 0u;
    onerom_rtt_query(0, &size, &free_now, &pending);
    CHECK(pending == 5u, "pending %u with a wild RdOff, want 5", pending);
    CHECK(size == free_now + pending + 1u,
          "wild RdOff: size %u != free %u + pending %u + 1", size, free_now,
          pending);

    r->read_offset = 0u;
}

static void test_read_inactive_channel(void) {
    onerom_rtt_up_t *r = up0();
    char *saved = r->buffer;
    unsigned saved_size = r->size;
    char got[8];
    unsigned size = 99u, avail = 99u, pending = 99u;

    // A retired channel reads empty and reports zeros.
    r->buffer = NULL;
    r->size = 0u;
    CHECK(onerom_rtt_read(0, got, sizeof(got)) == 0u,
          "read from a retired channel returned data");
    onerom_rtt_query(0, &size, &avail, &pending);
    CHECK(size == 0u && avail == 0u && pending == 0u,
          "retired channel reported %u/%u/%u, want 0/0/0", size, avail, pending);

    r->buffer = saved;
    r->size = saved_size;

    // Out of range channel index.
    CHECK(onerom_rtt_read(3, got, sizeof(got)) == 0u, "read of channel 3 accepted");
    CHECK(onerom_rtt_read(99, got, sizeof(got)) == 0u, "read of channel 99 accepted");

    // A NULL destination and a zero length, with data actually pending.  State
    // the precondition rather than inheriting it: with an empty channel both
    // return 0 whether or not they are checked, and the coverage is worthless.
    arm_at(0u);
    CHECK(onerom_rtt_write(0, "data", 4) == 4u, "setup write rejected");
    CHECK(onerom_rtt_read(0, NULL, sizeof(got)) == 0u, "read into NULL accepted");
    CHECK(onerom_rtt_read(0, got, 0u) == 0u, "zero length read returned data");

    pending = 0u;
    onerom_rtt_query(0, NULL, NULL, &pending);
    CHECK(pending == 4u, "pending %u after refused reads, want 4", pending);

    size = avail = pending = 99u;
    onerom_rtt_query(99, &size, &avail, &pending);
    CHECK(size == 0u && avail == 0u && pending == 0u,
          "out of range query reported %u/%u/%u, want 0/0/0", size, avail, pending);
}

void fmt_tests(int *checks, int *failures, const char *const **first,
               int *first_count);

int main(void) {
    printf("rtt ring tests\n");
    test_lazy_init();
    test_plugins_init();
    test_records_are_whole_and_ordered();
    test_wrap();
    test_skip_is_all_or_nothing();
    test_rdoff_clamp();
    test_write_when_wrapped();
    test_write_offset_clamp();
    test_inactive_channel_drops();
    test_read_round_trip();
    test_read_respects_max_len();
    test_read_serves_the_wrap();
    test_read_lands_exactly_on_end();
    test_read_frees_space_for_the_writer();
    test_query_arithmetic();
    test_query_offset_clamps();
    test_set_name();
    test_read_rdoff_clamp();
    test_read_wroff_clamp();
    test_read_inactive_channel();
    printf("  ring: %d checks, %d failures\n", checks, failures);

    int fchecks = 0, ffailures = 0, fcount = 0;
    const char *const *ffirst = NULL;
    fmt_tests(&fchecks, &ffailures, &ffirst, &fcount);
    printf("  formatter: %d checks, %d failures\n", fchecks, ffailures);
    for (int i = 0; i < fcount; i++) {
        printf("    %s\n", ffirst[i]);
    }

    checks += fchecks;
    failures += ffailures;
    printf("%d checks, %d failures\n", checks, failures);
    return failures ? 1 : 0;
}
