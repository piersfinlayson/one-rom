# Top level Makefile for One ROM
#
# Running this Makefile will generate the One ROM firmware binary in sdrr/build
# according to the settings below.
#
# You can override the settings in this file by creating a config file which
# sets them, and including it when running make like this:
#
# CONFIG=configs/your_config.mk make
#
# You can also override some of the below settings in a config file, and others
# on the command line, like this:
#
# MCU=f411rc CONFIG=configs/your_config.mk make
#

VERSION_MAJOR := 0
VERSION_MINOR := 6
VERSION_PATCH := 13
BUILD_NUMBER := 1
GIT_COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
export VERSION_MAJOR VERSION_MINOR VERSION_PATCH BUILD_NUMBER GIT_COMMIT

# Allow specifying config file to override the below settings
# CONFIG ?= configs/blank.mk
ifdef CONFIG
ifeq ($(wildcard $(CONFIG)),)
$(error CONFIG file $(CONFIG) does not exist)
endif
include $(CONFIG)
endif

#
# Settings
#
# Use these to specify your hardware variant and the ROM images and
# configuration.
#

# MCU types
# f446rc - STM32F446 256KB flash version
# f446re - STM32F446 512KB flash version
# f411rc - STM32F411 256KB flash version
# f411re - STM32F411 512KB flash version
# f405rg - STM32F405/GD32F405 1024KB flash version
# f401re - STM32F401 512KB flash version
# f401rb - STM32F401 128KB flash version
# f401rc - STM32F401 256KB flash version
# rp2350 - RP2350 2MB flash version

# MCU ?= rp2350
# MCU ?= f446rc
# MCU ?= f446re
# MCU ?= f411rc
# MCU ?= f411re
# MCU ?= f405rg
# MCU ?= f401re
# MCU ?= f401rb
# MCU ?= f401rc
MCU ?= rp2350

# Hardware revision
#
# Hardware revisions are contained in hw-config and sub-directories, and
# are defined as .json files.  The value to use here is the filename, without
# the .json extension.
#
# 24-d, 24-e and 24-f are all variants of the 24 pin (23xx ROM) board
# 28-a is a variant of the 28 pin (2364 ROM) board, and is not yet supported.
# 
# Values d, e and f from v0.1.0 are still supported and map to 24-d, 24-e and
# 24-f respectively.
#
# You can add your own hardware revisions by creating the appropriate file in
# hw-config/user, or, if you plan to submit a pull request for it and your
# hardware files, hw-config/third-party. 
HW_REV ?= fire-24-e

# ROM configurations - each ROM can have its own type and CS settings
#
# Format: file=path_or_url,type=XXXX,cs1=X[,cs2=X][,cs3=X][,dup|pad]
#
# dup - used where the provided image is two small for the specified ROM type,
#       and the image should be duplicated to fill the ROM
# pad - used where the provided image is too large for the specified ROM type,
#       and the image should be padded (after the image) to fill the ROM
#
# Examples:
#   Single 2364 ROM: file=images/rom1.rom,type=2364,cs1=0
#   Single 2332 ROM: file=images/rom2.rom,type=2332,cs1=1,cs2=0  
#   Single 2316 ROM: file=images/rom3.rom,type=2316,cs1=0,cs2=1,cs3=0
#   Multiple ROMs: file=images/rom1.rom,type=2364,cs1=0 file=images/rom2.rom,type=2332,cs1=1,cs2=0
#   URL: file=https://example.com/rom1.rom,type=2364,cs1=0
#
# See `config` for more extensive examples.

#ROM_CONFIGS ?= \
#	file=images/test/0_63_4096.rom,type=2332,cs1=0,cs2=1

# Status LED
#
# The status LED is used to indicate the status of One ROM.
# Only supported from HW revision e onwards.
#
# You can disable it in order to reduce the power consumption of the device.
# With a 1K resistor, this is likely to save around 5-10mW of power.
#
# 1 is enabled, 0 disabled.

