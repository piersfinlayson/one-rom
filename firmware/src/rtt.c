// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM RTT - ring and formatter
//
// A One ROM implementation of real time transfer logging, binary compatible
// with SEGGER RTT.  See include/rtt.h for exactly what "binary compatible"
// covers.  Everything here is One ROM's own.
//
// Design notes:
//
// - Only skip mode is implemented, whatever a host writes into a channel's
//   flags.  Blocking would stall the calling core until a host drained the
//   buffer, and on a device whose job is serving a ROM bus to a running retro
//   system that is a correctness hazard, not just a slow path.  A record that
//   does not fit is dropped whole, so a reader never sees a partial record.
// - read_offset is host written, and is range checked before it is used in any
//   arithmetic.  See onerom_rtt_avail().
// - Interrupts are masked across the ring's read-modify-write, because an
//   interrupt handler may log.  See onerom_rtt_lock().
// - No lock between cores.  There is one writer per channel.

#include "include.h"
#include <stddef.h>

// Built for every target, including the emulator and the host side tests.  The
// two Cortex-M specific operations - the data memory barrier and the interrupt
// mask - are overridable, and a host build substitutes them before including
// this file's header.  Everything else is portable C, so the ring a test
// exercises is the ring a device runs.

// Size of the up buffer.
#define ONEROM_RTT_UP_BUFFER_SIZE       3584

// Size of the down buffer.  Nothing reads host to target data yet, but the
// channel is declared so a probe sees a complete control block.
#define ONEROM_RTT_DOWN_BUFFER_SIZE     16

// Formatter chunk size.  Output is collected here and flushed to the ring as
// it fills, so a line longer than this spans more than one record.
#define ONEROM_RTT_PRINTF_BUFFER_SIZE   64

// Data memory barrier.  Cortex-M33 may reorder memory accesses, and a host
// reads this memory asynchronously over SWD, so the ordering between the data
// write and the write_offset update that publishes it has to be explicit.
//
// Defined only if not already provided, so that a host side test of the ring
// can substitute a compiler barrier for an instruction it cannot execute.
#if !defined(ONEROM_RTT_DMB)
#define ONEROM_RTT_DMB()    __asm volatile ("dmb" ::: "memory")
#endif

// Mask interrupts on this core, returning the previous PRIMASK so it can be
// restored.  Nested and pre-masked callers therefore stay masked.
//
// This exists because an interrupt handler may log: vbus_connect_handler() in
// rp235x.c does, on its way to the bootloader.  Without this, that handler can
// preempt a write part way through, and the interrupted write then republishes
// write_offset from its own stale copy, discarding the handler's record - the
// last line before a reboot, and the one most worth having.
//
// PRIMASK, not BASEPRI: nothing in this firmware sets an NVIC priority, so
// every interrupt sits at the reset default of 0, which a BASEPRI of 0x20 does
// not mask.
//
// The masked window covers the copy, so it scales with record length.  The
// formatter flushes in ONEROM_RTT_PRINTF_BUFFER_SIZE chunks, and a caller that
// passes a large buffer to onerom_rtt_write() directly should expect a
// correspondingly longer window.
#if !defined(ONEROM_RTT_LOCK)
static inline uint32_t onerom_rtt_lock(void) {
    uint32_t primask;
    __asm volatile ("mrs %0, primask \n\t"
                    "cpsid i"
                    : "=r" (primask) :: "memory");
    return primask;
}

static inline void onerom_rtt_unlock(uint32_t primask) {
    __asm volatile ("msr primask, %0" :: "r" (primask) : "memory");
}
#endif

// Release helper for the cleanup attribute below.
static inline void onerom_rtt_unlock_cleanup(const uint32_t *primask) {
    onerom_rtt_unlock(*primask);
}

// Take the interrupt lock for the enclosing scope.
//
// GCC has no "single exit" or "no early return" attribute, so rather than
// forbid early returns the unlock is tied to scope exit: cleanup runs the
// release helper however the scope is left.  An early return added here later
// is therefore correct by construction, instead of being a comment somebody
// has to notice.
#define ONEROM_RTT_CRITICAL_SECTION()                                         \
    uint32_t onerom_rtt_primask                                               \
        __attribute__((cleanup(onerom_rtt_unlock_cleanup))) =                 \
            onerom_rtt_lock()

