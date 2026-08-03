// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

/**
 * @file api.h
 * @brief One ROM's plugin API
 *
 * This file defines the API for plugins to interact with the One ROM firmware.
 * It includes an enumeration of API function identifiers, a lookup function
 * to retrieve function pointers based on these identifiers, and the prototypes
 * for the API functions themselves.
 */

#if !defined(PLUGIN_API_H)
#define PLUGIN_API_H

#include <stdint.h>
#include <stddef.h>

#include <onerom_metadata_keys_generated.h>

/**
 * @brief Place an object in a named linker section
 *
 * Plugins place certain objects — the plugin header, a ring buffer — at
 * addresses their linker script fixes, using a named section.  Use this macro
 * rather than the @c section attribute directly, so the same source also
 * compiles for a host-side test build, where those sections do not exist and
 * the attribute would be rejected outright by some host toolchains.
 *
 * Expands to nothing when @c ORA_HOST_TEST is defined; to the @c section
 * attribute otherwise.  Placement on a real device is therefore unchanged.
 *
 * @param sec Section name, e.g. ".ring_buf"
 */
#if defined(ORA_HOST_TEST)
#define ORA_SECTION(sec)
#else
#define ORA_SECTION(sec) __attribute__((section(sec)))
#endif

#if defined(ORA_HOST_TEST)
/**
 * @brief Host-test seam: translate a device SRAM address to a usable pointer
 *
 * Provided by the test harness, not by the firmware.  See @ref ORA_SRAM_PTR.
 */
void *ora_host_test_sram_ptr(uint32_t addr);

/**
 * @brief Host-test seam: hand control back to the harness
 *
 * Provided by the test harness, not by the firmware.  See @ref ORA_TEST_YIELD.
 */
void ora_host_test_yield(void);
#endif

/**
 * @brief Obtain a dereferenceable pointer for a device SRAM address
 *
 * Several API calls hand a plugin an address in the device's own address space
 * — @ref ora_get_ram_slot_info_fn_t returns a RAM slot's base, for instance.
 * Use this macro to turn such an address into a pointer before dereferencing
 * it.  Arithmetic on the address itself (bounds checks, offsets, alignment) is
 * unaffected and should carry on using the plain value; only the dereference
 * needs converting.
 *
 * On a device this is the identity — the address already is a pointer, and the
 * macro compiles away to nothing.  Under a host-side test build the plugin runs
 * in a process whose address space is not the device's, and whose pointers are
 * wider, so the harness maps the address into the emulated SRAM it serves from.
 *
 * Call it once per region and keep the pointer, exactly as you would already
 * hoist a slot base out of a loop; there is no reason to convert per byte.
 *
 * Only addresses within the device's SRAM can be mapped.  Flash addresses and
 * peripheral registers have no host equivalent and are not covered.
 *
 * @code
 * uint32_t slot_base;
 * get_ram_slot_info(slot, &slot_base, NULL, NULL);
 * volatile uint8_t *slot = ORA_SRAM_PTR(slot_base);
 * slot[offset] = value;
 * @endcode
 *
 * @param addr Device SRAM address, as returned by the plugin API
 *
 * @since firmware v0.7.1
 */
#if defined(ORA_HOST_TEST)
#define ORA_SRAM_PTR(addr) ora_host_test_sram_ptr(addr)
#else
#define ORA_SRAM_PTR(addr) ((void *)(uintptr_t)(addr))
#endif

/**
 * @brief Allow a host-side test build to make progress inside a busy-wait
 *
 * Place this in any loop whose exit condition depends on state only the other
 * core or external hardware can change — a spin on a ring-buffer write
 * pointer, a poll of a location the host writes, a wait on a DMA pointer.
 *
 * On a device it compiles to nothing whatsoever.  It is therefore *not* a
 * delay, *not* a scheduling primitive, and carries no timing guarantee of any
 * kind; never reach for it to pace a loop.  It is also unrelated to
 * @ref ORA_ID_YIELD, which is about co-operating with the other plugin over
 * exclusive access — a different concern entirely, despite the name.
 *
 * Under a host-side test build the emulated hardware only advances when the
 * harness advances it, so a loop like the above would spin forever with
 * nothing to change its exit condition.  This macro hands control back so the
 * harness can drive the next stimulus.
 *
 * Omitting it costs nothing on a device; the only consequence is that the loop
 * cannot be exercised on a host.
 *
 * @code
 * while (read_idx == write_idx) {
 *     ORA_TEST_YIELD();
 * }
 * @endcode
 *
 * @since firmware v0.7.1
 */
#if defined(ORA_HOST_TEST)
#define ORA_TEST_YIELD() ora_host_test_yield()
#else
#define ORA_TEST_YIELD() ((void)0)
#endif

/**
 * @defgroup plugin_device_facts Device facts
 * @brief Absolute addresses and pointer forms that only exist on the device
 *
 * A plugin that writes flash has to reach past the plugin API to a handful of
 * facts about the chip it runs on: where the bootrom's lookup table is, where
 * flash is mapped, how a pointer to staged code is formed.  Each is a bare
 * absolute address or a target-specific pointer conversion, so each is a place
 * a host-side test build would fault rather than misbehave.
 *
 * Every one of them is named individually here rather than covered by one
 * general "read this address" escape hatch.  A general one would let any
 * absolute access through unexamined, and the value of these is precisely that
 * the list is short and each entry says which device fact it stands for.  On a
 * device each compiles to the same expression the plugin used to write inline.
 *
 * @{
 */

/**
 * @brief Address holding the pointer to the bootrom's table-lookup routine
 *
 * Fixed by the RP2350 boot ROM.  Only meaningful on the device.
 */
#define ORA_BOOTROM_TABLE_LOOKUP_ADDR 0x00000016u

/** @brief Bootrom table flags selecting the Arm secure entry points */
#define ORA_BOOTROM_FLAG_ARM_SEC 0x0004u

/** @brief QMI memory-window 0 timing register, whose low byte is the clock
 * divisor.  Only meaningful on the device. */
#define ORA_XIP_QMI_M0_TIMING_ADDR 0x400D000Cu

/** @brief Base of the XIP-mapped flash window.  Only meaningful on the
 * device. */
#define ORA_FLASH_BASE_ADDR 0x10000000u

/**
 * @brief Most RAM slots the firmware will report
 *
 * A slot index travels in a single byte, and RBCP reserves 0xFF to mean "no
 * slot is active", so the highest usable index is 0xFE and the count cannot
 * exceed 255.
 *
 * Note that a *host* cannot name every one of those: RBCP rejects 0xAA in
 * every slot argument, so that a reset started mid-command stays detectable.
 * A plugin that exposes slots to a host must cap what it advertises at 170
 * accordingly — see the host-control plugin, which keeps the slots above that
 * for its own use.
 */
#define ORA_MAX_RAM_SLOTS 255u

#if !defined(ORA_HOST_TEST)
/**
 * @brief Device implementation of @ref ORA_BOOTROM_LOOKUP
 *
 * A function rather than a macro body so that the diagnostic suppression the
 * absolute dereference needs has somewhere ordinary to live.  The compiler
 * cannot know that address 0x16 is a valid object, and says so.
 */
static inline void *ora_bootrom_lookup_impl(uint32_t code, uint32_t mask) {
    typedef void *(*ora_rom_table_lookup_fn)(uint32_t code, uint32_t mask);
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Warray-bounds"
    ora_rom_table_lookup_fn lookup = (ora_rom_table_lookup_fn)(uintptr_t) *
        (uint16_t *)(ORA_BOOTROM_TABLE_LOOKUP_ADDR);
#pragma GCC diagnostic pop
    return lookup(code, mask);
}
#endif

#if defined(ORA_HOST_TEST)
/** @brief Host-test seam.  See @ref ORA_BOOTROM_LOOKUP. */
void *ora_host_test_bootrom_lookup(uint32_t code, uint32_t mask);
/** @brief Host-test seam.  See @ref ORA_XIP_CLKDIV. */
uint8_t ora_host_test_xip_clkdiv(void);
/** @brief Host-test seam.  See @ref ORA_FLASH_OFFSET. */
uint32_t ora_host_test_flash_offset(const void *addr);
/** @brief Host-test seam.  See @ref ORA_STAGED_FN_SIZE. */
uint32_t ora_host_test_staged_fn_size(const void *start, const void *end);
/** @brief Host-test seam.  See @ref ORA_STAGED_FN_PTR. */
void *ora_host_test_staged_fn_ptr(uint32_t addr);
#endif

/**
 * @brief Look a bootrom function up by its two-character code
 *
 * The RP2350 bootrom publishes a table of functions — flash erase, flash
 * program, XIP control — reached through a lookup routine whose address is
 * itself stored at a fixed absolute address low in the address map.  A plugin
 * needs these to touch flash at all, because flash cannot be read while it is
 * being written and the routines that do it must not themselves be in flash.
 *
 * On a device this reads that fixed address and calls through it.  Under a
 * host-side test build there is no bootrom, and address 0x16 is not mapped, so
 * the harness supplies stand-ins instead.
 *
 * @param code Two-character function code, packed low byte first
 * @param mask Bootrom table flags selecting the architecture and mode
 * @return The function's address, or NULL if the bootrom does not publish it
 *
 * @since firmware v0.7.2
 */
#if defined(ORA_HOST_TEST)
#define ORA_BOOTROM_LOOKUP(code, mask) ora_host_test_bootrom_lookup((code), (mask))
#else
#define ORA_BOOTROM_LOOKUP(code, mask) ora_bootrom_lookup_impl((code), (mask))
#endif

/**
 * @brief The XIP clock divisor currently in force
 *
 * The erase sequence takes flash out of XIP mode and must put it back the way
 * it found it, so the divisor has to be read before XIP is disabled and handed
 * to the routine that restores it.  It lives in a QMI timing register.
 *
 * On a device this reads that register.  Under a host-side test build there is
 * no QMI, and its address is not mapped.
 *
 * @since firmware v0.7.2
 */
#if defined(ORA_HOST_TEST)
#define ORA_XIP_CLKDIV() ora_host_test_xip_clkdiv()
#else
#define ORA_XIP_CLKDIV()                                                      \
    ((uint8_t)(*((volatile uint32_t *)(uintptr_t)ORA_XIP_QMI_M0_TIMING_ADDR)  \
               & 0xFFu))
#endif

/**
 * @brief Convert a pointer into flash to an offset the bootrom understands
 *
 * The bootrom's flash routines take an offset from the start of flash, while a
 * plugin knows its reserved region as a linker symbol — an address in the
 * XIP-mapped window.  This is the conversion between them.
 *
 * On a device it is a subtraction from the base of the mapped window.  Under a
 * host-side test build the object is somewhere in the process's own heap or
 * BSS, and that subtraction would produce a meaningless — and, on a 64-bit
 * host, truncated — number, so the harness supplies the offset into whatever
 * it is standing the region in for.
 *
 * @param addr Pointer into the device's flash
 *
 * @since firmware v0.7.2
 */
#if defined(ORA_HOST_TEST)
#define ORA_FLASH_OFFSET(addr) ora_host_test_flash_offset((const void *)(addr))
#else
#define ORA_FLASH_OFFSET(addr)                                                \
    ((uint32_t)((uintptr_t)(addr) - ORA_FLASH_BASE_ADDR))
#endif

/**
 * @brief Size of a routine bracketed by a pair of linker symbols
 *
 * A routine that runs while flash is unreadable must be copied into RAM first,
 * and the plugin's linker script brackets it with a start and end symbol so
 * that the plugin can find out how much to copy.
 *
 * On a device that is the difference between the two.  Under a host-side test
 * build there is no such section and therefore no bracketing symbols: whatever
 * the harness defines under those names are unrelated objects, and the
 * difference between pointers into unrelated objects is meaningless — it is
 * not merely inaccurate but arbitrary, and has been observed to be large
 * enough to fault.  So the harness answers for the size instead.
 *
 * @param start First byte of the routine
 * @param end   One past its last byte
 *
 * @since firmware v0.7.2
 */
