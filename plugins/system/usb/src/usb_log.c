// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Forwarding One ROM's log to the CDC serial port.
//
// The plugin claims the log channel for reading at startup and, while a
// terminal is attached, copies what the firmware and the other plugin write
// into the CDC IN endpoint.  Each session opens with a banner naming the
// device, written before any log content - and written whether or not there is
// any log content to come, because a reader that gets nothing at all cannot
// tell a quiet device from the wrong port.
//
// Nothing is forwarded until a terminal opens the port, so that a debug probe
// can still read the log on a device that merely has USB.  What has already
// accumulated is then forwarded rather than discarded - see log_drain_task().
//
// Two different conditions gate this, and they are not the same thing:
//
// - Whether a terminal has the port open, which is DTR, and which marks the
//   start of a session.  tinyusb reports the transition rather than the level,
//   so a close and reopen between two passes of the main loop is still seen as
//   both.
// - Whether the bus can carry data right now, which is tud_cdc_n_connected().
//   It also goes false on a bus suspend, where the terminal is still open, so a
//   host sleeping pauses the forwarding and resumes where it left off.

#include "include.h"
#include "usb_plugin.h"
#include "tusb.h"

// Bytes moved per pass.  The CDC TX FIFO is 64 bytes at full speed, so a
// larger buffer could not be handed on in one go anyway.
#define LOG_DRAIN_CHUNK     64

// The CDC interface carrying the log.  One is configured.
#define LOG_DRAIN_CDC_ITF   0

// How long after a terminal opens the port before anything is sent to it.
//
// DTR asserts when the port is opened, but a host is not necessarily reading
// yet: pyserial configures the line after opening, and the bytes that arrive in
// between are lost - measured at around 440 of them, which is the whole of a
// boot log.  Nothing on this side can tell a reader from an open file
// descriptor, so the only defence is to let the host finish.  Holding off costs
// nothing, because the channel keeps its contents until they are read.
//
// It is a heuristic, not a guarantee.  A host slower than this still loses the
// start.
#define LOG_DRAIN_SETTLE_MS 250u

// The banner's rules.
//
// The banner opens with a titled rule and closes with a plain one of the same
// width.  Both are deliberately unlike log_divider in
// firmware/src/constants.c, which is the five hyphens the boot log opens with:
// a banner that ruled its output the same way put two identical rules next to
// each other at the join, and left the reader to work out that the product line
// above them and the one below them describe the same device by different
// routes.
//
// The banner is written on every attach, whether or not a boot log follows, so
// it always closes - the closing rule is what says the port has finished
// introducing itself, and when nothing is forwarded it is the end of the whole
// message.
#define LOG_BANNER_TITLE "----- One ROM USB log -----"
#define LOG_BANNER_RULE "---------------------------"

_Static_assert(
    sizeof(LOG_BANNER_TITLE) == sizeof(LOG_BANNER_RULE),
    "banner title and closing rule must be the same width"
);

// What ends a banner line.
//
// The banner shares a stream with the log, so it ends a line the way do_log_v()
// in firmware/src/log.c ends one.  A change there is a change here.
#define LOG_BANNER_LINE_END "\r\n"
#define LOG_BANNER_LINE_END_LEN (sizeof(LOG_BANNER_LINE_END) - 1)

// Longest banner line, in characters including its terminator.
//
// A line is resumed from a byte offset held in the context, so a longer line
// could not be resumed.  Every line the banner writes is far shorter - the
// longest is the category list at 80 characters - and the metadata strings a
// line carries are bounded by the schema at MAX_UNIT_NAME_LEN and
// MAX_SERIAL_NUMBER_LEN.  The cap is here for the hardware revision string,
// which is the one input with no bound of its own.
#define LOG_BANNER_LINE_MAX 255u

// Room for the device version string, which is "vMAJOR.MINOR.PATCH".
//
// ora_get_device_version() reports a buffer smaller than the string as
// ORA_RESULT_INVALID_SIZE and offers no way to ask how much it wants, so this
// is sized well past any version it can return.
#define LOG_BANNER_VERSION_MAX 16u

