// One ROM configuration structures. 

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef CONFIG_BASE_H
#define CONFIG_BASE_H

#if __STDC_VERSION__ < 199901L
#error "C99 or later required"
#endif

#include <stdint.h>
#include <stddef.h>

// Pull in enums
#include "enums.h"

#if defined(TEST_BUILD)
#include "test/stub.h"
#endif // TEST_BUILD

#include "alg.h"

// Forward declarations
typedef struct onerom_metadata_header_t onerom_metadata_header_t;
typedef struct onerom_runtime_info_t onerom_runtime_info_t;
typedef struct onerom_rom_pin_map_t onerom_rom_pin_map_t;
typedef struct onerom_rom_info_t onerom_rom_info_t;
typedef struct onerom_rom_slot_t onerom_rom_slot_t;
typedef struct onerom_firmware_overrides_t onerom_firmware_overrides_t;
typedef struct onerom_hardware_info_t onerom_hardware_info_t;
typedef struct onerom_firmware_config_t onerom_firmware_config_t;

// Main SDRR information data structure
typedef struct {
    // Offset: 0

    // Magic bytes to identify the firmware and structure
    // 4 bytes
    const char magic[4];  // Magic bytes = "SDRR"

    // Offset: 4

    // Firmware version information
    // 4 x 2 bytes = 8 bytes
    const uint16_t major_version;
    const uint16_t minor_version;
    const uint16_t patch_version;
    const uint16_t build_number;

    // Offset: 12

    // Pointer to build date/time string
    // 4 bytes
    const char* build_date;

    // Offset: 16

    // Git commit hash, NULL terminated
    // 8 bytes
    char commit[8];

    // Firmware v0.7.0+ onwards this has changed.

    // Offset: 24

    // Version of the info structure.  2 as of v0.7.0.
#define ONEROM_INFO_VERSION 0x00000002
    const uint32_t version;

    // Offset: 28

    // Pointer to metadata.
    // 4 bytes
    const onerom_metadata_header_t *metadata;

    // Offset: 32

    // Pointer to RTT control block
    const void *rtt;

    // Offset: 36

    // Pointer to the firmare's base runtime info data structure.
    const onerom_runtime_info_t* runtime;

    // Offset: 40

    // Pad to 64 bytes.  Set to 0xFF.
    const uint8_t reserved[24];

    // Length: 64
} onerom_info_t;
STATIC_ASSERT(sizeof(onerom_info_t) == 64, "onerom_info_t must be 64 bytes");

// One ROM Metadata Header
//
// Placed at the start of the metadata flash area to indicate:
// - metadata version
// - location of the actual metadata
//
// Note that all sub structures MUST be 4 byte aligned by anything generating
// them, such as the One ROM pre-processor.  That does not include strings.
typedef struct onerom_metadata_header_t {
    // Magic bytes to identify the metadata header
    //
    // Offset: 0
#define ONEROM_METADATA_MAGIC "ONEROM_METADATA"
    const char magic[16];  // "ONEROM_METADATA\0"

    // Metadata version
    //
    // v0.7.0+ onwards uses metadata version 2.
    //
    // Offset: 16
#define CURRENT_METADATA_VERSION 0x00000002
    const uint32_t version; 

    // Hardware information about this One ROM.  Mandatory - must be non-NULL
    //
    // Offset: 20
    const onerom_hardware_info_t *hw;

    // Firmware configuration.  Mandatory - must be non-NULL.
    //
    // Offset: 24
    const onerom_firmware_config_t *fw;

    // Number of populated ROM slots.  A ROM slot is a storage location for
    // one or more ROM images that will be served simultaneously.
    // 
    // Offset: 28
    const uint8_t rom_slot_count;

    // Whether to enable boot logging (currently requires SWD)
    //
    // Offset: 29
    const uint8_t boot_logging;

    // Whether to enable SWD
    //
    // Offset: 30
    const uint8_t swd_enabled;

    // Whether to boot fast, disabling image select jumper reading.
    //
    // Offset: 31
    const uint8_t turbo_boot;

    // Pointer to array of ROM slots
    //
    // Offset: 32
    const onerom_rom_slot_t *rom_slots;

    // Reserved for future expansion, set to 0xff.
    //
    // Offset: 36
    const uint8_t reserved[220];
} onerom_metadata_header_t;
STATIC_ASSERT(sizeof(onerom_metadata_header_t) == 256, "onerom_metadata_header_t must be 256 bytes");

