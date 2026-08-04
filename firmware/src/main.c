// One ROM Main startup code (clock and GPIO initialisation)

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"

int firmware_main(void) {
    // Platform specific initialization
    platform_specific_init();

    // Next step is to validate the minimum metadata required to continue.
    // Beyond here, code can assume onerom_info.metadata_header is present
    // and valid, as are ->hardware_info and ->firmware_config.
    if (!metadata_valid()) {
        // Enter bootloader
        ERR("Invalid metadata");
        enter_bootloader();
    }

    // Update any runtime info based on the metadata.
    update_runtime_from_metadata();

    // Next initialize the GPIOs very early on, to turn off the status LED and
    // read the ROM slot select pins.  Pins are part of the metadata, which is
    // why we had to check that first.
    setup_initial_gpios();

    // Enable logging.  Done after GPIO setup, so SWD pins are configured.
    if (BOOT_LOGGING_EN && !TURBO) {
        LOG_INIT();
    }

    // Do initial plugin parsing.  The system plugin can potentially override
    // USB DFU support, which is why we do it now.
    // Must be done even when turbo booting.
    DEBUG("Initial plugin parse");
    uint8_t disable_vbus_det, plugins, num_plugins;
    plugins = initial_plugin_parse(&disable_vbus_det, &num_plugins);

    // Set up VBUS detect interrupt if there's no plugin replacing USB
    // support.
    if (!disable_vbus_det) {
        DEBUG("Init VBUS det");
        setup_vbus_interrupt();
    } else {
        LOG("Plugin disabled VBUS detect");
    }

    // Log ROM slot information
    if (BOOT_LOGGING_EN && !TURBO) {
        log_roms();
    }

    // If turbo booting, skip the image select jumper reading
    uint32_t sel_mask = 0;
    uint32_t sel_pins = 0;
    if (!TURBO) {
        // Read image select pin values, as this allows us to figure out what
        // ROM slot to serve, which in turn might have some firmware configuration
        // overrides, like overclocking.
        sel_pins = check_sel_pins(&sel_mask);
    }

    // Get the chosen ROM slot based on the image select pins.
    if (METADATA->rom_slot_count > num_plugins) {
        // Figure out what ROM slot to use
        RUNTIME->rom_slot_index = get_rom_slot_index(sel_pins, sel_mask, plugins);

        // Get pointer to the selected ROM slot
        RUNTIME->current_rom_slot = &ROM_SLOTS[RUNTIME->rom_slot_index];

        // Now process any firmware overrides from the selected ROM slot.
        process_firmware_overrides(RUNTIME->current_rom_slot);
    } else {
        ERR("No ROM slots to serve");
    }

    // Initialize clock
    DEBUG("Init clock");
    setup_clock();

    // Set up the RAM table to serve the ROM from
    if (RUNTIME->current_rom_slot != NULL) {
        // We always preload to RAM slot 0
        RUNTIME->current_ram_slot = 0;

        preload_rom_image();
    }

    // Set up the status LED hardware now (unconditionally - the pin is
    // configured as an output whether or not the LED is currently on), so we
    // don't need to call the function from the main loop, which might be
    // running from RAM.
    DEBUG("Init LED");
    setup_status_led();

    // If no ROM slot is selected, enter limp mode now that the status LED is
    // setup.
    if (RUNTIME->current_rom_slot == NULL) {
        limp_mode(LIMP_MODE_NO_ROMS);
    }

    // Turn the LED on as we're ready to serve the ROM.
    if (RUNTIME->status_led_enabled) {
        DEBUG("Status LED on");
        status_led_on(HW->gpio_status);
    }

    // Start serving the ROM.  This returns once the PIOs and DMAs have been
    // setup.  We return back to the reset handler in vector.c, which then
    // starts any plugins.  We do it from higher up the stack, so the minimum
    // stack is used for any plugin running on this core. 
    LOG("Setup ROM serving");

    // Shut SWD down before serving starts, if configured to do so.  Done last
    // so a probe is available for all of boot - including boot logging, which
    // rides RTT over SWD.  Nothing is logged beyond this point, and plugins
    // (started from vector.c once pio() returns) get no logging either.
    //
    // Cheap enough - a RAM test and a few register writes - to do even when
    // turbo booting.
    if (!RUNTIME->swd_enabled) {
        LOG("Disabling SWD");
        disable_swd();
    }

    return pio();
}
