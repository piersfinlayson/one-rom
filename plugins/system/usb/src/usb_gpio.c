// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// GPIO control for the USB system plugin: the device side of
// ONEROM_CMD_GPIO_SET and ONEROM_CMD_GPIO_QUERY.
//
// The firmware owns the safety model - ora_gpio_set refuses a GPIO One ROM is
// itself using unless ORA_GPIO_FLAG_FORCE is passed - and this file owns the
// timing of a bounded hold, which is the one thing the ORA call deliberately
// does not do.

#include "include.h"
#include "usb_plugin.h"

// Whether a wire state byte is one this plugin knows.  ora_gpio_set validates
// its own state argument, but after_state never reaches it at command time, so
// it has to be checked here or a bad value would only surface at the deadline -
// by which point the command has been acknowledged and the pin is latched.
static bool gpio_state_valid(uint8_t state) {
    return state == ONEROM_GPIO_STATE_LOW ||
           state == ONEROM_GPIO_STATE_HIGH ||
           state == ONEROM_GPIO_STATE_INPUT;
}

// Map an ORA result onto the picoboot status the host will read back from
// GET_COMMAND_STATUS.
//
// The mapping matters because status is the only channel the host has: a
// stalled endpoint on its own says nothing about why.  PB_STATUS_NOT_PERMITTED
// is "the device understood and refused", which is what the in-use gate is;
// PB_STATUS_INVALID_ARG is "the arguments were wrong".  Neither may be confused
// with PB_STATUS_UNKNOWN_CMD or PB_STATUS_INVALID_CMD_LENGTH, which the host
// reads as "this device is too old".
static pb_status_t gpio_status_from_ora(ora_result_t result) {
    switch (result) {
        case ORA_RESULT_OK:
            return PB_STATUS_OK;

        case ORA_RESULT_GPIO_IN_USE:
            return PB_STATUS_NOT_PERMITTED;

        case ORA_RESULT_INVALID_ARG:
            return PB_STATUS_INVALID_ARG;

        default:
            return PB_STATUS_UNKNOWN_ERROR;
    }
}

// The GPIO count of the running RP2350 variant, from device metadata, or 0 if
// the running firmware cannot tell us.
//
// This has to be exactly the firmware's own MAX_GPIOS, because that is what
// ora_gpio_set and ora_gpio_query range-check against and what the host sizes
// its ONEROM_CMD_GPIO_QUERY request from.  MAX_GPIOS is
// max_gpios[RUNTIME->rp235x] - indexed by the variant detected at boot, not the
// one the board's metadata expects - and ORA_METADATA_KEY_RP_VARIANT reports
// that same runtime field, so the two cannot drift apart.
static uint8_t gpio_num_gpios(void) {
    ora_get_metadata_uint_fn_t get_metadata_uint =
        context.ora_lookup_fn(ORA_ID_GET_METADATA_UINT);
    if (get_metadata_uint == NULL) {
        return 0;
    }

    uint32_t variant = 0;
    if (get_metadata_uint(ORA_METADATA_KEY_RP_VARIANT, &variant) != ORA_RESULT_OK) {
        // Firmware that has the getter but not this key: it predates the key,
        // so it predates the GPIO API too.
        return 0;
    }

    // Mirrors max_gpios[] in firmware/src/constants.c.
    switch ((rp235x_variant_t)variant) {
        case RP235XA:
            return 30u;

        case RP235XB:
            return 48u;

        default:
            // A variant this plugin has never heard of.  Reporting 0 disables
            // GPIO control rather than guessing a count that would let the host
            // ask for pins that may not exist.
            ERR("Unknown RP235x variant %u in metadata", variant);
            return 0;
    }
}