#STATUS_LED ?= 0
STATUS_LED ?= 1

# Count ROM access - increment a RAM word every time CS goes from inactive to
# active.
#
# The RAM word is located at 0x20000008 as of v0.3.0 and encoded in little
# endian format.
#
# On boot this word is initialized to 0xFFFFFFFF.
#
# On starting the main serving loop, if this feature is active, the RAM word
# is set to 0 and then increment any time all of the CS lines for the ROM type
# go from inactive to active - i.e. once per ROM access.
#
# Use [airfrog](https://piers.rocks/u/airfrog) or an SWD debugger to read the
# RAM value.
#
# While initial testing (C64 character ROM) suggest that this addition work
# does not affect performance, it may under certain circumstances.
#COUNT_ROM_ACCESS ?= 0
COUNT_ROM_ACCESS ?= 1

#
# Development and debug settings
#
# It is recommended to leave these settings as is unless you are developing or
# debugging the One ROM firmware.
#
# However, if you are having problems with the One ROM firmware, you may want to
# disable any enabled features.
#

# SWD - Serial Wire Debug protocol
#
# This is used to communicate with the MCU for debugging and programming.
#
# It is recommended to leave SWD enabled, as it makes it easier to reprogram
# the chip with new images.
#
# SWD is required for the logging options to work.

# SWD ?= 0
SWD ?= 1

# Boot logging configuration
#
# Outputs diagnostics boot messages via SWD during the boot process.
#
# This is generally safe to leave enabled, but it slows down the boot process.
# Boot takes ~300us (STM32F405) without this enabled, and ~2.5ms with it.
# If you are using the ROM as a boot ROM in a machine with a very short reset
# timer, you may want to disable this.
#
# SWD is required for this option to work.

BOOT_LOGGING ?= 0
# BOOT_LOGGING ?= 1

# Plugin logging configuration
#
# Enables logs from plugins that support it.

PLUGIN_LOGGING ?= 0
# PLUGIN_LOGGING ?= 1

# Main loop logging configuration
#
# This option enables/disabld logging within main_loop (see sdrr.rom_impl.c).
#
# It does not loop after every byte is served - that is enabled/disabled via
# MAIN_LOOP_ONE_SHOT.
#
# SWD, BOOT_LOGGING and MAIN_LOOP_LOGGING are required for this option to work.

MAIN_LOOP_LOGGING ?= 0
#MAIN_LOOP_LOGGING ?= 1

# Main loop one shot logging
#
# This option outputs logs after every byte is retrieved from the ROM.  While
# the main loop that processes the chip select and retrieves ROM data remanins
# functional, there is a gap between the CS line being released, and the ROM
# detecting the next CS activation - to make these logs.
#
# Therefore, with this option, the ROM will not meet its timing requirements,
# and it is not recommended to use this option unless you are debugging the
# ROM code.
#
# SWD and BOOT_LOGGING are required for this option to work.

MAIN_LOOP_ONE_SHOT ?= 0
# MAIN_LOOP_ONE_SHOT ?= 1

# Debug logging
#
# More extensive logging than the boot and main loop logging options.
#
# May overwhelm the SWD interface, meaning logs are dropped.
#
# This is not recommended for normal operation, but may be useful for debugging

DEBUG_LOGGING ?= 0
#DEBUG_LOGGING ?= 1

# MCO - Microcontroller Clock Output
#
# The MCO options output STM32 clocks on specific pins:
# - MCO is output on pin PA8.
# - MCO2 is output on pin PC9.
#
# This is useful to debugging the clock configuration of the STM32, and 
# ensuring it is being clocked at the correct frequency.
#
# MCO is used for MCO1 on STM32F4xx series chips.
# MCO2 is used for MCO2 on STM32F4xx series chips.
#
# On the STM32F4xx chip, PA8 is not used for ROM data, so MCO can be enabled
# relatively safely - although having a high frequency signal on the board may
# cause interference with other signals.
#
# MCO2 (STM32F4xx only) uses PC9, which is an address line, so enabling this
# should only be done during debugging.
#
# MCO2 may only be enabled when MCO is enabled.

