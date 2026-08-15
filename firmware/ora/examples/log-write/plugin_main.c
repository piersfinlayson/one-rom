// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Claims log channel 0 for writing, which gives the channel the name passed,
// writes a few lines to it as raw bytes, then releases the claim.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.  ora/plugin.mk defines
// PLUGIN_IS_SYSTEM to 1 or 0 for every build.
#ifndef PLUGIN_IS_SYSTEM
#error "PLUGIN_IS_SYSTEM is not defined - build this with ora/plugin.mk"
#endif
#if PLUGIN_IS_SYSTEM
ORA_DEFINE_SYSTEM_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 7, 2);
#else // User plugin
ORA_DEFINE_USER_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 7, 2);
#endif // Plugin type check

// The name must stay valid until the close - the firmware keeps the pointer.
static const char channel_name[] = "log-write";

static const char line1[] = "log-write: channel claimed\n";
static const char line2[] = "log-write: raw bytes, no formatting\n";
static const char line3[] = "log-write: releasing the claim\n";

static void park(void) {
    while (1) {
        __asm volatile ("wfe");
    }
}

void plugin_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the log channel functions from the API.
    ora_log_open_write_fn_t log_open_write = ora_lookup_fn(ORA_ID_LOG_OPEN_WRITE);
    ora_log_write_fn_t log_write = ora_lookup_fn(ORA_ID_LOG_WRITE);
    ora_log_close_write_fn_t log_close_write = ora_lookup_fn(ORA_ID_LOG_CLOSE_WRITE);

    if (log_open_write(ORA_LOG_CHANNEL_0, channel_name) ==
        ORA_RESULT_LOG_CHANNEL_IN_USE) {
        park();
    }

    // ORA_RESULT_LOG_FULL is normal when nothing drains the channel, not an error.
    log_write(ORA_LOG_CHANNEL_0, line1, sizeof(line1) - 1);
    log_write(ORA_LOG_CHANNEL_0, line2, sizeof(line2) - 1);
    log_write(ORA_LOG_CHANNEL_0, line3, sizeof(line3) - 1);

    log_close_write(ORA_LOG_CHANNEL_0);

    park();
}
