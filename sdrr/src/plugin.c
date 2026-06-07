// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Implement's One ROM Fire's plugin API 

#include "include.h"

#if defined(RP235X)

#include "plugin.h"
#include <stdio.h>

#if !defined(TEST_BUILD)

void ora_reboot_bootsel(void) {
    enter_bootloader();

    // Do not return
    while (1);
}

void *ora_alloc(size_t size) {
    (void)size;
    return NULL;
}

void *ora_get_firmware_info(void) {
    void *info = (void *)&sdrr_info;
    return info;
}

void *ora_get_runtime_info(void) {
    void *info = (void *)&sdrr_runtime_info;
    return info;
}

void ora_log(const char* msg, ...) {
#if defined(PLUGIN_LOGGING)
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else
    (void)msg;
#endif // PLUGIN_LOGGING
}

void ora_err_log(const char* msg, ...) {
#if defined(PLUGIN_LOGGING)
    do_err_log_prefix();
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else
    (void)msg;
#endif // PLUGIN_LOGGING
}

void ora_debug_log(const char* msg, ...) {
#if defined(BOOT_LOGGING) && defined(DEBUG_LOGGING)
    do_debug_log_prefix();
    va_list args;
    va_start(args, msg);
    do_log_v(msg, &args);
    va_end(args);
#else
    (void)msg;
#endif // BOOT_LOGGING && DEBUG_LOGGING
}

size_t plugin_get_free_mem(void) {
    return 0;
}

void ora_set_status_led(uint8_t on) {
    uint8_t pin = sdrr_info.pins->status;
    if (sdrr_info.status_led_enabled && 
        sdrr_info.extra->runtime_info->status_led_enabled &&
        sdrr_info.pins->status_port == PORT_0 &&
        pin <= MAX_USED_GPIOS) {
        if (on) {
            status_led_on(pin);
        } else {
            status_led_off(pin);
        }
    }
}

void ora_setup_usb(void) {
    setup_usb_pll();
    setup_usb_controller();
}

void ora_setup_adc(void) {
    setup_usb_pll();
    setup_adc();
}

void ora_enable_irq(ora_irq_t irq, uint8_t enable) {
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
}

void ora_register_irq(ora_irq_t irq, ora_irq_handler_t handler) {
    switch (irq) {
        case ORA_IRQ_TIMER0_IRQ_0:
            sdrr_runtime_info.timer0_irq_0_handler = handler;
            if (handler == NULL) {
                ora_enable_irq(ORA_IRQ_TIMER0_IRQ_0, 0);
            }
            break;
        case ORA_IRQ_USBCTRL_IRQ:
            sdrr_runtime_info.usbctrl_irq_handler = handler;
            if (handler == NULL) {
                ora_enable_irq(ORA_IRQ_USBCTRL_IRQ, 0);
            }
            break;
        default:
            ERR("Invalid IRQ number for registration: %d", irq);
            break;
    }
}

void ora_set_plugin_context(void *context) {
    sdrr_runtime_info.system_plugin_context = context;
}

void *ora_get_plugin_context(void) {
    return sdrr_runtime_info.system_plugin_context;
}