MCO ?= 0
# MCO ?= 1
MCO2 ?= 0
# MCO2 ?= 1

# Oscillator - Used for STM32 only
# 
# Which oscillator to use.  The options are:
# - HSI (High Speed Internal) - the internal oscillator
# - HSE (High Speed External) - an external crystal or clock source (no longer supported)
#
# One ROM has been designed to work primrily with the internal oscillator, in
# order to reduce component count and cost (and a more stable clock source is
# not required for this application).  Hence HSI operation is recommended.
#
# On the RP2350 an external crystal oscillator running at 12MHz is assumed

OSC ?= HSI
# OSC ?= HSE

# Frequency
#
# Target frequency of the processor
# Leave blank to use the max frequency of the chip:
# - STM32F401: 84MHz
# - STM32F411: 100MHz
# - STM32F405: 168MHz
# - STM32F446: 180MHz
# - RP2350: 150MHz
#
# Some guidance on speeds required to run stably:
# - PET 4032 character ROM >= 26MHz
# - PET 4032 kernal ROM >= 26MHz
# - C64 PAL kernal ROM >= 75MHz
# - C64 PAL character ROM >= 98MHz
# - VIC 20 PAL kernal ROM >= 37MHz
# - VIC 20 PAL basic ROM >= 37MHz
# - VIC 20 PAL character ROM >= 72MHz
#
# Your mileage may vary.  Faster machines will require the device to operate
# at higher frequencies.  Lower clock frequencies will cause One ROM to draw less
# power.  The F405 draws around 30mA at 30MHz and >60mA at 168MHz.  One ROM's
# voltage regulator also consumes some power.
#
# It is suggested to leave this blank (i.e. use the maximum frequency) for most
# applications, and only dial it down if you are trying to reduce power
# consumption.

# FREQ ?= 30

# Overclocking
#
# Can be used to allow the MCU to be run at a higher frequency than officially
# supported.  Use with caution, as this may damage the chip.
#
# You must manually set the desired frequency using the FREQ variable.

OVERCLOCK ?= 0
# OVERCLOCK ?= 1

# Bootloader
#
# When enabled, the STM32 device will enter its built in bootloader on boot
# when all the select jumpers are closed (high).
#
# This may be useful for reprogramming the device if it has locked up and SWD
# has become unresponsive - although it shouldn't.

BOOTLOADER ?= 0
# BOOTLOADER ?= 1

# Disable preload to RAM
#
# By default, the ROM image to be served is preloaded to RAM.  Setting this
# option disables that.  It speeds up startup by a fraction of a ms, at the
# cost of (presumably) serving the ROM data taking longer, as it has to be
# looked up from the flash, instead of RAM - and the flash lookup will have
# wait states, and the prefetch cache won't help, as this access is random.
# It might even slow down the instruction prefetch.
#
# Put a different way, you probably don't want to enable this.

DISABLE_PRELOAD_TO_RAM ?= 0
# DISABLE_PRELOAD_TO_RAM ?= 1

# Byte serving algorithm
#
# Byte default, the One ROM firmware checks for CS line changes twice as often as
# addr lines - due to the asymetric timings for these lines.
#
# When serving mulit-ROM sets, the address lines are only checked when CS goes
# active - as One ROM doesn't know which ROM image to pull the byte from until
# that happens.
#
# Another algorithm can be chosen for the single image/set case.  It has no
# effect in the mutli image set case.
#
# default = b
# a = check CS twice as often as address lines
# b = check CS and address as frequently as each other

SERVE_ALG ?= default
#SERVE_ALG ?= a
#SERVE_ALG ?= b