// The control block.
//
// Must live in .bss and be populated at runtime rather than statically
// initialised: common.ld asserts SIZEOF(.data) == 0.
onerom_rtt_cb_t _SEGGER_RTT;

// The channel buffers, pointed at by the control block once initialised.
static char onerom_rtt_up_buffer[ONEROM_RTT_UP_BUFFER_SIZE];
static char onerom_rtt_down_buffer[ONEROM_RTT_DOWN_BUFFER_SIZE];

// Channel name.  Shown by probe front ends, never used to identify a channel,
// which is done by index.
static const char onerom_rtt_channel_name[] = "Terminal";

// ---------------------------------------------------------------------------
// Ring
// ---------------------------------------------------------------------------

// Initialise the control block, once.
//
// Called from the write path rather than from log_init(), because the first
// log can happen before log_init() does: firmware_main() may ERR() on invalid
// metadata and enter the bootloader without ever reaching LOG_INIT().
static void onerom_rtt_init(void) {
    // The identifier is stored reversed and copied back to front, so that the
    // literal "SEGGER RTT" never appears in the firmware image.  A probe
    // locates the control block by scanning memory for that string, and would
    // otherwise be able to find this constant and take it for the block.
    static const char init_id[] = "\0\0\0\0\0\0TTR REGGES";

    // Volatile so the compiler cannot reorder the accesses below, in
    // particular so it cannot hoist the identifier stores above the rest.
    volatile onerom_rtt_cb_t *cb = &_SEGGER_RTT;
    unsigned ii;

    if (cb->id[0] != '\0') {
        return;
    }

    memset((void *)cb, 0, sizeof(_SEGGER_RTT));
    cb->max_up_buffers = ONEROM_RTT_MAX_UP_BUFFERS;
    cb->max_down_buffers = ONEROM_RTT_MAX_DOWN_BUFFERS;

    cb->up[0].name = onerom_rtt_channel_name;
    cb->up[0].buffer = onerom_rtt_up_buffer;
    cb->up[0].size = ONEROM_RTT_UP_BUFFER_SIZE;
    cb->up[0].read_offset = 0u;
    cb->up[0].write_offset = 0u;
    cb->up[0].flags = ONEROM_RTT_MODE_NO_BLOCK_SKIP;

    cb->down[0].name = onerom_rtt_channel_name;
    cb->down[0].buffer = onerom_rtt_down_buffer;
    cb->down[0].size = ONEROM_RTT_DOWN_BUFFER_SIZE;
    cb->down[0].read_offset = 0u;
    cb->down[0].write_offset = 0u;
    cb->down[0].flags = ONEROM_RTT_MODE_NO_BLOCK_SKIP;

    // The identifier goes in last, and behind a barrier: a host that finds it
    // must find a complete control block behind it.
    ONEROM_RTT_DMB();
    for (ii = 0; ii < (sizeof(init_id) - 1u); ii++) {
        cb->id[ii] = init_id[sizeof(init_id) - 2u - ii];
    }
    ONEROM_RTT_DMB();
}

// Prepare the log for plugins.
//
// Plugin launch is the one point where core 0 is in firmware code and core 1
// is not yet running, so everything here is straight-line code with nothing to
// race against.
void onerom_rtt_plugins_init(void) {
    // The control block is otherwise built on first use.  Both cores reach the
    // logging API, so building it here means every later call finds it built,
    // whatever BOOT_LOGGING is set to.
    onerom_rtt_init();
}

// Bytes that can be written without overtaking the host's read position.
//
// One byte is always left free, which is how a full buffer is told apart from
// an empty one.
//
// read_offset is the one field of an up channel the host owns, and it is
// therefore untrusted.  A confused or hostile host can leave a value outside
// the buffer there, and the arithmetic below would then hand back a free space
// figure large enough to walk the write off the end of the buffer.  A clamped
// local copy keeps the writer in bounds without fighting the host for
// ownership of the field.
static unsigned onerom_rtt_avail(const onerom_rtt_up_t *ring,
                                 unsigned size,
                                 unsigned write_offset) {
    unsigned read_offset = ring->read_offset;

    if (read_offset >= size) {
        read_offset = 0u;
    }

    if (read_offset <= write_offset) {
        return size - 1u - write_offset + read_offset;
    }

    return read_offset - write_offset - 1u;
}

