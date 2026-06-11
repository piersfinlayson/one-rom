// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

/**
 * @file ora/plugin.h
 * @brief One ROM plugin API
 *
 * This is the single header a One ROM plugin needs to include.  It provides
 * everything required to define a valid plugin binary, including the plugin
 * header structure, magic and version constants, load addresses, and
 * convenience macros for generating the plugin header.
 *
 * A minimal plugin source file looks like:
 *
 * @code
 * #include <plugin.h>
 *
 * ORA_DEFINE_USER_PLUGIN(plugin_main);
 *
 * void plugin_main(
 *    ora_lookup_fn_t ora_lookup_fn,
 *    ora_plugin_type_t plugin_type,
 *    ora_entry_args_t *entry_args
 * ) {
 *     // Lookup plugin API functions using ora_lookup_fn.
 *     // Implement desired plugin function.
 * }
 * @endcode
 * 
 * One ROM's plugin API is supported and available on all One ROM Fire
 * hardware revisions from firmware version v0.6.7 onwards, so long as PIO
 * serving mode is selected (this is the default for all Fire boards from
 * v0.6.7 onwards).
 * 
 * One ROM is a bare metal, source constrained environment so while much of
 * power of the RP2350 is available, developers must be mindful of resource
 * limitations and hardware constraints.  The tl;dr is that it possible to
 * impact, or even break, the ROM serving functionality of the core firmware.
 * 
 * On the other hand, the low-level access allows you to do some very cool
 * things.  For example, you could dynamically reconfigure the ROM serving
 * PIO state machines, to change timings or add additional functionality.
 * You can change the contents of RAM from under the ROM serving code,
 * causing it to instantly change the ROM image being served, or monitor ROM
 * access and dynamically change the ROM image being served in response to
 * certain access patterns.  Twiddle One ROM's external header pins to drive
 * external systems, flash the status LED, or even bitbang a protocol of
 * your own, all while One ROM is serving a ROM image to the target device.
 * 
 * Some example plugins are provided in `sdrr/ora/examples` to demonstrate the
 * API and build process.  A sample Makefile and linker script are also
 * provided.
 * 
 * Detailed guidelines for writing plugins:
 * 
 * - Your plugin_main() is called after the ROM serving functionality has been
 *   started.  Up to two plugins are supported - they may be started in either
 *   order (but the system plugin will likely be started first).  It is
 *   expected never to return.  If it returns it will not be scheduled again,
 *   unless One ROM is reset/rebooted.
 * 
 * - You do not have access to the RP2350 SDK.  This is a fully bare metal
 *   environment, so hopefully you are wearing your big girl/boy pants today.
 * 
 * - Once plugin_main() is called, you are likely to want to call @ref
 *   ora_lookup_fn to get pointers to the API functions you want to use.
 *   Store these on the stack, or in memory you allocate via @ref ora_alloc_fn_t.
 * 
 * - The plugin may be started before the full load of the ROM image to be
 *   served has loaded into SRAM from flash, as this is done asyncronously
 *   using DMA.  It would be possible to figure out the load state by querying
 *   the register for the approriate DMA channel.  This may be exposed via the
 *   API in the future.
 * 
 * - The plugin must manage its own data, bss (and any other RAM sections) and
 *   stack allocation.  This management includes initialisation of these
 *   sections on startup.
 * 
 * - The arguments provided to the plugin's entry point indicates its static
 *   RAM address, size, stack start and stack size.  The plugin may use
 *   ora_alloc to allocated additional memory from the free pool, but this is
 *   not guaranteed to succeed, and will likely fail on larger pin count One
 *   ROMs (32 and 40 pin).  The static RAM address, size, and stack details
 *   are provided in the plugin's entry point arguments as well as supplied
 *   by the plugin.ld linker script.
 * 
 * - The stack is limited to 1KB per core/plugin.  Some of this stack is used
 *   by the firmware before calling the plugin entry point, so less than 1KB
 *   will be available.  For user plugins, this 1KB _includes_ static RAM
 *   allocation.
 * 
 * - Different One ROM pin variants have different amount of free RAM, with
 *   One ROM 32 and One ROM 40 having minimal available RAM - small numbers
 *   of KB - so be prepared to program like it's 1980.  On the other hand, if
 *   your plugin targets One ROM 24 or 28, there will be at least 256KB of RAM
 *   available, although the other plugin might allocate some of it.  It's
 *   first come, first served.
 * 
 * - There are two types of plugins available - system and user.  See system.h
 *   for more details, but in summary each must be installed in the correct
 *   image slot on flash using One ROM's config JSON file, and system is
 *   reserved for offical One ROM plugins, such as One ROM's built-in USB
 *   stack.
 * 
 * - You have access to all of the RP2350 registers, but accessing these
 *   directly can have unintended consequences.
 * 
 * - The most likely way you are to break ROM serving is by:
 *   - Reconfiguring the PIOs
 *   - Modifying the DMA channel configuation
 *   - Long sustained SRAM access (contending with the ROM serving DMA access)
 *   - Rebooting the device (there is an API provided for this purpose)
 * 
 * - A plugin is limited to 64KB.  However, it could in theory load additional
 *   code or data into RAM from other image slots on flash.
 * 
 * - The plugin is run from flash, as one some One ROMs there simply isn't
 *   enough RAM to load meaningful amounts of code into RAM.  However, your
 *   plugin could load itself into RAM, and jump to it there. 
 * 
 * - Plugins may need to co-exist on the same One ROM.  From time to time, a
 *   plugin might need exclusive access to the device, for example, to erase
 *   and/or reprogram flash.  APIs are provided to allow a plugin to request
 *   and release exclusive access, and also to check if it should yield
 *   (pause) to allow the other plugin to gain exclusive access.  A plugin
 *   can indicate that it supports periodic yielding using the plugin header.
 */

