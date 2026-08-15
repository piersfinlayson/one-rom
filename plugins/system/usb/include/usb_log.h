// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef USB_LOG_H
#define USB_LOG_H

// What the banner tells a terminal about log forwarding.
//
// The two unavailable cases are different advice, so they are kept apart rather
// than collapsed into one message: only one of them is fixed by a newer
// firmware.
typedef enum {
    // Forwarding is available, and the banner says nothing about it.
    LOG_BANNER_NOTE_NONE = 0,

    // The running firmware has no logging API, so no plugin can read the log.
    LOG_BANNER_NOTE_FIRMWARE,

    // The firmware is capable, but another plugin holds the channel's read
    // claim.  Telling this user to upgrade would be wrong advice.
    LOG_BANNER_NOTE_IN_USE,
} log_banner_note_t;

// Claim One ROM's log channel for reading, so that the task below can forward
// it.  Call once at plugin start.  Forwarding stays off, with a line saying
// why, on firmware without the logging API and when another plugin already
// reads the channel.
void log_drain_init(void);

// Move what the log holds to the CDC serial port.  Call once per main loop
// pass.  Drains nothing until a terminal opens the port, and then writes a
// banner naming the device before any log content - including when there is no
// log content to come, since the banner is how a reader is told so.  What the
// log accumulated before the terminal attached is replayed rather than dropped.
void log_drain_task(void);

#endif // USB_LOG_H