unsigned onerom_rtt_write(unsigned channel, const void *buf, unsigned len) {
    const char *data = (const char *)buf;
    onerom_rtt_up_t *ring;
    unsigned size, write_offset, to_end;

    // The control block init and the read-modify-write of the ring are one
    // critical section, released on any exit from this function.
    ONEROM_RTT_CRITICAL_SECTION();

    onerom_rtt_init();

    if (channel >= (unsigned)_SEGGER_RTT.max_up_buffers) {
        return 0u;
    }
    ring = &_SEGGER_RTT.up[channel];

    // A channel with no buffer drops silently.  That is what an unpopulated
    // channel looks like, and what a retired one will look like.
    size = ring->size;
    if ((ring->buffer == 0) || (size == 0u)) {
        return 0u;
    }

    // write_offset is ours, so this should never fire.  Checking it costs two
    // instructions and removes any question of a value the host could have
    // reached reaching the copy below.
    write_offset = ring->write_offset;
    if (write_offset >= size) {
        write_offset = 0u;
        ring->write_offset = 0u;
    }

    // Skip mode: if the whole record does not fit, drop the whole record.
    if (onerom_rtt_avail(ring, size, write_offset) < len) {
        return 0u;
    }

    to_end = size - write_offset;
    if (to_end > len) {
        memcpy(ring->buffer + write_offset, data, len);
        // The data must be complete before write_offset publishes it.
        ONEROM_RTT_DMB();
        ring->write_offset = write_offset + len;
    } else {
        memcpy(ring->buffer + write_offset, data, to_end);
        memcpy(ring->buffer, data + to_end, len - to_end);
        ONEROM_RTT_DMB();
        ring->write_offset = len - to_end;
    }

    return len;
}

// Bytes written to a channel that a reader has not taken yet.
//
// Both offsets are taken by the caller, once, and passed in already clamped.
// Loading them here instead would sample read_offset a second time, and a host
// that moves it between the two samples makes the result disagree with
// whatever the caller derived from its own sample.
static unsigned onerom_rtt_pending(unsigned size,
                                   unsigned write_offset,
                                   unsigned read_offset) {
    if (write_offset >= read_offset) {
        return write_offset - read_offset;
    }

    return size - read_offset + write_offset;
}

unsigned onerom_rtt_read(unsigned channel, void *buf, unsigned max_len) {
    char *dest = (char *)buf;
    onerom_rtt_up_t *ring;
    unsigned size, write_offset, read_offset, pending, to_end;

    ONEROM_RTT_CRITICAL_SECTION();

    onerom_rtt_init();

    if ((channel >= (unsigned)_SEGGER_RTT.max_up_buffers) || (buf == 0)) {
        return 0u;
    }
    ring = &_SEGGER_RTT.up[channel];

    size = ring->size;
    if ((ring->buffer == 0) || (size == 0u)) {
        return 0u;
    }

    // One sample of each offset, clamped once, and everything below derived
    // from those locals.  read_offset is host written, so a second load could
    // return a different value: the copy would then start somewhere the
    // pending count never covered, and publishing read_offset afterwards could
    // put it past write_offset, which reads as a nearly full ring of stale
    // bytes until the writer laps it.
    write_offset = ring->write_offset;
    if (write_offset >= size) {
        write_offset = 0u;
    }
    read_offset = ring->read_offset;
    if (read_offset >= size) {
        read_offset = 0u;
    }

    pending = onerom_rtt_pending(size, write_offset, read_offset);
    if (pending > max_len) {
        pending = max_len;
    }
    if (pending == 0u) {
        return 0u;
    }

    // The pending count was derived from write_offset, so the data it covers
    // must be visible before it is copied.
    ONEROM_RTT_DMB();

    to_end = size - read_offset;
    if (to_end > pending) {
        memcpy(dest, ring->buffer + read_offset, pending);
        read_offset += pending;
    } else {
        memcpy(dest, ring->buffer + read_offset, to_end);
        memcpy(dest + to_end, ring->buffer, pending - to_end);
        read_offset = pending - to_end;
    }

    // The copy must be complete before read_offset releases the space, or a
    // writer can overwrite bytes still being read.
    ONEROM_RTT_DMB();
    ring->read_offset = read_offset;

    return pending;
}

