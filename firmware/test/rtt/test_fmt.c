// Host side test of firmware/src/rtt.c's formatter.
//
// Most of this is differential: the same format string and argument go to the
// host's vsnprintf and to onerom_rtt_vprintf, and the two must agree.  The
// host libc is a valid oracle for the standard conversions, which is what
// makes it worth generating combinations rather than hand writing cases.
//
// It is not an oracle everywhere.  %b and %B are C23 and may not be
// implemented by the host libc; %p is implementation defined; and passing NULL
// to %s is undefined behaviour on the host even though our formatter defines
// it.  Those are checked against explicit expectations instead.

#include "include.h"
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <stdlib.h>

static int fmt_checks;
static int fmt_failures;
static const char *fmt_first_failures[8];
static char fmt_failure_store[8][160];
static int fmt_failure_count;

// Collect our formatter's output by draining the ring it writes into.
static void fmt_capture(char *out, size_t max) {
    onerom_rtt_up_t *r = &_SEGGER_RTT.up[0];
    size_t n = 0;
    while ((r->read_offset != r->write_offset) && (n + 1 < max)) {
        out[n++] = r->buffer[r->read_offset];
        r->read_offset = (r->read_offset + 1u) % r->size;
    }
    out[n] = '\0';
}

static void fmt_reset(void) {
    onerom_rtt_up_t *r = &_SEGGER_RTT.up[0];
    // A write must have happened for the control block to be initialised.
    if (r->size == 0u) {
        onerom_rtt_write(0, "", 0);
        onerom_rtt_write(0, "x", 1);
        r = &_SEGGER_RTT.up[0];
    }
    r->read_offset = 0u;
    r->write_offset = 0u;
}

static void fmt_record(const char *fmt, const char *want, const char *got) {
    fmt_failures++;
    if (getenv("FMT_DUMP")) fprintf(stderr, "%s|%s|%s\n", fmt, want, got);
    if (fmt_failure_count < 8) {
        snprintf(fmt_failure_store[fmt_failure_count],
                 sizeof(fmt_failure_store[0]),
                 "fmt \"%s\": want \"%s\" got \"%s\"", fmt, want, got);
        fmt_first_failures[fmt_failure_count] = fmt_failure_store[fmt_failure_count];
        fmt_failure_count++;
    }
}

// Differential check.  The format string is built at run time so combinations
// can be generated, which is why -Wformat-nonliteral is silenced here.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wformat-nonliteral"
static void diff(const char *fmt, ...) {
    char want[512], got[512];
    va_list a;

    va_start(a, fmt);
    vsnprintf(want, sizeof(want), fmt, a);
    va_end(a);

    fmt_reset();
    va_start(a, fmt);
    onerom_rtt_vprintf(0, fmt, &a);
    va_end(a);
    fmt_capture(got, sizeof(got));

    fmt_checks++;
    if (strcmp(want, got) != 0) {
        fmt_record(fmt, want, got);
    }
}
#pragma GCC diagnostic pop

// Explicit check, for cases where the host is not an oracle.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wformat-nonliteral"
static void expect(const char *want, const char *fmt, ...) {
    char got[512];
    va_list a;

    fmt_reset();
    va_start(a, fmt);
    onerom_rtt_vprintf(0, fmt, &a);
    va_end(a);
    fmt_capture(got, sizeof(got));

    fmt_checks++;
    if (strcmp(want, got) != 0) {
        fmt_record(fmt, want, got);
    }
}
#pragma GCC diagnostic pop

static const char *const FLAGS[] = { "", "-", "0", "#", "+", " ", "0-", "+0", "-#", "# " };
static const int WIDTHS[]        = { -1, 0, 1, 2, 5, 8, 12 };
static const int PRECS[]         = { -1, 0, 1, 3, 8 };

static void build(char *out, const char *flags, int width, int prec,
                  const char *len, char conv) {
    size_t n = 0;
    out[n++] = '%';
    n += (size_t)snprintf(out + n, 24, "%s", flags);
    if (width >= 0) n += (size_t)snprintf(out + n, 24, "%d", width);
    if (prec >= 0)  n += (size_t)snprintf(out + n, 24, ".%d", prec);
    n += (size_t)snprintf(out + n, 24, "%s", len);
    out[n++] = conv;
    out[n] = '\0';
}