// ROM slot information structure
//
// One ROM serve one ROM slot, which can be filled with one or more ROM images
// to be served simultaneously.  Multiple ROMs are served using the X pins as
// additional chip select lines.
//
// If the multiple ROM image support is not used, there is a 1:1 mapping
// between slot and image - i.e. `rom_count` is 1.
typedef struct onerom_rom_slot_t {
    // Offset: 0

    // Pointer to the data for the ROM image(s) in this slot.  Copied to RAM
    // at startup if it is being served.
    const uint8_t* data;

    // Offset: 4

    // Size of the data for the ROM image(s) in this slot.  Used to copy the
    // ROM data to RAM at startup.  This is either:
    // - ROM_IMAGE_SIZE for a single ROM image
    // - ROM_SET_IMAGE_SIZE for a set of multiple ROM images
    const uint32_t size;

    // Offset: 8

    // Pointer to array of pointers to ROMs in this set.  Note it needs to be
    // a pointer to const pointer to const data, otherwise the linker will
    // decide that the sdrr_rom_info_t structs need to be relocated to RAM
    // on startup, which is unnecessary. 
    const onerom_rom_info_t* const * roms;

    // Offset: 12

    // The number of unique ROM images in this slot.  Used to index the above
    // array.
    const uint8_t rom_count;

    // Offset: 13

    // Type of slot - e.g. a plugin type, or a multi-ROM slot, etc.  Not used
    // by One ROM to figure out how to serve the ROM - typically used by
    // plugins.
    const rom_slot_type_t slot_type;

    // Offset: 14
    const uint8_t reserved1[2];

    // Offset: 16

    // Which ROM serving algorithms to use for this slot.
    const onerom_alg_config_t * const alg;

    // Offset: 20

    // Pointer to firmware configuration overrides when serving this ROM slot.
    // May be NULL if there are no overrides.
    const onerom_firmware_overrides_t *firmware_overrides;

    // Offset: 24

    // Padding to 32 bytes
    const uint8_t reserved2[8];
} onerom_rom_slot_t;

// onerom_runtime_info_t
//
// Contains information about the One ROM runtime environment.
//
// Modified from v0.7.0+
typedef struct onerom_runtime_info_t {
    // Offset: 0

    // Magic bytes to identify the firmware and structure
    // 4 bytes
    char magic[4];  // Magic bytes = "SDRR"

    // Offset: 4

    // From v0.7.0 onwards this is the version of this structure:
#define RUNTIME_INFO_VERSION 0x00000002
    // 4 bytes
    uint32_t version;

    // Offset: 8

    // Size of this structure in bytes
    // 1 byte
    uint8_t runtime_info_size;

    // Offset: 9

    /// Whether this MCU was detected as a RP235xA or RP235xB.
    rp235x_variant_t rp235x;

    // Offset: 10

    // Image select jumper state at boot.
    // Initialized to 0xFF.
    // 1 byte
    uint8_t  image_sel;

    // Offset: 11

    // Index of the currently selected ROM slot.  This is chosen at boot via
    // the image select jumpers.
    // Initialized to 0xFF.
    // 1 byte
    uint8_t rom_slot_index;

    // Offset: 12

    // Pointer to the location in RAM One ROM is serving from
    // 4 bytes
    void *rom_table;

    // Offset: 16

    // Length of the ROM table One ROM is serving from in bytes.
    // Initialized to 0.
    // 4 bytes
    uint32_t rom_table_size;

    // Offset: 20

    // Whether overlocking is enabled
    uint8_t overclock_enabled;

    // Offset: 21

    // Whether status LED is enabled
    uint8_t status_led_enabled;

    // Offset: 22

    // Whether SWD is enabled
    uint8_t swd_enabled;

    // Offset: 23

    // Fire VREG output setting
    fire_vreg_t fire_vreg;

    // Offset: 24

    // Fire frequency setting
    fire_freq_t fire_freq;

    // Offset: 26

    // SYSCLK frequency in MHz
    uint16_t sysclk_mhz;

    // Offset: 28

    // Pointer to TIMER0_IRQ_0 handler
    void (*timer0_irq_0_handler)(void);

    // Offset: 32

    // Pointer to USBCTRL_IRQ handler
    void (*usbctrl_irq_handler)(void);

    // Offset 36

    // Whether device is in limp mode
    limp_mode_pattern_t limp_mode;

    // Offset 37

    // Peripherals/PLLs enabled
    // Bit 0 = LSB = USB PLL
    // Bit 1 = ADC
    uint8_t peri_en;

    // Offset: 38

    // Indicates whether One ROM is serving in 8 or 16 bit mode.
    bit_modes_t bit_mode;

    // Offset: 39

    // Whether boot logging is enabled.
    uint8_t boot_logging;

    // Pointer to system plugin context
    // CANNOT MUST NOT MOVE!
    // Offset: 40
    void *system_plugin_context;

    // Pointer to user plugin context
    // CANNOT MUST NOT MOVE!
    // Offset: 44
    void *user_plugin_context;

    // offset: 48

    // Pointer to current ROM slot information.  Aligned with rom_slot_index
    // above
    onerom_rom_slot_t const * current_rom_slot;

    // Offset: 52

    // Top 2 bits hold the currently used PIO block for address reader
    // PIO(s).  Bottom 6 bits contain the length of PIO instruction (in
    // words) used by the address reader PIO(s).
    uint8_t addr_pio_block_info;

    // Bottom 4 bits hold the currently used PIO SMs for the address reader
    // PIO block.  Top 4 bits hold the current used IRQs for that block.
    uint8_t addr_pio_sm_info;

    // Offset: 54

    // Top 2 bits hold the currently used PIO block for CS/Data PIOs.
    // Bottom 6 bits contain the length of PIO instruction (in words) used
    // within that block, by the CS/Data PIOs.
    uint8_t cs_data_pio_block_info;

    // Bottom 4 bits hold the currently used PIO SMs for the CS/Data PIO
    // block.  Top 4 bits hold the currently used IRQs for that block.
    uint8_t cs_data_pio_sm_info;

    // Offset: 56

    // Indicates DMA channels used for serving.  Bit 0 = CH0, etc.
    uint16_t dma_pio_ch;

    // Offset: 58

    uint8_t reserved[2];
} onerom_runtime_info_t;
// Check system plug context is at 0x40, and the user at 0x44.  These CANNOT
// move without breaking the plugin API.
STATIC_ASSERT(offsetof(onerom_runtime_info_t, system_plugin_context) == 40, "system_plugin_context must be at offset 0x40");
STATIC_ASSERT(offsetof(onerom_runtime_info_t, user_plugin_context) == 44, "user_plugin_context must be at offset 0x44");
STATIC_ASSERT(sizeof(onerom_runtime_info_t) == 60, "onerom_runtime_info_t length unexpected");