unsigned onerom_rtt_set_name(unsigned channel, const char *name) {
    ONEROM_RTT_CRITICAL_SECTION();

    onerom_rtt_init();

    if (channel >= (unsigned)_SEGGER_RTT.max_up_buffers) {
        return 0u;
    }

    // A caller-supplied name is stored, not copied: the descriptor holds a
    // pointer a probe reads from target memory, and there is nowhere to copy
    // it to.  Passing NULL puts the channel back on the firmware's own name,
    // which is what makes it safe for the caller to reuse its string.
    //
    // The caller may have just written the characters, so they must be visible
    // before the pointer that exposes them, exactly as for a record and its
    // write_offset.
    ONEROM_RTT_DMB();
    _SEGGER_RTT.up[channel].name =
        (name != 0) ? name : onerom_rtt_channel_name;

    return 1u;
}

void onerom_rtt_query(unsigned channel,
                      unsigned *size_out,
                      unsigned *free_out,
                      unsigned *pending_out) {
    const onerom_rtt_up_t *ring;
    unsigned size = 0u, free = 0u, pending = 0u, write_offset, read_offset;

    // One sample of each offset, and both results derived from those locals.
    // That is what makes size == free + pending + 1 true: computing them from
    // separate loads lets a host move read_offset in between, and the two then
    // describe different moments.  free would be reported stale high, which is
    // precisely what a caller sizing a write to it must not be told.
    {
        ONEROM_RTT_CRITICAL_SECTION();

        onerom_rtt_init();

        if (channel < (unsigned)_SEGGER_RTT.max_up_buffers) {
            ring = &_SEGGER_RTT.up[channel];
            size = ring->size;
            if ((ring->buffer == 0) || (size == 0u)) {
                size = 0u;
            } else {
                write_offset = ring->write_offset;
                if (write_offset >= size) {
                    write_offset = 0u;
                }
                read_offset = ring->read_offset;
                if (read_offset >= size) {
                    read_offset = 0u;
                }
                pending = onerom_rtt_pending(size, write_offset, read_offset);
                free = size - 1u - pending;
            }
        }
    }

    if (size_out != 0) {
        *size_out = size;
    }
    if (free_out != 0) {
        *free_out = free;
    }
    if (pending_out != 0) {
        *pending_out = pending;
    }
}

// ---------------------------------------------------------------------------
// Formatter
// ---------------------------------------------------------------------------
//
// Supported, and this list is the contract - it is what plugin authors may
// rely on, and removing from it is a compatibility break:
//
//   conversions  d i u o x X b B c s p %
//   flags        0 - # + space
//   width        any decimal value, or * to take it from an argument
//   precision    .N or .*, on both strings and integers
//   length       hh h l ll z
//
// Not supported, and deliberately so: floating point (there is none in this
// firmware), %n (a security hazard with no use here), the j and t length
// modifiers, and C23's %wN bit precise forms.  An unsupported or unrecognised
// conversion is emitted as a visible marker - %!f and the like - rather than
// silently ignored, and the matching argument is still consumed so that
// everything after it in the same record stays in step.

#define FMT_LEFT    (1u << 0)
#define FMT_ZERO    (1u << 1)
#define FMT_ALT     (1u << 2)
#define FMT_PLUS    (1u << 3)
#define FMT_SPACE   (1u << 4)
// %p always carries its 0x prefix, including for a null pointer.  That differs
// from %#x, where C suppresses the prefix for a zero value.
#define FMT_PTR     (1u << 5)

typedef enum {
    FMT_LEN_INT,
    FMT_LEN_CHAR,
    FMT_LEN_SHORT,
    FMT_LEN_LONG,
    FMT_LEN_LLONG,
    FMT_LEN_SIZE,
} fmt_len_t;

typedef struct {
    char buf[ONEROM_RTT_PRINTF_BUFFER_SIZE];
    unsigned len;
    unsigned channel;
} fmt_state_t;

