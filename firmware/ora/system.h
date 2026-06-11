// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

/**
 * @file ora_system.h
 * @brief One ROM system configuration and plugin deployment model
 *
 * This header defines the memory map and execution model for One ROM plugins.
 *
 * One ROM distinguishes between two categories of plugin:
 *
 * - System plugins are maintained by the One ROM project and provide core
 *   functionality such as the USB stack.  They are loaded at a fixed base
 *   address and executed on one of the RP2350's cores
 *
 * - User plugins are community-developed extensions to One ROM.  They are
 *   loaded at a separate base address and executed on the other RP2350 core.
 *
 * Only one system plugin and one user plugin can be loaded at a time.
 *
 * The distinction is organisational rather than enforced:
 *
 * - There are no current differences in the API available to system vs user
 *   plugins.
 *
 * - The One ROM firmware does not verify the authenticity of either system or
 *   user plugins.
 *
 * The key goal in building the system plugin via the plugin API is to provide
 * a reference plugin implementation, and sure the plugin API is fit for
 * purpose.
 *
 * Users are welcome to build their own system plugins, but, given the above
 * restriction, they will run in place of the official One ROM system plugin.
 */

#ifndef ORA_SYSTEM_H
#define ORA_SYSTEM_H

/**
 * @brief Base load address for system plugins
 *
 * System plugins must be linked to run from this base address, must be
 * installed to this location on flash, and must be configured as the first
 * image in One ROM's metadata.
 *
 * Data conforming to the layout defined by @ref ora_plugin_header_t must be
 * placed at the start of the plugin binary.
 */
#define ORA_SYSTEM_PLUGIN_BASE  0x10010000U

/**
 * @brief Base load address for user plugins
 *
 * User plugins must be linked to run from this base address, must be
 * installed to this location on flash, and must be configured as the second
 * image in One ROM's metadata, whether or not a system plugin is present.
 *
 * Data conforming to the layout defined by @ref ora_plugin_header_t must be
 * placed at the start of the plugin binary.
 */
#define ORA_USER_PLUGIN_BASE    0x10020000U

#endif /* ORA_SYSTEM_H */