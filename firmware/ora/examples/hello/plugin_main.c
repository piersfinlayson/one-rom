// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// A minimal One ROM plugin example.  This plugin logs "Hello One ROM" in an
// infinite loop.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.
#if defined(PLUGIN_TYPE_NUM) && (PLUGIN_TYPE_NUM == ORA_PLUGIN_TYPE_USER)
ORA_DEFINE_SYSTEM_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 6, 7);
#else // User plugin
ORA_DEFINE_USER_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 6, 7);
#endif // Plugin type check

void plugin_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the logging function from the API.
    ora_log_fn_t log = ora_lookup_fn(ORA_ID_LOG);

    while (1) {
        // Log a message every so often
        log("Hello One ROM");
        for (volatile int i = 0; i < 10000000; i++);
    }
}