static void fmt_flush(fmt_state_t *f) {
    if (f->len != 0u) {
        onerom_rtt_write(f->channel, f->buf, f->len);
        f->len = 0u;
    }
}

static void fmt_putc(fmt_state_t *f, char c) {
    if (f->len >= sizeof(f->buf)) {
        fmt_flush(f);
    }
    f->buf[f->len++] = c;
}

static void fmt_pad(fmt_state_t *f, char c, int count) {
    while (count-- > 0) {
        fmt_putc(f, c);
    }
}

// Divide by ten without libgcc.
//
// The linker script discards libgcc.a, so a 64 bit division would not link -
// there is no __aeabi_uldivmod to call.  Values that fit in 32 bits use the
// Cortex-M33 hardware divider instead, so this runs at most a couple of times
// per conversion.  All shifts are by constants, which keeps the 64 bit shift
// helpers out of it too.
static uint64_t fmt_div10(uint64_t n, unsigned *rem) {
    uint64_t q;
    uint64_t r;

    q = (n >> 1) + (n >> 2);
    q += (q >> 4);
    q += (q >> 8);
    q += (q >> 16);
    q += (q >> 32);
    q >>= 3;

    r = n - (((q << 2) + q) << 1);
    if (r > 9u) {
        q += 1u;
        r -= 10u;
    }

    *rem = (unsigned)r;
    return q;
}

// Convert to digits, most significant last.  Returns the digit count.
static unsigned fmt_digits(uint64_t value, unsigned base, int upper, char *out) {
    static const char lower[] = "0123456789abcdef";
    static const char upper_digits[] = "0123456789ABCDEF";
    const char *map = upper ? upper_digits : lower;
    unsigned n = 0u;
    uint32_t v32;

    if (value == 0u) {
        out[n++] = '0';
        return n;
    }

    // 64 bit range, base 10 only: strip down to 32 bits without a libgcc call.
    if (base == 10u) {
        unsigned rem;
        while (value > 0xFFFFFFFFu) {
            value = fmt_div10(value, &rem);
            out[n++] = map[rem];
        }
        v32 = (uint32_t)value;
        while (v32 != 0u) {
            out[n++] = map[v32 % 10u];
            v32 /= 10u;
        }
        return n;
    }

    // Power of two bases need no division at all.
    while (value != 0u) {
        out[n++] = map[(unsigned)(value & (uint64_t)(base - 1u))];
        switch (base) {
        case 2u:  value >>= 1; break;
        case 8u:  value >>= 3; break;
        default:  value >>= 4; break;
        }
    }
    return n;
}

// The + and space flags request a sign, and a sign only exists for a signed
// conversion.  C ignores them for u, o, x, X and b.
static inline unsigned fmt_unsigned_flags(unsigned flags) {
    return flags & ~(FMT_PLUS | FMT_SPACE);
}

