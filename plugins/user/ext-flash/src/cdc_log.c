// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include <stdarg.h>
#include <stdint.h>

#include <plugin.h>

#include "cdc_log.h"

static ora_log_write_fn_t s_write;

// No C runtime here, so the compiler must not turn a scan into a strlen call.
static uint32_t str_len(const char *s) {
    const volatile char *p = s;
    uint32_t n = 0u;
    while (*p++ != '\0') {
        n++;
    }
    return n;
}

static void emit(const char *p, uint32_t len) {
    if (s_write != NULL && len > 0u) {
        s_write(ORA_LOG_CHANNEL_0, p, len);
    }
}

// Convert v in the given base, into a caller-supplied buffer, right aligned.
// Returns a pointer to the first digit.
//
// 12 bytes covers the longest output, which is 10 digits for a 32-bit decimal.
static char *convert(uint32_t v, uint32_t base, uint8_t upper, char *end) {
    const char *digits = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    char *p = end;
    *--p = '\0';
    do {
        *--p = digits[v % base];
        v /= base;
    } while (v != 0u);
    return p;
}

static void emit_padded(uint32_t v, uint32_t base, uint8_t upper, uint32_t width) {
    char buf[12];
    char *p = convert(v, base, upper, buf + sizeof(buf));
    uint32_t len = (uint32_t)((buf + sizeof(buf) - 1) - p);
    while (len < width) {
        emit("0", 1u);
        width--;
    }
    emit(p, len);
}

uint8_t cdc_log_init(ora_lookup_fn_t lookup, const char *name) {
    ora_log_open_write_fn_t open_write = lookup(ORA_ID_LOG_OPEN_WRITE);
    ora_log_write_fn_t      write      = lookup(ORA_ID_LOG_WRITE);
    if (open_write == NULL || write == NULL) {
        return 0u;
    }
    if (open_write(ORA_LOG_CHANNEL_0, name) != ORA_RESULT_OK) {
        return 0u;
    }
    s_write = write;
    return 1u;
}

void cdc_log(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);

    const char *run = fmt;
    for (const char *p = fmt; *p != '\0'; p++) {
        if (*p != '%') {
            continue;
        }

        // Flush the literal text ahead of this conversion.
        emit(run, (uint32_t)(p - run));
        p++;

        uint32_t width = 0u;
        while (*p >= '0' && *p <= '9') {
            width = (width * 10u) + (uint32_t)(*p - '0');
            p++;
        }

        switch (*p) {
            case 's': {
                const char *s = va_arg(args, const char *);
                emit(s, str_len(s));
                break;
            }
            case 'c': {
                char c = (char)va_arg(args, int);
                emit(&c, 1u);
                break;
            }
            case 'u':
                emit_padded(va_arg(args, uint32_t), 10u, 0u, width);
                break;
            case 'd': {
                int32_t v = va_arg(args, int32_t);
                if (v < 0) {
                    emit("-", 1u);
                    v = -v;
                }
                emit_padded((uint32_t)v, 10u, 0u, width);
                break;
            }
            case 'x':
                emit_padded(va_arg(args, uint32_t), 16u, 0u, width);
                break;
            case 'X':
                emit_padded(va_arg(args, uint32_t), 16u, 1u, width);
                break;
            default:
                emit(p, 1u);
                break;
        }
        run = p + 1;
    }

    emit(run, str_len(run));
    emit("\r\n", 2u);
    va_end(args);
}