static void sweep_signed(void) {
    static const long long values[] = {
        0, 1, -1, 42, -42, 9, -9, 255, -255,
        2147483647LL, -2147483648LL, 4294967295LL,
        9223372036854775807LL, -9223372036854775807LL - 1,
    };
    char fmt[32];

    for (size_t f = 0; f < sizeof(FLAGS)/sizeof(FLAGS[0]); f++) {
        for (size_t w = 0; w < sizeof(WIDTHS)/sizeof(WIDTHS[0]); w++) {
            for (size_t p = 0; p < sizeof(PRECS)/sizeof(PRECS[0]); p++) {
                for (size_t v = 0; v < sizeof(values)/sizeof(values[0]); v++) {
                    long long val = values[v];
                    for (int c = 0; c < 2; c++) {
                        char conv = (c == 0) ? 'd' : 'i';
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "", conv);
                        diff(fmt, (int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "hh", conv);
                        diff(fmt, (int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "h", conv);
                        diff(fmt, (int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "l", conv);
                        diff(fmt, (long)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "ll", conv);
                        diff(fmt, (long long)val);
                    }
                }
            }
        }
    }
}

static void sweep_unsigned(void) {
    static const unsigned long long values[] = {
        0u, 1u, 7u, 8u, 42u, 255u, 256u, 65535u, 1000000u,
        4294967295ULL, 4294967296ULL,
        18446744073709551615ULL, 12345678901234567890ULL,
    };
    static const char convs[] = { 'u', 'o', 'x', 'X' };
    char fmt[32];

    for (size_t f = 0; f < sizeof(FLAGS)/sizeof(FLAGS[0]); f++) {
        for (size_t w = 0; w < sizeof(WIDTHS)/sizeof(WIDTHS[0]); w++) {
            for (size_t p = 0; p < sizeof(PRECS)/sizeof(PRECS[0]); p++) {
                for (size_t v = 0; v < sizeof(values)/sizeof(values[0]); v++) {
                    unsigned long long val = values[v];
                    for (size_t c = 0; c < sizeof(convs); c++) {
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "", convs[c]);
                        diff(fmt, (unsigned int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "hh", convs[c]);
                        diff(fmt, (unsigned int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "h", convs[c]);
                        diff(fmt, (unsigned int)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "l", convs[c]);
                        diff(fmt, (unsigned long)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "ll", convs[c]);
                        diff(fmt, (unsigned long long)val);
                        build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "z", convs[c]);
                        diff(fmt, (size_t)val);
                    }
                }
            }
        }
    }
}

static void sweep_string_and_char(void) {
    static const char *const strings[] = { "", "a", "abc", "hello world",
                                           "0123456789012345678901234567890123456789" };
    char fmt[32];

    for (size_t f = 0; f < sizeof(FLAGS)/sizeof(FLAGS[0]); f++) {
        for (size_t w = 0; w < sizeof(WIDTHS)/sizeof(WIDTHS[0]); w++) {
            for (size_t p = 0; p < sizeof(PRECS)/sizeof(PRECS[0]); p++) {
                // The 0 flag is undefined for s and c conversions, so the
                // host libc is not an oracle for them: it zero pads, we space
                // pad, and neither is required to be right.  Skip rather than
                // encode one libc's choice as the expectation.
                if (strchr(FLAGS[f], '0') != NULL) {
                    continue;
                }
                for (size_t s = 0; s < sizeof(strings)/sizeof(strings[0]); s++) {
                    build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "", 's');
                    diff(fmt, strings[s]);
                }
                build(fmt, FLAGS[f], WIDTHS[w], PRECS[p], "", 'c');
                diff(fmt, 'A');
            }
        }
    }
}

// Width and precision taken from an argument.
static void sweep_star(void) {
    for (int w = -6; w <= 8; w++) {
        diff("%*d", w, 42);
        diff("%*s", w, "abc");
        diff("%*x", w, 0xabcu);
    }
    for (int p = 0; p <= 8; p++) {
        diff("%.*d", p, 42);
        diff("%.*s", p, "abcdefghij");
        diff("%.*u", p, 7u);
        diff("%8.*d", p, 42);
        diff("%-8.*s", p, "abcdefghij");
    }
    diff("%*.*d", 10, 4, 42);
    diff("%-*.*s", 12, 3, "abcdefghij");
}

// The formatter flushes in 64 byte chunks, so anything spanning a boundary is
// worth pinning: a bug there would corrupt long lines only.
static void sweep_chunking(void) {
    char big[300];
    for (size_t i = 0; i < sizeof(big) - 1; i++) {
        big[i] = (char)('a' + (i % 26));
    }
    big[sizeof(big) - 1] = '\0';

    for (int len = 55; len <= 75; len++) {
        char fmt[16];
        snprintf(fmt, sizeof(fmt), "%%.%ds", len);
        diff(fmt, big);
    }
    diff("%s", big);
    diff("prefix %s suffix", big);
    diff("%s%s%s", big, big, big);
    diff("%d %s %u %x %c", -12345, big, 4294967295u, 0xdeadbeefu, 'Z');
}

// Literal text, escapes, and the shapes the firmware actually logs.
static void sweep_real_world(void) {
    diff("100%% done");
    diff("%d%%", 50);
    diff("");
    diff("no conversions here");
    diff("trailing %");

    // Shapes taken from firmware/src and plugins.
    diff("One ROM v%d.%d.%d.%d", 0, 7, 2, 1);
    diff("Set #%d: %d ROM(s), size: %d bytes", 2, 3, 8192);
    diff("onerom_info: 0x%08X", 0x100001d4u);
    diff("RT Fire Freq: 0x%04X", 0x096u);
    diff("RT Overclock Enabled: 0x%02X", 1u);
    diff("Core 1 launching plugin at 0x%08x", 0x10001234u);
    diff("Painting core 1 stack from 0x%08x to 0x%08x with 0x%02x",
         0x20081800u, 0x20081c00u, 0x55u);
    diff("CS2: qual_base=%u qual_pins=%u inact_pat=0x%02x", 3u, 2u, 0xffu);
    diff("Sel GPIO val: 0x%08lX%08lX #: %lu mask 0x%08lX%08lX",
         0x12345678ul, 0x9abcdef0ul, 4ul, 0xfedcba98ul, 0x76543210ul);
    diff("  Chip #%d: %s, %s", 1, "kernal.rom", "2364");

    // The 64 bit form that firmware/src/utils.c splits in two today.
    diff("Sel GPIO val: 0x%016llX", 0x123456789abcdef0ull);
    diff("%llu", 18446744073709551615ull);
    diff("%llu", 12345678901234567890ull);
    diff("%lld", -9223372036854775807ll - 1);
}

// Cases where the host libc is not a valid oracle.
static void explicit_cases(void) {
    // Binary is C23 and may be absent from the host libc.
    expect("0",         "%b", 0u);
    expect("1",         "%b", 1u);
    expect("101010",    "%b", 42u);
    expect("11111111",  "%b", 255u);
    expect("  1010",    "%6b", 10u);
    expect("001010",    "%06b", 10u);
    expect("1010  ",    "%-6b", 10u);
    expect("0b1010",    "%#b", 10u);
    expect("0B1010",    "%#B", 10u);
    expect("1111111111111111111111111111111111111111111111111111111111111111",
                        "%llb", 18446744073709551615ull);

    // Pointer format is implementation defined; ours is 0x then lower hex.
    expect("0x1234abcd", "%p", (void *)0x1234abcdu);
    expect("0x0",        "%p", (void *)0);

    // NULL through %s is undefined on the host, defined here.
    expect("(null)",     "%s", (const char *)NULL);
    expect("  (null)",   "%8s", (const char *)NULL);

    // Refused conversions produce a visible marker, and still consume their
    // argument so that everything after them stays in step.
    expect("%!f",          "%f", 1.5);
    expect("%!f then 7",   "%f then %d", 1.5, 7);
    expect("%!e then 7",   "%e then %d", 1.5, 7);
    expect("%!g|%!a|9",    "%g|%a|%d", 1.5, 2.5, 9);
    expect("%!n after",    "%n after", (int *)NULL);
    expect("%!q",          "%q");
    expect("a%!qb",        "a%qb");
}

void fmt_tests(int *checks, int *failures, const char *const **first,
               int *first_count) {
    fmt_checks = 0;
    fmt_failures = 0;
    fmt_failure_count = 0;

    sweep_signed();
    sweep_unsigned();
    sweep_string_and_char();
    sweep_star();
    sweep_chunking();
    sweep_real_world();
    explicit_cases();

    *checks = fmt_checks;
    *failures = fmt_failures;
    *first = fmt_first_failures;
    *first_count = fmt_failure_count;
}