#if defined(ORA_HOST_TEST)
#define ORA_STAGED_FN_SIZE(start, end) ora_host_test_staged_fn_size((start), (end))
#else
#define ORA_STAGED_FN_SIZE(start, end)                                        \
    ((uint32_t)((const uint8_t *)(end) - (const uint8_t *)(start)))
#endif

/**
 * @brief Form a callable pointer to a routine the plugin has staged in SRAM
 *
 * Having copied a position-independent routine into a RAM slot, the plugin
 * calls it through a pointer built from that address.  On a Cortex-M the
 * pointer must carry the Thumb bit.
 *
 * On a device that is the address with its low bit set.  Under a host-side
 * test build the staged bytes are not executable, the host's instruction set
 * is not Thumb, and setting the low bit of a host function pointer corrupts
 * it, so the harness supplies a pointer to its own stand-in for the routine.
 *
 * @param type Function pointer type to produce
 * @param addr Device SRAM address the routine was copied to
 *
 * @since firmware v0.7.2
 */
#if defined(ORA_HOST_TEST)
#define ORA_STAGED_FN_PTR(type, addr) ((type)ora_host_test_staged_fn_ptr(addr))
#else
#define ORA_STAGED_FN_PTR(type, addr) ((type)(uintptr_t)((addr) | 1u))
#endif

/** @} */

/**
 * @defgroup plugin_api One ROM Plugin API
 * @brief The complete API for One ROM plugins
 * @{
 */

/**
 * @defgroup plugin_api_ids API Identifiers
 * @brief Enumeration of API function identifiers
 * @{
 */

/**
 * @brief API function identifiers
 *
 * This enumeration defines the identifiers for the API functions available
 * to plugins. Each identifier corresponds to a specific API function.
 *
 * Every new identifier MUST carry an `@since firmware vX.Y.Z` line in its doc
 * block, naming the firmware version in which it first became available (which
 * a plugin targets via its min_fw_version). Identifiers predating this
 * convention are left unannotated rather than labelled with a guessed version.
 */
typedef enum {
    /**
     * @brief Reboot into BOOTSEL mode
     * @sa ora_reboot_bootsel_fn_t
     */
    ORA_ID_REBOOT_BOOTSEL = 0x00000000,

    /**
     * @brief Allocate memory
     * @sa ora_alloc_fn_t
     */
    ORA_ID_ALLOC = 0x00000001,

    /**
     * @brief Get One ROM information
     * @sa ora_get_firmware_info_fn_t
     */
    ORA_ID_GET_FIRMWARE_INFO = 0x00000002,

    /**
     * @brief Log a message
     * @sa ora_log_fn_t
     */
    ORA_ID_LOG = 0x00000003,

    /**
     * @brief Log an error message
     * @sa ora_err_log_fn_t
     */
    ORA_ID_ERR_LOG = 0x00000004,

    /**
     * @brief Log a debug message
     * @sa ora_debug_log_fn_t
     */
    ORA_ID_DEBUG_LOG = 0x00000005,

    /**
     * @brief Get free memory size
     * @sa ora_get_free_mem_fn_t
     */
    ORA_ID_GET_FREE_MEM = 0x00000006,

    /**
     * @brief Set the status LED on or off
     * @sa ora_set_status_led_fn_t
     */
    ORA_ID_SET_STATUS_LED = 0x00000007,

    /**
     * @brief Setup the USB PLL
     * @sa ora_setup_usb_pll_fn_t
     */
    ORA_ID_SETUP_USB = 0x00000008,

    /**
     * @brief Setup the ADC
     * @sa ora_setup_adc_fn_t
     */
    ORA_ID_SETUP_ADC = 0x00000009,

    /**
     * @brief Register an IRQ handler
     * @sa ora_register_irq_fn_t
     */
    ORA_ID_REGISTER_IRQ = 0x0000000A,

    /**
     * @brief Set plugin context
     * @sa ora_set_plugin_context_fn_t
     */
    ORA_ID_SET_PLUGIN_CONTEXT = 0x0000000B,

    /**
     * @brief Get plugin context
     * @sa ora_get_plugin_context_fn_t
     */
    ORA_ID_GET_PLUGIN_CONTEXT = 0x0000000C,

    /**
     * @brief Get the current system clock frequency in MHz
     * @sa ora_get_sysclk_mhz_fn_t
     */
    ORA_ID_GET_SYSCLK_MHZ = 0x0000000D,

    /**
     * @brief Enable an IRQ
     * @sa ora_enable_irq_fn_t
     */
    ORA_ID_ENABLE_IRQ = 0x0000000E,

    /**
    * @brief Get the clkref frequency in MHz
    * @sa ora_get_clkref_mhz_fn_t
    */
    ORA_ID_GET_CLKREF_MHZ = 0x0000000F,

    /**
     * @brief Get a pointer to the runtime info structure
      * @sa ora_ntime_info_fn_t
     */
    ORA_ID_GET_RUNTIME_INFO = 0x00000010,

    /**
     * @brief Get the size of a ROM from its type
     * @sa ora_get_chip_size_from_type_fn_t
     */
    ORA_ID_GET_CHIP_SIZE_FROM_TYPE = 0x00000011,

    /**
     * @brief Check if a pin is configured as an output
     * @sa ora_is_pin_output_fn_t
     */
    ORA_ID_IS_PIN_OUTPUT = 0x00000012,

    /**
     * @brief Retrieve the first num_pins data pin numbers
     * @sa ora_get_data_pin_nums_fn_t
     */
    ORA_ID_GET_DATA_PIN_NUMS = 0x00000013,

    /**
     * @brief Set up the address monitor PIO and DMA
     * @sa ora_setup_address_monitor_fn_t
     */
    ORA_ID_SETUP_ADDRESS_MONITOR = 0x00000014,

    /**
     * @brief Remap a logical address to its physical GPIO/PIO representation
     * @sa ora_map_addr_to_phys_fn_t
     */
    ORA_ID_MAP_ADDR_TO_PHYS = 0x00000015,

    /**
     * @brief Remap a logical data byte to its physical GPIO/PIO representation
     * @sa ora_map_data_to_phys_fn_t
     */
    ORA_ID_MAP_DATA_TO_PHYS = 0x00000016,

    /**
     * @brief Demangle a captured physical address back to a logical address
     * @sa ora_demangle_addr_fn_t
     */
    ORA_ID_DEMANGLE_ADDR = 0x00000017,

    /**
     * @brief Initialise a knock sequence
     * @sa ora_init_knock_fn_t
     */
    ORA_ID_INIT_KNOCK = 0x00000018,

    /**
     * @brief Wait for a knock sequence to be detected
     * @sa ora_wait_for_knock_fn_t
     */
    ORA_ID_WAIT_FOR_KNOCK = 0x00000019,

    /** 
     * @brief Reprogram a RAM ROM slot with new data
     * @sa ora_reprogram_ram_rom_slot_fn_t
     */
    ORA_ID_REPROGRAM_RAM_ROM_SLOT = 0x0000001A,

    /**
     * @brief Start the address monitor PIO state machines
     * @sa ora_start_address_monitor_fn_t
     */
    ORA_ID_START_ADDRESS_MONITOR = 0x0000001B,

    /**
     * @brief Get a pointer to the address monitor ring buffer write position
     * @sa ora_get_address_monitor_ring_write_pos_fn_t
     */
    ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS = 0x0000001C,

    /**
     * @brief Get the number of RAM slots available for the current ROM type
     * @sa ora_get_ram_slot_count_fn_t
     */
    ORA_ID_GET_RAM_SLOT_COUNT = 0x0000001D,

    /**
     * @brief Get the SRAM address and size of a RAM slot
     * @sa ora_get_ram_slot_info_fn_t
     */
    ORA_ID_GET_RAM_SLOT_INFO = 0x0000001E,

    /**
     * @brief Get the index of the currently active RAM slot
     * @sa ora_get_active_ram_slot_fn_t
     */
    ORA_ID_GET_ACTIVE_RAM_SLOT = 0x0000001F,

    /**
     * @brief Atomically switch the active RAM slot
     * @sa ora_set_active_ram_slot_fn_t
     */
    ORA_ID_SET_ACTIVE_RAM_SLOT = 0x00000020,

    /**
     * @brief Get the number of flash slots available
     * @sa ora_get_flash_slot_count_fn_t
     */
    ORA_ID_GET_FLASH_SLOT_COUNT = 0x00000021,

    /**
     * @brief Get information about a flash slot
     * @sa ora_get_flash_slot_info_fn_t
     */
    ORA_ID_GET_FLASH_SLOT_INFO = 0x00000022,

    /**
     * @brief Get extended, per-ROM information about a flash slot
     * @sa ora_get_flash_slot_ext_info_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GET_FLASH_SLOT_EXT_INFO = 0x00000023,

    /**
     * @brief Copy a flash slot into a RAM slot
     * @sa ora_copy_flash_slot_to_ram_slot_fn_t
     */
    ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT = 0x00000024,

    /**
     * @brief Get the device version string
     * @sa ora_get_device_version_fn_t
     */
    ORA_ID_GET_DEVICE_VERSION = 0x00000025,

    /**
     * @brief Demangle a captured physical data byte back to a logical byte
     * @sa ora_demangle_data_fn_t
     */
    ORA_ID_DEMANGLE_DATA = 0x00000026,

    /**
     * @brief Enter exclusive mode
     * @sa ora_enter_exclusive_mode_fn_t
     */
    ORA_ID_ENTER_EXCLUSIVE_MODE = 0x00000027,

    /**
     * @brief Exit exclusive mode
     * @sa ora_exit_exclusive_mode_fn_t
     */
    ORA_ID_EXIT_EXCLUSIVE_MODE  = 0x00000028,

    /**
     * @brief Yield to allow another core to enter exclusive mode
     * @sa ora_yield_fn_t
     */
    ORA_ID_YIELD                = 0x00000029,

    /** 
     * @brief Read a logical region of a RAM ROM slot into a buffer.
     * @sa ora_read_ram_rom_slot_fn_t
     */
    ORA_ID_READ_RAM_ROM_SLOT         = 0x0000002A,

    /**
     * @brief Get a device-level metadata string by key
     * @sa ora_get_metadata_str_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GET_METADATA_STR          = 0x0000002B,

    /**
     * @brief Get a device-level unsigned metadata value by key
     *
     * Numeric sibling of ORA_ID_GET_METADATA_STR over the same unified key
     * space. Resolves, among others, ORA_METADATA_KEY_STATUS_LED_STATE - the
     * live status-LED state and cross-plugin coordination channel (see
     * ora_set_status_led_fn_t).
     * @sa ora_get_metadata_uint_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GET_METADATA_UINT         = 0x0000002C,

    /**
     * @brief Demangle a captured physical address to the address the device
     *        observes on its address lines
     * @sa ora_demangle_observed_addr_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_DEMANGLE_OBSERVED_ADDR    = 0x0000002D,

    /**
     * @brief Get the number of least-significant address bits the device does
     *        not observe for the current ROM
     * @sa ora_get_unobserved_addr_bits_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GET_UNOBSERVED_ADDR_BITS  = 0x0000002E,

    /**
     * @brief Drive a GPIO high or low, or release it to high impedance
     * @sa ora_gpio_set_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GPIO_SET                  = 0x0000002F,

    /**
     * @brief Query what One ROM is using a GPIO for, and its current state
     * @sa ora_gpio_query_fn_t
     * @since firmware 0.7.1
     */
    ORA_ID_GPIO_QUERY                = 0x00000030,

    /** Invalid API identifier */
    ORA_ID_INVALID = 0xFFFFFFFF,
} api_id_t;
STATIC_ASSERT(sizeof(api_id_t) == 4, "api_id_t must be 4 bytes");

/** @} */ // plugin_api_ids

/**
 * @defgroup plugin_api_types API Types
 * @brief Types use by the plugin API
 * @{
 */

/**
 * @brief Plugin types
 */
typedef enum {
    ORA_PLUGIN_TYPE_SYSTEM = 0,
    ORA_PLUGIN_TYPE_USER   = 1,
    ORA_PLUGIN_TYPE_PIO    = 2,
} ora_plugin_type_t;
STATIC_ASSERT(sizeof(ora_plugin_type_t) == 1, "ora_plugin_type_t must be 1 byte");