# Output directory
#
# Directory to store intermediate files which are autogenerated during the
# build process.
GEN_OUTPUT_DIR ?= output

# Cargo profile
#
# Whether to run cargo as release or debug.
# Needs to be blank for debug, because cargo.
CARGO_PROFILE ?= release

# Extra C Flags
#
# Use to pass extra flags into One ROM's compile stage.
#
# For example this enables the C main loop test function/
# EXTRA_C_FLAGS ?= -DC_MAIN_LOOP

EXTRA_C_FLAGS ?=

# Do not include Metadata or ROM imags

EXCLUDE_METADATA ?= 0
# EXCLUDE_METADATA ?= 1

#
# End of settings
#

COLOUR_YELLOW := $(shell echo -e '\033[33m')
COLOUR_RESET := $(shell echo -e '\033[0m')
COLOUR_RED := $(shell echo -e '\033[31m')

ifneq ($(SUPPRESS_OUTPUT),1)
  $(info ==========================================)
  $(info Building One ROM v$(VERSION_MAJOR).$(VERSION_MINOR).$(VERSION_PATCH))
  $(info ==========================================)
  $(info One ROM Makefile env settings:)
endif

# MCU - handle deprecated STM variable
ifneq ($(MCU),)
  ifneq ($(STM),)
    $(error Cannot set both MCU and STM - STM is deprecated, use MCU instead)
  endif
else ifneq ($(STM),)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info STM variable is deprecated, use MCU instead)
endif
  MCU := $(STM)
endif

# Sort out binary prefix
ifneq ($(MCU),)
  # Make MCU lowercase
  MCU := $(shell echo $(MCU) | tr '[:upper:]' '[:lower:]')
  MCU_PREFIX=
  ifneq ($(filter f%,$(MCU)),)
    MCU_PREFIX=stm32
  endif
  BIN_PREFIX=sdrr-$(MCU_PREFIX)$(MCU)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - MCU=$(MCU))
endif
endif
ifeq ($(BIN_PREFIX),)
 $(error - MCU not set)
else
  ifneq ($(SUPPRESS_OUTPUT),1)
    $(info - BIN_PREFIX=$(BIN_PREFIX))
  endif
endif

# DFU support - only available for STM32 MCUs
DFU_SUPPORTED := 0
ifneq ($(filter f%,$(MCU)),)
  DFU_SUPPORTED := 1
endif

ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - DFU_SUPPORTED=$(DFU_SUPPORTED))
endif

ifneq ($(CONFIG),)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - CONFIG=$(CONFIG))
endif
endif

# Extra C flags
ifneq ($(EXTRA_C_FLAGS),)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - EXTRA_C_FLAGS=$(EXTRA_C_FLAGS))
endif
endif

# Exclude metadata
ifeq ($(EM),1)
EXCLUDE_METADATA=1
endif
ifeq ($(EXCLUDE_METADATA),1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - $(COLOUR_YELLOW)EXCLUDE_METADATA=$(EXCLUDE_METADATA)$(COLOUR_RESET))
endif
endif

# Set hardware revision flag
ifneq ($(HW_REV),)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - HW_REV=$(HW_REV))
endif
  HW_REV_FLAG = --hw $(HW_REV)
else
  $(info - $(COLOUR_RED)HW_REV not set$(COLOUR_RESET) - please set it to a valid value, e.g. d or e)
  exit 1
endif

# Set STATUS_LED_FLAG based on STATUS_LED variable
ifeq ($(STATUS_LED), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - STATUS_LED=$(STATUS_LED))
endif
  STATUS_LED_FLAG = --status-led
else
  STATUS_LED_FLAG =
endif

# Set count_rom_access flag based on COUNT_ROM_ACCESS variable
ifeq ($(COUNT_ROM_ACCESS), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - COUNT_ROM_ACCESS=$(COUNT_ROM_ACCESS))
endif
  COUNT_ROM_ACCESS_FLAG = --count-rom-access
