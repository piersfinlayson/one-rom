# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License

TOOLCHAIN ?=
CC      := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-gcc,arm-none-eabi-gcc)
OBJCOPY := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objcopy,arm-none-eabi-objcopy)
OBJDUMP := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objdump,arm-none-eabi-objdump)

MAJOR_VERSION := 0
MINOR_VERSION := 3
PATCH_VERSION := 0
BUILD_VERSION := 0

TINYUSB_DIR   := tinyusb
PICOBOOTX_DIR := picobootx
PICO_SDKLESS_DIR := pico-sdkless

# Version of picobootx to build against.  Remove the picobootx/ directory after
# changing this, so it is re-cloned.
PICOBOOTX_VERSION ?= v0.6.0

# Version of pico-sdkless to build against.  Remove the pico-sdkless/ directory
# after changing this, so it is re-cloned.
PICO_SDKLESS_VERSION ?= v0.1.1

# Commit of the One ROM tinyusb fork to build against.  A commit rather than a
# tag, because the fork tracks a project this one does not version.  Remove the
# tinyusb/ directory after changing this, so it is re-cloned.
TINYUSB_COMMIT ?= 05fb334d0

ORA_DIR := ../../../firmware/ora
ORA_INCLUDE := -I$(ORA_DIR)
SDRR_INCLUDE := -I../../../firmware/include -I../../../firmware/generated
PLUGIN_INCLUDE := -I.

BUILD_DIR := build

# The three dependencies below are cloned while this file is being read, before
# any target runs, so a clone that takes a while is make appearing to hang with
# nothing on screen.  Three things keep that from happening:
#
# - Each says what it is about to do, since $(shell) has no output until it
#   finishes.
# - git's own progress goes to stderr, which $(shell) does not capture, so it
#   reaches the terminal as it happens.
# - The exit status is checked.  A failed clone used to surface much later as a
#   missing header, naming a file rather than the network.
#
# tinyusb is pinned to a commit rather than a tag, so `--depth 1 --branch`
# cannot reach it.  `--filter=blob:none` fetches the history without the file
# contents, which leaves an arbitrary commit checkout working on a fraction of
# the data.  The other two are pinned to tags and take a shallow clone.