/**
 * @brief MCU Cores
 */
typedef enum {
    ORA_CORE_0 = 0,
    ORA_CORE_1 = 1,
} ora_core_t;
STATIC_ASSERT(sizeof(ora_core_t) == 1, "ora_core_t must be 1 byte");

/**
 * @brief IRQ numbers
 */
typedef enum {
    ORA_IRQ_TIMER0_IRQ_0 = 0,
    ORA_IRQ_USBCTRL_IRQ = 14,
    ORA_IRQ_INVALID = 0xFF,
} ora_irq_t;
STATIC_ASSERT(sizeof(ora_irq_t) == 1, "ora_irq_t must be 1 byte");

/**
 * @brief Knock sequence state structure
 *
 * Holds the precomputed mask and match values for a knock sequence as
 * initialised by @ref ora_init_knock_fn_t. The matches array is a flexible
 * array member — this structure must never be declared directly. Instead, use
 * @ref ORA_KNOCK_DECLARE to declare a correctly sized instance on the stack or
 * as a static, or allocate sufficient space via @ref ora_alloc_fn_t using
 * @ref ORA_KNOCK_SIZE.
 *
 * The contents of this structure must not be modified directly after
 * initialisation.
 */
typedef struct {
    /** @brief Mask to apply to each captured GPIO value */
    uint32_t mask;

    /** @brief Number of entries in the knock sequence */
    uint8_t len;

    /** @brief Number of low-order address bits used for matching */
    uint8_t bits;

    /** @brief Number of bits to capture for each address */
    uint8_t data_size;

    /** @brief Multi-ROM mode flag */
    uint8_t multi_rom_mode;

    /** @brief Chip select mask to use for debouncing */
    uint32_t cs_mask;

    /** @brief X pin mask for multi-rom sets */
    uint32_t x_mask;

    /**
     * @brief Precomputed match values, one per knock sequence entry.
     * Length determined at declaration time via @ref ORA_KNOCK_DECLARE or
     * @ref ORA_KNOCK_SIZE.
     */
    uint32_t matches[];

} ora_knock_t;

/**
 * @brief Calculate the size in bytes of an ora_knock_t for a given sequence length
 *
 * For use when allocating dynamically via @ref ora_alloc_fn_t.
 *
 * @param knock_len Number of entries in the knock sequence
 */
#define ORA_KNOCK_SIZE(knock_len) \
    (sizeof(ora_knock_t) + ((knock_len) * sizeof(uint32_t)))

/**
 * @brief Declare a correctly sized knock structure and a pointer to it
 *
 * Declares a backing store named @p name##_storage of the correct size for
 * @p knock_len entries, zero-initialised, and an @ref ora_knock_t pointer
 * named @p name pointing to it. The pointer is suitable for passing to
 * @ref ora_init_knock_fn_t and @ref ora_wait_for_knock_fn_t.
 *
 * Example:
 * @code
 * ORA_KNOCK_DECLARE(knock, 7);
 * init_knock(seq, 7, 8, knock);
 * @endcode
 *
 * @param name      Name for the pointer variable and backing store
 * @param knock_len Number of entries in the knock sequence
 */
#define ORA_KNOCK_DECLARE(name, knock_len)                          \
    uint8_t name##_storage[ORA_KNOCK_SIZE(knock_len)];              \
    ora_knock_t *name = (ora_knock_t *)name##_storage

/**
 * @brief Calculate the size in bytes of a ring buffer for a given log2 entry count
 *
 * @param ring_entries_log2 Log2 of the number of 8-bit entries in the ring buffer
 */
#define ORA_RING_BUF_SIZE_8BIT(ring_entries_log2) \
        ((1u << (ring_entries_log2)) * sizeof(uint8_t))

        /**
 * @brief Calculate the size in bytes of a ring buffer for a given log2 entry count
 *
 * @param ring_entries_log2 Log2 of the number of 16-bit entries in the ring buffer
 */
#define ORA_RING_BUF_SIZE_16BIT(ring_entries_log2) \
        ((1u << (ring_entries_log2)) * sizeof(uint16_t))

        /**
 * @brief Calculate the size in bytes of a ring buffer for a given log2 entry count
 *
 * @param ring_entries_log2 Log2 of the number of 32-bit entries in the ring buffer
 */
#define ORA_RING_BUF_SIZE_32BIT(ring_entries_log2) \
        ((1u << (ring_entries_log2)) * sizeof(uint32_t))

/**
 * @brief Declare a correctly sized and aligned ring buffer
 *
 * Declares a static volatile uint32_t array of the correct size and alignment
 * for use with @ref ora_setup_address_monitor_fn_t. The buffer is placed in
 * static storage and must remain valid for the lifetime of the monitor.
 * 
 * Note that this is defined as a uint32_t array regardless of the data_size
 * parameter, as the API expects this type.
 *
 * Example:
 * @code
 * ORA_RING_BUF_DECLARE_32BIT(ring_buf, 6);  // 64 entry uint32_t ring buffer
 * setup_address_monitor(ring_buf, 6, ORA_MONITOR_MODE_CONTROL, NULL);
 * @endcode
 *
 * Under a host-side test build (@c ORA_HOST_TEST) these declare a pointer
 * instead of an array, with external linkage, which the harness points at a
 * suitably aligned region of the SRAM it emulates before the plugin's entry
 * point runs.  The capture DMA writes into emulated SRAM specifically, so a
 * buffer in ordinary host memory would not receive anything; and a plugin's
 * uses — passing it to @ref ora_setup_address_monitor_fn_t, indexing it,
 * differencing it against the write pointer — read identically either way.
 *
 * @param name              Name for the ring buffer array
 * @param ring_entries_log2 Log2 of the number of entries, e.g. 6 for 64 entries
 */
#if defined(ORA_HOST_TEST)
#define ORA_RING_BUF_DECLARE_8BIT(name, ring_entries_log2)  \
    volatile uint32_t *name
#define ORA_RING_BUF_DECLARE_16BIT(name, ring_entries_log2) \
    volatile uint32_t *name
#define ORA_RING_BUF_DECLARE_32BIT(name, ring_entries_log2) \
    volatile uint32_t *name
#else
#define ORA_RING_BUF_DECLARE_8BIT(name, ring_entries_log2)  \
    static volatile uint32_t __attribute__((aligned(        \
        ORA_RING_BUF_SIZE_8BIT(ring_entries_log2)           \
    ))) name[1u << (ring_entries_log2-2)]
#define ORA_RING_BUF_DECLARE_16BIT(name, ring_entries_log2) \
    static volatile uint32_t __attribute__((aligned(        \
        ORA_RING_BUF_SIZE_16BIT(ring_entries_log2)          \
    ))) name[1u << (ring_entries_log2-1)]
#define ORA_RING_BUF_DECLARE_32BIT(name, ring_entries_log2) \
    static volatile uint32_t __attribute__((aligned(        \
        ORA_RING_BUF_SIZE_32BIT(ring_entries_log2)          \
    ))) name[1u << (ring_entries_log2)]
#endif

/**
 * @brief IRQ handler function type
 *
 * This is used by plugins to define a function that will be called when a
 * specific IRQ occurs.
 */
typedef void (*ora_irq_handler_t)(void);

/** @} */ // plugin_api_types

/**
 * @defgroup plugin_api_macros API Macros
 * @brief Macros for use in plugins
 * @{
 */

/**
 * @brief Get the context for the system plugin
 *
 * This macro retrieves the context pointer previously stored by the system
 * plugin via @ref ora_set_plugin_context_fn_t. It calls the firmware's
 * get_plugin_context function at its fixed absolute address, which is
 * guaranteed to remain stable across firmware versions that support this
 * API version.
 *
 * Intended for use inside IRQ handlers where @ref ora_lookup_fn_t is not
 * available.
 *
 * @return void* pointer to the system plugin's context, or NULL if not set
 */
#define ORA_GET_PLUGIN_CONTEXT_SYSTEM   (uintptr_t)(0x20080000 + 40)

/**
 * @brief Get the context for the user plugin
 *
 * Equivalent to @ref ORA_GET_PLUGIN_CONTEXT_SYSTEM but for the user plugin.
 * Retrieves the context pointer previously stored by the user plugin via
 * @ref ora_set_plugin_context_fn_t.
 *
 * Intended for use inside IRQ handlers where @ref ora_lookup_fn_t is not
 * available.
 *
 * @return void* pointer to the user plugin's context, or NULL if not set
 */
#define ORA_GET_PLUGIN_CONTEXT_USER     (uintptr_t)(0x20080000 + 44)

/** @} */ // plugin_api_macros

/**
 * @defgroup plugin_api_functions Protoypes for API Functions
 * @brief API lookup function, plugin entry point, and individual API function prototypes
 * @{
 */

 /**
 * @brief System plugin entry point arguments
 * 
 * Passed to the plugin entry point function as a pointer.
 */
typedef struct {
    /**
     * @brief The core the plugin is running on
     */
    ora_core_t core;

    /**
     * @brief The plugin's static RAM base address
     * 
     * Can be used by the plugin at runtime to validate it has been built with
     * the correct linker settings, and exit early if not.
     */
    uint32_t static_ram_base;

    /**
     * @brief The plugin's static RAM size in bytes
     * 
     * Can be used by the plugin at runtime to validate it has been built with
     * the correct linker settings, and exit early if not.
     */
    uint32_t static_ram_size;

    /**
     * @brief The top address of the stack for the core running this plugin
     * 
     * Some of the stack will have been used before the plugin entry point is
     * called.  The rest can be assumed unused.
     * 
     * Can be used by the plugin to check it isn't exceeding the stack limits.
     * If careful, a plugin can use unused stack space in this core's stack as
     * additional RAM.
     */
    uint32_t stack_top;

    /**
     * @brief The total size of the stack for the core running this plugin
     * @sa stack_top
     */
    uint32_t stack_size;
} ora_entry_args_t;

/**
 * @brief Monitor mode
 *
 * This enumeration defines the modes in which the address monitor can
 * operate, determining how One ROM responds to detected knock sequences.
 */
typedef enum {
    /**
     * @brief Observe mode - passive monitoring only
     *
     * The plugin observes address bus activity without affecting ROM serving.
     * The ROM image is untouched and the host is unaware of the plugin's
     * presence.
     */
    ORA_MONITOR_MODE_OBSERVE  = 0,

    /**
     * @brief Control mode - plugin controls ROM image content
     *
     * The plugin takes control of what One ROM serves, modifying or replacing
     * the ROM image. The host must be running from RAM or another ROM and be
     * tolerant of the ROM image changing underneath it.
     */
    ORA_MONITOR_MODE_CONTROL  = 1,

    /**
     * @brief Override mode - plugin overrides ROM serving entirely
     *
     * The plugin will take over individual read cycles in real time, deciding
     * the value of every byte served to the host. The autonomous PIO serving
     * mechanism will be disabled and replaced by the plugin.
     * 
     * Note yet implemented.
     */
    //ORA_MONITOR_MODE_OVERRIDE = 2,
} ora_monitor_mode_t;

/**
 * @brief Return code for One ROM API functions
 */
typedef enum {
    ORA_RESULT_OK    = 0,
    ORA_RESULT_ERROR = 1,
    ORA_RESULT_INVALID_SIZE = 2,
    ORA_RESULT_INVALID_ARG = 3,
    ORA_RESULT_INTERNAL_ERROR = 4,
    ORA_RESULT_CONTROL_PIN_ACTIVE = 5,
    ORA_RESULT_INSUFFICIENT_FREE_MEM = 6,
    ORA_RESULT_SLOT_ACTIVE = 7,
    ORA_RESULT_INVALID_SLOT = 8,
    ORA_RESULT_NO_SLOT_ACTIVE = 9,
    ORA_RESULT_NOT_SUPPORTED = 10,
    ORA_RESULT_TYPE_MISMATCH = 11,
    /** @since firmware 0.7.1 */
    ORA_RESULT_GPIO_IN_USE = 12,
} ora_result_t;