// The names the banner gives the log categories, indexed by ora_log_category_t,
// which is also the order it lists them in.
static const char *const banner_categories[] = {
    "boot",               // ORA_LOG_CATEGORY_BOOT
    "plugin-internal",    // ORA_LOG_CATEGORY_PLUGIN_INTERNAL
    "debug",              // ORA_LOG_CATEGORY_DEBUG
    "error",              // ORA_LOG_CATEGORY_ERROR
    "plugin-application", // ORA_LOG_CATEGORY_PLUGIN_APPLICATION
    "plugin-debug",       // ORA_LOG_CATEGORY_PLUGIN_DEBUG
};

#define BANNER_CATEGORY_COUNT \
    (sizeof(banner_categories) / sizeof(banner_categories[0]))

// The lines of the banner, in the order they are written.  The note line sits
// outside the closing divider, because it is about the port rather than about
// the device the divider encloses.  LOG_BANNER_LINE_COMPLETE is one past the
// last, and is the banner being finished.
typedef enum {
    LOG_BANNER_LINE_TOP = 0,
    LOG_BANNER_LINE_PRODUCT,
    LOG_BANNER_LINE_IDENTITY,
    LOG_BANNER_LINE_LOGGING,
    LOG_BANNER_LINE_BOTTOM,
    LOG_BANNER_LINE_NOTE,
    LOG_BANNER_LINE_COMPLETE,
} log_banner_line_t;

// Where a line of the banner is being assembled.
//
// A line is generated afresh on every pass, from the cursor onwards, so that
// nothing but the cursor survives between passes.  banner_put() appends a piece
// of a line: what falls before the cursor is counted and dropped, what falls
// after it is copied while there is room, and total ends up as the line's whole
// length however small the buffer was.  That is what says whether the line is
// finished.
typedef struct {
    uint8_t *out;
    uint32_t max;   // Capacity of out
    uint32_t skip;  // Characters of this line still to drop
    uint32_t len;   // Characters placed in out
    uint32_t total; // Length of this line so far, dropped characters included
    uint32_t cap;   // Length this line may not exceed
} banner_sink_t;

// Append a piece of the current line.  A NULL string appends nothing, so that a
// value the device could not supply drops out of its line without the caller
// testing for it twice.
static void banner_put(banner_sink_t *sink, const char *str) {
    if (str == NULL) {
        return;
    }

    while (*str != '\0' && sink->total < sink->cap) {
        if (sink->skip > 0) {
            sink->skip--;
        } else if (sink->len < sink->max) {
            sink->out[sink->len] = (uint8_t)*str;
            sink->len++;
        }
        sink->total++;
        str++;
    }
}

// Close the line the sink holds.
//
// A line with nothing in it is not written at all, terminator included, so a
// line the device cannot fill - no identity, no category list, no note - is
// absent rather than blank.  The terminator is appended past the cap that
// bounds a line's content, so no line can be capped out of its own ending.
static void banner_end_line(banner_sink_t *sink) {
    if (sink->total == 0) {
        return;
    }
    sink->cap = LOG_BANNER_LINE_MAX;
    banner_put(sink, LOG_BANNER_LINE_END);
}

// A metadata string, or NULL when the field is unset.  That is not an error - a
// value the banner has no source for is left out of its line.
static const char *banner_metadata_str(ora_metadata_key_t key) {
    ora_get_metadata_str_fn_t get_metadata_str =
        context.ora_lookup_fn(ORA_ID_GET_METADATA_STR);
    if (get_metadata_str == NULL) {
        return NULL;
    }

    const char *value = NULL;
    if (get_metadata_str(key, &value) != ORA_RESULT_OK) {
        return NULL;
    }
    return value;
}

