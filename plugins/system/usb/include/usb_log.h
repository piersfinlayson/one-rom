// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef USB_LOG_H
#define USB_LOG_H

// What the banner tells a terminal about log forwarding.
typedef enum {
    // The banner says nothing about forwarding.
    LOG_BANNER_NOTE_NONE = 0,

    // Another plugin holds the channel's read claim.
    LOG_BANNER_NOTE_IN_USE,
} log_banner_note_t;

// Claim One ROM's log channel for reading, so that the task below can forward
// it.  Call once at plugin start.  Forwarding stays off, with a line in the
// banner saying why, when another plugin already reads the channel.
void log_drain_init(void);

// Move what the log holds to the CDC serial port.  Call once per main loop
// pass.  Drains nothing until a terminal opens the port, and then writes a
// banner naming the device before any log content - including when there is no
// log content to come, since the banner is how a reader is told so.  What the
// log accumulated before the terminal attached is replayed rather than dropped.
void log_drain_task(void);

// Claim the input channel for writing, so that the task below can fill it.
// Called once at plugin start.  Input remains off when if the channel is
// unavailable.
void log_input_init(void);

// Move data sent to the CDC OUT endpoint into the input channel.
// Call once per main loop pass.  Only takes as much as the input chanel can
// hold, ensuring back pressure is applied to the host.
void log_input_task(void);

#endif // USB_LOG_H