/**
 * @brief Lookup an API function pointer by its identifier
 *
 * This function takes an API function identifier and returns a pointer
 * to the corresponding API function. If the identifier is invalid,
 * it returns NULL.
 *
 * If the idenfier is valid and implemented in the current version of the
 * firmware, the returned pointer is guaranteed to be non-NULL, even if the
 * underlying capability (logging, status LED, etc) is not present.
 *
 * For discovery, a pointer to this function is provided to the plugin as an
 * argument to the plugin's entry point.
 *
 * @param id The identifier of the API function to look up.
 * @return A pointer to the API function corresponding to the given identifier,
 * or NULL if the identifier is invalid.
 */
typedef void *(*ora_lookup_fn_t)(api_id_t id);

/**
 * @brief Plugin entry point function type
 *
 * This typedef defines the signature that all plugin entry point functions
 * must conform to.  Use @ref ORA_DEFINE_SYSTEM_PLUGIN or
 * @ref ORA_DEFINE_USER_PLUGIN to define your entry point.
 *
 * @param ora_lookup_fn A pointer to the API lookup function
 * @param plugin_type   Whether this is a system or user plugin
 * @param entry_args    A pointer to a structure containing the plugin's entry
 * arguments
 */
typedef void (*ora_plugin_entry_t)(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
);

/**
 * @brief Reboot into BOOTSEL mode
 * @sa ORA_ID_REBOOT_BOOTSEL
 * 
 * Reboots One ROM into the RP2350's BOOTSEL mode, for repogramming or
 * recovery.  This (obviously) stops One ROM serving the ROM image and does
 * not return.  There may be a slight (e.g. 10ms) delay before the reboot.
 */
typedef void (*ora_reboot_bootsel_fn_t)(void);

/**
 * @brief Allocate memory
 * @sa ORA_ID_ALLOC
 *
 * This allocates memory on a 4 byte boundary from the start of One ROM's
 * spare pool of memory.  Allocations are for the lifetime of the firmware
 * and are not freed until the device is reset.  The firmware does not track
 * individual allocations, so it is the caller's responsibility to ensure that
 * they do not exceed the allocated memory.
 *
 * The amount of free memory will vary dramatically depending on the ROM image
 * currently being served.  A One ROM 24 will have around 450KB of free memory,
 * while a One ROM 32 or 40 will have 1-2KB.
 *
 * @param size The number of bytes to allocate.
 * @return A pointer to the allocated memory, or NULL if allocation failed.
 */
typedef void *(*ora_alloc_fn_t)(size_t size);

/**
 * @brief Get One ROM information
 * @sa ORA_ID_GET_FIRMWARE_INFO
 *
 * DEPRECATED AS OF v0.7.0 - attempt to retrieve this function pointer returns
 * NULL.
 *
 * @return A pointer to a structure containing information about the One ROM
 * firmware, the device it is running on, configured ROM sets, and runtime
 * information. The exact structure of this data is defined by the One ROM
 * firmware - see `onerom_info_t` in `firmware/generated/onerom_metadata.h` for details.
 *
 * Plugins must consider this data and any data pointed to it as read-only.
 * Some of the data resides on flash, and other data in SRAM.  However, the
 * firmware itself may rely on the immutability of any data contained within.
 * 
 * Prefer provided calls to access specific firmware information to this
 * function.  This function is provided for accessing details firmware
 * information before specific API calls have been implemented to expose it.
 * You should raise an enhancement issue if you find yourself needing to use
 * this function.
 *  
 * This function may be deprecated in a future version of the API with
 * additional targetted API calls replacing it, and the format of the
 * returned data might change in a non-backwards compatible way.
 */
typedef const void *(*ora_get_firmware_info_fn_t)(void);

/**
 * @brief Log a message
 * @sa ORA_ID_LOG
 * 
 * If the One ROM firmware is compiled with logging support enabled, this
 * function logs via RTT.  If there is no logging support this function
 * silently fails.
 *
 * @param msg printf-style format string
 * @param ... Format arguments
 */
typedef void (*ora_log_fn_t)(const char *msg, ...);

/**
 * @brief Log an error message
 *
 * Equivalent to @ref ora_log but flags the log as an error.
 * @sa ORA_ID_ERR_LOG
 * 
 * If the One ROM firmware is compiled with logging support enabled, this
 * function logs via RTT.  If there is no logging support this function
 * silently fails.
 *
 * @param msg printf-style format string
 * @param ... Format arguments
 */
typedef void (*ora_err_log_fn_t)(const char *msg, ...);

/**
 * @brief Log a debug message
 *
 * Equivalent to @ref ora_log but flags the log as a debug message.
 * @sa ORA_ID_DEBUG_LOG
 *
 * If the One ROM firmware is compiled with debug logging support enabled,
 * this function logs via RTT.  If there is no logging support this function
 * silently fails.
 *
 * @param msg printf-style format string
 * @param ... Format arguments
 */
typedef void (*ora_debug_log_fn_t)(const char *msg, ...);

/**
 * @brief Get the amount of free memory
 * @sa ORA_ID_GET_FREE_MEM
 *
 * @return The number of bytes of free memory available.
 */
typedef size_t (*ora_get_free_mem_fn_t)(void);

/**
 * @brief Set the status LED on or off
 * @sa ORA_ID_SET_STATUS_LED
 *
 * Sets the live status-LED state. This is the single coordination channel for
 * the status LED: the call records the new state (readable by any plugin as
 * ORA_METADATA_KEY_STATUS_LED_STATE via ora_get_metadata_uint_fn_t) and drives
 * the status-LED GPIO. The board's configured default (the `led` option) only
 * seeds the initial state; a plugin may turn the LED on even if it was
 * configured off. If the board has no status-LED GPIO the call does nothing.
 *
 * Coordination without cross-plugin awareness: on a board where the status LED
 * and a neopixel share a GPIO, the neopixel-driving plugin owns the pin and
 * should render the status LED by reading STATUS_LED_STATE each frame - so a
 * write here is reflected by that plugin without either plugin knowing about
 * the other. Neither plugin references the other; they meet at this flag.
 *
 * @param on Set to 1 to turn the LED on, or 0 to turn it off.
 */
typedef void (*ora_set_status_led_fn_t)(uint8_t on);

/**
 * @brief Setup the USB controller including clocking it
 * @sa ORA_ID_SETUP_USB
 * 
 * This is used by plugins that want to use the USB functionality of the
 * RP2350, to set up the USB PLL.
 */
typedef void (*ora_setup_usb_fn_t)(void);

/**
 * @brief Setup the ADC
 * @sa ORA_ID_SETUP_ADC
 *
 * This is used by plugins that want to use the ADC functionality of the
 * RP2350, to set up the ADC.  This function setups up the USB PLL as well, if
 * it has not already been set up.
 */
typedef void (*ora_setup_adc_fn_t)(void);

/**
 * @brief Register an IRQ handler
 * @sa ORA_ID_REGISTER_IRQ
 * 
 * This is used by plugins that want to register an IRQ handler for a specific
 * IRQ.
 *
 * An IRQ handler can also be deregistered by calling this function with a NULL
 * handler pointer.
 */
typedef void (*ora_register_irq_fn_t)(ora_irq_t irq, ora_irq_handler_t handler);

/**
 * @brief Set plugin context
 * @sa ORA_ID_SET_PLUGIN_CONTEXT
 *
 * This is used by plugins to set a pointer to their context structure, which
 * the One ROM firmware will store and make available to the plugin when it
 * calls into it.  This is useful for storing RAM dynamically allocated by the
 * plugin and accessing it from elsewhere, like an IRQ handler.
 *
 * @param plugin The type of plugin (system or user) for which to set the context
 * @param context A pointer to the plugin's context structure
 */
typedef void (*ora_set_plugin_context_fn_t)(ora_plugin_type_t plugin, void *context);

/**
 * @brief Get plugin context
 * @sa ORA_ID_GET_PLUGIN_CONTEXT
 *
 * This is used by plugins to get a pointer to their context structure, which
 * the One ROM firmware stores and makes available to the plugin when it calls
 * and accessing it from elsewhere, like an IRQ handler.
 *
 * @param plugin The type of plugin (system or user) for which to get the context
 * @return A pointer to the plugin's context structure, or NULL if no context
 * into it.  This is useful for storing RAM dynamically allocated by the plugin
 * has been set.
 */
typedef void *(*ora_get_plugin_context_fn_t)(ora_plugin_type_t plugin);

/**
 * @brief Get the SYSCLK frequency in MHz
 * @sa ORA_ID_GET_SYSCLK_MHZ
 * 
 * Returns the current SYSCLK frequency in MHz, as configured by the main
 * firmware.
 */
typedef uint32_t (*ora_get_sysclk_mhz_fn_t)(void);

/**
 * @brief Enable an IRQ
 * @sa ORA_ID_ENABLE_IRQ
 * 
 * This is used by plugins to enable an IRQ for which they have registered a
 * handler.  If the plugin has not registered a handler for the specified IRQ,
 * this function silently fails.
 *
 * @param irq The IRQ to enable
 * @param enable Set to 1 to enable the IRQ, or 0 to disable it.
 */
typedef void (*ora_enable_irq_fn_t)(ora_irq_t irq, uint8_t enable);

/**
 * @brief Get the CLKREF frequency in MHz
 * @sa ORA_ID_GET_CLKREF_MHZ
 * 
 * Returns the current CLKREF frequency in MHz, as defined by the hardware.
 * Currently returns a fixed 12 MHz.
 */
typedef uint32_t (*ora_get_clkref_mhz_fn_t)(void);

/**
 * @brief Get a pointer to the runtime info structure
 * @sa ORA_ID_GET_RUNTIME_INFO
 * 
 * DEPRECATED AS OF v0.7.0 - attempt to retrieve this function pointer returns
 * NULL.
 *
 * Returns a pointer to the runtime info structure, which contains information
 * about the current state of the firmware and device that may be useful for
 * plugins. The exact structure of this data is defined by the One ROM firmware
 * - see `onerom_runtime_info_t` in `firmware/generated/onerom_metadata.h` for details.
 * 
 * Plugins must consider this data and any data pointed to it as read-only.
 * Some of the data resides on flash, and other data in SRAM.  However, the
 * firmware itself may rely on the immutability of any data contained within.
 *
 * Prefer provided calls to access specific firmware information to this
 * function.  This function is provided for accessing details firmware
 * information before specific API calls have been implemented to expose it.
 * You should raise an enhancement issue if you find yourself needing to use
 * this function.
 *  
 * This function may be deprecated in a future version of the API with
 * additional targetted API calls replacing it, and the format of the
 * returned data might change in a non-backwards compatible way.
 */
typedef const void *(*ora_get_runtime_info_fn_t)(void);

/**
 * @brief Get the size of a chip type
 * @sa ORA_ID_GET_CHIP_SIZE_FROM_TYPE
 *
 * This is used by plugins to get the size of a chip type in bytes, for use in
 * memory management and bounds checking.
 *
 * @param chip_type The chip type to get the size of
 * @return The size of the specified chip type in bytes, or 0 if the chip type
 * is invalid
 */
typedef uint32_t (*ora_get_chip_size_from_type_fn_t)(uint32_t chip_type);

/**
 * @brief Check if a pin is configured as an output
 * @sa ORA_ID_IS_PIN_OUTPUT
 *
 * This is used by plugins to check if a specific pin is currently configured as an output.
 *
 * @param pin The pin to check
 * @return 1 if the pin is configured as an output, 0 otherwise or 0xFF for an invalid pin
 */
typedef uint8_t (*ora_is_pin_output_fn_t)(uint8_t pin);

/**
 * @brief Retrieve the first num_pins data pin numbers
 * @sa ORA_ID_GET_DATA_PIN_NUMS
 * 
 * This is used by plugins to retrieve the pin numbers for the data pins,
 * which is useful for plugins that want to monitor or interact with the data
 * lines.
 * 
 * The data pin numbers are returned in the data_pins_out array, which must be
 * allocated by the caller and have space for at least num_pins elements.  The
 * function returns the number of data pins actually returned, which may be less
 * than num_pins if there are not that many data pins available.
 * 
 * @param data_pins_out Output array to be filled with the data pin numbers
 * @param num_pins The maximum number of data pin numbers to return
 * @return The number of data pin numbers returned in data_pins_out
 */