#ifndef ORA_PLUGIN_H
#define ORA_PLUGIN_H

#include <api.h>
#include <system.h>

/**
 * @brief Define a One ROM plugin header for a given base address and main function
 *
 * Places an @ref ora_plugin_header_t in the @c .plugin_header section with
 * the correct magic, version, and main function offset.  Not normally used
 * directly — prefer @ref ORA_DEFINE_SYSTEM_PLUGIN or
 * @ref ORA_DEFINE_USER_PLUGIN.
 *
 * This should only be used for simple plugins.  For complex plugins, which
 * use statically allocated RAM, overrides, etc, define the @ref
 * ora_plugin_header_t directly.
 *
 * @param base  Base load address of the plugin
 * @param fn    Plugin main function @ref ora_plugin_entry_t
 * @param major Plugin major version number
 * @param minor Plugin minor version number
 * @param patch Plugin patch version number
 * @param build Plugin build version number
 * @param min_fw_major Minimum major version of the firmware this plugin is compatible with
 * @param min_fw_minor Minimum minor version of the firmware this plugin is compatible with
 * @param min_fw_patch Minimum patch version of the firmware this plugin is compatible with
 */
#define ORA_DEFINE_PLUGIN_HEADER(plugin, fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch) \
    void fn( \
        ora_lookup_fn_t ora_lookup_fn, \
        ora_plugin_type_t plugin_type, \
        const ora_entry_args_t *entry_args \
    ); \
    __attribute__((section(".plugin_header"))) \
    const ora_plugin_header_t ora_plugin_header = { \
        .magic    = ORA_PLUGIN_MAGIC, \
        .api_version  = ORA_PLUGIN_VERSION_1, \
        .major_version = major, \
        .minor_version = minor, \
        .patch_version = patch, \
        .build_version = build, \
        .entry  = fn, \
        .plugin_type = plugin, \
        .sam_usage = 0, \
        .overrides1 = 0, \
        .min_fw_major_version = min_fw_major, \
        .min_fw_minor_version = min_fw_minor, \
        .min_fw_patch_version = min_fw_patch, \
        .reserved = {0}, \
    }

/**
 * @brief Define the header for a One ROM system plugin
 *
 * Place this macro at the top of your plugin's main source file, passing the
 * name of your main function.  It generates the @ref ora_plugin_header_t
 * required by the One ROM firmware at the correct location in the binary.
 *
 * @param fn    Plugin main function
 * @param major Plugin major version number
 * @param minor Plugin minor version number
 * @param patch Plugin patch version number
 * @param build Plugin build version number
 * @param min_fw_major Minimum major version of the firmware this plugin is compatible with
 * @param min_fw_minor Minimum minor version of the firmware this plugin is compatible with
 * @param min_fw_patch Minimum patch version of the firmware this plugin is compatible with
 *
 * @sa ORA_DEFINE_USER_PLUGIN, ORA_DEFINE_PIO_PLUGIN
 */
#define ORA_DEFINE_SYSTEM_PLUGIN(fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch) \
    ORA_DEFINE_PLUGIN_HEADER(ORA_PLUGIN_TYPE_SYSTEM, fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch)

/**
 * @brief Define the header for a One ROM user plugin
 *
 * Place this macro at the top of your plugin's main source file, passing the
 * name of your main function.  It generates the @ref ora_plugin_header_t
 * required by the One ROM firmware at the correct location in the binary.
 *
 * @param fn    Plugin main function
 * @param major Plugin major version number
 * @param minor Plugin minor version number
 * @param patch Plugin patch version number
 * @param build Plugin build version number
 * @param min_fw_major Minimum major version of the firmware this plugin is compatible with
 * @param min_fw_minor Minimum minor version of the firmware this plugin is compatible with
 * @param min_fw_patch Minimum patch version of the firmware this plugin is compatible with
 *
 * @sa ORA_DEFINE_SYSTEM_PLUGIN, ORA_DEFINE_PIO_PLUGIN
 */
#define ORA_DEFINE_USER_PLUGIN(fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch) \
    ORA_DEFINE_PLUGIN_HEADER(ORA_PLUGIN_TYPE_USER, fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch)

/**
 * @brief Define the header for a One ROM PIO plugin
 *
 * Place this macro at the top of your plugin's main source file, passing the
 * name of your main function.  It generates the @ref ora_plugin_header_t
 * required by the One ROM firmware at the correct location in the binary.
 *
 * @param fn    Plugin main function
 * @param major Plugin major version number
 * @param minor Plugin minor version number
 * @param patch Plugin patch version number
 * @param build Plugin build version number
 * @param min_fw_major Minimum major version of the firmware this plugin is compatible with
 * @param min_fw_minor Minimum minor version of the firmware this plugin is compatible with
 * @param min_fw_patch Minimum patch version of the firmware this plugin is compatible with
 *
 * @sa ORA_DEFINE_SYSTEM_PLUGIN, ORA_DEFINE_USER_PLUGIN
 */
#define ORA_DEFINE_PIO_PLUGIN(fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch) \
    ORA_DEFINE_PLUGIN_HEADER(ORA_PLUGIN_TYPE_PIO, fn, major, minor, patch, build, min_fw_major, min_fw_minor, min_fw_patch)

#endif /* ORA_PLUGIN_H */