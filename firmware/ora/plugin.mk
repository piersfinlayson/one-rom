# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License

# One ROM sample plugin Makefile
#
# Usage:
#   make                        - build a user plugin
#   make PLUGIN_TYPE=SYSTEM     - build a system plugin
#   make PLUGIN_TYPE=USER       - build a user plugin (default)
#   make clean                  - remove build artifacts
#
# To use a non-default toolchain, set TOOLCHAIN to the directory containing
# the arm-none-eabi-* binaries, e.g.:
#   make TOOLCHAIN=/opt/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi/bin

# Plugin type: SYSTEM or USER
PLUGIN_TYPE ?= USER

TOOLCHAIN ?=
CC      := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-gcc,arm-none-eabi-gcc)
OBJCOPY := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objcopy,arm-none-eabi-objcopy)
OBJDUMP := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objdump,arm-none-eabi-objdump)

ORA_INCLUDE ?= .
EXTRA_C_FLAGS ?=

BUILD_DIR := build
SRC       := plugin_main.c

ifeq ($(PLUGIN_TYPE),SYSTEM)
$(info Building system plugin)
PLUGIN_BASE := 0x10010000
PLUGIN_TYPE_NUM := 0
PLUGIN_PREFIX := plugin_system
else ifeq ($(PLUGIN_TYPE),USER)
$(info Building user plugin)
PLUGIN_BASE := 0x10020000
PLUGIN_TYPE_NUM := 1
PLUGIN_PREFIX := plugin_user
else
    $(error PLUGIN_TYPE must be SYSTEM or USER)
endif

ELF       := $(BUILD_DIR)/$(PLUGIN_PREFIX).elf
BIN       := $(BUILD_DIR)/$(PLUGIN_PREFIX).bin

CFLAGS  := -mcpu=cortex-m33 -mthumb -mfloat-abi=hard -mfpu=fpv5-sp-d16 \
           -nostdlib -O2 -Wall -Wextra -Werror \
           -ffunction-sections -fdata-sections \
           -DPLUGIN_TYPE_NUM=$(PLUGIN_TYPE_NUM) \
           -I $(ORA_INCLUDE) $(EXTRA_C_FLAGS)

LDFLAGS := -nostdlib \
           -T $(ORA_INCLUDE)/plugin.ld \
           -Wl,--defsym,PLUGIN_TYPE=$(PLUGIN_TYPE_NUM) \
           -Wl,--fatal-warnings

.PHONY: all clean

all: $(BIN)

$(BUILD_DIR):
	@echo "Creating build directory..."
	@mkdir -p $@

$(ELF): $(SRC) $(ORA_INCLUDE)/plugin.h $(ORA_INCLUDE)/api.h $(ORA_INCLUDE)/system.h $(ORA_INCLUDE)/plugin.ld | $(BUILD_DIR)
	@echo "Building plugin..."
	@$(CC) $(CFLAGS) $(LDFLAGS) $< -o $@

$(BIN): $(ELF)
	@echo "Converting ELF to binary..."
	@$(OBJCOPY) -O binary $< $@
	@$(OBJDUMP) -s -j .plugin_header $(ELF) > $(BUILD_DIR)/$(PLUGIN_PREFIX).dis
	@$(OBJDUMP) -d -S $(ELF) >> $(BUILD_DIR)/$(PLUGIN_PREFIX).dis

clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)