void gpio_init_caps(void) {
    context.features = 0;
    context.num_gpios = 0;

    context.gpio_set = context.ora_lookup_fn(ORA_ID_GPIO_SET);
    context.gpio_query = context.ora_lookup_fn(ORA_ID_GPIO_QUERY);

    // Runtime detection, not min_fw_version, is what makes this plugin run on
    // firmware older than the GPIO API: ora_lookup returns NULL for an ID the
    // firmware does not implement, and the capability bits stay clear so the
    // host never sends the commands.  The same precedent is in usb_get_serial().
    if (context.gpio_set == NULL || context.gpio_query == NULL) {
        LOG("Firmware has no GPIO API; GPIO control unavailable");
        return;
    }

    // Everything the host does is sized from num_gpios, including a single-GPIO
    // set, so without it neither command is usable.  The two arrive together in
    // practice - the same firmware version added the key and the ORA calls - so
    // this is a belt-and-braces path, not an expected one.
    context.num_gpios = gpio_num_gpios();
    if (context.num_gpios == 0) {
        LOG("GPIO count unavailable; GPIO control unavailable");
        return;
    }

    context.features = ONEROM_FEAT_GPIO_SET |
                       ONEROM_FEAT_GPIO_QUERY |
                       ONEROM_FEAT_GPIO_HOLD;
    DEBUG("GPIO control available on %u GPIOs", context.num_gpios);
}

// The pending release for a GPIO, or NULL if it has none.
//
// At most one slot is ever active for a given GPIO: gpio_handle_set() reuses
// this slot rather than claiming a second one.
static gpio_release_t *gpio_find_release(uint8_t gpio) {
    for (uint32_t i = 0u; i < ONEROM_GPIO_RELEASES; i++) {
        gpio_release_t *release = &context.gpio_status.releases[i];
        if (release->active && release->gpio == gpio) {
            return release;
        }
    }
    return NULL;
}

// Claim a free release slot, or NULL if all are in use.
static gpio_release_t *gpio_claim_release(void) {
    for (uint32_t i = 0u; i < ONEROM_GPIO_RELEASES; i++) {
        if (!context.gpio_status.releases[i].active) {
            return &context.gpio_status.releases[i];
        }
    }
    return NULL;
}

pb_status_t gpio_handle_set(const onerom_gpio_set_args_t *args) {
    if (!(context.features & ONEROM_FEAT_GPIO_SET)) {
        return PB_STATUS_NOT_PERMITTED;
    }

    // gpio and state are left to ora_gpio_set, which range-checks both against
    // the firmware's own idea of what exists; duplicating that here would be a
    // second source of truth.  after_state and duration_ms are ours.
    if (args->duration_ms != 0u) {
        if (args->duration_ms > ONEROM_GPIO_MAX_HOLD_MS) {
            return PB_STATUS_INVALID_ARG;
        }
        if (!gpio_state_valid(args->after_state)) {
            return PB_STATUS_INVALID_ARG;
        }
    }

    // Only flags this plugin understands are passed on.  Unknown bits are
    // ignored rather than rejected, matching the reserved-field rule: a stale
    // host's garbage must not be mistaken for intent, so a future flag will be
    // gated by a ONEROM_FEAT_* bit rather than inferred from the byte.
    uint32_t ora_flags = (args->flags & ONEROM_GPIO_FLAG_FORCE) ?
                         ORA_GPIO_FLAG_FORCE : 0u;

    // This command supersedes any release already pending for the GPIO - but
    // only once it has actually taken effect.  Dropping the old release up
    // front and then having ora_gpio_set refuse would leave the *earlier*
    // assertion latched with nothing scheduled to end it, which is exactly the
    // outcome bounded holds exist to rule out.  So the old release is found
    // now and disturbed only on success.
    gpio_release_t *existing = gpio_find_release(args->gpio);

    // The slot is settled before anything is driven, for the same reason: a pin
    // must never be asserted with no way to release it.  Reusing this GPIO's
    // own slot when it has one also means a repeated hold on the same pin can
    // never run out of room, however many other pins are held.
    gpio_release_t *release = NULL;
    if (args->duration_ms != 0u) {
        release = (existing != NULL) ? existing : gpio_claim_release();
        if (release == NULL) {
            LOG("No free GPIO release slot for GPIO %u", args->gpio);
            return PB_STATUS_PRECONDITION_NOT_MET;
        }
    }

    // Applied here rather than deferred to the task loop, unlike SET_LED.
    // Dispatch already runs on this plugin's core in the task loop's own
    // context (usb_picoboot_task -> picoboot_task -> pb_task_idle -> dispatch),
    // so calling ORA is safe, and doing it here is the only way a refusal
    // reaches the host as a status instead of being swallowed.
    ora_result_t result = context.gpio_set(args->gpio, args->state, ora_flags);
    if (result != ORA_RESULT_OK) {
        // Nothing has been changed, including any release already pending.
        DEBUG("GPIO %u set to %u refused: %u", args->gpio, args->state, result);
        return gpio_status_from_ora(result);
    }

    if (release != NULL) {
        release->gpio = args->gpio;
        release->after_state = args->after_state;
        release->deadline_ms = context.timer_ms + args->duration_ms;
        release->active = 1;
        DEBUG("GPIO %u held at %u for %ums", args->gpio, args->state, args->duration_ms);
    } else if (existing != NULL) {
        // duration_ms 0 latches indefinitely, so the pending release goes.
        existing->active = 0;
    }

    return PB_STATUS_OK;
}

