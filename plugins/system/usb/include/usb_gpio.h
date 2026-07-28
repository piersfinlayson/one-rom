// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(USB_GPIO_H)
#define USB_GPIO_H

#include <stdint.h>

#include "picobootx.h"
#include "usb_custom_pbx.h"

// Longest bounded hold ONEROM_CMD_GPIO_SET accepts, in milliseconds, and the
// value reported as onerom_caps_t.max_hold_ms.
//
// A minute is two to three orders of magnitude beyond any plausible reset pulse
// or diagnostic assertion, so the bound never gets in a real caller's way, and
// it keeps the deadline comparison in gpio_handle_pending_releases() far inside
// the window where unsigned millisecond arithmetic is unambiguous (context
// .timer_ms wraps every ~49.7 days; the comparison is safe for intervals under
// half that).
//
// The bound exists because a hold is only honoured while this plugin keeps
// running.  A caller wanting a pin held for longer than a minute wants it
// latched, not held: ONEROM_CMD_GPIO_SET with duration_ms 0 latches
// indefinitely and a later command releases it, which says what it means
// instead of pretending a multi-hour timer is dependable.
#define ONEROM_GPIO_MAX_HOLD_MS  60000u

// How many bounded holds can be outstanding at once.
//
// One per GPIO being held; a second ONEROM_CMD_GPIO_SET on a GPIO replaces that
// GPIO's pending release rather than taking a second slot, so this is a limit on
// distinct GPIOs held simultaneously, not on commands.  Eight covers driving a
// whole header at once and costs 64 bytes of .bss.
#define ONEROM_GPIO_RELEASES  8u

// A bounded hold waiting for its deadline.
//
// The plugin times the release rather than the host because the host may not
// survive the hold: if the CLI is interrupted, the terminal closes or the cable
// is pulled between assert and release, a host-timed pulse would leave the
// target latched - held in reset, in the motivating case - indefinitely.
typedef struct {
    // GPIO to release.  Meaningless unless active.
    uint8_t gpio;

    // onerom_gpio_state_t to apply at the deadline.
    uint8_t after_state;

    // 0 when this slot is free.
    uint8_t active;

    // context.timer_ms value at or after which after_state is applied.
    uint32_t deadline_ms;
} gpio_release_t;

typedef struct {
    gpio_release_t releases[ONEROM_GPIO_RELEASES];
} gpio_status_t;

// Work out what the GPIO commands can offer on the running firmware and record
// it in the plugin context, for ONEROM_CMD_GET_CAPS to report.  Call once, from
// usb_init(), after the ORA lookups.
void gpio_init_caps(void);

// Apply an ONEROM_CMD_GPIO_SET, synchronously.  Returns the picoboot status to
// answer the command with.
pb_status_t gpio_handle_set(const onerom_gpio_set_args_t *args);

// Apply any bounded holds whose deadline has passed.  Call from the plugin task
// loop.
void gpio_handle_pending_releases(void);

// Fill in one ONEROM_CMD_GPIO_QUERY entry.
pb_status_t gpio_fill_entry(uint8_t gpio, onerom_gpio_entry_t *entry_out);

#endif // USB_GPIO_H