static void fmt_number(fmt_state_t *f, uint64_t value, unsigned base, int upper,
                       int negative, unsigned flags, int width, int precision) {
    char digits[64];
    char lead[3];
    unsigned ndigits = 0u;
    unsigned nlead = 0u;
    int zeros = 0;
    int pad;
    unsigned ii;

    // A precision of zero prints nothing at all for a zero value.
    if (!((precision == 0) && (value == 0u))) {
        ndigits = fmt_digits(value, base, upper, digits);
    }

    if (negative) {
        lead[nlead++] = '-';
    } else if ((flags & FMT_PLUS) != 0u) {
        lead[nlead++] = '+';
    } else if ((flags & FMT_SPACE) != 0u) {
        lead[nlead++] = ' ';
    }

    if ((flags & FMT_ALT) != 0u) {
        if ((base == 16u) && ((value != 0u) || ((flags & FMT_PTR) != 0u))) {
            lead[nlead++] = '0';
            lead[nlead++] = upper ? 'X' : 'x';
        } else if ((base == 2u) && (value != 0u)) {
            lead[nlead++] = '0';
            lead[nlead++] = upper ? 'B' : 'b';
        } else if (base == 8u) {
            // Force the first digit to zero, if and only if necessary.  A
            // zero value already begins with one, and "%#.0o" of zero still
            // prints a single zero.
            if (ndigits == 0u) {
                precision = 1;
            } else if ((digits[ndigits - 1u] != '0') &&
                       (precision <= (int)ndigits)) {
                precision = (int)ndigits + 1;
            }
        }
    }

    if ((precision >= 0) && ((int)ndigits < precision)) {
        zeros = precision - (int)ndigits;
    }

    pad = width - (int)nlead - zeros - (int)ndigits;

    // The zero flag is ignored when a precision is given for an integer
    // conversion, and when left justifying.
    if ((pad > 0) && ((flags & FMT_ZERO) != 0u) && ((flags & FMT_LEFT) == 0u) &&
        (precision < 0)) {
        zeros += pad;
        pad = 0;
    }

    if ((flags & FMT_LEFT) == 0u) {
        fmt_pad(f, ' ', pad);
    }
    for (ii = 0u; ii < nlead; ii++) {
        fmt_putc(f, lead[ii]);
    }
    fmt_pad(f, '0', zeros);
    while (ndigits-- > 0u) {
        fmt_putc(f, digits[ndigits]);
    }
    if ((flags & FMT_LEFT) != 0u) {
        fmt_pad(f, ' ', pad);
    }
}

static void fmt_string(fmt_state_t *f, const char *s, unsigned flags, int width,
                       int precision) {
    unsigned len = 0u;
    int pad;
    unsigned ii;

    if (s == NULL) {
        s = "(null)";
    }

    while ((s[len] != '\0') && ((precision < 0) || ((int)len < precision))) {
        len++;
    }

    pad = width - (int)len;

    if ((flags & FMT_LEFT) == 0u) {
        fmt_pad(f, ' ', pad);
    }
    for (ii = 0u; ii < len; ii++) {
        fmt_putc(f, s[ii]);
    }
    if ((flags & FMT_LEFT) != 0u) {
        fmt_pad(f, ' ', pad);
    }
}

static uint64_t fmt_uarg(va_list *args, fmt_len_t len) {
    switch (len) {
    case FMT_LEN_CHAR:  return (uint64_t)(unsigned char)va_arg(*args, unsigned int);
    case FMT_LEN_SHORT: return (uint64_t)(unsigned short)va_arg(*args, unsigned int);
    case FMT_LEN_LONG:  return (uint64_t)va_arg(*args, unsigned long);
    case FMT_LEN_LLONG: return (uint64_t)va_arg(*args, unsigned long long);
    case FMT_LEN_SIZE:  return (uint64_t)va_arg(*args, size_t);
    default:            return (uint64_t)va_arg(*args, unsigned int);
    }
}

static int64_t fmt_sarg(va_list *args, fmt_len_t len) {
    switch (len) {
    case FMT_LEN_CHAR:  return (int64_t)(signed char)va_arg(*args, int);
    case FMT_LEN_SHORT: return (int64_t)(short)va_arg(*args, int);
    case FMT_LEN_LONG:  return (int64_t)va_arg(*args, long);
    case FMT_LEN_LLONG: return (int64_t)va_arg(*args, long long);
    case FMT_LEN_SIZE:  return (int64_t)va_arg(*args, ptrdiff_t);
    default:            return (int64_t)va_arg(*args, int);
    }
}