uint32_t ora_get_sysclk_mhz(void) {
    uint16_t sysclk_mhz = sdrr_runtime_info.sysclk_mhz;
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

uint32_t ora_get_chip_size_from_type(uint32_t chip_type) {
    if (chip_type < NUM_CHIP_TYPES) {
        return chip_size_from_type[chip_type];
    }
    return 0u;
}

uint8_t ora_is_pin_output(uint8_t pin) {
    if (pin <= MAX_USED_GPIOS) {
        return GPIO_IS_OUTPUT(pin);
    }
    return 0xFF;
}

uint8_t ora_get_data_pin_nums(uint8_t *data_pins_out, uint8_t num_pins) {
    uint8_t got_pins = 0;

    // Stop searching for data pins when the first invalid pin is reached

    // Retrieve first 8 data pins
    for (uint8_t ii = 0; (ii < 8) && (got_pins < num_pins); ii++) {
        if (sdrr_info.pins->data[ii] <= MAX_USED_GPIOS) {
            data_pins_out[got_pins] = sdrr_info.pins->data[ii];
            got_pins++;
        } else {
            return got_pins;
        }
    }

    // Retrieve next 8 data pins from data2
    for (uint8_t ii = 0; (ii < 8) && (got_pins < num_pins); ii++) {
        if (sdrr_info.pins->data2[ii] <= MAX_USED_GPIOS) {
            data_pins_out[got_pins] = sdrr_info.pins->data2[ii];
            got_pins++;
        } else {
            return got_pins;
        }
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
    return pio_map_addr_to_phys(logical_addr);
}

uint8_t ora_map_data_to_phys(uint8_t logical_data) {
    return pio_map_data_to_phys(logical_data);
}

ora_result_t ora_demangle_addr(
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
) {
    return pio_demangle_addr(physical_addr, logical_addr_out, check_control_pins);
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

uint8_t ora_get_ram_slot_count(void) {
    uint8_t effective_addr_pins = pio_get_effective_addr_pins();

    // Slot count based on ROM size:
    // - 2^16=64KB=7
    // - 2^17=128KB=3
    // - 2^18=256KB=2
    // - 2^19+=512KB=1
    if (effective_addr_pins <= 16) return 7;
    if (effective_addr_pins <= 17) return 3;
    if (effective_addr_pins <= 18) return 2;
    return 1;
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
        uint8_t idx = sdrr_runtime_info.rom_set_index;
        *rom_type_out =
            (uint32_t)sdrr_info.metadata_header->rom_sets[idx].roms[0]->rom_type;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_get_active_ram_slot(uint8_t *ram_slot_out) {
    if (ram_slot_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    piorom_config_t *piorom_config = &sdrr_runtime_info.piorom_config;
    uint32_t rom_table_addr = piorom_config->rom_table_addr;

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

    return pio_switch_rom_region(addr);
}

static uint8_t is_plugin_type(sdrr_rom_type_t rom_type) {
    return (rom_type == CHIP_TYPE_SYSTEM_PLUGIN ||
            rom_type == CHIP_TYPE_USER_PLUGIN   ||
            rom_type == CHIP_TYPE_PIO_PLUGIN);
}

static uint8_t include_flash_slot(sdrr_rom_type_t rom_type, uint32_t flags) {
    uint8_t plugin = is_plugin_type(rom_type);
    if (plugin && (flags & ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS)) return 0;
    if (!plugin && (flags & ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS)) return 0;
    return 1;
}

static const sdrr_rom_set_t *get_flash_slot_set(uint8_t flash_slot, uint32_t flags) {
    uint8_t rom_set_count = sdrr_info.metadata_header->rom_set_count;
    uint8_t filtered_idx = 0;

    for (uint8_t i = 0; i < rom_set_count; i++) {
        const sdrr_rom_set_t *set = &sdrr_info.metadata_header->rom_sets[i];
        sdrr_rom_type_t rom_type = set->roms[0]->rom_type;

        if (!include_flash_slot(rom_type, flags)) continue;

        if (filtered_idx == flash_slot) {
            return set;
        }

        filtered_idx++;
    }

    return NULL;
}

uint8_t ora_get_flash_slot_count(uint32_t flags) {
    uint8_t count = 0;
    uint8_t rom_set_count = sdrr_info.metadata_header->rom_set_count;

    for (uint8_t i = 0; i < rom_set_count; i++) {
        sdrr_rom_type_t rom_type = sdrr_info.metadata_header->rom_sets[i].roms[0]->rom_type;
        if (include_flash_slot(rom_type, flags)) count++;
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
    const sdrr_rom_set_t *set = get_flash_slot_set(flash_slot, flags);
    if (set == NULL) {
        return ORA_RESULT_INVALID_SLOT;
    }

    if (name_out != NULL) {
        *name_out = set->roms[0]->filename;
    }
    if (rom_type_out != NULL) {
        *rom_type_out = (uint32_t)set->roms[0]->rom_type;
    }
    if (rom_count_out != NULL) {
        *rom_count_out = set->rom_count;
    }

    return ORA_RESULT_OK;
}

ora_result_t ora_get_flash_slot_ext_info(uint8_t flash_slot, uint32_t flags) {
    (void)flash_slot;
    (void)flags;
    return ORA_RESULT_ERROR;
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
    const sdrr_rom_set_t *set = get_flash_slot_set(flash_slot, flags);
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
    memcpy((void *)addr, set->data, size);

    return ORA_RESULT_OK;
}

ora_result_t ora_get_device_version(uint8_t *version_out, uint32_t max_len) {
    if (max_len < version_str_len) {
        return ORA_RESULT_INVALID_SIZE;
    }
    memcpy(version_out, version_str, version_str_len);
    return ORA_RESULT_OK;
}

ora_result_t ora_demangle_data(uint8_t physical_data, uint8_t *logical_data_out) {
    if (logical_data_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    *logical_data_out = pio_demangle_data(physical_data);
    return ORA_RESULT_OK;
}

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

ora_result_t ora_yield(uint8_t *was_paused_out) {
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
}

// Returns  0: no plugin on other core, safe to proceed without FIFO
//          1: plugin present and supports yield
//         -1: plugin present but does not support yield
static int other_core_yield_capability(void) {
    uint32_t this_core = SIO_CPUID;

    const sdrr_rom_set_t *set;
    sdrr_rom_type_t expected_type;

    if (this_core == 0) {
        if (sdrr_info.metadata_header->rom_set_count < 1) {
            return 0;
        }
        set = &sdrr_info.metadata_header->rom_sets[0];
        expected_type = CHIP_TYPE_SYSTEM_PLUGIN;
    } else {
        if (sdrr_info.metadata_header->rom_set_count < 2) {
            // Return 1 even though there's no plugin - as the firmware _is_
            // calling yield
            return 1;
        }
        set = &sdrr_info.metadata_header->rom_sets[1];
        expected_type = CHIP_TYPE_USER_PLUGIN;
    }

    if (set->roms[0]->rom_type != expected_type) {
        return 0;
    }

    const ora_plugin_header_t *header = (const ora_plugin_header_t *)set->data;
    return (header->properties1 & ORA_PROPERTY1_SUPPORTS_YIELD) ? 1 : -1;
}

ora_result_t ora_enter_exclusive_mode(void) {
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
}

ora_result_t ora_exit_exclusive_mode(void) {
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
}

void *ora_fn_lookup(api_id_t id) {
    switch (id) {
        case ORA_ID_REBOOT_BOOTSEL:
            return ora_reboot_bootsel;
        case ORA_ID_ALLOC:
            return ora_alloc;
        case ORA_ID_GET_FIRMWARE_INFO:
            return ora_get_firmware_info;
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
        case ORA_ID_GET_RUNTIME_INFO:
            return ora_get_runtime_info;
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
        case ORA_ID_YIELD:
            return ora_yield;
        case ORA_ID_ENTER_EXCLUSIVE_MODE:
            return ora_enter_exclusive_mode;
        case ORA_ID_EXIT_EXCLUSIVE_MODE:
            return ora_exit_exclusive_mode;
        default:
            return NULL;
    }
}

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
        ERR("Unexpected value from core 1 bootrom: 0x%08x", value);
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
    DEBUG("Core 1 launching plugin at 0x%08x", core1_plugin_entry);
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
    DEBUG("Painting core 1 stack from 0x%08x to 0x%08x with 0x%02x",
          core1_stack_bottom, core1_stack_top, paint_val);
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

__attribute__((noinline)) ora_plugin_entry_t launch_plugins_inner(const sdrr_info_t *info) {
    // Launch any available system plugin on core 1
    uint8_t system_plugin = 0;
    if (info->metadata_header->rom_set_count >= 1) {
        const sdrr_rom_set_t *set0 = &info->metadata_header->rom_sets[0];
        if (set0->roms[0]->rom_type == CHIP_TYPE_SYSTEM_PLUGIN) {
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
            }
        }
    }

    // Launch any available user plugin on core 0 (this core)
    if (info->metadata_header->rom_set_count >= 2) {
        const sdrr_rom_set_t *set1 = &info->metadata_header->rom_sets[1];
        if (set1->roms[0]->rom_type == CHIP_TYPE_USER_PLUGIN) {
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

void ora_launch_plugins(const sdrr_info_t *info) {
    ora_plugin_entry_t core0_entry = launch_plugins_inner(info);

    // We launch the user plugin from this outer function in order to save as
    // much stack space as possible.
    if (core0_entry != NULL) {
        core0_entry(ora_fn_lookup, ORA_PLUGIN_TYPE_USER, &user_plugin_args);
        ERR("User plugin returned unexpectedly");
    }

    while (1) {
        ora_yield(NULL);
    }
}

void irq_handler_timer0_irq_0(void) {
    if (sdrr_runtime_info.timer0_irq_0_handler) {
        ora_irq_handler_t handler = (ora_irq_handler_t)sdrr_runtime_info.timer0_irq_0_handler;
        handler();
    }
}

void irq_handler_usbctrl_irq(void) {
    if (sdrr_runtime_info.usbctrl_irq_handler) {
        ora_irq_handler_t handler = (ora_irq_handler_t)sdrr_runtime_info.usbctrl_irq_handler;
        handler();
    }
}

#endif // !TEST_BUILD
#endif // RP235X