else
  COUNT_ROM_ACCESS_FLAG =
endif

# Set swd flag based on SWD variable
ifeq ($(SWD), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - SWD=$(SWD))
endif
  SWD_FLAG = --swd
else
  SWD_FLAG =
endif

# Set boot logging flag based on BOOT_LOGGING variable
ifeq ($(BOOT_LOGGING), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - BOOT_LOGGING=$(BOOT_LOGGING))
endif
  BOOT_LOGGING_FLAG = --boot-logging
else
  BOOT_LOGGING_FLAG = 
endif

# Set plugin logging flag based on PLUGIN_LOGGING variable
ifeq ($(PLUGIN_LOGGING), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - PLUGIN_LOGGING=$(PLUGIN_LOGGING))
endif
  PLUGIN_LOGGING_FLAG = --plugin-logging
else
  PLUGIN_LOGGING_FLAG =
endif

# Set main loop logging flag based on MAIN_LOOP_LOGGING variable
ifeq ($(MAIN_LOOP_LOGGING), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - MAIN_LOOP_LOGGING=$(MAIN_LOOP_LOGGING))
endif
  MAIN_LOOP_LOGGING_FLAG = --main-loop-logging
else
  MAIN_LOOP_LOGGING_FLAG = 
endif

# Set debug logging flag based on DEBUG_LOGGING variable
ifeq ($(DEBUG_LOGGING), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - DEBUG_LOGGING=$(DEBUG_LOGGING))
endif
  DEBUG_LOGGING_FLAG = --debug-logging
else
  DEBUG_LOGGING_FLAG =
endif

# Set mco flag based on MCO variable
ifeq ($(MCO), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - MCO=$(MCO))
endif
  MCO_FLAG = --mco
else
  MCO_FLAG = 
endif

# Set mco2 flag based on MCO2 variable
ifeq ($(MCO2), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - MCO2=$(MCO2))
endif
  MCO2_FLAG = --mco2
else
  MCO2_FLAG =
endif

# Set oscillator flag based on OSC variable
ifeq ($(OSC), HSE)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - OSC=$(OSC))
endif
  OSC_FLAG = --hse
else
  OSC_FLAG = --hsi
endif

# Set FREQ_FLAG based on FREQ variable
ifeq ($(FREQ),)
  FREQ_FLAG =
else
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - FREQ=$(FREQ))
endif
  FREQ_FLAG = --freq $(FREQ)
endif

# Set OVERCLOCK_FLAG based on OVERCLOCK variable
ifeq ($(OVERCLOCK), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - OVERCLOCK=$(OVERCLOCK))
endif
  OVERCLOCK_FLAG = --overclock
else
  OVERCLOCK_FLAG =
endif

# Set BOOTLOADER_FLAG based on BOOTLOADER variable
ifeq ($(BOOTLOADER), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - BOOTLOADER=$(BOOTLOADER))
endif
  BOOTLOADER_FLAG = --bootloader
else
  BOOTLOADER_FLAG =
endif

# Set DISABLE_PRELOAD_TO_RAM_FLAG based on DISABLE_PRELOAD_TO_RAM variable
ifeq ($(DISABLE_PRELOAD_TO_RAM), 1)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - DISABLE_PRELOAD_TO_RAM=$(DISABLE_PRELOAD_TO_RAM))
endif
  DISABLE_PRELOAD_TO_RAM_FLAG = --disable-preload-to-ram
else
  DISABLE_PRELOAD_TO_RAM_FLAG =
endif

# Set SERVE_ALG_FLAG based on SERVE_ALG variable
ifeq ($(SERVE_ALG), b)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - SERVE_ALG=$(SERVE_ALG))
endif
  SERVE_ALG_FLAG = --serve-alg b