void gpio_handle_pending_releases(void) {
    // No slot can be claimed without the feature bit, so there is nothing to do
    // and - more to the point - context.gpio_set below cannot be NULL.
    if (!(context.features & ONEROM_FEAT_GPIO_HOLD)) {
        return;
    }

    uint32_t now = context.timer_ms;

    for (uint32_t i = 0u; i < ONEROM_GPIO_RELEASES; i++) {
        gpio_release_t *release = &context.gpio_status.releases[i];
        if (!release->active) {
            continue;
        }

        // Signed difference, so a timer_ms wrap part-way through a hold does not
        // defer the release by another 49.7 days.  ONEROM_GPIO_MAX_HOLD_MS keeps
        // the interval far inside the range where this is unambiguous.
        if ((int32_t)(now - release->deadline_ms) < 0) {
            continue;
        }

        release->active = 0;

        // Forced, always.  The assertion that scheduled this release was itself
        // permitted - either the pin was free, or the host passed force - so the
        // release must not be second-guessed: refusing it would leave the pin
        // latched, which is the one outcome a bounded hold exists to rule out.
        // On a pin that was free anyway the flag changes nothing.
        ora_result_t result = context.gpio_set(release->gpio,
                                               release->after_state,
                                               ORA_GPIO_FLAG_FORCE);
        if (result != ORA_RESULT_OK) {
            ERR("GPIO %u release to %u failed: %u",
                release->gpio, release->after_state, result);
        } else {
            DEBUG("GPIO %u released to %u", release->gpio, release->after_state);
        }
    }
}

pb_status_t gpio_fill_entry(uint8_t gpio, onerom_gpio_entry_t *entry_out) {
    // Zeroed before the call, then told how large our copy is.  The firmware
    // writes at most that many bytes and reports back how many it wrote, so a
    // firmware whose ora_gpio_info_t is shorter than ours leaves the fields it
    // does not know about at zero rather than undefined.
    ora_gpio_info_t info = {0};
    info.size = sizeof(info);

    ora_result_t result = context.gpio_query(gpio, &info);
    if (result != ORA_RESULT_OK) {
        // The run was range-checked when the command was dispatched, so this is
        // not a bad argument reaching us late; treat it as the internal error it
        // would be.
        ERR("GPIO %u query failed: %u", gpio, result);
        return PB_STATUS_UNKNOWN_ERROR;
    }

    entry_out->use = info.use;
    entry_out->level = info.level;
    entry_out->is_output = info.is_output;
    entry_out->reserved = 0;

    return PB_STATUS_OK;
}