// onerom_hardware_info_t
//
// Metadata structure containing information about this physical One ROM board
typedef struct onerom_hardware_info_t {
    // Offset: 0

    // Hardware revision string.  Must be present.
    const char *hw_rev;

    // Offset: 4

    // Whether this One ROM is expected to have an RP235xA or RP235xB
    rp235x_variant_t rp235x;

    // Number of physical pins on this One ROM.  As in the number of pins the
    // ROM being emulated has.
    uint8_t num_phys_pins;

    // Whether this One ROM has a USB port.
    uint8_t usb_capable;

    // GPIO pin for VBUS detection.  0xFF if not present.
    uint8_t gpio_vbus;

    // Offset: 8

    // Secondary flash chip select GPIO.  0xFF if not present (no secondary
    // flash).
    uint8_t gpio_ext_flash_cs;

    // GPIO pin for status LED.  0xFF if not present.
    uint8_t gpio_status;

    // GPIO pin for Neopixel LED.  0xFF if not present.
    uint8_t gpio_neopixel;

    // GPIO pin also connected to SWDIO.  0xFF if not present.
    uint8_t gpio_swdio;

    // Offset: 12

    // GPIO pin also connected to SWCLK.  0xFF if not present.
    uint8_t gpio_swclk;

    // GPIOs connected to X1 and X2.  0xFF if not present.
    uint8_t gpio_x1;
    uint8_t gpio_x2;
    
    // X pin pull direction (i.e. when closed, does it pull the line high or
    // low?).  1 = pull high, 0 = pull low.
    uint8_t x_jumper_pull;

    // Offset: 16

    // Image select pins.  May be 0xFF, but must be contiguously filled in
    // prior to 0xFF, as this is how the firmware infers the quantity (it
    // looks for 0xFF and then stops.
#define MAX_IMG_SEL_PINS 7
    uint8_t gpio_sel[MAX_IMG_SEL_PINS];

    // Image select jumper pull direction.  1 = pull high, 0 = pull low.
    uint8_t sel_jumper_pull;

    // Offset: 24

    // Mapping from physical pin to GPIO pins.  Indexed by physical pin minus
    // 1 - so the first entry is physical pin 1.  0xFF if no GPIO is connected
    // to this physical pin.  The second element allows for an optional second
    // GPIO, where a physical pin is connected to 2 GPIOs.
#define GPIO_NONE 0xFF
#define MAX_PHYS_PINS 40
#define MAX_GPIOS_PER_PHYS_PIN 2  // Max 2 GPIOs per physical pin
    uint8_t gpio_from_phys_pin[MAX_PHYS_PINS][MAX_GPIOS_PER_PHYS_PIN];

    // Offset: 104

    uint8_t reserved[24];
} onerom_hardware_info_t;
STATIC_ASSERT(MAX_PHYS_PINS * MAX_GPIOS_PER_PHYS_PIN == 80, "gpio_from_phys_pin expected to be 80 bytes");
STATIC_ASSERT(sizeof(onerom_hardware_info_t) == 128, "onerom_hardware_info_t expected to be 128 bytes");