else ifeq ($(SERVE_ALG), a)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - SERVE_ALG=$(SERVE_ALG))
endif
  SERVE_ALG_FLAG = --serve-alg a
else ifeq ($(SERVE_ALG), default)
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - SERVE_ALG=$(SERVE_ALG))
endif
  SERVE_ALG_FLAG =
else
  $(error - $(COLOUR_RED)Invalid SERVE_ALG value$(COLOUR_RESET) - please set it to a valid value, e.g. a or b)
  exit 1
endif

# Set up CARGO_TARGET_DIR
ifeq ($(CARGO_PROFILE),)
  CARGO_TARGET_DIR := rust/target/debug
else
  CARGO_TARGET_DIR := rust/target/$(CARGO_PROFILE)
endif
ifneq ($(SUPPRESS_OUTPUT),1)
$(info - CARGO_TARGET_DIR=$(CARGO_TARGET_DIR))
endif

ifneq ($(ARGS),)
ifneq ($(SUPPRESS_OUTPUT),1)
$(info - ARGS=$(ARGS))
endif
endif

ifeq ($(ROM_CONFIGS),)
  $(info - $(COLOUR_YELLOW)ROM_CONFIGS not set$(COLOUR_RESET) - no ROMs will be included in firmware)
else
ifneq ($(SUPPRESS_OUTPUT),1)
  $(info - ROM_CONFIGS=$(ROM_CONFIGS))
endif
endif

ifneq ($(SUPPRESS_OUTPUT),1)
$(info -----)
endif

