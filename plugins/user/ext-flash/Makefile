# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License
#
# Builds the One ROM external flash probe plugin.
#
# The shared firmware/ora/plugin.mk compiles a single source file and this
# plugin has four, keeping the routines that run from RAM apart so they are
# clear from the file they are in.  It also needs a linker script of its own,
# to bracket those routines so the plugin can find and copy them.  So the build
# is spelled out here.
#
# Usage:
#   make
#   make TOOLCHAIN=/opt/arm-gnu-toolchain/bin

TOOLCHAIN ?=
CC      := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-gcc,arm-none-eabi-gcc)
OBJCOPY := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objcopy,arm-none-eabi-objcopy)
OBJDUMP := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objdump,arm-none-eabi-objdump)

ORA_DIR := ../../../firmware/ora

BUILD_DIR     := build
PLUGIN_PREFIX := plugin_user

# User plugin.  Matches USER_PLUGIN_TYPE in the plugin linker script.
PLUGIN_TYPE_NUM := 1

ELF := $(BUILD_DIR)/$(PLUGIN_PREFIX).elf
BIN := $(BUILD_DIR)/$(PLUGIN_PREFIX).bin

# Stack each staged routine's copy is given, in 32-bit words.
#
# One definition reaches both the C that allocates the buffer and the linker
# script that checks the routine fits, so a routine that outgrows its buffer
# fails the link instead of running off the end of a 1KB stack on the device.
# Grow the number when the link says to.
STAGE_PROBE_WORDS    := 44
STAGE_QSPI_WORDS     := 79
STAGE_OP_WORDS       := 26
STAGE_BOOTPROG_WORDS := 43
STAGE_M1_WORDS       := 4

SRC := src/ext_flash_main.c src/qmi_probe.c src/recover.c src/cdc_log.c
OBJ := $(patsubst %.c,$(BUILD_DIR)/%.o,$(SRC))

# qmi_probe.c holds the routines the plugin copies onto its stack and runs from
# there, so they must be position independent.  -fno-function-sections keeps
# each staged section whole, since the plugin copies a section at a time and
# every call inside one has to stay inside it.
$(BUILD_DIR)/src/qmi_probe.o: EXTRA_CFLAGS := -fPIC -fno-plt -fno-function-sections
DEP := $(OBJ:.o=.d)

CFLAGS := -mcpu=cortex-m33 -mthumb -mfloat-abi=hard -mfpu=fpv5-sp-d16 \
          -nostdlib -O2 -Wall -Wextra -Werror \
          -ffunction-sections -fdata-sections \
          -MMD -MP \
          -DPLUGIN_IS_SYSTEM=0 \
          -DPLUGIN_TYPE_NUM=$(PLUGIN_TYPE_NUM) \
          -DSTAGE_PROBE_WORDS=$(STAGE_PROBE_WORDS)u \
          -DSTAGE_QSPI_WORDS=$(STAGE_QSPI_WORDS)u \
          -DSTAGE_OP_WORDS=$(STAGE_OP_WORDS)u \
          -DSTAGE_BOOTPROG_WORDS=$(STAGE_BOOTPROG_WORDS)u \
          -DSTAGE_M1_WORDS=$(STAGE_M1_WORDS)u \
          -I $(ORA_DIR) -I $(dir $(ORA_DIR))include -I src \
          -std=c11

# --gc-sections is deliberately absent.  The staged routines are reached through
# a copy the plugin makes at run time, which the linker cannot see.
LDFLAGS := -nostdlib \
           -T ext_flash_plugin.ld \
           -Wl,--defsym,PLUGIN_TYPE=$(PLUGIN_TYPE_NUM) \
           -Wl,--defsym,STAGE_PROBE_WORDS=$(STAGE_PROBE_WORDS) \
           -Wl,--defsym,STAGE_QSPI_WORDS=$(STAGE_QSPI_WORDS) \
           -Wl,--defsym,STAGE_OP_WORDS=$(STAGE_OP_WORDS) \
           -Wl,--defsym,STAGE_BOOTPROG_WORDS=$(STAGE_BOOTPROG_WORDS) \
           -Wl,--defsym,STAGE_M1_WORDS=$(STAGE_M1_WORDS) \
           -Wl,--fatal-warnings \
           -Wl,--no-warn-rwx-segments \
           -Wl,-Map=$(BUILD_DIR)/$(PLUGIN_PREFIX).map

# Rebuild every object when the flags change, so a flag added to a built tree
# does not leave stale objects behind.  The stamp is rewritten only when the
# flags differ, so an unchanged build reuses its objects.
CFLAGS_STAMP := $(BUILD_DIR)/.cflags
$(shell mkdir -p $(BUILD_DIR); \
        printf '%s' '$(CFLAGS)' | cmp -s - $(CFLAGS_STAMP) || \
        printf '%s' '$(CFLAGS)' > $(CFLAGS_STAMP))

.PHONY: all clean distclean help

all: $(BIN)

$(BUILD_DIR)/src:
	@mkdir -p $@

$(BUILD_DIR)/%.o: %.c $(CFLAGS_STAMP) | $(BUILD_DIR)/src
	@echo "Compiling $<..."
	@$(CC) $(CFLAGS) $(EXTRA_CFLAGS) -c $< -o $@

$(ELF): $(OBJ) ext_flash_plugin.ld
	@echo "Linking $@..."
	@$(CC) $(CFLAGS) $(LDFLAGS) $(OBJ) -o $@

$(BIN): $(ELF)
	@echo "Converting ELF to binary..."
	@$(OBJCOPY) -O binary $< $@
	@$(OBJDUMP) -s -j .plugin_header $(ELF) > $(BUILD_DIR)/$(PLUGIN_PREFIX).dis
	@$(OBJDUMP) -d -S $(ELF) >> $(BUILD_DIR)/$(PLUGIN_PREFIX).dis

clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)

distclean: clean

help:
	@echo "Usage:"
	@echo "  make            - build plugin"
	@echo "  make clean      - remove build artifacts"

-include $(DEP)