// onerom_firmware_config_t
//
// Metadata structure containing One ROM-wide firmware configuration
typedef struct onerom_firmware_config_t {
    // Name of this One ROM.  May be NULL if not supplied.
    //
    // Offset: 0
    const char *name;

    // Override serial number of this One ROM.  May be NULL if the standard
    // unique MCU chip ID is to be used.
    // Offset: 4
    const char *serial_number; 
} onerom_firmware_config_t;

// Firmware Overrides
//
// This is linked to via the ROM set structure, and allows per-ROM-set
// overrides of firmware configuration options.
//
// Where each ROM slot requires the same overrides, only once instance of
// this structure need be created, and all ROM sets can point to it.
//
// Although Ice boards are not supported by firmware v0.7.0, the Ice
// overrides have been left to avoid needing to reimplement the encoding
// of this structure.  Its size has, however, been reduced to 32 bytes (from
// 64).
typedef struct onerom_firmware_overrides_t {
    // Bitfield indicating which overrides are present.
    //
    // Table below shows byte | bit, where bit 0 = LSB.
    // A set bit indicates that the corresponding override is configured.
    //
    // 0 | 0 = UNUSED Ice MCU frequency
    // 0 | 1 = UNUSED Ice overclock overridden
    // 0 | 2 = Fire MCU frequency
    // 0 | 3 = Fire overclock overridden
    // 0 | 4 = Fire VREQ overridden
    // 0 | 5 = Status LED overridden
    // 0 | 6 = SWD overridden
    // 0 | 7 = UNUSED Fire serve mode overridden
    // 1 | 0 = UNUSED Fire DMA ROM preload overridden
    // 1 | 1 = UNUSED Fire Force 16 bit mode
    // 
    // Unused (reserved) values MUST be set to 0.
    const uint8_t override_present[8];

    // 8 bytes to here

    // STM32F4 (Ice) MCU clock frequency override in MHz.  0 = max rated clock
    // speed for the MCU.
    const ice_freq_t ice_freq;

    // RP2350 (Fire) MCU clock frequency override in MHz.  Uses values from
    // fire_freq_t enum.
    const fire_freq_t fire_freq;

    // RP2350 (Fire) VREQ voltage override.  Uses values for VREQ_CTRL
    // register.
    const fire_vreg_t fire_vreg;

    const uint8_t pad1[3];

    // 16 bytes to here

    // Bitfields indicating boolean values for specific overrides
    //
    // Byte | Bit : Description, bit 0 = LSB
    // 0 | 0 : Ice overclocking enabled/disabled 1/0
    // 0 | 1 : Fire overclocking enabled/disabled 1/0
    // 0 | 2 : Status LED enabled/disabled 1/0
    // 0 | 3 : SWD enabled/disabled 1/0
    // 0 | 4 : Fire serve mode: 1 = PIO, 0 = CPU
    // 0 | 5 : Fire ROM DMA preload enabled/disabled 1/0
    // 0 | 6 : Force 16 bit mode 1/0
    const uint8_t override_value[8];

    // 24 bytes to here

    // Padding to 32 bytes
    const uint8_t pad3[8];
} onerom_firmware_overrides_t;

// onerom_rom_pin_map_t
//
// Provides the pin mapping used to mangle and demangling a particular ROM.
//
// Any address or data bits unused by this ROM are set to 0xFF.
typedef struct onerom_rom_pin_map_t {
    // Address pin mapping.  This indicates the MCU GPIO pin the address pin
    // is connected to.  The pin is indexed by the Ax value.  On a 16-bit ROM
    // A-1 is the first entry, A0 the second, etc.
    // GPIO_NONE is used for any address bits not used by this ROM.
#define MAX_ADDR_PINS 24
    const uint8_t addr[MAX_ADDR_PINS];

    // Data pin mapping.  Indexed by data pin D0, D1, etc, and indicates the
    // MCU GPIO. 
    // GPIO_NONE is used for any data bits not used by this ROM.
#define MAX_DATA_PINS 16
    const uint8_t data[MAX_DATA_PINS];
} onerom_rom_pin_map_t;
STATIC_ASSERT(sizeof(onerom_rom_pin_map_t) == 40, "onerom_rom_pin_map_t must be 40 bytes");

// ROM information structure
typedef struct onerom_rom_info_t {
    // Human readable string of this ROM type.  Must not be NULL.
    const char *rom_type;
    
    // Filename of the ROM image source - may be NULL if not supplied.
    const char *filename;
    
    // Pin mapping for this ROM.  Must not be NULL.
    const onerom_rom_pin_map_t *const pin_map;

    // Reserved for future expansion.
    uint8_t reserved[4];
} onerom_rom_info_t;

#endif // CONFIG_BASE_H