# Convert ROM_CONFIGS to --chip arguments, adjusting local file paths only
#
# HTTP/HTTPS URLs are used as-is
# Absolute file paths (starting with /) are used as-is  
# Relative file paths get ../ prepended
define process_rom_config
$(if $(findstring http://,$(1)),
    $(1),
    $(if $(findstring https://,$(1)),
        $(1),
        $(if $(filter /%,$(patsubst file=%,%,$(1))),
            $(1),
            $(1)
        )
    )
)
endef
ROM_ARGS = $(foreach config,$(ROM_CONFIGS),--chip $(strip $(call process_rom_config,$(config))))
ifneq ($(SUPPRESS_OUTPUT),1)
$(info Generated ROM_ARGS required by sdrr-gen:)
$(info - ROM_ARGS=$(ROM_ARGS))
$(info -----)
endif

WARNINGS=0

ifneq ($(SUPPRESS_OUTPUT),1)
$(info One ROM Build Warnings: )
ifneq ($(SWD), 1)
  $(info - $(COLOUR_YELLOW)SWD is disabled$(COLOUR_RESET) - this will prevent you from reprogramming the device via SWD after flashing with this image)
  WARNINGS += 1
endif

ifeq ($(WARNINGS),0)
  $(info - None)
endif
endif

.PHONY: all clean clean-firmware clean-firmware-build clean-gen clean-sdrr-gen sdrr-gen gen clean-sdrr-info sdrr-info info info-detail firmware run run-actual flash flash-actual test test-pio $(CARGO_TARGET_DIR)/sdrr-gen

ifeq ($(DFU_SUPPORTED),1)
all: firmware info dfu-binary
else
all: firmware info
endif
	@echo "=========================================="
	@echo "One ROM firmware build complete:"
	@echo "- firmware files are in sdrr/build/"
	@echo "-----"
	@ls -ltr sdrr/build/$(BIN_PREFIX).elf
	@ls -ltr sdrr/build/$(BIN_PREFIX).bin
ifeq ($(MCU),rp2350)
	-@ls -ltr sdrr/build/$(BIN_PREFIX).uf2
endif
ifeq ($(DFU_SUPPORTED),1)
	-@ls -ltr sdrr/build/$(BIN_PREFIX).dfu
endif
	@echo "=========================================="

sdrr-gen:
	@echo "=========================================="
	@echo "Building sdrr-gen, to:"
	@echo "- generate firmware settings"
	@echo "- retrieve ROM data"
	@echo "- process ROM data into One ROM firmware files"
	@echo "-----"
	@cd rust && cargo build --$(CARGO_PROFILE)

$(CARGO_TARGET_DIR)/sdrr-gen: sdrr-gen

gen: $(CARGO_TARGET_DIR)/sdrr-gen
	@echo "=========================================="
	@echo "Running sdrr-gen, to:"
	@echo "- generate firmware settings"
	@echo "- retrieve ROM data"
	@echo "- process ROM data into One ROM firmware files"
	@echo "- Args: --mcu $(MCU) $(HW_REV_FLAG) $(OSC_FLAG) $(ROM_ARGS) $(SWD_FLAG) $(COUNT_ROM_ACCESS_FLAG) $(BOOT_LOGGING_FLAG) $(PLUGIN_LOGGING_FLAG) $(MAIN_LOOP_LOGGING_FLAG) $(DEBUG_LOGGING_FLAG) $(MCO_FLAG) $(MCO2_FLAG) $(FREQ_FLAG) $(OVERCLOCK_FLAG) $(STATUS_LED_FLAG) $(BOOTLOADER_FLAG) $(DISABLE_PRELOAD_TO_RAM_FLAG) $(SERVE_ALG_FLAG) $(ARGS) --overwrite --output-dir $(GEN_OUTPUT_DIR)"
	@echo "-----"
	@mkdir -p $(GEN_OUTPUT_DIR)
	@$(CARGO_TARGET_DIR)/sdrr-gen --mcu $(MCU) $(HW_REV_FLAG) $(OSC_FLAG) $(ROM_ARGS) $(SWD_FLAG) $(COUNT_ROM_ACCESS_FLAG) $(BOOT_LOGGING_FLAG) $(PLUGIN_LOGGING_FLAG) $(MAIN_LOOP_LOGGING_FLAG) $(DEBUG_LOGGING_FLAG) $(MCO_FLAG) $(MCO2_FLAG) $(FREQ_FLAG) $(OVERCLOCK_FLAG) $(STATUS_LED_FLAG) $(BOOTLOADER_FLAG) $(DISABLE_PRELOAD_TO_RAM_FLAG) $(SERVE_ALG_FLAG) $(ARGS) --overwrite --output-dir $(GEN_OUTPUT_DIR)

sdrr-info:
	@echo "=========================================="
	@echo "Building sdrr-info, to:"
	@echo "- Validate One ROM firmware"
	@echo "- Extract key One ROM firmware properties"
	@echo "-----"
	@cd rust && cargo build --$(CARGO_PROFILE)

info: sdrr-info firmware
	@echo "=========================================="
	@echo "Running sdrr-info, to:"
	@echo "- Validate One ROM firmware"
	@echo "- Extract key One ROM firmware properties"
	@echo "-----"
	@$(CARGO_TARGET_DIR)/sdrr-info info --strict sdrr/build/$(BIN_PREFIX).bin
	@echo "-----"
	@echo "Use <SAME_ARGS> make info-detail to see more details about the firmware"

info-detail: sdrr-info firmware
	@echo "=========================================="
	@echo "Running sdrr-info, to:"
	@echo "- Validate One ROM firmware"
	@echo "- Extract key One ROM firmware properties"
	@echo "- Show detailed One ROM firmware properties"
	@echo "-----"
	@$(CARGO_TARGET_DIR)/sdrr-info info -d --strict sdrr/build/$(BIN_PREFIX).bin
	@echo "=========================================="

firmware: gen
	@echo "=========================================="
	@echo "Building One ROM firmware for:"
	@echo "- MCU variant: $(shell echo $(MCU) | tr '[:lower:]' '[:upper:]')"
	@echo "- HW revision: $(HW_REV)"
	@echo "-----"
	@GEN_OUTPUT_DIR=$(GEN_OUTPUT_DIR) EXCLUDE_METADATA="$(EXCLUDE_METADATA)" EXTRA_C_FLAGS="$(EXTRA_C_FLAGS)" make --no-print-directory -C sdrr
ifeq ($(MCU), rp2350)
	@if command -v picotool >/dev/null 2>&1; then \
		picotool uf2 convert sdrr/build/$(BIN_PREFIX).bin sdrr/build/$(BIN_PREFIX).uf2; \
	else \
		echo "Warning: picotool not found, skipping UF2 conversion"; \
	fi
endif

# Call make run-actual - this causes a new instance of make to be invoked and generated.mk exists, so it can load PROBE_RS_CHIP_ID
run: firmware info
	@echo "=========================================="
	@echo "Flash One ROM firmware to device and attach to logging:"
	@echo "-----"
	@SUPPRESS_OUTPUT=1 make --no-print-directory run-actual

flash: firmware info
	@echo "=========================================="
	@echo "Flash One ROM firmware to device:"
	@echo "-----"
	@SUPPRESS_OUTPUT=1 make --no-print-directory flash-actual

test: firmware
	@echo "=========================================="
	@echo "Building tests to:"
	@echo "- verify generated firmware ROM images"
	@echo "-----"
	@GEN_OUTPUT_DIR=$(GEN_OUTPUT_DIR) EXTRA_C_FLAGS="$(EXTRA_C_FLAGS)" make --no-print-directory -C test
	@echo "=========================================="
	@echo "Running tests to:"
	@echo "- verify generated firmware ROM images"
	@echo "-----"
	@ROM_CONFIGS="$(ROM_CONFIGS)" HW_REV=$(HW_REV) test/build/image-test
	@echo "-----"
	@echo "Tests completed"

test-pio: firmware
	@echo "=========================================="
	@echo "Running tests to:"
	@echo "- verify generated firmware ROM images (PIO version)"
	@echo "-----"
	@GEN_OUTPUT_DIR=$(GEN_OUTPUT_DIR) EXTRA_C_FLAGS="$(EXTRA_C_FLAGS)" make --no-print-directory -C sdrr -f test.mk
	@echo "-----"
	@echo "Tests completed"


-include $(GEN_OUTPUT_DIR)/generated.mk

run-actual:
	@probe-rs run --chip $(PROBE_RS_CHIP_ID) sdrr/build/$(BIN_PREFIX).elf

flash-actual:
	@probe-rs download --chip $(PROBE_RS_CHIP_ID) --binary-format bin --base-address $(FLASH_BASE) sdrr/build/$(BIN_PREFIX).bin

dfu-binary: firmware
ifeq ($(DFU_SUPPORTED),1)
	@echo "=========================================="
	@echo "Creating DFU binary:"
	@echo "-----"
	@cp sdrr/build/$(BIN_PREFIX).bin sdrr/build/$(BIN_PREFIX).dfu
	@dfu-suffix -v 0x0483 -p 0xdf11 -a sdrr/build/$(BIN_PREFIX).dfu
	@echo "DFU binary created: sdrr/build/$(BIN_PREFIX).dfu"
else
	@echo "DFU not supported for MCU type: $(MCU)"
	@exit 1
endif

dfu-flash: dfu-binary
ifeq ($(DFU_SUPPORTED),1)
	@echo "=========================================="
	@echo "Flash One ROM firmware via DFU:"
	@echo "-----"
	@dfu-util -a 0 -s 0x08000000 -D sdrr/build/$(BIN_PREFIX).dfu -R
else
	@echo "DFU not supported for MCU type: $(MCU)"
	@exit 1
endif

clean-firmware-build:
	+cd sdrr && make clean-build

clean-firmware:
	+cd sdrr && make clean

clean-gen:
	rm -fr $(GEN_OUTPUT_DIR)

clean-rust:
	cd rust && cargo clean

clean: clean-firmware clean-rust
