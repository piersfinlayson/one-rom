// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Implement's One ROM Fire's plugin API 

#include "include.h"

#include "plugin.h"
#include <stdio.h>

uint8_t check_plugin_valid(
    const ora_plugin_header_t *header,
    const ora_plugin_type_t expected_type,
    uint8_t index
) {
    if (header->magic != ORA_PLUGIN_MAGIC) {
        ERR("ORA badmagic 0x%08lx", (unsigned long)header->magic);
        return 0;
    }
    if (header->api_version != ORA_PLUGIN_VERSION_1) {
        ERR("ORA version 0x%08lx", (unsigned long)header->api_version);
        return 0;
    }
    if (header->plugin_type != expected_type) {
        ERR("ORA type %d, expected %d", header->plugin_type, expected_type);
        return 0;
    }

    // A plugin is expected to be located at 0x10010000, 0x10020000, etc based
    // on the specific ROM set it is.
    uint32_t expected_launch_region = (0x1001 + index) << 16;
    uint32_t entry_addr = (uint32_t)(uintptr_t)header->entry;
    if ((entry_addr & ~expected_launch_region) >= 0x10000) {
        ERR("ORA 0x%08lx vs ep 0x%08lx", (unsigned long)entry_addr,
        (unsigned long)expected_launch_region);
        return 0;
    }

    uint16_t min_fw_major = header->min_fw_major_version;
    uint16_t min_fw_minor = header->min_fw_minor_version;
    uint16_t min_fw_patch = header->min_fw_patch_version;
    if (min_fw_major > INFO->major_version ||
        (min_fw_major == INFO->major_version && min_fw_minor > INFO->minor_version) ||
        (min_fw_major == INFO->major_version && min_fw_minor == INFO->minor_version && min_fw_patch > INFO->patch_version)) {
        ERR("ORA reqd v%d.%d.%d vs v%d.%d.%d",
            min_fw_major, min_fw_minor, min_fw_patch,
            INFO->major_version, INFO->minor_version, INFO->patch_version);
        return 0;
    }

    return 1;
}

uint8_t initial_plugin_parse(uint8_t *disable_vbus_det, uint8_t *num_plugins) {
    uint8_t plugins = 0;
    *disable_vbus_det = 0;
    *num_plugins = 0;

    if (METADATA->rom_slot_count == 0) {
        DEBUG("No ROM sets defined, skipping plugin parsing");
        return plugins;
    } else {
        const onerom_rom_slot_t *set = &METADATA->rom_slots[0];
        if (set->slot_type == ROM_SLOT_TYPE_PLUGIN_SYSTEM) {
            const ora_plugin_header_t *header = (ora_plugin_header_t *)(uintptr_t)(set->data);
            if (check_plugin_valid(header, ORA_PLUGIN_TYPE_SYSTEM, 0)) {
                *disable_vbus_det = header->overrides1 & ORA_OVERRIDE1_DISABLE_VBUS_DETECT ? 1 : 0;
                LOG("Valid system plugin=, disable_vbus_det=%d", *disable_vbus_det);
            }

            // Have system plugin (1)
            plugins |= 1;
        } else {
            DEBUG("ROM is not a plugin, skipping plugin parsing");
        }
    }

    if (METADATA->rom_slot_count > 1) {
        const onerom_rom_slot_t *other_set = &METADATA->rom_slots[1];
        if (other_set->slot_type == ROM_SLOT_TYPE_PLUGIN_USER) {
            if (plugins & 0x01) {
                // Have user plugin (2) so check it
                const ora_plugin_header_t *header = (ora_plugin_header_t *)(uintptr_t)(other_set->data);
                if (check_plugin_valid(header, ORA_PLUGIN_TYPE_USER, 1)) {
                    if (header->overrides1 & ORA_OVERRIDE1_DISABLE_VBUS_DETECT) {
                        *disable_vbus_det = 1;
                    }
                    LOG("Valid user plugin, disable_vbus_det=%d", *disable_vbus_det);
                }
                DEBUG("User plugin");
                plugins |= 2;
            } else {
                ERR("Ignoring user plugin without system plugin");
            }
        }
    }


    for (int ii = 0; ii < 2; ii++) {
        if (plugins & (1 << ii)) {
            (*num_plugins)++;
        }
    }


    return plugins;
}

void ora_reboot_bootsel(void) {
#if !defined(TEST_BUILD)
    enter_bootloader();
#endif // !TEST_BUILD

    // Do not return
    while (1);
}

void *ora_alloc(size_t size) {
    (void)size;
    return NULL;
}

void ora_log(const char* msg, ...) {
#if defined(PLUGIN_LOGGING)
#if !defined(TEST_BUILD)
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else // TEST_BUILD
    va_list args;
    va_start(args, msg);
    stub_log_v(msg, args);
    va_end(args);
#endif // !TEST_BUILD
#else
    (void)msg;
#endif // PLUGIN_LOGGING
}

// Not gated on PLUGIN_LOGGING.  A plugin's errors are worth the wrapper this
// costs, and the formatter it calls is in the build regardless.
void ora_err_log(const char* msg, ...) {
#if !defined(TEST_BUILD)
    do_err_log_prefix();
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else // TEST_BUILD
    va_list args;
    va_start(args, msg);
    stub_log_prefix_v("ERROR: ", msg, args);
    va_end(args);
#endif // !TEST_BUILD
}

void ora_debug_log(const char* msg, ...) {
#if defined(PLUGIN_LOGGING) && defined(DEBUG_LOGGING)
#if !defined(TEST_BUILD)
    do_debug_log_prefix();
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else // TEST_BUILD
    va_list args;
    va_start(args, msg);
    stub_log_prefix_v("DEBUG:", msg, args);
    va_end(args);
#endif // !TEST_BUILD
#else
    (void)msg;
#endif // PLUGIN_LOGGING && DEBUG_LOGGING
}

size_t plugin_get_free_mem(void) {
    return 0;
}

void ora_set_status_led(uint8_t on) {
#if !defined(TEST_BUILD)
    uint8_t pin = HW->gpio_status;
    // Pin presence is the only gate: a plugin may drive the status LED even if
    // it was configured off. status_led_enabled is the live state and plugin
    // coordination channel (see ora_set_status_led_fn_t in api.h), so record
    // the new state here as well as driving the pin.
    if (pin < MAX_GPIOS) {
        RUNTIME->status_led_enabled = on ? 1 : 0;
        if (on) {
            status_led_on(pin);
        } else {
            status_led_off(pin);
        }
    }
#else // TEST_BUILD
    LOG("ORA set status LED %d", on);
#endif // !TEST_BUILD
}

void ora_setup_usb(void) {
#if !defined(TEST_BUILD)
    setup_usb_pll();
    setup_usb_controller();
#else // TEST_BUILD
    LOG("ORA setup USB");
#endif // !TEST_BUILD
}

void ora_setup_adc(void) {
#if !defined(TEST_BUILD)
    setup_usb_pll();
    setup_adc();
#else // TEST_BUILD
    LOG("ORA setup ADC");
#endif // !TEST_BUILD
}

void ora_enable_irq(ora_irq_t irq, uint8_t enable) {
#if !defined(TEST_BUILD)
    if (enable) {
        if (irq < 32) {
            NVIC_ISER0 = (1u << irq);
        } else {
            NVIC_ISER1 = (1u << (irq - 32));
        }
    } else {
        if (irq < 32) {
            NVIC_ICER0 = (1u << irq);
        } else {
            NVIC_ICER1 = (1u << (irq - 32));
        }
    }
#else // TEST_BUILD
    LOG("ORA enable IRQ %d %d", irq, enable);
#endif // !TEST_BUILD
}

void ora_register_irq(ora_irq_t irq, ora_irq_handler_t handler) {
#if !defined(TEST_BUILD)
    switch (irq) {
        case ORA_IRQ_TIMER0_IRQ_0:
            RUNTIME->timer0_irq_0_handler = handler;
            if (handler == NULL) {
                ora_enable_irq(ORA_IRQ_TIMER0_IRQ_0, 0);
            }
            break;
        case ORA_IRQ_USBCTRL_IRQ:
            RUNTIME->usbctrl_irq_handler = handler;
            if (handler == NULL) {
                ora_enable_irq(ORA_IRQ_USBCTRL_IRQ, 0);
            }
            break;
        default:
            ERR("Invalid IRQ number for registration: %d", irq);
            break;
    }
#else // TEST_BUILD
    LOG("ORA register IRQ %d %p", irq, handler);
#endif // !TEST_BUILD
}

