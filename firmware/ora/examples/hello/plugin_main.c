// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// A minimal One ROM plugin example.  This plugin logs "Hello One ROM" once a
// second.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.  ora/plugin.mk defines
// PLUGIN_IS_SYSTEM to 1 or 0 for every build.
#ifndef PLUGIN_IS_SYSTEM
#error "PLUGIN_IS_SYSTEM is not defined - build this with ora/plugin.mk"
#endif
#if PLUGIN_IS_SYSTEM
ORA_SYSTEM_PLUGIN(plugin_setup, plugin_loop, 0, 1, 0, 0, 0, 7, 2);
#else // User plugin
ORA_USER_PLUGIN(plugin_setup, plugin_loop, 0, 1, 0, 0, 0, 7, 2);
#endif // Plugin type check

static ora_log_fn_t log_msg;

void plugin_setup(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the logging function from the API.
    log_msg = ora_lookup_fn(ORA_ID_LOG);
}

void plugin_loop(void) {
    if (log_msg != NULL) {
        log_msg("Hello One ROM");
    }
    ora_delay_ms(1000);
}