void onerom_rtt_vprintf(unsigned channel, const char *fmt, va_list *args) {
    fmt_state_t f;
    unsigned flags;
    fmt_len_t length;
    int width, precision;
    char conv;

    f.len = 0u;
    f.channel = channel;

    while (*fmt != '\0') {
        if (*fmt != '%') {
            fmt_putc(&f, *fmt++);
            continue;
        }
        fmt++;

        flags = 0u;
        for (;;) {
            if (*fmt == '-')      { flags |= FMT_LEFT;  fmt++; }
            else if (*fmt == '0') { flags |= FMT_ZERO;  fmt++; }
            else if (*fmt == '#') { flags |= FMT_ALT;   fmt++; }
            else if (*fmt == '+') { flags |= FMT_PLUS;  fmt++; }
            else if (*fmt == ' ') { flags |= FMT_SPACE; fmt++; }
            else break;
        }

        width = 0;
        if (*fmt == '*') {
            width = va_arg(*args, int);
            if (width < 0) {
                flags |= FMT_LEFT;
                width = -width;
            }
            fmt++;
        } else {
            while ((*fmt >= '0') && (*fmt <= '9')) {
                width = (width * 10) + (*fmt++ - '0');
            }
        }

        precision = -1;
        if (*fmt == '.') {
            fmt++;
            if (*fmt == '*') {
                precision = va_arg(*args, int);
                fmt++;
            } else {
                precision = 0;
                while ((*fmt >= '0') && (*fmt <= '9')) {
                    precision = (precision * 10) + (*fmt++ - '0');
                }
            }
        }

        length = FMT_LEN_INT;
        if (*fmt == 'h') {
            fmt++;
            length = FMT_LEN_SHORT;
            if (*fmt == 'h') { fmt++; length = FMT_LEN_CHAR; }
        } else if (*fmt == 'l') {
            fmt++;
            length = FMT_LEN_LONG;
            if (*fmt == 'l') { fmt++; length = FMT_LEN_LLONG; }
        } else if (*fmt == 'z') {
            fmt++;
            length = FMT_LEN_SIZE;
        }

        conv = *fmt;
        if (conv == '\0') {
            break;
        }
        fmt++;

        switch (conv) {
        case 'd':
        case 'i': {
            int64_t sv = fmt_sarg(args, length);
            int negative = (sv < 0);
            uint64_t uv = negative ? (uint64_t)(-(uint64_t)sv) : (uint64_t)sv;
            fmt_number(&f, uv, 10u, 0, negative, flags, width, precision);
            break;
        }
        case 'u':
            fmt_number(&f, fmt_uarg(args, length), 10u, 0, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'o':
            fmt_number(&f, fmt_uarg(args, length), 8u, 0, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'x':
            fmt_number(&f, fmt_uarg(args, length), 16u, 0, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'X':
            fmt_number(&f, fmt_uarg(args, length), 16u, 1, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'b':
            fmt_number(&f, fmt_uarg(args, length), 2u, 0, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'B':
            fmt_number(&f, fmt_uarg(args, length), 2u, 1, 0,
                       fmt_unsigned_flags(flags), width, precision);
            break;
        case 'c': {
            char ch = (char)va_arg(*args, int);
            int pad = width - 1;
            if ((flags & FMT_LEFT) == 0u) {
                fmt_pad(&f, ' ', pad);
            }
            fmt_putc(&f, ch);
            if ((flags & FMT_LEFT) != 0u) {
                fmt_pad(&f, ' ', pad);
            }
            break;
        }
        case 's':
            fmt_string(&f, va_arg(*args, const char *), flags, width, precision);
            break;
        case 'p': {
            uintptr_t p = (uintptr_t)va_arg(*args, void *);
            fmt_number(&f, (uint64_t)p, 16u, 0, 0,
                       fmt_unsigned_flags(flags) | FMT_ALT | FMT_PTR, width,
                       precision);
            break;
        }
        case '%':
            fmt_putc(&f, '%');
            break;
        // Unsupported.  Consume the argument so that the rest of the record
        // stays in step - a double is eight bytes after promotion, and
        // skipping it would corrupt every conversion after this one.
        case 'f': case 'F': case 'e': case 'E': case 'g': case 'G':
        case 'a': case 'A':
            (void)va_arg(*args, double);
            fmt_putc(&f, '%'); fmt_putc(&f, '!'); fmt_putc(&f, conv);
            break;
        case 'n':
            (void)va_arg(*args, void *);
            fmt_putc(&f, '%'); fmt_putc(&f, '!'); fmt_putc(&f, conv);
            break;
        default:
            // Unrecognised.  Nothing can be assumed about the argument, so
            // none is consumed.
            fmt_putc(&f, '%'); fmt_putc(&f, '!'); fmt_putc(&f, conv);
            break;
        }
    }

    fmt_flush(&f);
}

void onerom_rtt_printf(unsigned channel, const char *fmt, ...) {
    va_list args;

    va_start(args, fmt);
    onerom_rtt_vprintf(channel, fmt, &args);
    va_end(args);
}