// Each plugin type has its own context slot, and ORA_GET_PLUGIN_CONTEXT_SYSTEM
// and ORA_GET_PLUGIN_CONTEXT_USER are the addresses of those two slots, so a
// context stored here must land in the one the matching macro reads.
//
// Only the system and user plugins have a slot.  A type without one stores
// nothing and reads back NULL, rather than sharing another type's.
void ora_set_plugin_context(ora_plugin_type_t plugin, void *context) {
    switch (plugin) {
        case ORA_PLUGIN_TYPE_SYSTEM:
            RUNTIME->system_plugin_context = context;
            break;
        case ORA_PLUGIN_TYPE_USER:
            RUNTIME->user_plugin_context = context;
            break;
        default:
            break;
    }
}

void *ora_get_plugin_context(ora_plugin_type_t plugin) {
    switch (plugin) {
        case ORA_PLUGIN_TYPE_SYSTEM:
            return RUNTIME->system_plugin_context;
        case ORA_PLUGIN_TYPE_USER:
            return RUNTIME->user_plugin_context;
        default:
            return NULL;
    }
}

uint32_t ora_get_sysclk_mhz(void) {
    uint16_t sysclk_mhz = RUNTIME->sysclk_mhz;
    return (uint32_t)sysclk_mhz;
}

uint32_t ora_get_clkref_mhz(void) {
    // The clkref frequency is fixed at 12 MHz on Fire, so we can just return
    // that here.
#define CLKREF_MHZ 12
    uint32_t clk_ref_div = (CLOCK_REF_DIV >> 16) & 0xFF;
    clk_ref_div = clk_ref_div ? clk_ref_div : 1;
    return (CLKREF_MHZ / clk_ref_div);
}

// The two halves of the raw microsecond counter.  Split out so that the
// assembly below is one piece of code in both builds: on a device these read
// the registers, and under a test build they draw from a scripted sequence,
// which is what lets a test drive a high-half change a device only produces
// once every 71 minutes.
#if !defined(TEST_BUILD)
static inline uint32_t timer_raw_hi(void) {
    return TIMER0_TIMERAWH;
}

static inline uint32_t timer_raw_lo(void) {
    return TIMER0_TIMERAWL;
}
#else // TEST_BUILD
static inline uint32_t timer_raw_hi(void) {
    return stub_timer_raw_hi();
}

static inline uint32_t timer_raw_lo(void) {
    return stub_timer_raw_lo();
}
#endif // !TEST_BUILD

// Assemble a consistent 64-bit microsecond count from the two halves.
//
// TIMERAWH/TIMERAWL, not the TIMELR/TIMEHR pair: reading TIMELR latches the
// high half, and that latch is one piece of peripheral state shared by both
// cores, so two plugins reading the time would hand each other the wrong high
// half.  The raw registers have no read side effects, at the cost of the
// reader assembling a consistent pair itself.
//
// Read the high half either side of the low one and retry while it moved.  On
// exit the low half was read between two reads of an unchanged high half, so
// the pair belongs to a single instant.
static uint64_t timer_read_us64(void) {
    uint32_t hi = timer_raw_hi();
    uint32_t lo = timer_raw_lo();
    uint32_t hi_again = timer_raw_hi();
    while (hi_again != hi) {
        hi = hi_again;
        lo = timer_raw_lo();
        hi_again = timer_raw_hi();
    }
    return ((uint64_t)hi << 32) | lo;
}

uint32_t ora_get_plugin_uptime_ms(void) {
    // Divide the full 64-bit microsecond count, then truncate.  That is what
    // puts the wrap at 49.7 days.  Dividing a 32-bit microsecond read instead
    // would wrap every 71 minutes.
    return (uint32_t)(timer_read_us64() / 1000u);
}

uint32_t ora_get_chip_size_from_type(uint32_t chip_type) {
    if (chip_type < NUM_CHIP_TYPES) {
        return onerom_chip_type_sizes[chip_type];
    }
    return 0u;
}

uint8_t ora_is_pin_output(uint8_t pin) {
#if !defined(TEST_BUILD)
    if (pin < MAX_GPIOS) {
        return GPIO_IS_OUTPUT(pin);
    }
    return 0xFF;
#else // TEST_BUILD
    LOG("ORA is pin output %d", pin);
    return 0xFF;
#endif // !TEST_BUILD
}

uint8_t ora_get_data_pin_nums(uint8_t *data_pins_out, uint8_t num_pins) {
    uint8_t got_pins = 0;

    // First, get the pin map from the current ROM and the base address pin
    const onerom_rom_pin_map_t *pin_map = RUNTIME->current_rom_slot->roms[0]->pin_map;
    uint8_t base_data_pin = BASE_DATA_PIN;
    uint8_t num_data_pins;
    if (RUNTIME->bit_mode == BIT_MODE_16) {
        num_data_pins = 16;
    } else {
        num_data_pins = 8;
    }

    // Retrieve the data pins
    for (uint8_t ii = 0; (ii < num_data_pins) && (got_pins < num_pins); ii++) {
        data_pins_out[got_pins] = pin_map->data[ii] + base_data_pin;
        got_pins++;
    }

    return got_pins;
}

ora_result_t ora_setup_address_monitor(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
) {
    return pio_setup_address_monitor(ring_buf, ring_entries_log2, mode, data_size, reserved);
}

uint32_t ora_map_addr_to_phys(uint32_t logical_addr) {
    return pio_map_addr_to_phys(CURRENT_SLOT, logical_addr);
}

uint8_t ora_map_data_to_phys(uint8_t logical_data) {
    return pio_map_data_to_phys(CURRENT_SLOT, logical_data);
}

ora_result_t ora_demangle_addr(
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
) {
    return pio_demangle_addr(CURRENT_SLOT, physical_addr, logical_addr_out, check_control_pins);
}

ora_result_t ora_demangle_observed_addr(
    uint32_t physical_addr,
    uint32_t *observed_addr_out,
    uint8_t check_control_pins
) {
    return pio_demangle_observed_addr(CURRENT_SLOT, physical_addr, observed_addr_out, check_control_pins);
}

ora_result_t ora_get_unobserved_addr_bits(uint8_t *bits_out) {
    return pio_get_unobserved_addr_bits(CURRENT_SLOT, bits_out);
}

ora_result_t ora_init_knock(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint8_t data_size,
    ora_knock_t *knock
) {
    return pio_init_knock(knock_seq, knock_len, knock_bits, data_size, knock);
}

ora_result_t ora_wait_for_knock(
    const ora_knock_t *knock,
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    uint32_t flags,
    uint32_t *payload_out,
    uint8_t payload_len,
    volatile uint32_t *start_pos,
    volatile uint32_t **next_read_out
) {
    return pio_wait_for_knock(
        knock,
        ring_buf,
        ring_entries_log2,
        flags, 
        payload_out,
        payload_len,
        start_pos,
        next_read_out
    );
}

ora_result_t ora_reprogram_ram_rom_slot(
    uint8_t slot,
    uint32_t offset,
    const uint8_t *data,
    uint32_t len,
    uint8_t allow_active
) {
    return pio_reprogram_ram_rom_slot(slot, offset, data, len, allow_active);
}

ora_result_t ora_start_address_monitor(void) {
    return pio_start_address_monitor();
}

volatile uint32_t * volatile *ora_get_address_monitor_ring_write_pos(void) {
    return pio_get_address_monitor_ring_write_pos();
}

// The region the RAM slots are carved out of, from the linker script.
extern uint32_t _ram_rom_image_start[];
extern uint32_t _ram_rom_image_end[];

// Bytes available for RAM slots.
static uint32_t ram_rom_image_size(void) {
#if defined(TEST_BUILD)
    // The native test build has no linker script; its stub reserves the same
    // region as a fixed-size object.
    return RAM_ROM_TABLE_SIZE;
#else
    return (uint32_t)((uintptr_t)_ram_rom_image_end
                      - (uintptr_t)_ram_rom_image_start);
#endif
}

uint8_t ora_get_ram_slot_count(void) {
    uint32_t region_size = pio_get_rom_region_size();
    if (region_size == 0u) {
        return 1u;
    }

    // As many as the region holds.  Previously this was a table keyed on the
    // ROM size, capped at 7 so that a slot would be at least 64KB; but a slot
    // has to be exactly one ROM region — that is what makes it servable — so
    // the cap only ever threw away slots that a small ROM would have had.
    uint32_t count = ram_rom_image_size() / region_size;
    if (count < 1u) {
        return 1u;
    }
    if (count > ORA_MAX_RAM_SLOTS) {
        count = ORA_MAX_RAM_SLOTS;
    }
    return (uint8_t)count;
}