typedef uint8_t (*ora_get_data_pin_nums_fn_t)(uint8_t *data_pins_out, uint8_t num_pins);

/**
 * @brief Set up the address monitor PIO and DMA
 * @sa ORA_ID_SETUP_ADDRESS_MONITOR
 *
 * Sets up the PIO state machines and DMA channel required to capture address
 * bus activity into the plugin-allocated ring buffer. The firmware uses its
 * internal knowledge of pin assignments to configure the PIO correctly for
 * the current hardware variant.
 *
 * The ring buffer must be allocated by the caller, aligned to its own size
 * in bytes (i.e. aligned to 2^ring_size_log2 * sizeof(uint32_t) bytes), and
 * remain valid for the lifetime of the monitor. Each entry is a uint32_t
 * containing a raw capture of the GPIO pins at the point a CS-active address
 * was detected. Upper bytes will be zero for hardware variants with fewer
 * than 32 address pins.
 *
 * The DMA write pointer can be read directly from the DMA channel registers
 * to determine how many entries have been captured since the last read.
 *
 * @param ring_buf        Pointer to the plugin-allocated ring buffer
 * @param ring_entries_log2  Log2 of the number of entries in the ring buffer,
 *                        e.g. 6 for 64 entries
 * @param mode            The monitor mode to operate in
 * @param data_size       Number of bits to capture for each address.  Must be
 *                        8, 16 or 32.
 * @param reserved        Reserved for future use, must be NULL
 */
typedef ora_result_t (*ora_setup_address_monitor_fn_t)(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
);

/**
 * @brief Calculate knock sequence match values
 * @sa ORA_ID_CALC_KNOCK_MATCHES
 *
 * Precomputes the mask and per-entry match values required to detect a knock
 * sequence from raw GPIO captures in the ring buffer. This must be called
 * once at init time before the main monitoring loop.
 *
 * The mask covers the physical GPIO bits corresponding to the knock_bits
 * lowest order address lines, as mapped by the firmware's internal pin
 * configuration. The match values are the physical GPIO representations of
 * each logical address in the knock sequence.
 *
 * Both mask_out and matches_out contain raw GPIO bit patterns suitable for
 * direct comparison against values captured in the ring buffer.
 *
 * @param knock_seq     Pointer to the knock sequence as logical addresses
 * @param knock_len     Number of entries in the knock sequence
 * @param knock_bits    Number of low-order address bits to include in the
 *                      mask. Must not exceed the number of address pins
 *                      configured for the current hardware variant.
 * @param mask_out      Output mask to apply to each captured GPIO value
 * @param matches_out   Output array of match values, one per knock sequence
 *                      entry. Must be allocated by the caller with space for
 *                      at least knock_len entries.
 * @return ORA_RESULT_OK on success, or an error code on failure
 */
typedef ora_result_t (*ora_calc_knock_matches_fn_t)(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint32_t *mask_out,
    uint32_t *matches_out
);

/**
 * @brief Map a logical address to its physical GPIO/PIO representation
 * @sa ORA_ID_MAP_ADDR_TO_PHYS
 *
 * Converts a logical ROM address to the physical bit pattern as it appears
 * in the GPIO capture, based on the firmware's internal pin mapping for the
 * current hardware variant. This is the offset into the SRAM table where the
 * byte at this logical address is stored and served by One ROM.
 *
 * @param logical_addr  The logical ROM address to remap
 * @return The physical GPIO/PIO bit pattern corresponding to the logical address
 */
typedef uint32_t (*ora_map_addr_to_phys_fn_t)(uint32_t logical_addr);

/**
 * @brief Map a logical data byte to its physical GPIO/PIO representation
 * @sa ORA_ID_REMAP_DATA_TO_PHYS
 *
 * Converts a logical data byte to the physical bit pattern as it must be
 * stored in SRAM, based on the firmware's internal pin mapping for the
 * current hardware variant.
 *
 * @param logical_data  The logical data byte to remap
 * @return The physical GPIO bit pattern corresponding to the logical data byte
 */
typedef uint8_t (*ora_map_data_to_phys_fn_t)(uint8_t logical_data);

/**
 * @brief Demangle a captured physical address back to a logical byte address
 * @sa ORA_ID_DEMANGLE_ADDR
 *
 * Converts a raw GPIO capture from the ring buffer to the logical ROM address
 * it represents — the byte-addressable index into the ROM image, and the exact
 * inverse of @ref ora_map_addr_to_phys_fn_t. This is the space for reading and
 * writing ROM image bytes.
 *
 * On the 24-, 28- and 32-pin variants this also equals the address observed on
 * the bus. On the 40-pin variant the ROM's least-significant address line is
 * served through a separately-read pin the address monitor does not sample, so
 * bit 0 of this logical address is not a bus line — it is the byte-within-word
 * select (A-1) on a word-organised ×16 ROM (e.g. 27C400/27C200), or A0 on an
 * 8-bit 40-pin ROM. To decode signalling captured from the address bus, use
 * @ref ora_demangle_observed_addr_fn_t instead.
 *
 * If check_control_pins is non-zero, the function will validate that the
 * control pins (X1, X2, CS1) are inactive in the capture. If any are active,
 * the function returns ORA_RESULT_CONTROL_PIN_ACTIVE and logical_addr_out is
 * left unchanged.
 *
 * @param physical_addr         Raw GPIO capture from the ring buffer
 * @param logical_addr_out      Output logical (byte) ROM address
 * @param check_control_pins    If non-zero, fail if any control pins are active
 * @return ORA_RESULT_OK on success, ORA_RESULT_CONTROL_PIN_ACTIVE if
 *         check_control_pins is set and a control pin is found to be active,
 *         or ORA_RESULT_ERROR on failure
 */
typedef ora_result_t (*ora_demangle_addr_fn_t)(
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
);

/**
 * @brief Demangle a captured physical address to the observed bus address
 * @sa ORA_ID_DEMANGLE_OBSERVED_ADDR
 *
 * Converts a raw GPIO capture from the ring buffer to the address present on
 * the address lines the device physically observes — least-significant observed
 * line as bit 0. This is the address space host-to-device signalling travels
 * in, so it is the function to use when decoding commands captured by the
 * address monitor. To read or write ROM image bytes instead, use
 * @ref ora_demangle_addr_fn_t / @ref ora_map_addr_to_phys_fn_t.
 *
 * Whether this differs from @ref ora_demangle_addr_fn_t is fixed by the One ROM
 * variant, not the individual ROM:
 *
 *   - 24-, 28- and 32-pin variants observe every address line. The observed
 *     address equals the logical byte address, and this returns exactly what
 *     @ref ora_demangle_addr_fn_t returns.
 *   - The 40-pin variant serves the ROM's least-significant address line through
 *     a separately-read pin the address monitor does not sample. The observed
 *     address omits that line, so this returns one bit fewer than
 *     @ref ora_demangle_addr_fn_t, for every ROM on the variant. The omitted
 *     line is the byte-within-word select (A-1) on a word-organised ×16 ROM such
 *     as the 27C400 or 27C200, and A0 on an 8-bit 40-pin ROM; the arithmetic is
 *     the same either way.
 *
 * The exact number of low bits dropped for the current ROM is reported by
 * @ref ora_get_unobserved_addr_bits_fn_t (0 on 24/28/32-pin, 1 on 40-pin).
 *
 * If check_control_pins is non-zero, validates that the control pins (X1, X2,
 * CS1) are inactive in the capture, returning ORA_RESULT_CONTROL_PIN_ACTIVE and
 * leaving observed_addr_out unchanged if any are active.
 *
 * @param physical_addr        Raw GPIO capture from the ring buffer
 * @param observed_addr_out    Output observed bus address
 * @param check_control_pins   If non-zero, fail if any control pins are active
 * @return ORA_RESULT_OK on success, ORA_RESULT_CONTROL_PIN_ACTIVE if
 *         check_control_pins is set and a control pin is active, or
 *         ORA_RESULT_ERROR on failure
 */
typedef ora_result_t (*ora_demangle_observed_addr_fn_t)(
    uint32_t physical_addr,
    uint32_t *observed_addr_out,
    uint8_t check_control_pins
);

/**
 * @brief Get the number of least-significant address bits the device does not
 *        observe for the current ROM
 * @sa ORA_ID_GET_UNOBSERVED_ADDR_BITS
 *
 * Reports how the device treats the current ROM's addresses: the number of
 * low-order logical address bits not present on the lines the address monitor
 * observes, written to *bits_out. This is the bit difference between
 * @ref ora_demangle_addr_fn_t (logical byte address) and
 * @ref ora_demangle_observed_addr_fn_t (observed bus address) for this ROM. The
 * host's signalling stride is 1 << that value.
 *
 * The value is fixed by the One ROM variant:
 *
 *   - 0 on the 24-, 28- and 32-pin variants: every address line is observed;
 *     stride 1; the least-significant address line carries command data.
 *   - 1 on the 40-pin variant: the least-significant line is served through a
 *     separately-read pin the monitor does not sample; stride 2; that line
 *     carries no command data. This holds for every ROM on the variant — the
 *     ×16 parts (27C400/27C200) and 8-bit 40-pin parts alike.
 *
 * A plugin can combine this with the ROM byte size
 * (@ref ora_get_chip_size_from_type_fn_t) to bound host-supplied address pages:
 * the observed address span is (byte size >> the returned value).
 *
 * This is device-side information for the plugin's own use. The addressing
 * behaviour cannot be discovered over the wire at session time — a host must
 * already know it to frame the initiating knock — so it is never reported to
 * the host; a host obtains it from the plugin's documentation, agreed in
 * advance.
 *
 * @param bits_out  Output: number of unobserved least-significant address bits
 *                  for the current ROM. Unchanged on failure.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_ARG if bits_out is NULL,
 *         or an error result if no ROM is currently being served
 */
typedef ora_result_t (*ora_get_unobserved_addr_bits_fn_t)(
    uint8_t *bits_out
);

/**
 * @brief Initialise a knock sequence
 * @sa ORA_ID_INIT_KNOCK
 *
 * Precomputes the mask and per-entry match values required to detect a knock
 * sequence from raw GPIO captures in the ring buffer, storing the results in
 * the caller-allocated @ref ora_knock_t structure. This must be called once
 * before passing the structure to @ref ora_wait_for_knock_fn_t.
 *
 * Use @ref ORA_KNOCK_DECLARE to declare a correctly sized @ref ora_knock_t on
 * the stack, or @ref ORA_KNOCK_SIZE with @ref ora_alloc_fn_t to allocate one
 * dynamically.
 *
 * @param knock_seq   Pointer to the knock sequence as logical addresses
 * @param knock_len   Number of entries in the knock sequence
 * @param knock_bits  Number of low-order address bits to include in the mask.
 *                    Must not exceed the number of address pins configured for
 *                    the current hardware variant
 * @param data_size   Number of bits the address monitor is configured to
 *                    capture for each address.  Must be 8, 16 or 32.
 * @param knock       Pointer to a caller-allocated @ref ora_knock_t structure
 *                    to be filled in by this function. Must have been declared
 *                    with @ref ORA_KNOCK_DECLARE or allocated with
 *                    @ref ORA_KNOCK_SIZE
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_ARG if any pointer is
 *         NULL or knock_len is zero, or ORA_RESULT_INTERNAL_ERROR if the pin
 *         configuration is invalid
 */
typedef ora_result_t (*ora_init_knock_fn_t)(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint8_t data_size,
    ora_knock_t *knock
);

/**
 * @brief Flag to enable CS debounce filtering in @ref ora_wait_for_knock_fn_t
 *
 * When set, captured entries where CS is inactive are discarded, preventing
 * spurious matches against addresses captured during very short CS pulses.
 */
#define ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS 0x00000001