// The running firmware's version, written into buf, or NULL if it cannot be
// had.
static const char *banner_version(char *buf, uint32_t size) {
    ora_get_device_version_fn_t get_device_version =
        context.ora_lookup_fn(ORA_ID_GET_DEVICE_VERSION);
    if (get_device_version == NULL) {
        return NULL;
    }

    if (get_device_version((uint8_t *)buf, size) != ORA_RESULT_OK) {
        return NULL;
    }

    // The firmware copies its own terminator, so this only guards against a
    // future one that does not.
    buf[size - 1] = '\0';
    return buf;
}

// Generate one line of the banner into the sink.
static void banner_line(uint8_t line, banner_sink_t *sink) {
    switch (line) {
        case LOG_BANNER_LINE_TOP:
            banner_put(sink, LOG_BANNER_TITLE);
            break;

        case LOG_BANNER_LINE_BOTTOM:
            banner_put(sink, LOG_BANNER_RULE);
            break;

        case LOG_BANNER_LINE_PRODUCT: {
            char version[LOG_BANNER_VERSION_MAX];
            banner_put(sink, "One ROM");

            // On firmware before 0.7.1 the board is unreachable, so the token
            // is left out rather than filled with a placeholder the reader
            // would have to know to discount.  The version is treated the same
            // way, though nothing supported reaches that.
            const char *board = banner_metadata_str(ORA_METADATA_KEY_HW_REV);
            if (board != NULL) {
                banner_put(sink, " ");
                banner_put(sink, board);
            }
            const char *ver = banner_version(version, sizeof(version));
            if (ver != NULL) {
                banner_put(sink, " ");
                banner_put(sink, ver);
            }
            break;
        }

        case LOG_BANNER_LINE_IDENTITY: {
            // Either token can be absent - no instance name is set, or no
            // serial could be read - so both are settled before either is
            // written, and the space between them belongs to whichever pair
            // survives.
            char serial[USB_SERIAL_MAX_CHARS + 1];
            usb_get_serial(serial, sizeof(serial));
            const char *name = banner_metadata_str(ORA_METADATA_KEY_UNIT_NAME);
            if (name != NULL) {
                banner_put(sink, "Name: ");
                banner_put(sink, name);
                if (serial[0] != '\0') {
                    banner_put(sink, " ");
                }
            }
            if (serial[0] != '\0') {
                banner_put(sink, "Serial: ");
                banner_put(sink, serial);
            }
            break;
        }

        case LOG_BANNER_LINE_LOGGING: {
            // A list assembled from anything but the firmware's own answer
            // would be a claim the device has not made, so the line is left out
            // entirely.
            ora_log_category_enabled_fn_t category_enabled =
                context.ora_lookup_fn(ORA_ID_LOG_CATEGORY_ENABLED);
            if (category_enabled == NULL) {
                break;
            }

            banner_put(sink, "Logging:");
            const char *separator = " ";
            for (uint32_t ii = 0; ii < BANNER_CATEGORY_COUNT; ii++) {
                // A category this header declares that the running firmware
                // does not know reports ORA_RESULT_NOT_SUPPORTED, which is no
                // output appearing.
                uint32_t enabled = 0;
                if (category_enabled((ora_log_category_t)ii, &enabled)
                        != ORA_RESULT_OK
                    || enabled == 0) {
                    continue;
                }
                banner_put(sink, separator);
                banner_put(sink, banner_categories[ii]);
                separator = ", ";
            }
            break;
        }

        case LOG_BANNER_LINE_NOTE:
            switch (context.log_banner_note) {
                case LOG_BANNER_NOTE_IN_USE:
                    banner_put(
                        sink,
                        "!!! USB logging unavailable - another plugin is "
                        "already reading the log !!!"
                    );
                    break;

                default:
                    break;
            }
            break;

        default:
            break;
    }

    banner_end_line(sink);
}