ora_result_t ora_get_ram_slot_info(
    uint8_t   ram_slot,
    uint32_t *addr_out,
    uint32_t *size_out,
    uint32_t *rom_type_out
) {
    if (ram_slot >= ora_get_ram_slot_count()) {
        return ORA_RESULT_INVALID_SLOT;
    }

    uint32_t region_size = pio_get_rom_region_size();

    if (addr_out != NULL) {
        *addr_out = SRAM_BASE + (ram_slot * region_size);
    }
    if (size_out != NULL) {
        *size_out = region_size;
    }
    if (rom_type_out != NULL) {
        *rom_type_out = CURRENT_SLOT->roms[0]->rbcp_rom_type;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_get_active_ram_slot(uint8_t *ram_slot_out) {
    if (ram_slot_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint32_t rom_table_addr = (uint32_t)(uintptr_t)RUNTIME->rom_table;

    if (rom_table_addr == 0 || rom_table_addr == 0xFFFFFFFF) {
        return ORA_RESULT_NO_SLOT_ACTIVE;
    }

    uint32_t region_size = pio_get_rom_region_size();
    uint32_t sram_limit = SRAM_BASE + (region_size * ora_get_ram_slot_count());

    if (rom_table_addr < SRAM_BASE || rom_table_addr >= sram_limit) {
        return ORA_RESULT_INTERNAL_ERROR;
    }

    if ((rom_table_addr - SRAM_BASE) % region_size != 0) {
        return ORA_RESULT_INTERNAL_ERROR;
    }

    *ram_slot_out = (uint8_t)((rom_table_addr - SRAM_BASE) / region_size);

    return ORA_RESULT_OK;
}

ora_result_t ora_set_active_ram_slot(uint8_t ram_slot) {
    uint32_t addr, size;
    ora_result_t result = ora_get_ram_slot_info(ram_slot, &addr, &size, NULL);
    if (result != ORA_RESULT_OK) {
        return result;
    }

    result = pio_switch_rom_region(addr);
    if (result == ORA_RESULT_OK) {
        RUNTIME->current_ram_slot = ram_slot;
    }
    return result;
}

static uint8_t is_plugin_type(const onerom_rom_slot_t *slot) {
    return (slot->slot_type == ROM_SLOT_TYPE_PLUGIN_SYSTEM ||
            slot->slot_type == ROM_SLOT_TYPE_PLUGIN_USER   ||
            slot->slot_type == ROM_SLOT_TYPE_PLUGIN_PIO);
}

static uint8_t include_flash_slot(
    const onerom_rom_slot_t *slot,
    const onerom_rom_info_t *rom,
    uint32_t flags
) {
    (void)rom;
    uint8_t plugin = is_plugin_type(slot);
    if (plugin && (flags & ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS)) return 0;
    if (!plugin && (flags & ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS)) return 0;
    return 1;
}

static const onerom_rom_slot_t *get_flash_slot_slot(uint8_t flash_slot, uint32_t flags) {
    uint8_t rom_slot_count = METADATA->rom_slot_count;
    uint8_t filtered_idx = 0;

    for (uint8_t i = 0; i < rom_slot_count; i++) {
        const onerom_rom_slot_t *slot = &METADATA->rom_slots[i];
        const onerom_rom_info_t *rom = slot->roms[0];

        if (!include_flash_slot(slot, rom, flags)) continue;

        if (filtered_idx == flash_slot) {
            return slot;
        }

        filtered_idx++;
    }

    return NULL;
}

uint8_t ora_get_flash_slot_count(uint32_t flags) {
    uint8_t count = 0;
    uint8_t rom_slot_count = METADATA->rom_slot_count;

    for (uint8_t i = 0; i < rom_slot_count; i++) {
        const onerom_rom_slot_t *slot = &METADATA->rom_slots[i];
        const onerom_rom_info_t *rom = slot->roms[0];
        if (include_flash_slot(slot, rom, flags)) count++;
    }

    return count;
}

ora_result_t ora_get_flash_slot_info(
    uint8_t flash_slot,
    uint32_t flags,
    const char **name_out,
    uint32_t *rom_type_out,
    uint8_t *rom_count_out
) {
    const onerom_rom_slot_t *set = get_flash_slot_slot(flash_slot, flags);
    if (set == NULL) {
        return ORA_RESULT_INVALID_SLOT;
    }

    if (name_out != NULL) {
        *name_out = set->roms[0]->filename;
    }
    if (rom_type_out != NULL) {
        *rom_type_out = (uint32_t)set->roms[0]->rbcp_rom_type;
    }
    if (rom_count_out != NULL) {
        *rom_count_out = set->rom_count;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_get_flash_slot_ext_info(
    uint8_t flash_slot,
    uint8_t rom_index,
    uint32_t flags,
    const char **rom_type_out,
    const char **filename_out,
    uint32_t *chip_size_out,
    uint32_t *rbcp_rom_type_out
) {
    const onerom_rom_slot_t *set = get_flash_slot_slot(flash_slot, flags);
    if (set == NULL) {
        return ORA_RESULT_INVALID_SLOT;
    }
    if (rom_index >= set->rom_count) {
        return ORA_RESULT_INVALID_ARG;
    }

    const onerom_rom_info_t *rom = set->roms[rom_index];
    if (rom_type_out != NULL) {
        *rom_type_out = rom->rom_type;
    }
    if (filename_out != NULL) {
        *filename_out = rom->filename;
    }
    if (chip_size_out != NULL) {
        *chip_size_out = rom->chip_size;
    }
    if (rbcp_rom_type_out != NULL) {
        *rbcp_rom_type_out = (uint32_t)rom->rbcp_rom_type;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_copy_flash_slot_to_ram_slot(
    uint8_t flash_slot,
    uint32_t flags,
    uint8_t ram_slot,
    uint32_t copy_flags
) {
    // Async copy via DMA not yet supported
    if (copy_flags & ORA_COPY_FLAG_ASYNC) {
        // Just use synchronous copy for now
    }

    // Find the flash slot, respecting the filter flags
    const onerom_rom_slot_t *set = get_flash_slot_slot(flash_slot, flags);
    if (set == NULL) {
        return ORA_RESULT_INVALID_SLOT;
    }

    // Get the target RAM slot address and size
    uint32_t addr, size;
    ora_result_t result = ora_get_ram_slot_info(ram_slot, &addr, &size, NULL);
    if (result != ORA_RESULT_OK) {
        return result;
    }

    // Refuse to copy if the flash image size doesn't match the RAM slot size,
    // as this indicates an incompatible ROM type
    if (set->size != size) {
        return ORA_RESULT_INVALID_SIZE;
    }

    // Flash data is already in physical layout so copy directly to SRAM
#if REAL_HARDWARE
    memcpy((void *)(uintptr_t)addr, set->data, size);
#else
    memcpy(sram_to_host(addr), set->data, size);
#endif

    return ORA_RESULT_OK;
}

ora_result_t ora_get_device_version(uint8_t *version_out, uint32_t max_len) {
    if (max_len < version_str_len) {
        return ORA_RESULT_INVALID_SIZE;
    }
    memcpy(version_out, version_str, version_str_len);
    return ORA_RESULT_OK;
}

ora_result_t ora_get_metadata_str(ora_metadata_key_t key, const char **out) {
    if (out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    // The per-key arms are generated from the schema `plugin_key` fields and
    // expanded from ONEROM_METADATA_STR_CASES (onerom_metadata.h). A string key
    // resolves its stored value verbatim - NULL when the optional field is
    // unset, returned as OK, not an error, and no serial policy is applied. Any
    // non-string key returns ORA_RESULT_TYPE_MISMATCH. Keys unknown to this
    // firmware fall through to the default below.
    switch (key) {
        ONEROM_METADATA_STR_CASES(out)
        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }
}

ora_result_t ora_get_metadata_uint(ora_metadata_key_t key, uint32_t *out) {
    if (out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    // The per-key arms are generated from the schema `plugin_key` fields and
    // expanded from ONEROM_METADATA_UINT_CASES (onerom_metadata.h). An unsigned
    // scalar/enum key resolves its stored value zero-extended to uint32_t; any
    // non-numeric key returns ORA_RESULT_TYPE_MISMATCH. Keys unknown to this
    // firmware fall through to the default below.
    switch (key) {
        ONEROM_METADATA_UINT_CASES(out)
        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }
}

ora_result_t ora_get_metadata_uint_at(
    ora_metadata_key_t key,
    uint32_t index,
    uint32_t *out
) {
    if (out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    // The per-key arms are generated from the schema `plugin_key` fields and
    // expanded from ONEROM_METADATA_UINT_AT_CASES (onerom_metadata.h). A key
    // whose datum is an array of unsigned elements resolves element `index`,
    // zero-extended to uint32_t, or returns ORA_RESULT_INVALID_ARG if `index`
    // is past the end. Any key that is not such an array returns
    // ORA_RESULT_TYPE_MISMATCH. Keys unknown to this firmware fall through to
    // the default below.
    switch (key) {
        ONEROM_METADATA_UINT_AT_CASES(index, out)
        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }
}

ora_result_t ora_demangle_data(uint8_t physical_data, uint8_t *logical_data_out) {
    if (logical_data_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    *logical_data_out = pio_demangle_data(CURRENT_SLOT, physical_data);
    return ORA_RESULT_OK;
}

#if !defined(TEST_BUILD)

// Private to the framework — not exposed to plugins
#define EXCLUSIVE_MODE_REQUEST  0x584D5251u  // XMRQ
#define EXCLUSIVE_MODE_ACK      0x584D414Bu  // XMAK
#define EXCLUSIVE_MODE_RESUME   0x584D5245u  // XMRE

#define YIELD_BUF_SIZE 64u

// Runs from a stack copy while XIP may be disabled — must be PIC.
// SIO register accesses compile to MOVW/MOVT+LDR/STR, so no PC-relative
// data loads are generated.
extern const uint8_t __yield_wait_for_resume_start[];
extern const uint8_t __yield_wait_for_resume_end[];
__attribute__((section(".yield_wait_for_resume"), noinline, used, naked))
static void yield_wait_for_resume(void) {
    __asm volatile (
        "cpsid i                    \n"
        "movw r1, #0x0000           \n"
        "movt r1, #0xD000           \n"   // r1 = SIO_BASE (0xD0000000)

        // Wait for TX FIFO ready (bit 1 of FIFO_ST)
        "1:                         \n"
        "ldr  r2, [r1, #0x50]       \n"   // SIO_FIFO_ST
        "tst  r2, #2                \n"
        "beq  1b                    \n"

        // Send ACK
        "movw r2, #0x414B           \n"
        "movt r2, #0x584D           \n"   // EXCLUSIVE_MODE_ACK
        "str  r2, [r1, #0x54]       \n"   // SIO_FIFO_WR

        // Pre-load RESUME value
        "movw r0, #0x5245           \n"
        "movt r0, #0x584D           \n"   // EXCLUSIVE_MODE_RESUME

        // Wait for RESUME
        "2:                         \n"
        "ldr  r2, [r1, #0x50]       \n"   // SIO_FIFO_ST
        "tst  r2, #1                \n"
        "beq  2b                    \n"
        "ldr  r2, [r1, #0x58]       \n"   // SIO_FIFO_RD
        "cmp  r2, r0                \n"
        "bne  2b                    \n"

        "cpsie i                    \n"
        "bx   lr                    \n"
    );
}
#endif // !TEST_BUILD

ora_result_t ora_yield(uint8_t *was_paused_out) {
#if !defined(TEST_BUILD)
    if (was_paused_out != NULL) {
        *was_paused_out = 0;
    }

    if (!(SIO_FIFO_ST & 1u)) {
        return ORA_RESULT_OK;
    }

    uint32_t val = SIO_FIFO_RD;
    if (val != EXCLUSIVE_MODE_REQUEST) {
        ERR("ora_yield: unexpected FIFO value 0x%08X", (unsigned)val);
        return ORA_RESULT_OK;
    }

    // The other core has requested to enter exclusive mode, so we need to
    // wait for it to finish before processing.  We do this by copying
    // yield_wait_for_resume onto the stack then executing it.
    LOG("Core pausing...");

    uintptr_t fn_start = (uintptr_t)__yield_wait_for_resume_start & ~1u;
    // fn_size fitting in YIELD_BUF_SIZE is asserted by the linker in
    // flash_rodata.ld
    size_t fn_size = __yield_wait_for_resume_end - __yield_wait_for_resume_start;

    uint8_t buf[YIELD_BUF_SIZE] __attribute__((aligned(4)));
    for (size_t i = 0; i < fn_size; i++) {
        buf[i] = ((const uint8_t *)fn_start)[i];
    }

    ((void (*)(void))((uintptr_t)buf | 1u))();

    LOG("Core resumed");

    if (was_paused_out != NULL) {
        *was_paused_out = 1;
    }

    return ORA_RESULT_OK;
#else // TEST_BUILD
    LOG("ORA yield");
    if (was_paused_out != NULL) {
        *was_paused_out = 0;
    }
    return ORA_RESULT_OK;
#endif // !TEST_BUILD
}

#if !defined(TEST_BUILD)
// Returns  0: no plugin on other core, safe to proceed without FIFO
//          1: plugin present and supports yield
//         -1: plugin present but does not support yield
static int other_core_yield_capability(void) {
    uint32_t this_core = SIO_CPUID;

    const onerom_rom_slot_t *set;
    rom_slot_type_t expected_type;

    if (this_core == 0) {
        if (METADATA->rom_slot_count < 1) {
            return 0;
        }
        set = &METADATA->rom_slots[0];
        expected_type = ROM_SLOT_TYPE_PLUGIN_SYSTEM;
    } else {
        if (METADATA->rom_slot_count < 2) {
            // Return 1 even though there's no plugin - as the firmware _is_
            // calling yield
            return 1;
        }
        set = &METADATA->rom_slots[1];
        expected_type = ROM_SLOT_TYPE_PLUGIN_USER;
    }

    if (set->slot_type != expected_type) {
        return 0;
    }

    const ora_plugin_header_t *header = (const ora_plugin_header_t *)set->data;
    return (header->properties1 & ORA_PROPERTY1_SUPPORTS_YIELD) ? 1 : -1;
}
#endif // !TEST_BUILD

ora_result_t ora_enter_exclusive_mode(void) {
#if !defined(TEST_BUILD)
    int cap = other_core_yield_capability();
    if (cap < 0) return ORA_RESULT_NOT_SUPPORTED;
    if (cap == 0) return ORA_RESULT_OK;

    LOG("Requesting exclusive mode");

    while (!(SIO_FIFO_ST & 2u)) {
        ;
    }
    SIO_FIFO_WR = EXCLUSIVE_MODE_REQUEST;

    uint32_t ack;
    do {
        while (!(SIO_FIFO_ST & 1u)) {
            ;
        }
        ack = SIO_FIFO_RD;
    } while (ack != EXCLUSIVE_MODE_ACK);

    LOG("Exclusive mode granted");

    for (volatile int i = 0; i < 100000; i++) {}

    return ORA_RESULT_OK;
#else // TEST_BUILD
    LOG("ORA enter exclusive mode");
    return ORA_RESULT_OK;
#endif // !TEST_BUILD
}

ora_result_t ora_exit_exclusive_mode(void) {
#if !defined(TEST_BUILD)
    //int cap = other_core_yield_capability();
    //if (cap < 0) return ORA_RESULT_NOT_SUPPORTED;
    //if (cap == 0) return ORA_RESULT_OK;

    LOG("Exiting exclusive mode");

    for (volatile int i = 0; i < 100000; i++) {}

    while (!(SIO_FIFO_ST & 2u)) {
        ;
    }
    SIO_FIFO_WR = EXCLUSIVE_MODE_RESUME;

    LOG("Exclusive mode exit signaled");

    for (volatile int i = 0; i < 1000000; i++) {}

    return ORA_RESULT_OK;
#else // TEST_BUILD
    LOG("ORA exit exclusive mode");
    return ORA_RESULT_OK;
#endif // !TEST_BUILD
}

ora_result_t ora_read_ram_rom_slot(
    uint8_t   slot,
    uint32_t  offset,
    uint8_t  *buf,
    uint32_t  len
) {
    return pio_read_ram_rom_slot(CURRENT_SLOT, slot, offset, buf, len);
}

// Returns what One ROM is using gpio for, as an ora_gpio_use_t.  gpio must
// already have been range checked.
//
// The serving set of the active slot comes from pio_get_gpio_use(), which
// derives it from the configuration the serving path itself acts on.  Board
// system pins are checked afterwards, so a pin doing both is reported as what
// serving is using it for.  Deliberately excluded: the image select pads, whose
// primary use for GPIO control is a wire soldered to a pad whose jumper has been
// removed, and SWCLK/SWDIO, which are not GPIOs on the boards that expose them.
static uint8_t ora_gpio_get_use(uint8_t gpio) {
    uint8_t use = ORA_GPIO_USE_FREE;

    const onerom_rom_slot_t *slot = CURRENT_SLOT;
    if (slot != NULL) {
        if (pio_get_gpio_use(slot, gpio, &use) != ORA_RESULT_OK) {
            use = ORA_GPIO_USE_FREE;
        }
        if (use != ORA_GPIO_USE_FREE) {
            return use;
        }
    }

    // System pins.  Each is GPIO_NONE when the board does not have it, and gpio
    // is known to be less than MAX_GPIOS, so a direct comparison cannot match a
    // missing pin.
    if (gpio == HW->gpio_status ||
        gpio == HW->gpio_neopixel ||
        gpio == HW->gpio_vbus ||
        gpio == HW->gpio_ext_flash_cs) {
        return ORA_GPIO_USE_SYSTEM;
    }

    return ORA_GPIO_USE_FREE;
}

ora_result_t ora_gpio_set(uint8_t gpio, uint8_t state, uint32_t flags) {
    if (gpio >= MAX_GPIOS) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (state != ORA_GPIO_STATE_LOW &&
        state != ORA_GPIO_STATE_HIGH &&
        state != ORA_GPIO_STATE_INPUT) {
        return ORA_RESULT_INVALID_ARG;
    }

    if (!(flags & ORA_GPIO_FLAG_FORCE) &&
        (ora_gpio_get_use(gpio) != ORA_GPIO_USE_FREE)) {
        return ORA_RESULT_GPIO_IN_USE;
    }

#if !defined(TEST_BUILD)
    // Only the function select, the output enables and the pad's output-disable
    // bit are touched.  Pulls, drive strength, slew and the input override are
    // left as they are, so on an ORA_GPIO_USE_SERVING_READ pin - all of which
    // serving leaves as SIO inputs - releasing back to ORA_GPIO_STATE_INPUT
    // restores exactly the configuration serving set up, including any polarity
    // inversion the select lines rely on.
    if (state == ORA_GPIO_STATE_INPUT) {
        // Disable the pad's output driver first: that stops the pin driving
        // whatever peripheral currently owns it.
        GPIO_PAD(gpio) |= (PAD_OUTPUT_DISABLE | PAD_INPUT);
        SIO_GPIO_OE_CLR_PIN(gpio);
        GPIO_CTRL(gpio) = (GPIO_CTRL(gpio) & ~(uint32_t)GPIO_CTRL_FUNC_MASK) |
                          GPIO_CTRL_FUNC_SIO;
    } else {
        // Set the output value before enabling the driver, so the pin never
        // momentarily drives the wrong level.
        if (state == ORA_GPIO_STATE_HIGH) {
            SIO_GPIO_OUT_SET_PIN(gpio);
        } else {
            SIO_GPIO_OUT_CLR_PIN(gpio);
        }
        GPIO_CTRL(gpio) = (GPIO_CTRL(gpio) & ~(uint32_t)GPIO_CTRL_FUNC_MASK) |
                          GPIO_CTRL_FUNC_SIO;
        GPIO_PAD(gpio) &= ~(PAD_OUTPUT_DISABLE | (1u << PAD_ISO_BIT));
        GPIO_PAD(gpio) |= PAD_INPUT;
        SIO_GPIO_OE_SET_PIN(gpio);
    }
#else // TEST_BUILD
    LOG("ORA gpio set %d state %d flags 0x%08x", gpio, state, flags);
#endif // !TEST_BUILD

    return ORA_RESULT_OK;
}

ora_result_t ora_gpio_query(uint8_t gpio, ora_gpio_info_t *info_out) {
    if (info_out == NULL || gpio >= MAX_GPIOS) {
        return ORA_RESULT_INVALID_ARG;
    }

    // The caller tells us how large its own copy of the structure is; we write
    // no more than that, and report back how much we wrote.
    uint8_t caller_size = info_out->size;
    if (caller_size == 0) {
        return ORA_RESULT_INVALID_SIZE;
    }
    if (caller_size > sizeof(ora_gpio_info_t)) {
        caller_size = sizeof(ora_gpio_info_t);
    }

    ora_gpio_info_t info;
    info.size = caller_size;
    info.use = ora_gpio_get_use(gpio);
#if !defined(TEST_BUILD)
    info.level = GPIO_READ(gpio) ? 1 : 0;
    info.is_output = GPIO_IS_OUTPUT(gpio) ? 1 : 0;
#else // TEST_BUILD
    info.level = 0;
    info.is_output = 0;
#endif // !TEST_BUILD

    memcpy(info_out, &info, caller_size);

    return ORA_RESULT_OK;
}

// ---------------------------------------------------------------------------
// Logging API
// ---------------------------------------------------------------------------

// Which plugin holds each channel's write and read claim, indexed by channel.
//
// Zero means unclaimed, so .bss zeroing is the initialiser and this stays
// correct however many channels the ring gains; a held claim stores the
// claiming plugin's ora_plugin_type_t plus one.  Sized by the ring's channel
// count rather than by the number of channels that currently have buffers, so
// adding a buffer needs no change here.
static uint8_t ora_log_writer[ONEROM_RTT_MAX_UP_BUFFERS];
static uint8_t ora_log_reader[ONEROM_RTT_MAX_UP_BUFFERS];

#if REAL_HARDWARE
// Core 1 runs the system plugin and core 0 the user plugin; see
// ora_launch_plugins().  An ORA call runs on the calling plugin's own core, so
// the core identifies the caller with nothing for the plugin to pass and
// nothing for it to spoof.
static ora_plugin_type_t ora_calling_plugin(void) {
    return (SIO_CPUID == 1u) ? ORA_PLUGIN_TYPE_SYSTEM : ORA_PLUGIN_TYPE_USER;
}
#else // !REAL_HARDWARE
// There is no SIO_CPUID under emulation, and the harness drives the firmware
// from one thread, so it says which plugin is calling instead.
static ora_plugin_type_t ora_host_calling_plugin = ORA_PLUGIN_TYPE_SYSTEM;

void set_host_calling_plugin(ora_plugin_type_t plugin) {
    ora_host_calling_plugin = plugin;
}

static ora_plugin_type_t ora_calling_plugin(void) {
    return ora_host_calling_plugin;
}
#endif // REAL_HARDWARE

// A channel exists if it is in range and has a buffer.  Asking the ring rather
// than keeping a count here means P6's extra channels appear on their own.
static uint8_t ora_log_channel_exists(ora_log_channel_t channel) {
    unsigned size = 0u;

    if ((unsigned)channel >= ONEROM_RTT_MAX_UP_BUFFERS) {
        return 0u;
    }
    onerom_rtt_query((unsigned)channel, &size, NULL, NULL);

    return (size != 0u) ? 1u : 0u;
}

// Does the calling plugin hold this claim on this channel?
static uint8_t ora_log_holds(const uint8_t *claims, ora_log_channel_t channel) {
    if (!ora_log_channel_exists(channel)) {
        return 0u;
    }

    return (claims[(unsigned)channel] ==
            (uint8_t)(ora_calling_plugin() + 1u)) ? 1u : 0u;
}

// Spinlock guarding the log claim tables.
//
// A claim is a test and set across two cores, which PRIMASK cannot cover,
// because masking interrupts on one core says nothing about the other.  The
// exclusive monitor is not an alternative: it spans cores only when the memory
// is marked shareable, and this firmware configures no MPU, so LDREX/STREX
// would look correct and silently fail to exclude the other core.
//
// Lock 31 rather than any free number.  See SIO_SPINLOCK in reg-rp235x.h for
// which locks erratum RP2350-E2 leaves usable.  Of those, an allocator handing
// out locks from the SDK's claim free base of 26 reaches 31 last.
#define ORA_LOG_SPINLOCK    31

// Interrupts are masked for as long as the lock is held.  Plugins can register
// interrupt handlers, and nothing stops one calling into the logging API, so a
// handler could preempt its own core mid-claim and then spin forever on a lock
// only the code it interrupted can release.  Masking removes that: the holder
// cannot be preempted on its own core, and the other core is excluded by the
// lock itself.
#if defined(TEST_BUILD)
static uint32_t ora_log_lock(void) { return 0u; }
static void ora_log_unlock(uint32_t primask) { (void)primask; }
#else
static uint32_t ora_log_lock(void) {
    uint32_t primask;

    __asm volatile ("mrs %0, primask \n\t"
                    "cpsid i"
                    : "=r" (primask) :: "memory");

    while (SIO_SPINLOCK(ORA_LOG_SPINLOCK) == 0u)
        ;
    __asm volatile ("dmb" ::: "memory");

    return primask;
}

static void ora_log_unlock(uint32_t primask) {
    __asm volatile ("dmb" ::: "memory");
    SIO_SPINLOCK(ORA_LOG_SPINLOCK) = 0u;
    __asm volatile ("msr primask, %0" :: "r" (primask) : "memory");
}
#endif // TEST_BUILD

static ora_result_t ora_log_claim(uint8_t *claims, ora_log_channel_t channel) {
    ora_result_t result;

    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }

    uint32_t primask = ora_log_lock();
    if (claims[(unsigned)channel] != 0u) {
        result = ORA_RESULT_LOG_CHANNEL_IN_USE;
    } else {
        claims[(unsigned)channel] = (uint8_t)(ora_calling_plugin() + 1u);
        result = ORA_RESULT_OK;
    }
    ora_log_unlock(primask);

    return result;
}

ora_result_t ora_log_open_write(ora_log_channel_t channel, const char *name) {
    ora_result_t result;

    if (name == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    result = ora_log_claim(ora_log_writer, channel);
    if (result != ORA_RESULT_OK) {
        return result;
    }

    // The name is stored, not copied, which is why open documents that it must
    // outlive the claim.  Closing puts the firmware's own name back.
    onerom_rtt_set_name((unsigned)channel, name);

    return ORA_RESULT_OK;
}

ora_result_t ora_log_write(ora_log_channel_t channel, const void *buf,
                           uint32_t len) {
    // A channel this firmware does not have is answered before the claim is
    // considered.  Folded together, a plugin built for a firmware with more
    // channels would be told it failed to claim one that does not exist here.
    // A NULL buffer is the caller's own mistake whatever the channel, so it
    // keeps its own answer and is tested first.
    if (buf == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }
    if (!ora_log_holds(ora_log_writer, channel)) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Nothing to store is not a dropped record.
    if (len == 0u) {
        return ORA_RESULT_OK;
    }

    if (onerom_rtt_write((unsigned)channel, buf, (unsigned)len) == 0u) {
        return ORA_RESULT_LOG_FULL;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_log_close_write(ora_log_channel_t channel) {
    ora_result_t result;

    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }

    uint32_t primask = ora_log_lock();
    if (ora_log_writer[(unsigned)channel] !=
        (uint8_t)(ora_calling_plugin() + 1u)) {
        result = ORA_RESULT_INVALID_ARG;
    } else {
        // The name goes back before the claim is released, so the next owner
        // cannot have its own name reverted under it.
        onerom_rtt_set_name((unsigned)channel, NULL);
        ora_log_writer[(unsigned)channel] = 0u;
        result = ORA_RESULT_OK;
    }
    ora_log_unlock(primask);

    return result;
}

ora_result_t ora_log_open_read(ora_log_channel_t channel) {
    return ora_log_claim(ora_log_reader, channel);
}

ora_result_t ora_log_read(ora_log_channel_t channel, void *buf,
                          uint32_t max_len, uint32_t *copied_out) {
    if ((buf == NULL) || (copied_out == NULL)) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }
    if (!ora_log_holds(ora_log_reader, channel)) {
        return ORA_RESULT_INVALID_ARG;
    }

    *copied_out =
        (uint32_t)onerom_rtt_read((unsigned)channel, buf, (unsigned)max_len);

    return ORA_RESULT_OK;
}

ora_result_t ora_log_close_read(ora_log_channel_t channel) {
    ora_result_t result;

    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }

    uint32_t primask = ora_log_lock();
    if (ora_log_reader[(unsigned)channel] !=
        (uint8_t)(ora_calling_plugin() + 1u)) {
        result = ORA_RESULT_INVALID_ARG;
    } else {
        ora_log_reader[(unsigned)channel] = 0u;
        result = ORA_RESULT_OK;
    }
    ora_log_unlock(primask);

    return result;
}

ora_result_t ora_log_query(ora_log_channel_t channel, uint32_t *size_out,
                           uint32_t *free_out, uint32_t *pending_out) {
    unsigned size = 0u, avail = 0u, pending = 0u;

    if (!ora_log_channel_exists(channel)) {
        return ORA_RESULT_NOT_SUPPORTED;
    }

    onerom_rtt_query((unsigned)channel, &size, &avail, &pending);

    if (size_out != NULL) {
        *size_out = (uint32_t)size;
    }
    if (free_out != NULL) {
        *free_out = (uint32_t)avail;
    }
    if (pending_out != NULL) {
        *pending_out = (uint32_t)pending;
    }

    return ORA_RESULT_OK;
}

// ---------------------------------------------------------------------------
// Compile options and log categories
// ---------------------------------------------------------------------------

// The compile options as values, so each accessor arm is an expression rather
// than a preprocessor branch of its own.  Nothing carries these into
// onerom_runtime_info_t: a build's switches are settled before it runs, and a
// runtime structure is for what changes while it does.
#if defined(PLUGIN_LOGGING)
#define ORA_BUILT_PLUGIN_LOGGING    1u
#else
#define ORA_BUILT_PLUGIN_LOGGING    0u
#endif
#if defined(DEBUG_LOGGING)
#define ORA_BUILT_DEBUG_LOGGING     1u
#else
#define ORA_BUILT_DEBUG_LOGGING     0u
#endif
#if defined(BOOT_LOGGING)
#define ORA_BUILT_BOOT_LOGGING      1u
#else
#define ORA_BUILT_BOOT_LOGGING      0u
#endif

ora_result_t ora_get_compile_option_uint(ora_compile_option_t option,
                                         uint32_t *out) {
    if (out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    switch (option) {
        case ORA_COMPILE_OPTION_PLUGIN_LOGGING:
            *out = ORA_BUILT_PLUGIN_LOGGING;
            return ORA_RESULT_OK;
        case ORA_COMPILE_OPTION_DEBUG_LOGGING:
            *out = ORA_BUILT_DEBUG_LOGGING;
            return ORA_RESULT_OK;
        case ORA_COMPILE_OPTION_BOOT_LOGGING:
            *out = ORA_BUILT_BOOT_LOGGING;
            return ORA_RESULT_OK;
        case ORA_COMPILE_OPTION_BUILD_NUMBER:
            *out = (uint32_t)ONEROM_BUILD_NUMBER;
            return ORA_RESULT_OK;
        case ORA_COMPILE_OPTION_GIT_COMMIT:
            return ORA_RESULT_TYPE_MISMATCH;
        // An option this firmware does not know is a plugin built against
        // newer firmware, which is a version difference for the caller to fall
        // back from - not the caller getting the call wrong.
        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }
}

ora_result_t ora_get_compile_option_str(ora_compile_option_t option,
                                        const char **out) {
    if (out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    switch (option) {
        case ORA_COMPILE_OPTION_GIT_COMMIT:
            *out = ONEROM_GIT_COMMIT;
            return ORA_RESULT_OK;
        case ORA_COMPILE_OPTION_PLUGIN_LOGGING:
        case ORA_COMPILE_OPTION_DEBUG_LOGGING:
        case ORA_COMPILE_OPTION_BOOT_LOGGING:
        case ORA_COMPILE_OPTION_BUILD_NUMBER:
            return ORA_RESULT_TYPE_MISMATCH;
        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }
}

ora_result_t ora_log_category_enabled(ora_log_category_t category,
                                      uint32_t *enabled_out) {
    if (enabled_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    switch (category) {
        case ORA_LOG_CATEGORY_BOOT:
            // BOOT_LOGGING_EN is the same test LOG() and DEBUG() make, so this
            // tracks the gate rather than a copy of it.  do_log then drops
            // everything on a turbo boot device, which is the second gate.
            *enabled_out = (BOOT_LOGGING_EN && !TURBO) ? 1u : 0u;
            break;

        case ORA_LOG_CATEGORY_PLUGIN_INTERNAL:
            // ora_log reaches the channel with no runtime test, so the compile
            // gate is the whole answer.
            *enabled_out = ORA_BUILT_PLUGIN_LOGGING;
            break;

        case ORA_LOG_CATEGORY_DEBUG:
            // DEBUG() is a boot message that a build without debug logging
            // does not contain at all, so it carries the boot gates and the
            // compile gate on top.
            *enabled_out =
                (ORA_BUILT_DEBUG_LOGGING && BOOT_LOGGING_EN && !TURBO) ? 1u : 0u;
            break;

        case ORA_LOG_CATEGORY_ERROR:
            // Neither ERR() nor ora_err_log carries a gate of any kind, by
            // design: whoever hits an error is the least likely to have turned
            // logging on first.  One answer therefore serves both.
            *enabled_out = 1u;
            break;

        case ORA_LOG_CATEGORY_PLUGIN_APPLICATION:
            // The ora_log_write family is a plugin's own channel, and the
            // firmware never gates what a plugin puts there.
            *enabled_out = 1u;
            break;

        case ORA_LOG_CATEGORY_PLUGIN_DEBUG:
            // ora_debug_log is compiled away unless both options are on, and
            // is not runtime gated, so the build settles this on its own.
            *enabled_out =
                (ORA_BUILT_PLUGIN_LOGGING && ORA_BUILT_DEBUG_LOGGING) ? 1u : 0u;
            break;

        default:
            return ORA_RESULT_NOT_SUPPORTED;
    }

    return ORA_RESULT_OK;
}

void *ora_fn_lookup(api_id_t id) {
    switch (id) {
        case ORA_ID_REBOOT_BOOTSEL:
            return ora_reboot_bootsel;
        case ORA_ID_ALLOC:
            return ora_alloc;
        case ORA_ID_LOG:
            return ora_log;
        case ORA_ID_ERR_LOG:
            return ora_err_log;
        case ORA_ID_DEBUG_LOG:
            return ora_debug_log;
        case ORA_ID_GET_FREE_MEM:
            return plugin_get_free_mem;
        case ORA_ID_SET_STATUS_LED:
            return ora_set_status_led;
        case ORA_ID_SETUP_USB:
            return ora_setup_usb;
        case ORA_ID_SETUP_ADC:
            return ora_setup_adc;
        case ORA_ID_REGISTER_IRQ:
            return ora_register_irq;
        case ORA_ID_SET_PLUGIN_CONTEXT:
            return ora_set_plugin_context;
        case ORA_ID_GET_PLUGIN_CONTEXT:
            return ora_get_plugin_context;
        case ORA_ID_GET_SYSCLK_MHZ:
            return ora_get_sysclk_mhz;
        case ORA_ID_ENABLE_IRQ:
            return ora_enable_irq;
        case ORA_ID_GET_CLKREF_MHZ:
            return ora_get_clkref_mhz;
        case ORA_ID_GET_PLUGIN_UPTIME_MS:
            return ora_get_plugin_uptime_ms;
        case ORA_ID_GET_CHIP_SIZE_FROM_TYPE:
            return ora_get_chip_size_from_type;
        case ORA_ID_IS_PIN_OUTPUT:
            return ora_is_pin_output;
        case ORA_ID_GET_DATA_PIN_NUMS:
            return ora_get_data_pin_nums;
        case ORA_ID_SETUP_ADDRESS_MONITOR:
            return ora_setup_address_monitor;
        case ORA_ID_MAP_ADDR_TO_PHYS:
            return ora_map_addr_to_phys;
        case ORA_ID_MAP_DATA_TO_PHYS:
            return ora_map_data_to_phys;
        case ORA_ID_DEMANGLE_ADDR:
            return ora_demangle_addr;
        case ORA_ID_DEMANGLE_OBSERVED_ADDR:
            return ora_demangle_observed_addr;
        case ORA_ID_GET_UNOBSERVED_ADDR_BITS:
            return ora_get_unobserved_addr_bits;
        case ORA_ID_INIT_KNOCK:
            return ora_init_knock;
        case ORA_ID_WAIT_FOR_KNOCK:
            return ora_wait_for_knock;
        case ORA_ID_REPROGRAM_RAM_ROM_SLOT:
            return ora_reprogram_ram_rom_slot;
        case ORA_ID_START_ADDRESS_MONITOR:
            return ora_start_address_monitor;
        case ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS:
            return ora_get_address_monitor_ring_write_pos;
        case ORA_ID_GET_RAM_SLOT_COUNT:
            return ora_get_ram_slot_count;
        case ORA_ID_GET_RAM_SLOT_INFO:
            return ora_get_ram_slot_info;
        case ORA_ID_GET_ACTIVE_RAM_SLOT:
            return ora_get_active_ram_slot;
        case ORA_ID_SET_ACTIVE_RAM_SLOT:
            return ora_set_active_ram_slot;
        case ORA_ID_GET_FLASH_SLOT_COUNT:
            return ora_get_flash_slot_count;
        case ORA_ID_GET_FLASH_SLOT_INFO:
            return ora_get_flash_slot_info;
        case ORA_ID_GET_FLASH_SLOT_EXT_INFO:
            return ora_get_flash_slot_ext_info;
        case ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT:
            return ora_copy_flash_slot_to_ram_slot;
        case ORA_ID_GET_DEVICE_VERSION:
            return ora_get_device_version;
        case ORA_ID_DEMANGLE_DATA:
            return ora_demangle_data;
        case ORA_ID_ENTER_EXCLUSIVE_MODE:
            return ora_enter_exclusive_mode;
        case ORA_ID_EXIT_EXCLUSIVE_MODE:
            return ora_exit_exclusive_mode;
        case ORA_ID_YIELD:
            return ora_yield;
        case ORA_ID_READ_RAM_ROM_SLOT:
            return ora_read_ram_rom_slot; 

        case ORA_ID_GET_METADATA_STR:
            return ora_get_metadata_str;
        case ORA_ID_GET_METADATA_UINT:
            return ora_get_metadata_uint;
        case ORA_ID_GET_METADATA_UINT_AT:
            return ora_get_metadata_uint_at;

        case ORA_ID_GPIO_SET:
            return ora_gpio_set;
        case ORA_ID_GPIO_QUERY:
            return ora_gpio_query;

        case ORA_ID_LOG_OPEN_WRITE:
            return ora_log_open_write;
        case ORA_ID_LOG_WRITE:
            return ora_log_write;
        case ORA_ID_LOG_CLOSE_WRITE:
            return ora_log_close_write;
        case ORA_ID_LOG_OPEN_READ:
            return ora_log_open_read;
        case ORA_ID_LOG_READ:
            return ora_log_read;
        case ORA_ID_LOG_CLOSE_READ:
            return ora_log_close_read;
        case ORA_ID_LOG_QUERY:
            return ora_log_query;

        case ORA_ID_GET_COMPILE_OPTION_UINT:
            return ora_get_compile_option_uint;
        case ORA_ID_GET_COMPILE_OPTION_STR:
            return ora_get_compile_option_str;
        case ORA_ID_LOG_CATEGORY_ENABLED:
            return ora_log_category_enabled;

        // Deprecated functions
        case ORA_ID_GET_FIRMWARE_INFO:
        case ORA_ID_GET_RUNTIME_INFO:
        default:
            return NULL;
    }
}

#if !defined(TEST_BUILD)
static void fifo_drain(void) {
    while (SIO_FIFO_ST & 1u)
        (void)SIO_FIFO_RD;
}

static void fifo_push_blocking(uint32_t val) {
    while (!(SIO_FIFO_ST & 2u))
        ;
    SIO_FIFO_WR = val;

    // Wake up core 1 if it's in WFE waiting for data
    __asm volatile ("sev");
}

static uint32_t fifo_pop_blocking(void) {
    while (!(SIO_FIFO_ST & 1u))
        ;
    return SIO_FIFO_RD;
}

static void reset_core1(void) {
    // Hard reset core 1
    PSM_FRCE_OFF_SET = PSM_PROC1_BIT;
    // Read back to confirm and fence any store buffering
    while (!(PSM_FRCE_OFF & PSM_PROC1_BIT))
        ;

    // Bring core 1 out of reset - its bootrom will drain its FIFO
    // then push a 0 to tell us it's ready
    PSM_FRCE_OFF_CLR = PSM_PROC1_BIT;

    // Wait for core 1 bootrom ready signal
    uint32_t value = fifo_pop_blocking();
    if (value != 0) {
        ERR("Unexpected value from core 1 bootrom: 0x%08lx", (unsigned long)value);
    }
}

// MUST be kept in sync with the values in plugin.ld and changing them forces
// a change to the plugin version.
static const ora_entry_args_t system_plugin_args = {
    .core = ORA_CORE_1,
    .static_ram_base = 0x20081000,
    .static_ram_size = 0x800,
    .stack_top = 0x20081C00,
    .stack_size = 0x400,
};
static const ora_entry_args_t user_plugin_args = {
    .core = ORA_CORE_0,
    .static_ram_base = 0x20081C00,
    .static_ram_size = 0x200,
    .stack_top = 0x20082000,
    .stack_size = 0x200,
    // Note this arbitrarily splits the available 0x400 bytes in half, with
    // one half for static RAM and the other half for stack - it's up to the
    // plugin to manage.
};

ora_result_t ora_yield(uint8_t *was_paused_out);

static void core1_main(void) {
    // Enable hard FP support
    SCB_CPACR |= SCB_CPACR_ENABLE_FP;
    __asm volatile ("dsb");
    __asm volatile ("isb");

    // Read a single uint32_t from the FIFO
    DEBUG("Core 1 started");
    uint32_t core1_plugin_entry = fifo_pop_blocking();
    core1_plugin_entry |= 1;
    ora_plugin_entry_t entry = (ora_plugin_entry_t)(uintptr_t)core1_plugin_entry;
    DEBUG("Core 1 launching plugin at 0x%08lx", (unsigned long)core1_plugin_entry);
    entry(ora_fn_lookup, ORA_PLUGIN_TYPE_SYSTEM, &system_plugin_args);

    ERR("System plugin returned unexpectedly");
    while (1) {
        ora_yield(NULL);
    }
}

extern uint32_t _Min_Stack_Size;
extern uint32_t _estack;

// Paint core 1's stack with a known value to make it easier to detect stack
// usage
void paint_stack_core1(void) {
    uint32_t stack_top = (uint32_t)&_estack;
    uint8_t paint_val = 0x55;
    uint32_t total_stack_size = (uint32_t)&_Min_Stack_Size;
    uint32_t core1_stack_size = total_stack_size / 2;
    uint32_t core1_stack_bottom = stack_top - total_stack_size;
    uint32_t core1_stack_top = core1_stack_bottom + core1_stack_size;
    DEBUG("Painting core 1 stack from 0x%08lx to 0x%08lx with 0x%02x",
          (unsigned long)core1_stack_bottom, (unsigned long)core1_stack_top,
          paint_val);
    for (uint32_t addr = core1_stack_bottom; addr < core1_stack_top; addr++) {
        ((uint8_t *)addr)[0] = paint_val;
    }
}
// Implemented in assembly, as this function clears this core's free stack.
void __attribute__((naked)) paint_stack_core0(void) {
    __asm volatile (
        "ldr  r0, =_estack          \n"  // r0 = top of all stack (_estack addr is the value)
        "ldr  r1, =_Min_Stack_Size  \n"  // r1 = total stack size (linker symbol addr is the value)
        "lsr  r1, r1, #1            \n"  // r1 = core 0 stack size (half of total)
        "sub  r2, r0, r1            \n"  // r2 = core 0 stack bottom (top - size)
        "mov  r3, sp                \n"  // r3 = current SP (caller's frame fully established)
        "movs r0, #0x33             \n"  // r0 = paint value
        "1:                         \n"  // loop start
        "cmp  r2, r3                \n"  // have we reached SP?
        "bhs  2f                    \n"  // if addr >= SP, done
        "strb r0, [r2]              \n"  // paint byte at addr
        "adds r2, r2, #1            \n"  // advance addr
        "b    1b                    \n"  // loop
        "2:                         \n"  // done
        "bx   lr                    \n"  // return
    );
}

void launch_core1(ora_plugin_entry_t plugin_entry) {
    uint32_t core1_stack_top = (uint32_t)&_estack - 1024;

    // Paint core 1's stack
    paint_stack_core1();

    // Reset core 1
    DEBUG("Resetting core 1");
    reset_core1();

    uint32_t cmd_sequence[] = {
        0,
        0,
        1,
        SCB_VTOR,   // Share vector table with core 0
        core1_stack_top,
        (uint32_t)(uintptr_t)core1_main | 1, // Set thumb bit
    };

    uint32_t seq = 0;
    uint32_t count = sizeof(cmd_sequence) / sizeof(cmd_sequence[0]);

    do {
        uint32_t cmd = cmd_sequence[seq];
        if (!cmd) {
            fifo_drain();
            __asm volatile ("sev");
        }
        fifo_push_blocking(cmd);
        uint32_t response = fifo_pop_blocking();
        seq = (cmd == response) ? seq + 1 : 0;
    } while (seq < count);

    uint32_t entry = (uint32_t)(uintptr_t)plugin_entry | 1; // Set thumb bit
    fifo_push_blocking(entry);
}

__attribute__((noinline)) ora_plugin_entry_t launch_plugins_inner(uint8_t *launched_plugins) {
    *launched_plugins = 0;

    // Launch any available system plugin on core 1
    uint8_t system_plugin = 0;
    if (METADATA->rom_slot_count >= 1) {
        const onerom_rom_slot_t *set0 = &METADATA->rom_slots[0];
        if (set0->slot_type == ROM_SLOT_TYPE_PLUGIN_SYSTEM) {
            ora_plugin_header_t *header = (ora_plugin_header_t *)set0->data;
            if (!check_plugin_valid(header, ORA_PLUGIN_TYPE_SYSTEM, 0)) {
                ERR("Invalid system plugin");
            } else {
                const char *filename = set0->roms[0]->filename;
                if (filename != NULL) {
                    LOG("Launching system plugin: %s", filename);
                } else {
                    LOG("Launching system plugin");
                }
                launch_core1(header->entry);
                system_plugin = 1;
                *launched_plugins |= 1u;
            }
        }
    }

    // Launch any available user plugin on core 0 (this core)
    if (METADATA->rom_slot_count >= 2) {
        const onerom_rom_slot_t *set1 = &METADATA->rom_slots[1];
        if (set1->slot_type == ROM_SLOT_TYPE_PLUGIN_USER) {
            ora_plugin_header_t *header = (ora_plugin_header_t *)set1->data;
            if (!check_plugin_valid(header, ORA_PLUGIN_TYPE_USER, 1)) {
                ERR("Invalid user plugin");
            } else if (!system_plugin) {
                ERR("User plugin present but no valid system plugin - not launching");
            } else {
                const char *filename = set1->roms[0]->filename;
                if (filename != NULL) {
                    LOG("Launching user plugin: %s", filename);
                } else {
                    LOG("Launching user plugin");
                }

                paint_stack_core0();

                // Set thumb bit
                uint32_t entry_addr = (uint32_t)(uintptr_t)header->entry | 1;
                ora_plugin_entry_t entry = (ora_plugin_entry_t)(uintptr_t)entry_addr;
                return entry;
            }
        }
    }

    return NULL; // No user plugin to launch
}

void ora_launch_plugins(void) {
    // Plugin-facing setup, in the window where core 0 is in firmware code and
    // core 1 is not yet running.  The timer starts here so a plugin reading
    // ora_get_plugin_uptime_ms() sees time measured from just before launch.
    onerom_rtt_plugins_init();
    DEBUG("Init timer");
    setup_timer0();

    uint8_t launched_plugins = 0;
    ora_plugin_entry_t core0_entry = launch_plugins_inner(&launched_plugins);

    // We launch the user plugin from this outer function in order to save as
    // much stack space as possible.
    if (core0_entry != NULL) {
        core0_entry(ora_fn_lookup, ORA_PLUGIN_TYPE_USER, &user_plugin_args);
        ERR("User plugin returned unexpectedly");
    }

    if (launched_plugins &= 1u) {
        // No user plugin, so just yield to the system plugin forever.
        while (1) {
            ora_yield(NULL);
        }
    }

    // No system plugin, no need to yield, just return.
}

void irq_handler_timer0_irq_0(void) {
    if (RUNTIME->timer0_irq_0_handler) {
        ora_irq_handler_t handler = (ora_irq_handler_t)RUNTIME->timer0_irq_0_handler;
        handler();
    }
}

void irq_handler_usbctrl_irq(void) {
    if (RUNTIME->usbctrl_irq_handler) {
        ora_irq_handler_t handler = (ora_irq_handler_t)RUNTIME->usbctrl_irq_handler;
        handler();
    }
}

#endif // !TEST_BUILD