/**
 * @brief Wait for a knock sequence to be detected
 * @sa ORA_ID_WAIT_FOR_KNOCK
 *
 * Blocks until the knock sequence described by @p knock is detected in the
 * ring buffer, then collects @p payload_len further address captures and
 * returns them in @p payload_out. The read pointer is initialised from the
 * current DMA write pointer on entry, discarding any captures that occurred
 * before the call.
 *
 * Behaviour can be modified via @p flags. Pass 0 for default behaviour.
 * @sa ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS
 *
 * Payload captures are returned as raw physical addresses. Use
 * @ref ora_demangle_addr_fn_t to convert to logical addresses if required.
 *
 * @param knock              Pointer to an initialised @ref ora_knock_t
 *                           structure
 * @param ring_buf           Pointer to the ring buffer, as passed to
 *                           @ref ora_setup_address_monitor_fn_t
 * @param ring_entries_log2  Log2 of the number of entries in the ring buffer,
 *                           as passed to @ref ora_setup_address_monitor_fn_t
 * @param flags              Behavioural flags. Pass 0 for default behaviour.
 *                           @sa ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS
 * @param payload_out        Output array to receive address captures collected
 *                           after the knock sequence is detected. May be NULL
 *                           if @p payload_len is 0
 * @param payload_len        Number of address captures to collect after the
 *                           knock sequence is detected. Pass 0 to return
 *                           immediately after detection
 * @param start_pos          Optional pointer to receive the initial read
 *                           position in the ring buffer at the time the function
 *                           starts waiting.  Avoids missing bytes if provided.
 * @param next_read_out      Optional output pointer to receive the next read
 *                           position in the ring buffer after the knock sequence
 *                           and payload captures have been consumed. This can be
 *                           used to update the read pointer to capture subsequent
 *                           data without losing some.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_ARG if @p knock or
 *         @p ring_buf is NULL, or @p payload_out is NULL when @p payload_len
 *         is non-zero
 */
typedef ora_result_t (*ora_wait_for_knock_fn_t)(
    const ora_knock_t *knock,
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    uint32_t flags,
    uint32_t *payload_out,
    uint8_t payload_len,
    volatile uint32_t *start_pos,
    volatile uint32_t **next_read_out
);

/**
 * @brief Reprogram a region of the selected RAM ROM slot using logical
 * addresses and bytes
 * @sa ORA_ID_REPROGRAM_RAM_ROM_SLOT
 *
 * Updates a contiguous logical region of the ROM image currently being
 * served from SRAM, remapping addresses and data bytes according to the
 * firmware's internal pin configuration.
 * 
 * This function guarantees to write bytes in an ascending logical address
 * order.
 * 
 * This is not atomic.  For an atomic update, you must switch in a new region
 * of SRAM using the appropriate function (not yet supported). 
 *
 * @param slot    RAM ROM slot to update
 * @param offset  Logical start address within the ROM image to update
 * @param buf     Pointer to the logical data bytes to write
 * @param len     Number of bytes to write
 * @param allow_active If non-zero, allow writing to the region currently
 * being served by One ROM. If zero, fail with ORA_RESULT_SLOT_ACTIVE if the
 * active rom slot is selected.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_ARG if @p buf is NULL
 *         or @p len is zero
 */
typedef ora_result_t (*ora_reprogram_ram_rom_slot_fn_t)(
    uint8_t slot,
    uint32_t offset,
    const uint8_t *buf,
    uint32_t len,
    uint8_t allow_active
);

/**
 * @brief Start the address monitor PIO state machines
 * @sa ORA_ID_START_ADDRESS_MONITOR
 *
 * Enables the PIO state machines configured by @ref ora_setup_address_monitor_fn_t,
 * beginning capture of address bus activity into the ring buffer. This must be
 * called after @ref ora_setup_address_monitor_fn_t and after any other
 * initialisation the plugin needs to perform before monitoring begins, to
 * avoid missing captures during setup.
 */
typedef void (*ora_start_address_monitor_fn_t)(void);

/**
 * @brief Get a pointer to the address monitor ring buffer write position
 * @sa ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS
 *
 * Returns a pointer to the location that holds the current write position
 * within the ring buffer passed to @ref ora_setup_address_monitor_fn_t. The
 * value at this location advances as new address captures are written by the
 * DMA engine.
 *
 * This function is intended to be called once during initialisation. The
 * returned pointer may then be dereferenced directly in the monitoring loop
 * to read the current write position without any function call overhead.
 *
 * The returned pointer remains valid for the lifetime of the address monitor.
 * The plugin must not write to this location.
 *
 * @return Pointer to the current ring buffer write position, or NULL if the
 *         address monitor has not been set up via
 *         @ref ora_setup_address_monitor_fn_t
 */
typedef volatile uint32_t * volatile *(*ora_get_address_monitor_ring_write_pos_fn_t)(void);

/**
 * @brief Get the number of RAM slots available for the current ROM type
 * @sa ORA_ID_GET_RAM_SLOT_COUNT
 *
 * Returns the total number of RAM slots available for the ROM type currently
 * being served. Slot 0 is always the primary slot, pre-populated by the
 * firmware on boot.
 *
 * A slot is exactly one ROM region, so the count is however many of those fit
 * in the RAM reserved for them — with a small ROM that can be a great many,
 * and with a 512KB one it is a single slot. Plugins must not assume any
 * particular number, and in particular must not assume a slot is large enough
 * for some purpose of their own: a slot is as small as the ROM being served,
 * which can be 2KB or less.
 *
 * @return Total number of RAM slots available, at most @ref ORA_MAX_RAM_SLOTS
 */
typedef uint8_t (*ora_get_ram_slot_count_fn_t)(void);

/**
 * @brief Get information about a RAM slot
 * @sa ORA_ID_GET_RAM_SLOT_INFO
 *
 * Returns the absolute SRAM address, size, and ROM type of the specified RAM
 * slot. All output parameters are optional (nullable); pass NULL for any
 * value not required.
 *
 * The ROM type reflects the last ROM image loaded into the slot via
 * @ref ora_copy_flash_slot_to_ram_slot_fn_t.  For slot 0, which is
 * pre-populated by the firmware at boot, the type is derived from the first
 * non-plugin flash slot.  Slots that have never been explicitly loaded report
 * 0xFF (invalid).
 *
 * @param ram_slot      Index of the RAM slot to query
 * @param addr_out      Output pointer to receive the SRAM address of the slot.
 *                      May be NULL if not required.
 * @param size_out      Output pointer to receive the size of the slot in bytes.
 *                      May be NULL if not required.
 * @param rom_type_out  Output pointer to receive the ROM type loaded into the
 *                      slot, or 0xFF if not known.  May be NULL if not required.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_SLOT if ram_slot is
 *         out of range
 */
typedef ora_result_t (*ora_get_ram_slot_info_fn_t)(
    uint8_t   ram_slot,
    uint32_t *addr_out,
    uint32_t *size_out,
    uint32_t *rom_type_out
);

/**
 * @brief Get the index of the currently active RAM slot
 * @sa ORA_ID_GET_ACTIVE_RAM_SLOT
 *
 * Returns the index of the RAM slot currently being served to the host. In
 * normal operation this is slot 0, pre-populated by the firmware on boot.
 *
 * If no slot is currently active (for example if a plugin has suppressed
 * firmware ROM loading), returns ORA_RESULT_NO_SLOT_ACTIVE and
 * @p ram_slot_out is left unchanged.
 *
 * @param ram_slot_out  Output pointer to receive the active RAM slot index
 * @return ORA_RESULT_OK on success, ORA_RESULT_NO_SLOT_ACTIVE if no slot is
 *         currently active, ORA_RESULT_INVALID_ARG if ram_slot_out is NULL
 */
typedef ora_result_t (*ora_get_active_ram_slot_fn_t)(uint8_t *ram_slot_out);

/**
 * @brief Atomically switch the active RAM slot
 * @sa ORA_ID_SET_ACTIVE_RAM_SLOT
 *
 * Atomically switches the ROM image being served to the host to the specified
 * RAM slot. If no slot is currently active, this activates the specified slot
 * without requiring an atomic transition.
 *
 * The target slot must have been populated with a valid ROM image before
 * calling this function, either via @ref ora_reprogram_ram_slot_fn_t or
 * @ref ora_copy_flash_slot_to_ram_slot_fn_t or by some other means, as
 * otherwise it contains uninitialised data.
 *
 * @param ram_slot  Index of the RAM slot to make active
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_SLOT if ram_slot is
 *         out of range
 */
typedef ora_result_t (*ora_set_active_ram_slot_fn_t)(uint8_t ram_slot);

/** @brief Exclude plugin slots from flash slot enumeration */
#define ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS      0x00000001

/** @brief Exclude non-plugin slots from flash slot enumeration */
#define ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS  0x00000002

/**
 * @brief Get the number of flash slots available
 * @sa ORA_ID_GET_FLASH_SLOT_COUNT
 *
 * Returns the number of ROM images stored in flash, optionally filtered to
 * exclude plugin or non-plugin slots.
 *
 * Indices returned by this function are stable across calls with the same
 * flags, and are suitable for use as handles to pass to
 * @ref ora_get_flash_slot_info_fn_t and
 * @ref ora_copy_flash_slot_to_ram_slot_fn_t.
 *
 * @param flags  Filtering flags. Pass 0 for all slots.
 *               @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS
 *               @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS
 * @return Number of flash slots matching the specified filter
 */
typedef uint8_t (*ora_get_flash_slot_count_fn_t)(uint32_t flags);

/**
 * @brief Get information about a flash slot
 * @sa ORA_ID_GET_FLASH_SLOT_INFO
 *
 * Returns information about the specified flash slot, optionally filtered
 * to exclude plugin or non-plugin slots. The index must be within the range
 * returned by @ref ora_get_flash_slot_count_fn_t with the same flags.
 *
 * @p name_out receives a pointer directly into flash memory — no allocation
 * is required. If the slot has no associated name, @p name_out is set to
 * NULL.
 *
 * @param flash_slot    Index of the flash slot to query
 * @param flags         Filtering flags, must match those passed to
 *                      @ref ora_get_flash_slot_count_fn_t
 *                      @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS
 *                      @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS
 * @param name_out      Output pointer to receive a pointer to the slot's name
 *                      string, or NULL if no name is available. May be NULL
 *                      if the name is not required.
 * @param rom_type_out  Output pointer to receive the ROM type of the slot.
 *                      May be NULL if not required.
 * @param rom_count_out Output pointer to receive the number of ROM images in
 *                      this slot. May be NULL if not required.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_SLOT if flash_slot is
 *         out of range for the given flags
 */
typedef ora_result_t (*ora_get_flash_slot_info_fn_t)(
    uint8_t flash_slot,
    uint32_t flags,
    const char **name_out,
    uint32_t *rom_type_out,
    uint8_t *rom_count_out
);

/**
 * @brief Get extended, per-ROM information about a flash slot
 * @sa ORA_ID_GET_FLASH_SLOT_EXT_INFO
 * @since firmware 0.7.1
 *
 * Returns details for a single ROM image within a flash slot, addressed by
 * @p rom_index. A multi-ROM set exposes each constituent ROM; a single-ROM
 * slot has exactly one ROM at index 0. The number of ROMs in a slot is the
 * @p rom_count_out returned by @ref ora_get_flash_slot_info_fn_t.
 *
 * Unlike @ref ora_get_flash_slot_info_fn_t, which reports the ROM type only as
 * its numeric RBCP value, this reports @p rom_type_out — the ROM type string
 * exactly as the user specified it when programming (e.g. "27LC512" rather than
 * the canonical "27512"). All string outputs receive pointers directly into
 * flash memory — no allocation is required. All output pointers are optional;
 * pass NULL for any value not required.
 *
 * @param flash_slot        Index of the flash slot to query, filtered as per
 *                          @ref ora_get_flash_slot_count_fn_t
 * @param rom_index         Index of the ROM within the slot (0-based, less than
 *                          the slot's rom_count)
 * @param flags             Filtering flags, must match those passed to
 *                          @ref ora_get_flash_slot_count_fn_t
 * @param rom_type_out      Receives the ROM type string as specified by the
 *                          user. Never NULL on success. May be NULL if not
 *                          required.
 * @param filename_out      Receives the ROM's filename string, or NULL if none.
 *                          May be NULL if not required.
 * @param chip_size_out     Receives the ROM's size in bytes. May be NULL if not
 *                          required.
 * @param rbcp_rom_type_out Receives the ROM type as its RBCP wire value. May be
 *                          NULL if not required.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_SLOT if flash_slot is
 *         out of range for the given flags, ORA_RESULT_INVALID_ARG if rom_index
 *         is out of range for the slot
 */
