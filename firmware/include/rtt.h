// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM RTT
//
// A One ROM implementation of real time transfer logging, binary compatible
// with SEGGER RTT.  probe-rs, OpenOCD, pyOCD and Black Magic Probe find and
// drain the log with no host side change and no plugin.
//
// Binary compatibility means three things, and only these three:
//
// - the control block and channel descriptor layouts, pinned by the static
//   asserts at the end of this header;
// - the symbol name _SEGGER_RTT, which is how a probe resolves the control
//   block when it looks it up rather than scanning for it;
// - the "SEGGER RTT" identifier string a probe scans memory for.
//
// Everything else - functions, types, constants - is One ROM's.
//
// SEGGER's copyright notice is retained in ACKNOWLEDGEMENTS.md at the repo
// root.  Their sources are no longer part of the build.

#if !defined(ONEROM_RTT_H)
#define ONEROM_RTT_H

#include <stdint.h>
#include <stdarg.h>

// Number of channel descriptors declared in the control block.
//
// Held at SEGGER's default of 3 for now.  Only channel 0 is populated; a probe
// reads the counts from the control block, so unpopulated descriptors cost 96
// bytes of RAM and nothing else.  Sizing these to what is actually used is a
// question for the per core channel split, which owns the channel count.
#define ONEROM_RTT_MAX_UP_BUFFERS       3
#define ONEROM_RTT_MAX_DOWN_BUFFERS     3

// Channel operating mode.  Only skip is implemented: a record that does not
// fit is dropped whole, and the calling core is never blocked.
#define ONEROM_RTT_MODE_NO_BLOCK_SKIP   0u

// Log channel indices.  Fixed by contract - never identified by name.
#define ONEROM_RTT_CH_BOOT              0u

// Target to host channel.
//
// read_offset is written by the host, and is therefore untrusted; see
// onerom_rtt_write().
typedef struct {
    const char *name;
    char *buffer;
    unsigned size;
    unsigned write_offset;
    volatile unsigned read_offset;
    unsigned flags;
} onerom_rtt_up_t;

// Host to target channel.  Declared for layout compatibility; nothing in the
// firmware reads host to target data yet.
typedef struct {
    const char *name;
    char *buffer;
    unsigned size;
    volatile unsigned write_offset;
    unsigned read_offset;
    unsigned flags;
} onerom_rtt_down_t;

// The control block.  A probe locates this and walks the descriptors.
typedef struct {
    char id[16];
    int max_up_buffers;
    int max_down_buffers;
    onerom_rtt_up_t up[ONEROM_RTT_MAX_UP_BUFFERS];
    onerom_rtt_down_t down[ONEROM_RTT_MAX_DOWN_BUFFERS];
} onerom_rtt_cb_t;

// The control block object.  Named _SEGGER_RTT because that is the symbol a
// probe resolves; it is One ROM's control block in every other respect.
extern onerom_rtt_cb_t _SEGGER_RTT;

// Prepare the log for plugins.  Call once at plugin launch, on core 0, before
// either plugin runs.
void onerom_rtt_plugins_init(void);

// Append a record to a channel.  Returns len if the record was stored, 0 if it
// was dropped.  Records are all or nothing.
unsigned onerom_rtt_write(unsigned channel, const void *buf, unsigned len);

// Take up to max_len bytes from a channel, freeing the space for the writer.
// Returns the number copied, 0 if the channel is empty.  Serves the wrap
// internally, so a caller never sees it.
unsigned onerom_rtt_read(unsigned channel, void *buf, unsigned max_len);

// Set the name a reader displays for a channel.  The string is stored, not
// copied, so it must outlive the setting; NULL restores the firmware's own
// name.  Returns 0 if the channel index is out of range.
unsigned onerom_rtt_set_name(unsigned channel, const char *name);

// Report a channel's total size, the bytes writable now, and the bytes written
// but not yet read.  One consistent snapshot; any output pointer may be NULL.
// A channel with no buffer reports zero for all three.
//
// The ring always leaves one byte free, so size = free + pending + 1 for a
// populated channel.
void onerom_rtt_query(unsigned channel,
                      unsigned *size_out,
                      unsigned *free_out,
                      unsigned *pending_out);

// Formatted output.  See the supported conversion list in rtt.c; anything
// outside it is emitted as a visible marker rather than silently ignored.
void onerom_rtt_printf(unsigned channel, const char *fmt, ...)
    __attribute__((format(printf, 2, 3)));
void onerom_rtt_vprintf(unsigned channel, const char *fmt, va_list *args)
    __attribute__((format(printf, 2, 0)));

// Binary compatibility with SEGGER RTT, enforced rather than asserted in a
// comment.  _Static_assert directly, not the STATIC_ASSERT macro in macros.h,
// because that compiles away under TEST_BUILD, and this contract is what the
// whole design rests on.
//
// Layout, from SEGGER_RTT.h: 16 byte id, two ints, then 24 byte descriptors,
// up buffers before down.
//
// The layout is a property of the 32 bit target, where the two pointers in a
// descriptor are four bytes each.  A 64 bit host - firmware/test/rtt/ compiles
// this header natively - cannot satisfy it and is not asked to; the firmware
// build is what enforces it, and that is the build that has to be right.
#if UINTPTR_MAX == 0xFFFFFFFFu
_Static_assert(sizeof(onerom_rtt_up_t) == 24,
               "up descriptor must be 24 bytes for SEGGER RTT compatibility");
_Static_assert(sizeof(onerom_rtt_down_t) == 24,
               "down descriptor must be 24 bytes for SEGGER RTT compatibility");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, name) == 0, "up.name offset");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, buffer) == 4, "up.buffer offset");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, size) == 8, "up.size offset");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, write_offset) == 12, "up.write_offset offset");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, read_offset) == 16, "up.read_offset offset");
_Static_assert(__builtin_offsetof(onerom_rtt_up_t, flags) == 20, "up.flags offset");
// The down descriptor swaps the two offsets: the host owns write_offset there.
_Static_assert(__builtin_offsetof(onerom_rtt_down_t, write_offset) == 12, "down.write_offset offset");
_Static_assert(__builtin_offsetof(onerom_rtt_down_t, read_offset) == 16, "down.read_offset offset");
_Static_assert(__builtin_offsetof(onerom_rtt_cb_t, id) == 0, "cb.id offset");
_Static_assert(__builtin_offsetof(onerom_rtt_cb_t, max_up_buffers) == 16, "cb.max_up_buffers offset");
_Static_assert(__builtin_offsetof(onerom_rtt_cb_t, max_down_buffers) == 20, "cb.max_down_buffers offset");
_Static_assert(__builtin_offsetof(onerom_rtt_cb_t, up) == 24, "cb.up offset");
_Static_assert(__builtin_offsetof(onerom_rtt_cb_t, down) == 24 + (ONEROM_RTT_MAX_UP_BUFFERS * 24),
               "cb.down must follow the up descriptors");
_Static_assert(sizeof(onerom_rtt_cb_t) ==
                   24 + (ONEROM_RTT_MAX_UP_BUFFERS * 24) + (ONEROM_RTT_MAX_DOWN_BUFFERS * 24),
               "control block must have no trailing padding");
#endif // UINTPTR_MAX == 0xFFFFFFFFu

#endif // ONEROM_RTT_H
