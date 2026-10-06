// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "test/ffi.h"
#include "test/stub.h"
#include "piodma/piodma.h"
#include <epio.h>
#include <apio.h>

extern onerom_runtime_info_t onerom_runtime_info;

void *ffi_runtime_info_ptr(void) {
    return &onerom_runtime_info;
}

uint32_t ffi_runtime_info_size(void) {
    return (uint32_t)sizeof(onerom_runtime_info);
}

// The two plugin context slots, read where an interrupt handler on a device
// reads them.
//
// These are returned through a call rather than by binding the runtime info
// struct itself.  Binding it would put its layout, and the layout of every
// struct it contains, across the FFI boundary - and the bindings are parsed
// for the host while also being compiled for wasm, where the pointer width
// differs, so the sizes bindgen asserts do not hold there.
void *ffi_system_plugin_context(void) {
    return onerom_runtime_info.system_plugin_context;
}

void *ffi_user_plugin_context(void) {
    return onerom_runtime_info.user_plugin_context;
}

uint8_t ffi_limp_mode(void) {
    return (uint8_t)limp_mode_value;
}

uint8_t ffi_pios_enabled(void) {
    return (uint8_t)_apio_emulated_pio.pios_enabled;
}

// The image-select value the firmware read from the sel pins on this boot.
// Lets a test confirm the firmware selected the image the case drove the pins
// for, rather than trusting the stub's own view of what it drove.
uint8_t ffi_image_sel(void) {
    return (uint8_t)RUNTIME->image_sel;
}

// The board size the firmware recorded at boot, an onerom_board_size_t.
uint8_t ffi_board_size(void) {
    return (uint8_t)RUNTIME->board_size;
}

// See ffi.h.
uint32_t ffi_firmware_states(void) {
    return RUNTIME->firmware_states;
}

void ffi_set_plugins_started(void) {
    set_firmware_states(FIRMWARE_STATE_PLUGINS_STARTED);
}

// See ffi.h.
uint8_t ffi_firmware_flags(void) {
    return RUNTIME->firmware_flags;
}

// See ffi.h.  gen-config.c defines the host build's metadata root, and leaves
// it writable so the harness can stand where the CLI does on a device.
extern onerom_metadata_header_t _metadata_start;

uint32_t ffi_metadata_generation(void) {
    return _metadata_start.version;
}

void ffi_set_metadata_generation(uint32_t generation) {
    // Metadata members are const-qualified, which is what stops firmware code
    // writing one.  The harness stands where the CLI does, not where the
    // firmware does.
    *(uint32_t *)(uintptr_t)&_metadata_start.version = generation;
}

// See ffi.h.
uint32_t ffi_rom_slot_flash_addr(uint8_t index) {
    assert(index < METADATA->rom_slot_count);
    return host_rom_slot_flash_addrs[index];
}

void ffi_set_rom_slot_flash_addr(uint8_t index, uint32_t addr) {
    assert(index < METADATA->rom_slot_count);
    host_rom_slot_flash_addrs[index] = addr;
}

uint32_t ffi_rom_slot_size(uint8_t index) {
    assert(index < METADATA->rom_slot_count);
    return ROM_SLOTS[index].size;
}

uint8_t ffi_rom_slot_in_flash(uint8_t index) {
    assert(index < METADATA->rom_slot_count);
    return rom_slot_in_flash(&ROM_SLOTS[index]);
}

uint8_t ffi_check_plugin_valid(
    uint8_t index,
    ora_plugin_type_t expected_type,
    uint8_t plugin_index
) {
    assert(index < METADATA->rom_slot_count);
    return check_plugin_valid(&ROM_SLOTS[index], expected_type, plugin_index);
}

// See ffi.h.
static ora_plugin_header_t plugin_headers[2];