typedef ora_result_t (*ora_get_flash_slot_ext_info_fn_t)(
    uint8_t flash_slot,
    uint8_t rom_index,
    uint32_t flags,
    const char **rom_type_out,
    const char **filename_out,
    uint32_t *chip_size_out,
    uint32_t *rbcp_rom_type_out
);

/** @brief Perform the copy asynchronously via DMA */
#define ORA_COPY_FLAG_ASYNC  0x00000001

/**
 * @brief Copy a flash slot into a RAM slot
 * @sa ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT
 *
 * Copies the ROM image from the specified flash slot into the specified RAM
 * slot. The flash data is already in physical layout so is copied directly.
 * The RAM slot may then be activated via @ref ora_set_active_ram_slot_fn_t.
 *
 * This function does not activate the RAM slot. To serve the copied image,
 * call @ref ora_set_active_ram_slot_fn_t after this function returns.
 *
 * If @ref ORA_COPY_FLAG_ASYNC is set, the copy is performed via DMA and the
 * function returns immediately. The caller must not activate the RAM slot or
 * assume the copy is complete until @ref ora_copy_complete_fn_t returns
 * ORA_RESULT_OK.
 *
 * @param flash_slot  Index of the flash slot to copy from, relative to the
 *                    filtered set specified by @p flags
 * @param flags       Filtering flags, must match those passed to
 *                    @ref ora_get_flash_slot_count_fn_t and
 *                    @ref ora_get_flash_slot_info_fn_t
 *                    @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS
 *                    @sa ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS
 * @param ram_slot    Index of the RAM slot to copy into
 * @param copy_flags  Copy behaviour flags. Pass 0 for synchronous copy.
 *                    @sa ORA_COPY_FLAG_ASYNC.  Currently unsupported.
 * @return ORA_RESULT_OK on success, ORA_RESULT_INVALID_SLOT if either index
 *         is out of range for the given flags, ORA_RESULT_INVALID_SIZE if
 *         the flash slot's ROM image size does not match the currently active
 *         ROM type
 */
typedef ora_result_t (*ora_copy_flash_slot_to_ram_slot_fn_t)(
    uint8_t flash_slot,
    uint32_t flags,
    uint8_t ram_slot,
    uint32_t copy_flags
);

/**
 * @brief Populates a string with the device's version information
 * @sa ORA_ID_GET_DEVICE_VERSION
 *
 * @param version_out  Output pointer to a buffer to receive the device's
 *                     version string. May be NULL if not required.
 * @param max_len      The maximum length of the version string, including the
 *                     null terminator.
 * @return ORA_RESULT_OK on success, ORA_RESULT_ERROR on failure
 */
typedef ora_result_t (*ora_get_device_version_fn_t)(uint8_t *version_out, uint32_t max_len);

/**
 * @brief Get a device-level metadata string by key
 * @sa ORA_ID_GET_METADATA_STR
 *
 * Retrieves a device-level metadata string identified by @p key. The metadata
 * key space is unified across accessors; this accessor resolves only keys whose
 * datum is a string.
 *
 * On success @p out receives a pointer directly into flash - no allocation is
 * required, and the pointer is valid for the lifetime of the firmware. If the
 * requested field is present but unset (for example an absent serial number
 * override), the call succeeds with @p out set to NULL: an unset field is data,
 * not an error.
 *
 * The firmware returns stored metadata verbatim and applies no interpretation.
 * Deriving effective values (e.g. a serial from the MCU chip ID) or formatting
 * for presentation is the caller's responsibility.
 *
 * @param key  The metadata datum to retrieve. @sa ora_metadata_key_t
 * @param out  Output pointer to receive the string pointer, or NULL if the
 *             field is unset. Must not itself be NULL.
 * @return ORA_RESULT_OK on success (including the unset case, @p out = NULL);
 *         ORA_RESULT_NOT_SUPPORTED if @p key is unknown to this firmware;
 *         ORA_RESULT_TYPE_MISMATCH if @p key is valid but not a string;
 *         ORA_RESULT_INVALID_ARG if @p out is NULL.
 */
typedef ora_result_t (*ora_get_metadata_str_fn_t)(ora_metadata_key_t key, const char **out);

/**
 * @brief Get a device-level unsigned metadata value by key
 * @sa ORA_ID_GET_METADATA_UINT
 *
 * Numeric sibling of ora_get_metadata_str_fn_t over the same unified key space.
 * Resolves keys whose datum is an unsigned scalar or enum, zero-extending the
 * stored value into @p out. Hardware-topology keys (e.g. ORA_METADATA_KEY_GPIO_
 * STATUS, ORA_METADATA_KEY_GPIO_NEOPIXEL) resolve from device metadata; live
 * keys (e.g. ORA_METADATA_KEY_STATUS_LED_STATE) resolve from runtime state and
 * therefore reflect the current value on each call.
 *
 * @param key  The metadata datum to retrieve. @sa ora_metadata_key_t
 * @param out  Output pointer to receive the value. Must not be NULL.
 * @return ORA_RESULT_OK on success;
 *         ORA_RESULT_NOT_SUPPORTED if @p key is unknown to this firmware;
 *         ORA_RESULT_TYPE_MISMATCH if @p key is valid but not an unsigned value;
 *         ORA_RESULT_INVALID_ARG if @p out is NULL.
 */
typedef ora_result_t (*ora_get_metadata_uint_fn_t)(ora_metadata_key_t key, uint32_t *out);

/**
 * @brief Demangle a captured physical data byte back to a logical byte
 * @sa ORA_ID_DEMANGLE_DATA
 *
 * Converts a raw physical data byte as stored in SRAM back to the logical
 * byte value it represents, based on the firmware's internal pin mapping for
 * the current hardware variant. This is the inverse of
 * @ref ora_map_data_to_phys_fn_t.
 *
 * @param physical_data     Raw physical data byte from SRAM
 * @param logical_data_out  Output logical data byte
 * @return ORA_RESULT_OK on success, ORA_RESULT_ERROR on failure
 */
typedef ora_result_t (*ora_demangle_data_fn_t)(
    uint8_t  physical_data,
    uint8_t *logical_data_out
);

/**
 * @brief Enter exclusive mode
 * @sa ORA_ID_ENTER_EXCLUSIVE_MODE
 *
 * Requests exclusive use of the MCU by pausing the other core. Sends a pause
 * request via the inter-core FIFO and blocks until the other core acknowledges.
 * The other core must be calling @ref ora_yield_fn_t in its main loop for this
 * to complete.
 *
 * Once this function returns, the other core is suspended with interrupts
 * disabled and it is safe to perform operations that require exclusive MCU
 * access, such as erasing or programming flash.
 *
 * The caller must call @ref ora_exit_exclusive_mode_fn_t when the exclusive
 * operation is complete.
 *
 * @return ORA_RESULT_OK once the other core is suspended and exclusive mode
 *         is active.  May fail if the other core does not support yielding.
 */
typedef ora_result_t (*ora_enter_exclusive_mode_fn_t)(void);

/**
 * @brief Exit exclusive mode
 * @sa ORA_ID_EXIT_EXCLUSIVE_MODE
 *
 * Releases exclusive mode by signalling the other core to resume. Must be
 * called after @ref ora_enter_exclusive_mode_fn_t, and only after all
 * operations requiring exclusive MCU access have completed.
 *
 * @return ORA_RESULT_OK on success.  May fail if the other core does not 
 *         support yielding, or if another error condition occurs.
 */
typedef ora_result_t (*ora_exit_exclusive_mode_fn_t)(void);

/**
 * @brief Yield to allow another core to enter exclusive mode
 * @sa ORA_ID_YIELD
 *
 * Checks whether another core has requested exclusive mode via
 * @ref ora_enter_exclusive_mode_fn_t or the core firmware has work to do.
 * If so, acknowledges the request and suspends the calling core with
 * interrupts disabled until exclusive mode is released by the other core
 * calling @ref ora_exit_exclusive_mode_fn_t, or the firmware completes its
 * work.
 *
 * This function must be called regularly from the plugin's main loop. If no
 * exclusive mode request is pending it returns immediately.
 *
 * @param was_paused_out  Optional output pointer set to 1 if the calling
 *                        core was paused during this call, or 0 if not.
 *                        May be NULL if not required.
 * @return ORA_RESULT_OK on success, whether or not a pause occurred
 */
typedef ora_result_t (*ora_yield_fn_t)(uint8_t *was_paused_out);


/**
 * @brief Read a logical region of a RAM ROM slot into a buffer.
 *
 * Reads @p len logical bytes starting at @p offset from the RAM slot
 * identified by @p slot, reversing the address and data pin mappings
 * applied by the pre-processor when the ROM image was built.  This
 * function is the symmetric counterpart to
 * @ref ora_reprogram_ram_rom_slot_fn_t.
 *
 * Unlike @ref ora_reprogram_ram_rom_slot_fn_t, there is no @c allow_active
 * flag: reads from the active slot are always permitted.  The DMA engine
 * and a concurrent read race in the same way a write does, so the caller
 * is responsible for any consistency requirements (for example, quiescing
 * host accesses before reading back a region that has just been written).
 *
 * Address and data mappings are derived from the current slot's pin map and
 * algorithm configuration in the same way as
 * @ref ora_reprogram_ram_rom_slot_fn_t.  Logical offsets and byte values
 * correspond to the chip-visible address space and data values, with all
 * GPIO address scrambling and data pin permutation reversed.
 *
 * @param[in]  slot    RAM slot index (0-based) to read from.  Must
 *                     identify a slot that has been allocated via the
 *                     firmware slot management API.
 * @param[in]  offset  Logical byte offset within the slot, in the range
 *                     [0, chip_size).  @c chip_size is the size of the
 *                     ROM image in logical bytes as reported by the slot
 *                     metadata.
 * @param[out] buf     Caller-supplied buffer to receive the logical bytes.
 *                     Must be at least @p len bytes long and must not be
 *                     @c NULL.
 * @param[in]  len     Number of logical bytes to read.  Must be greater
 *                     than zero and must satisfy @p offset + @p len <=
 *                     chip_size.
 *
 * @retval ORA_RESULT_OK           All @p len bytes were read successfully
 *                                  and @p buf has been filled with the
 *                                  logical values.
 * @retval ORA_RESULT_INVALID_ARG  @p buf is @c NULL, @p len is zero, or
 *                                  @p offset + @p len exceeds the slot's
 *                                  chip_size.
 * @retval ORA_RESULT_NOT_FOUND    @p slot is out of range or has not been
 *                                  allocated.
 *
 * @sa ORA_ID_READ_RAM_ROM_SLOT
 * @sa ora_reprogram_ram_rom_slot_fn_t
 * @sa ORA_ID_REPROGRAM_RAM_ROM_SLOT
 */
typedef ora_result_t (*ora_read_ram_rom_slot_fn_t)(
    uint8_t   slot,
    uint32_t  offset,
    uint8_t  *buf,
    uint32_t  len
);

/**
 * @brief State a GPIO can be placed in by @ref ora_gpio_set_fn_t
 * @since firmware 0.7.1
 */
typedef enum {
    /** @brief Drive the GPIO low */
    ORA_GPIO_STATE_LOW   = 0,

    /** @brief Drive the GPIO high */
    ORA_GPIO_STATE_HIGH  = 1,

    /**
     * @brief Release the GPIO - output driver disabled, high impedance
     *
     * The input path stays enabled, so the pin can still be read via
     * @ref ora_gpio_query_fn_t.
     */
    ORA_GPIO_STATE_INPUT = 2,
} ora_gpio_state_t;

/**
 * @brief What One ROM itself is using a GPIO for
 * @since firmware 0.7.1
 *
 * Reports only what the firmware has claimed the GPIO for: the serving set of
 * the currently active ROM slot, plus the board's own system pins. It says
 * nothing about what is attached to the pin electrically - whether a jumper is
 * fitted, what the far end of a wire is connected to, or whether something else
 * is driving the net. That remains the user's responsibility.
 *
 * Serving reads its address, chip select and /BYTE pins as SIO inputs with the
 * output driver disabled, so they are indistinguishable from unused pins by
 * register inspection alone; this enumeration is the only way to tell them
 * apart.
 *
 * What is reported is the *consequence* of driving the GPIO, not the role it
 * plays. The roles - address, data, chip select, /BYTE, X - are deliberately
 * not reported: naming them needs the board pin map and the active chip's
 * metadata, which a host already has, whereas whether One ROM drives or merely
 * reads a pin is knowledge only the firmware has.
 */