// Write as much of the banner as the endpoint will take.
//
// The banner is bigger than the 64-byte CDC TX FIFO, so it spans several
// passes.  The cursor in the context says which line the next pass resumes on
// and how far into it, and the line is generated again from there - every value
// in it is either a pointer into flash or something the device can produce
// again, so nothing has to be kept.  For the same reason a byte the endpoint
// would not take is not lost: the cursor advances by what was written rather
// than by what was generated.
static void log_banner_write(void) {
    uint8_t buf[LOG_DRAIN_CHUNK];

    while (context.log_banner_line < LOG_BANNER_LINE_COMPLETE) {
        uint32_t avail = tud_cdc_n_write_available(LOG_DRAIN_CDC_ITF);
        if (avail == 0) {
            return;
        }
        if (avail > sizeof(buf)) {
            avail = sizeof(buf);
        }

        banner_sink_t sink = {
            .out = buf,
            .max = avail,
            .skip = context.log_banner_offset,
            .len = 0,
            .total = 0,
            .cap = LOG_BANNER_LINE_MAX - LOG_BANNER_LINE_END_LEN,
        };
        banner_line(context.log_banner_line, &sink);

        uint32_t written = 0;
        if (sink.len > 0) {
            written = tud_cdc_n_write(LOG_DRAIN_CDC_ITF, buf, sink.len);

            // Flush rather than waiting for a full packet, so the banner
            // reaches a terminal that has just attached instead of waiting on
            // whatever the device logs next.
            tud_cdc_n_write_flush(LOG_DRAIN_CDC_ITF);
        }

        uint32_t done = (uint32_t)context.log_banner_offset + written;
        if (done >= sink.total) {
            context.log_banner_line++;
            context.log_banner_offset = 0;
        } else if (written == 0) {
            // The endpoint took nothing, so generating the same bytes again
            // would get the same answer.  Leave the rest to the next pass.
            return;
        } else {
            context.log_banner_offset = (uint8_t)done;
        }
    }
}

void log_drain_init(void) {
    ora_log_open_read_fn_t open_read =
        context.ora_lookup_fn(ORA_ID_LOG_OPEN_READ);

    if (open_read == NULL) {
        DEBUG("No logging API, CDC log forwarding disabled");
        return;
    }

    // The channel may not exist, or another plugin may already read it.  Only
    // one reader is allowed and this one loses gracefully, so report the code
    // rather than guessing which it was.
    //
    // The banner has one message for all of these, because a firmware carrying
    // the logging API carries channel 0 with it, leaving the claim being held
    // as the way this fails.  What it must not say is that the firmware is too
    // old, which is the one thing it is not.
    ora_result_t result = open_read(ORA_LOG_CHANNEL_0);
    if (result != ORA_RESULT_OK) {
        LOG("Log channel unavailable (%d), CDC forwarding disabled", result);
        context.log_banner_note = LOG_BANNER_NOTE_IN_USE;
        return;
    }

    // Only the read is worth a pointer of its own: it runs every pass, where
    // the open ran once.
    ora_log_read_fn_t log_read = context.ora_lookup_fn(ORA_ID_LOG_READ);
    if (log_read == NULL) {
        // Nothing can use the claim, so give it back rather than locking the
        // channel out of every other plugin for the life of the device.
        ora_log_close_read_fn_t close_read =
            context.ora_lookup_fn(ORA_ID_LOG_CLOSE_READ);
        if (close_read != NULL) {
            close_read(ORA_LOG_CHANNEL_0);
        }
        ERR("Log channel readable but not read, CDC log forwarding disabled");
        return;
    }

    context.log_read = log_read;
}

// tinyusb calls this when a host asserts or drops DTR, which is a terminal
// opening or closing the port.  Taking the transition here rather than
// sampling the level in the task means a close and reopen inside one
// tud_task() is still seen as a detach and an attach.
void tud_cdc_line_state_cb(uint8_t itf, bool dtr, bool rts) {
    (void)rts;

    if (itf != LOG_DRAIN_CDC_ITF) {
        return;
    }

    context.log_cdc_attached = dtr ? 1 : 0;
    if (dtr) {
        // The work this implies is left to the task, because this runs from
        // tud_task().
        context.log_cdc_new_session = 1;
    }
}

