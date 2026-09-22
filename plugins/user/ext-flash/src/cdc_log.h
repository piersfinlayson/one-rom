// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// Logging that works on any firmware build.
//
// ora_log() is compiled out unless the firmware was built with PLUGIN_LOGGING,
// so a plugin that relies on it needs a special firmware to say anything.  The
// log channel API has no such condition, and the USB plugin drains channel 0
// to CDC either way, which is what `onerom monitor log` and `onerom console`
// read.
//
// The channel API takes bytes rather than a format string, so the formatting
// is here.  It writes each piece as it converts it, so it needs no line buffer.

#if !defined(CDC_LOG_H)
#define CDC_LOG_H

#include <stdint.h>

#include <plugin.h>

// Claim channel 0 for writing.  name is kept by the firmware rather than
// copied, so it must outlive the claim.
//
// Returns non-zero when the channel is ours and cdc_log can be used.
uint8_t cdc_log_init(ora_lookup_fn_t lookup, const char *name);

// Write one line, appending CRLF.
//
// Supports %s, %c, %u, %d, %x and %X, with an optional zero-padded width, as
// in %08X.  Anything else is emitted as written.
void cdc_log(const char *fmt, ...) __attribute__((format(printf, 1, 2)));

#endif // CDC_LOG_H
