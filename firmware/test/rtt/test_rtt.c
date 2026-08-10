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

void fmt_tests(int *checks, int *failures, const char *const **first,
               int *first_count);

int main(void) {
    printf("rtt ring tests\n");
    test_lazy_init();
    test_records_are_whole_and_ordered();
    test_wrap();
    test_skip_is_all_or_nothing();
    test_rdoff_clamp();
    test_inactive_channel_drops();
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