typedef enum {
    /** @brief Not used by One ROM */
    ORA_GPIO_USE_FREE           = 0,

    /**
     * @brief Serving reads this GPIO; driving it is reversible
     *
     * Covers the whole address span of the active ROM slot - including any X
     * expansion pins folded into it on Multi and Banked slots - the whole chip
     * select span, including any position the select field masks out and any
     * excess address line acting as a half-select, and the /BYTE pin.
     *
     * These are all SIO inputs, and PIO keeps reading the pin whatever its
     * function select says, so driving one and then releasing it with
     * ORA_GPIO_STATE_INPUT restores serving exactly.
     * @sa ora_gpio_set_fn_t
     */
    ORA_GPIO_USE_SERVING_READ   = 1,

    /**
     * @brief Serving drives this GPIO; driving it breaks serving until reboot
     *
     * The data pins of the active ROM slot, which PIO owns and drives. Taking
     * one away from PIO is not undone by releasing it.
     * @sa ora_gpio_set_fn_t
     */
    ORA_GPIO_USE_SERVING_DRIVEN = 2,

    /** @brief A board system pin - status LED, neopixel, VBUS or ext flash CS */
    ORA_GPIO_USE_SYSTEM         = 3,
} ora_gpio_use_t;

/**
 * @brief Drive a GPIO even if One ROM is using it
 * @sa ora_gpio_set_fn_t
 * @since firmware 0.7.1
 */
#define ORA_GPIO_FLAG_FORCE  (1u << 0)

/**
 * @brief GPIO information returned by @ref ora_gpio_query_fn_t
 * @since firmware 0.7.1
 */
typedef struct {
    /**
     * @brief In: sizeof this structure as known to the caller.  Out: the
     * number of bytes the firmware actually wrote.
     *
     * The caller sets this to sizeof(ora_gpio_info_t) as it knows it before
     * calling. The firmware writes at most that many bytes and sets this field
     * to how many it wrote, so a plugin built against an older or newer version
     * of this structure than the running firmware still interoperates.
     */
    uint8_t size;

    /** @brief What One ROM is using this GPIO for. @sa ora_gpio_use_t */
    uint8_t use;

    /** @brief The level currently present on the pad, 0 or 1 */
    uint8_t level;

    /** @brief 1 if the pin's output driver is currently enabled, 0 if not */
    uint8_t is_output;
} ora_gpio_info_t;
STATIC_ASSERT(sizeof(ora_gpio_info_t) == 4, "ora_gpio_info_t must be 4 bytes");

/**
 * @brief Drive a GPIO high or low, or release it to high impedance
 * @sa ORA_ID_GPIO_SET
 * @since firmware 0.7.1
 *
 * Takes the GPIO under SIO control and applies @p state immediately. The call
 * is instantaneous: it starts no timer and schedules nothing. A caller wanting
 * a bounded assertion must time the release itself.
 *
 * By default the call refuses, with ORA_RESULT_GPIO_IN_USE, any GPIO whose
 * @ref ora_gpio_use_t is not ORA_GPIO_USE_FREE. This is the firmware's whole
 * safety model - it knows what One ROM itself has claimed, and nothing about
 * what is wired to the pin. @ref ORA_GPIO_FLAG_FORCE overrides the refusal.
 *
 * How recoverable a forced pin is is exactly what @ref ora_gpio_use_t reports:
 *
 *   - ORA_GPIO_USE_SERVING_DRIVEN - the pin is driven by PIO. Forcing it takes
 *     the pin's function select away from PIO, and the serving path does not
 *     restore it, so serving is broken until the device reboots. Setting the
 *     pin back to ORA_GPIO_STATE_INPUT does not undo this.
 *   - ORA_GPIO_USE_SERVING_READ - the pin is an SIO input with its output
 *     driver disabled, and PIO keeps reading it whatever its function select
 *     says. Forcing one is therefore reversible: set it back to
 *     ORA_GPIO_STATE_INPUT and serving reads the pin as before. While it is
 *     driven, of course, serving sees the level being driven.
 *
 * Only the function select, the output enable and the pad's output-disable bit
 * are altered. Pulls, drive strength, slew rate and any input polarity
 * inversion are left untouched, which is what makes the release above exact.
 *
 * @param gpio   The GPIO to set, less than the running variant's GPIO count
 * @param state  The state to place the GPIO in. @sa ora_gpio_state_t
 * @param flags  Behavioural flags. Pass 0 for default behaviour.
 *               @sa ORA_GPIO_FLAG_FORCE
 * @return ORA_RESULT_OK on success; ORA_RESULT_GPIO_IN_USE if the GPIO is in
 *         use by One ROM and ORA_GPIO_FLAG_FORCE was not set;
 *         ORA_RESULT_INVALID_ARG if @p gpio is out of range or @p state is not
 *         a valid @ref ora_gpio_state_t
 */
typedef ora_result_t (*ora_gpio_set_fn_t)(
    uint8_t  gpio,
    uint8_t  state,
    uint32_t flags
);

/**
 * @brief Query what One ROM is using a GPIO for, and its current state
 * @sa ORA_ID_GPIO_QUERY
 * @since firmware 0.7.1
 *
 * Reports the GPIO's use, the level present on the pad, and whether its output
 * driver is enabled. There is deliberately no bulk form - a caller wanting the
 * whole device loops over this call.
 *
 * The caller must set @p info_out->size to its own sizeof(ora_gpio_info_t)
 * before calling; see @ref ora_gpio_info_t.
 *
 * @param gpio      The GPIO to query, less than the running variant's GPIO count
 * @param info_out  Structure to fill in, with its size field already set
 * @return ORA_RESULT_OK on success; ORA_RESULT_INVALID_ARG if @p info_out is
 *         NULL or @p gpio is out of range; ORA_RESULT_INVALID_SIZE if
 *         @p info_out->size is zero
 */
typedef ora_result_t (*ora_gpio_query_fn_t)(
    uint8_t gpio,
    ora_gpio_info_t *info_out
);

/** @} */ // plugin_api_functions

/**
 * @defgroup plugin_header Plugin Header
 * @brief Constants defining the plugin header format
 * @{
 */

/**
 * @brief Magic number identifying a valid One ROM plugin
 *
 * This magic number is used to verify that a plugin is valid and compatible
 * with the One ROM firmware. Plugins must include this magic number in their
 * header to be recognized by the firmware.
 */
#define ORA_PLUGIN_MAGIC 0x2041524F  // 'ORA ' backwards, so it appears forwards in little-endian

/**
 * @brief Plugin API version - major version bump indicates breaking changes
 *
 * This version number is used to ensure compatibility between plugins and the
 * One ROM firmware. Plugins must specify the API version they are compatible
 * with in their header.  A One ROM firmware only supports a defined set of API
 * versions, and will refuse to run plugins with incompatible versions.
 */
#define ORA_PLUGIN_VERSION_1 0x00000001

/**
 * @brief Plugin header structure
 *
 * This structure defines the header for a One ROM plugin and the appropriate
 * data in this format must be placed at the start of the plugin binary for it
 * to be recognized and loaded by the One ROM firmware.
 */
typedef struct {
    /**
     * @brief Magic number identifying a valid One ROM plugin
     * @sa ORA_PLUGIN_MAGIC
     */
    uint32_t magic;

    /**
     * @brief Plugin API version
     * @sa ORA_PLUGIN_VERSION
     */
    // offset 4
    uint32_t api_version;

    /** 
     * @brief Plugin's major version
     */
    // offset 8
    uint16_t major_version;

    /**
     * @brief Plugin's minor version
     */
    // offset 10
    uint16_t minor_version;

    /**
    * @brief Plugin's patch version
    */
    // offset 12
    uint16_t patch_version;

    /**
    * @brief Plugin's build version
    */
    // offset 14
    uint16_t build_version;

    /**
     * @brief Plugin's main function location.
     *
     * One ROM launches the pluging by calling the @ref ora_plugin_entry_t
     * function at this location.
     */
    // offset 16
    ora_plugin_entry_t entry;

    /**
     * @brief Plugin type
     * @sa ora_plugin_type_t
     */
    // offset 20
    ora_plugin_type_t plugin_type;

    /**
     * @brief Statically allocated memory usage
     * 
     * Each type of plugin is reserved a portion of SRAM at link time, so it
     * can use this without needing to call the API to allocate memory, if it
     * is sufficient.
     *
     * Each of system and user plugins are reserved different amounts of RAM
     * and in different locations.
     *
     * This field indicates how much of this plugin type's reserved RAM the
     * plugin uses, so that the firmware can allow the remainder to be used
     * as dynamically allocated pool memory.
     *
     * If this field is n, the memory used is 2 to the power of (n+1) bytes,
     * with 0 meaning no statically allocated RAM is used.
     * - 0 = 0 bytes used
     * - 1 = 4 bytes used
     * - 2 = 8 bytes used
     * ...
     * - 255 = maximum available statically allocated RAM used.
     */
    // offset 21
    uint8_t sam_usage;

    /**
     * @brief Firmware overrides this plugin wants to apply
     * 
     * Only supported by system plugins, used to indicate to the firmware that
     * defaults built into the firmware, or overriden by config, should
     * be overridden.
     *
     * For future compatibility, any unused bits must be set to 0.
     *
     * This is a bit field, with 1 indicating override.  Bit 0 = LSB
     * Bit 0 - Disable VBUS detect @sa ORA_OVERRIDE1_DISABLE_VBUS_DETECT
     */
    // offset 22
    uint8_t overrides1;

    /**
     * @brief Plugin properties
     * 
     * 
     * This is used to indicate properties of the plugin to the firmware, to
     * allow the core firmware and firmware parses to make informed decisions
     * about how to use the plugin.
     * 
     * For future compatibility, any unused bits must be set to 0.
     * 
     * This is a bit field, with 1 indicating the presence of the property.
     * Bit 0 = LSB
     * Bit 0 = supports running while USB is connected @sa ORA_PROPERTY1_SUPPORTS_USB_RUNNING
     * Bit 1 = supports yielding to allow exclusive mode @sa ORA_PROPERTY1_SUPPORTS_YIELD
     */
    // offset 23
    uint8_t properties1;

    /**
     * @brief Minimum major version of the firmware this plugin is compatible with
     * 
     * This is used to ensure that a plugin is only run on compatible versions of the
     * firmware.  If the firmware's major version is less than this, the firmware
     * refuses to run the plugin.
     */
    // offset 24
    uint16_t min_fw_major_version;

    /**
     * @brief Minimum minor version of the firmware this plugin is compatible with
     */
    // offset 26
    uint16_t min_fw_minor_version;

    /**
     * @brief Minimum patch version of the firmware this plugin is compatible with
     */
    // offset 28
    uint16_t min_fw_patch_version;

    /**
     * @brief Reserved for future use
     *
     * This field is reserved for future use and must be set to 0.
     */
    uint8_t reserved[226];
} ora_plugin_header_t;
#define ORA_PLUGIN_HEADER_SIZE 256  // Must not change without version bump
STATIC_ASSERT(sizeof(ora_plugin_header_t) == ORA_PLUGIN_HEADER_SIZE, "ora_plugin_header_t must be 256 bytes");

/**
 * @brief Firmware override flag for VBUS detect
 */
#define ORA_OVERRIDE1_DISABLE_VBUS_DETECT (1 << 0)

/** 
 * @brief Plugin property flag for USB support
 */
#define ORA_PROPERTY1_SUPPORTS_USB_RUNNING (1 << 0)

/** 
 * @brief Plugin property flag for yield support
 */
#define ORA_PROPERTY1_SUPPORTS_YIELD (1 << 1)

/** @} */

/** @} */ // plugin_api

#endif // PLUGIN_API_H