// Move what the channel holds to the endpoint.  The banner is complete and the
// bus can carry data by the time this is called.
static void log_drain_bytes(void) {
    uint8_t buf[LOG_DRAIN_CHUNK];
    uint32_t copied = 0;

    // Read only what the endpoint can take, so that a record consumed from the
    // channel is one the transport has room for.  Reading more would leave the
    // remainder in this plugin, which has nowhere to keep it.
    uint32_t avail = tud_cdc_n_write_available(LOG_DRAIN_CDC_ITF);
    if (avail == 0) {
        return;
    }
    if (avail > sizeof(buf)) {
        avail = sizeof(buf);
    }

    if (context.log_read(ORA_LOG_CHANNEL_0, buf, avail, &copied) !=
        ORA_RESULT_OK) {
        return;
    }
    if (copied == 0) {
        return;
    }

    // The space was reserved above, so a short write means something else is
    // writing this endpoint and the reservation no longer holds.  The bytes
    // are already out of the channel and cannot be put back, so say so - a
    // silent hole in a log is worse than a line about it.
    uint32_t written = tud_cdc_n_write(LOG_DRAIN_CDC_ITF, buf, copied);
    if (written != copied) {
        ERR("CDC log dropped %lu bytes", (unsigned long)(copied - written));
    }

    // Flush rather than waiting for a full packet, so a short line appears
    // when it is written instead of when the next one fills the endpoint.
    tud_cdc_n_write_flush(LOG_DRAIN_CDC_ITF);
}

void log_drain_task(void) {
    // Nothing is drained until a terminal opens the port, so that a debug
    // probe remains able to read the log on a device that merely has USB.  The
    // channel filling and the writer dropping records is the same behaviour as
    // a device with no reader at all.
    if (!context.log_cdc_attached) {
        return;
    }

    // Nothing goes out before the settle window elapses, so a banner already
    // written means it has passed and the clock need not be read at all.
    if (context.log_cdc_new_session ||
        context.log_banner_line != LOG_BANNER_LINE_COMPLETE) {
        uint32_t now = context.get_plugin_uptime_ms();

        if (context.log_cdc_new_session) {
            // What is still in the channel is kept, not discarded.  The ring
            // drops the newest record when it is full rather than evicting the
            // oldest, so what is waiting here is the earliest output since the
            // log was last drained - the boot log, on a device nothing has been
            // listening to.  That is the output a terminal attaching most
            // wants, and it is bounded by the ring, so there is no flood to
            // guard against.
            //
            // The endpoint is a different matter.  Bytes already handed to it
            // came out of the channel for the session that has just ended, and
            // would reach this terminal as a leading partial line.
            tud_cdc_n_write_clear(LOG_DRAIN_CDC_ITF);

            // This terminal gets the banner from the top, whatever the last one
            // was given.
            context.log_banner_line = LOG_BANNER_LINE_TOP;
            context.log_banner_offset = 0;

            context.log_cdc_settled_ms = now + LOG_DRAIN_SETTLE_MS;
            context.log_cdc_new_session = 0;
        }

        // Signed difference, so a clock wrap inside the settle window does not
        // mute the port for another 49.7 days.
        if ((int32_t)(now - context.log_cdc_settled_ms) < 0) {
            return;
        }
    }

    // A suspended or unconfigured bus cannot carry anything.  The terminal is
    // still open, so the backlog is left where it is and resumes from here.
    if (!tud_cdc_n_connected(LOG_DRAIN_CDC_ITF)) {
        return;
    }

    // The banner goes out whole before any log byte does, so that what a reader
    // sees first is what it has attached to.  It runs on a device that forwards
    // nothing too, because there the banner is the only thing that will arrive
    // and it says why.
    if (context.log_banner_line != LOG_BANNER_LINE_COMPLETE) {
        log_banner_write();
        return;
    }

    if (context.log_read == NULL) {
        return;
    }

    log_drain_bytes();
}