static const onerom_rom_slot_t plugin_slots[2] = {
    {
        .data = (const uint8_t *)&plugin_headers[0],
        .size = SYSTEM_PLUGIN_SIZE,
        .slot_type = ROM_SLOT_TYPE_PLUGIN_SYSTEM,
    },
    {
        .data = (const uint8_t *)&plugin_headers[1],
        .size = USER_PLUGIN_SIZE,
        .slot_type = ROM_SLOT_TYPE_PLUGIN_USER,
    },
};

static const onerom_rom_slot_t *metadata_rom_slots;
static uint8_t metadata_rom_slot_count;

static void save_rom_slots(void) {
    metadata_rom_slots = _metadata_start.rom_slots;
    metadata_rom_slot_count = _metadata_start.rom_slot_count;
}

void ffi_install_plugin_slots(void) {
    assert(METADATA->rom_slot_count >= 2);
    save_rom_slots();
    _metadata_start.rom_slots = plugin_slots;
    *(uint8_t *)(uintptr_t)&_metadata_start.rom_slot_count = 2;
}

// See ffi.h.  The metadata's slots and overrides are const, so the slot table
// and the one slot's overrides are copied to host memory and changed there.
static onerom_rom_slot_t override_slots[UINT8_MAX];
static union {
    onerom_firmware_overrides_t overrides;
    uint8_t bytes[sizeof(onerom_firmware_overrides_t)];
} override_storage;

void ffi_set_rom_slot_override_states(uint8_t index, uint8_t states) {
    uint8_t count = METADATA->rom_slot_count;
    assert(index < count);
    save_rom_slots();
    memcpy(override_slots, metadata_rom_slots, count * sizeof(onerom_rom_slot_t));

    const onerom_firmware_overrides_t *overrides =
        metadata_rom_slots[index].firmware_overrides;
    memset(override_storage.bytes, 0, sizeof(override_storage.bytes));
    if ((overrides != NULL) && (overrides != (void *)0xFFFFFFFF)) {
        memcpy(override_storage.bytes, overrides, sizeof(override_storage.bytes));
    }
    override_storage.bytes[offsetof(onerom_firmware_overrides_t, override_states_stored)] =
        states;

    override_slots[index].firmware_overrides = &override_storage.overrides;
    _metadata_start.rom_slots = override_slots;
}

void ffi_restore_rom_slots(void) {
    _metadata_start.rom_slots = metadata_rom_slots;
    *(uint8_t *)(uintptr_t)&_metadata_start.rom_slot_count =
        metadata_rom_slot_count;
}

void ffi_set_plugin_header(
    uint8_t index,
    uint32_t entry,
    uint8_t overrides1,
    uint8_t properties1
) {
    assert(index < 2);
    ora_plugin_header_t *header = &plugin_headers[index];
    memset(header, 0, sizeof(*header));
    header->magic = ORA_PLUGIN_MAGIC;
    header->api_version = ORA_PLUGIN_VERSION_1;
    header->entry = (ora_plugin_entry_t)(uintptr_t)entry;
    header->plugin_type =
        (index == 0) ? ORA_PLUGIN_TYPE_SYSTEM : ORA_PLUGIN_TYPE_USER;
    header->overrides1 = overrides1;
    header->properties1 = properties1;
}

// See ffi.h.  base_addr_pin is an offset within the PIO's GPIOBASE window, so
// the absolute first GPIO sampled is gpio_base + base_addr_pin.
uint8_t ffi_serving_alg(ffi_serving_alg_t *out) {
    if (out == NULL || CURRENT_SLOT == NULL || CURRENT_SLOT->alg == NULL) {
        return 0u;
    }
    const onerom_alg_config_t *alg = CURRENT_SLOT->alg;
    if (alg->alg_addr == NULL || alg->alg_cs == NULL || alg->alg_data == NULL) {
        return 0u;
    }

    out->addr_alg = (uint8_t)alg->alg_addr->alg;
    out->cs_alg = (uint8_t)alg->alg_cs->alg;
    out->data_alg = (uint8_t)alg->alg_data->alg;
    out->addr_window_base =
        (uint8_t)(alg->alg_addr->gpio_base + alg->alg_addr->base_addr_pin);
    out->addr_window_pins = alg->alg_addr->num_addr_pins;

    return 1u;
}