ifeq ($(wildcard $(TINYUSB_DIR)),)
$(info Cloning tinyusb into $(TINYUSB_DIR) ...)
TINYUSB_CLONE := $(shell git clone --filter=blob:none https://github.com/piersfinlayson/tinyusb.git $(TINYUSB_DIR) >&2; echo $$?)
ifneq ($(TINYUSB_CLONE),0)
$(error Failed to clone tinyusb - see the git output above)
endif
endif
TINYUSB_CHECKOUT := $(shell git -c advice.detachedHead=false -C $(TINYUSB_DIR) checkout --quiet $(TINYUSB_COMMIT) >&2; echo $$?)
ifneq ($(TINYUSB_CHECKOUT),0)
$(error Failed to check out tinyusb $(TINYUSB_COMMIT) - see the git output above)
endif
include tinyusb.mk

ifeq ($(wildcard $(PICOBOOTX_DIR)),)
$(info Cloning picobootx $(PICOBOOTX_VERSION) into $(PICOBOOTX_DIR) ...)
PICOBOOTX_CLONE := $(shell git -c advice.detachedHead=false clone --depth 1 --branch $(PICOBOOTX_VERSION) https://github.com/piersfinlayson/picobootx.git $(PICOBOOTX_DIR) >&2; echo $$?)
ifneq ($(PICOBOOTX_CLONE),0)
$(error Failed to clone picobootx $(PICOBOOTX_VERSION) - see the git output above)
endif
endif
include picobootx/picobootx.mk

ifeq ($(wildcard $(PICO_SDKLESS_DIR)),)
$(info Cloning pico-sdkless $(PICO_SDKLESS_VERSION) into $(PICO_SDKLESS_DIR) ...)
PICO_SDKLESS_CLONE := $(shell git -c advice.detachedHead=false clone --depth 1 --branch $(PICO_SDKLESS_VERSION) https://github.com/piersfinlayson/pico-sdkless.git $(PICO_SDKLESS_DIR) >&2; echo $$?)
ifneq ($(PICO_SDKLESS_CLONE),0)
$(error Failed to clone pico-sdkless $(PICO_SDKLESS_VERSION) - see the git output above)
endif
endif

PICOBOOTX_SRC_C := $(addprefix picobootx/,$(PICOBOOTX_SRC_C))
PICOBOOTX_INCLUDE_DIRS := $(addprefix picobootx/,$(PICOBOOTX_INCLUDE_DIRS))

PLUGIN_BASE   := 0x10010000
PLUGIN_PREFIX := usb_system_plugin

ELF := $(BUILD_DIR)/$(PLUGIN_PREFIX).elf
BIN := $(BUILD_DIR)/$(PLUGIN_PREFIX).bin

SRC := src/usb_descriptors.c src/usb_picobootx.c src/usb_rom.c src/usb_led.c src/usb_gpio.c src/usb_log.c src/usb_main.c

CFLAGS := -mcpu=cortex-m33 -mthumb -mfloat-abi=hard -mfpu=fpv5-sp-d16 \
          -O2 -Wall -Wextra -Werror \
          -ffunction-sections -fdata-sections \
          -MMD -MP -g \
		  -DMAJOR_VERSION=$(MAJOR_VERSION) -DMINOR_VERSION=$(MINOR_VERSION) \
		  -DPATCH_VERSION=$(PATCH_VERSION) -DBUILD_VERSION=$(BUILD_VERSION) \
          $(SDRR_INCLUDE) $(ORA_INCLUDE) $(PLUGIN_INCLUDE) \
		  -Iinclude -I$(PICO_SDKLESS_DIR)/include \
          $(addprefix -I,$(TINYUSB_INCLUDE_DIRS)) \
		  $(addprefix -I,$(PICOBOOTX_INCLUDE_DIRS)) \
		  -std=c11

LDFLAGS := -nostdlib \
           -T ../../../firmware/ora/plugin.ld \
           -Wl,--defsym,PLUGIN_TYPE=0 \
           -Wl,--fatal-warnings \
		   -Wl,--no-warn-rwx-segments \
           -Wl,-Map=$(BUILD_DIR)/$(PLUGIN_PREFIX).map

PLUGIN_OBJ := $(patsubst %.c,$(BUILD_DIR)/%.o,$(SRC))
TINYUSB_OBJ := $(patsubst %.c,$(BUILD_DIR)/%.o,$(TINYUSB_SRC_C))
PICOBOOTX_OBJ := $(patsubst %.c,$(BUILD_DIR)/%.o,$(PICOBOOTX_SRC_C))
ALL_OBJ     := $(PLUGIN_OBJ) $(TINYUSB_OBJ) $(PICOBOOTX_OBJ)

.PHONY: all clean distclean

all: $(BIN)

$(BUILD_DIR)/%.o: %.c | $(BUILD_DIR)
	@mkdir -p $(dir $@)
	@echo "Compiling $<..."
	@$(CC) $(CFLAGS) -c $< -o $@

$(ELF): $(ALL_OBJ) $(ORA_DIR)/plugin.h $(ORA_DIR)/api.h $(ORA_DIR)/system.h $(ORA_DIR)/plugin.ld
	@echo "Linking..."
	@$(CC) $(CFLAGS) $(LDFLAGS) $(ALL_OBJ) -lc -lgcc -o $@

$(BIN): $(ELF)
	@echo "Converting ELF to binary..."
	@$(OBJCOPY) -O binary $< $@
	@$(OBJDUMP) -s -j .plugin_header $(ELF) > $(BUILD_DIR)/$(PLUGIN_PREFIX).dis
	@$(OBJDUMP) -d -S $(ELF) >> $(BUILD_DIR)/$(PLUGIN_PREFIX).dis

$(BUILD_DIR):
	@mkdir -p $@

# ---------------------------------------------------------------------------
# Host build
#
# Compiles the plugin natively so it can be run against the firmware emulator
# for regression testing — see rust/usb-tester.  This produces object files, not
# a plugin binary: there is no linker script and no plugin header placement, and
# the harness supplies the linker symbols and the emulation seams (ORA_SECTION
# and friends are no-ops under ORA_HOST_TEST).
#
# The flags here must match the firmware's own native test build
# (firmware/test.mk), because the two link together:
# - -fshort-enums, or ora_result_t is a different width on each side.
# - -DTEST_BUILD=1, which selects REAL_HARDWARE 0 and drops the metadata layout
#   STATIC_ASSERTs, which assert device offsets a 64-bit host cannot satisfy.
#
# tinyusb and picobootx are on the include path but neither is compiled: the
# plugin only needs their headers to declare what it calls, and the harness
# supplies the handful of entry points behind them.
# ---------------------------------------------------------------------------

HOST_CC        ?= cc
HOST_BUILD_DIR ?= build-host
HOST_OBJ       := $(patsubst src/%.c,$(HOST_BUILD_DIR)/%.o,$(SRC))

# Extra flags for the host objects, and the directory they land in.  A
# coverage build sets both: --coverage here, and a build directory of its own.
# The two must not share a directory, because nothing here compares the flags
# an existing object was built with - a coverage run that reused
# uninstrumented objects would report no coverage rather than fail.
HOST_EXTRA_CFLAGS ?=

HOST_CFLAGS := -DORA_HOST_TEST=1 -DTEST_BUILD=1 -fshort-enums \
               -O1 -g -Wall -Wextra -Werror \
               -DMAJOR_VERSION=$(MAJOR_VERSION) -DMINOR_VERSION=$(MINOR_VERSION) \
               -DPATCH_VERSION=$(PATCH_VERSION) -DBUILD_VERSION=$(BUILD_VERSION) \
               $(SDRR_INCLUDE) $(ORA_INCLUDE) $(PLUGIN_INCLUDE) \
               -Iinclude -I$(PICO_SDKLESS_DIR)/include \
               $(addprefix -I,$(TINYUSB_INCLUDE_DIRS)) \
               $(addprefix -I,$(PICOBOOTX_INCLUDE_DIRS)) \
               -std=c11 -MMD -MP $(HOST_EXTRA_CFLAGS)

.PHONY: host clean-host

host: $(HOST_OBJ)

$(HOST_BUILD_DIR):
	@mkdir -p $@

$(HOST_BUILD_DIR)/%.o: src/%.c | $(HOST_BUILD_DIR)
	@echo "Compiling $< for host..."
	@$(HOST_CC) $(HOST_CFLAGS) -c $< -o $@

clean-host:
	@rm -rf $(HOST_BUILD_DIR)

clean: clean-host
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)

distclean: clean
	@echo "Removing cloned repositories..."
	@rm -rf $(TINYUSB_DIR) $(PICOBOOTX_DIR) $(PICO_SDKLESS_DIR)

# The host objects are listed too, or a header change would rebuild nothing on
# the host side and a test would go on asserting against the last build.
-include $(ALL_OBJ:.o=.d) $(HOST_OBJ:.o=.d)