void ffi_epio_setup_sram(epio_t *epio) {
    uint64_t *source = get_ram_rom_image_table_aligned();
    epio_sram_set(epio, SRAM_BASE, (uint8_t *)source, RAM_ROM_TABLE_SIZE);
}

// See ffi.h.
void ffi_epio_update_from_apio(epio_t *epio) {
    for (uint8_t block = 0; block < APIO_MAX_PIO_BLOCKS; block++) {
        uint8_t enabled = _apio_emulated_pio.enabled_sms[block];
        for (uint8_t sm = 0; sm < APIO_MAX_SMS_PER_BLOCK; sm++) {
            if (!(enabled & (1u << sm)) && epio_is_sm_enabled(epio, block, sm)) {
                epio_disable_sm(epio, block, sm);
            }
        }
    }
    epio_update_from_apio(epio);
}

void ffi_epio_setup_dma_chain(epio_t *epio, uint8_t word_size) {
    epio_dma_setup_read_pio_chain(
        epio,
        DMA_CH_ADDR_READ,
        BLOCK_ADDR,
        SM_ADDR_READ,
        4,
        BLOCK_CS_DATA,
        SM_DATA_WRITE,
        4,
        word_size
    );
}

// Address-monitor capture DMA wiring.
//
// The firmware's pio_setup_address_monitor_dma has no DMA registers under
// emulation; it calls monitor_dma_configure_cb (installed by
// ffi_epio_arm_monitor) with the block/SM/ring it CHOSE.  We wire epio's
// capture channel from that choice — so a wrong block choice by the firmware
// is caught — and point the firmware's ring-write-position slot at epio's live
// capture write pointer.
static epio_t *s_monitor_epio;

static void monitor_dma_configure_cb(
    uint8_t src_block,
    uint8_t src_sm,
    void *ring_buf,
    uint8_t ring_size_log2,
    uint8_t data_size
) {
    uint32_t ring_base = SRAM_BASE +
        (uint32_t)((uint8_t *)ring_buf - epio_get_sram_ptr(s_monitor_epio));
    epio_dma_setup_capture_pio_ring(s_monitor_epio, DMA_CH_ADDR_MONITOR,
                                    src_block, src_sm, 1,
                                    ring_base, ring_size_log2, data_size);
    set_host_monitor_write_slot((volatile uint32_t * volatile *)
        epio_dma_capture_write_slot(s_monitor_epio, DMA_CH_ADDR_MONITOR));
}

// Arm the address-monitor emulation seam.  Call once after setup_epio and
// before the firmware configures the address monitor.
void ffi_epio_arm_monitor(epio_t *epio) {
    s_monitor_epio = epio;
    set_host_monitor_dma_configure(monitor_dma_configure_cb);
}

// The LED engine's frame, which a device reaches through TIMER0 alarm 1.  There
// is no alarm in this process, so a harness stands where the interrupt does:
// it moves the clock to ffi_led_next_deadline() and calls this.
void ffi_led_frame(void) {
    pio_led_frame();
}

// When the engine next wants a frame, in the milliseconds a plugin sees.
// Returns 0 when nothing is animating and no hold is running.
uint8_t ffi_led_next_deadline(uint32_t *ms_out) {
    return pio_led_next_deadline(ms_out);
}

// The last colour the engine sent to the RGB LED, and how many it has sent.
uint32_t ffi_led_last_pixel(uint32_t *count_out) {
    return pio_led_last_pixel(count_out);
}

// Take the engine back to the state it holds before boot sets an LED, so a
// test starts from a known one rather than from what the test before it left.
// Neither LED is driven from here - what a channel is doing is forgotten, not
// turned off - so a test that cares about the pin sets a mode after it.
void ffi_led_reset(void) {
    pio_led_reset();
}

void ffi_set_logging(uint8_t enabled) {
    logging_enabled = enabled;
}