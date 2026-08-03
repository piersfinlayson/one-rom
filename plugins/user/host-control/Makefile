# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License

TOOLCHAIN ?=
CC      := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-gcc,arm-none-eabi-gcc)
OBJCOPY := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objcopy,arm-none-eabi-objcopy)
OBJDUMP := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objdump,arm-none-eabi-objdump)

MAJOR_VERSION := 0
MINOR_VERSION := 1
PATCH_VERSION := 2
BUILD_VERSION := 0

ORA_DIR := ../../../firmware/ora
ORA_INCLUDE := -I$(ORA_DIR)
SDRR_INCLUDE := -I../../../firmware/include -I../../../firmware/generated
PLUGIN_INCLUDE := -I.

BUILD_DIR := build

PLUGIN_BASE   := 0x10020000
PLUGIN_PREFIX := host_control_plugin

ELF := $(BUILD_DIR)/$(PLUGIN_PREFIX).elf
BIN := $(BUILD_DIR)/$(PLUGIN_PREFIX).bin

SRC := src/host_control_main.c

FLASH_ERASE_SRC := src/flash_erase.c
FLASH_ERASE_OBJ := $(BUILD_DIR)/src/flash_erase.o
FLASH_ERASE_EXTRA_CFLAGS := -fPIC -fno-plt

CFLAGS := -mcpu=cortex-m33 -mthumb -mfloat-abi=hard -mfpu=fpv5-sp-d16 \
          -fstack-usage -fconserve-stack \
          -O3 -Wall -Wextra -Werror \
          -ffunction-sections -fdata-sections \
          -MMD -MP \
		  -DMAJOR_VERSION=$(MAJOR_VERSION) -DMINOR_VERSION=$(MINOR_VERSION) \
		  -DPATCH_VERSION=$(PATCH_VERSION) -DBUILD_VERSION=$(BUILD_VERSION) \
          $(SDRR_INCLUDE) $(ORA_INCLUDE) $(PLUGIN_INCLUDE) \
		  -Iinclude

LDFLAGS := -nostdlib \
           -T host_control_plugin.ld \
           -Wl,--defsym,PLUGIN_TYPE=0 \
           -Wl,--fatal-warnings \
		   -Wl,--no-warn-rwx-segments \
           -Wl,-Map=$(BUILD_DIR)/$(PLUGIN_PREFIX).map

PLUGIN_OBJ := $(patsubst %.c,$(BUILD_DIR)/%.o,$(SRC))
ALL_OBJ    := $(PLUGIN_OBJ) $(FLASH_ERASE_OBJ)

# ---------------------------------------------------------------------------
# Host build
#
# Compiles the plugin natively so it can be run against the firmware emulator
# for regression testing — see rust/plugin-tester.  This produces an object
# file, not a plugin binary: there is no linker script and no plugin header
# placement, and the harness supplies the linker symbols and the emulation
# seams (ORA_SECTION and friends are no-ops under ORA_HOST_TEST).
#
# The flags here must match the firmware's own native test build
# (firmware/test.mk), because the two link together:
# - -fshort-enums, or ora_result_t is a different width on each side.
# - -DTEST_BUILD=1, which selects REAL_HARDWARE 0 and drops the metadata
#   layout STATIC_ASSERTs.  Those assert device offsets that a 64-bit host
#   cannot satisfy — its pointers are twice the width — and the firmware test
#   build already accepts that.  Both sides must agree, so both define it.
#
# -Werror matches the ARM build.  The two casts in the NV commit path that used
# to warn on a 64-bit host — the flash offset computed from __nv_storage_start,
# and the erase routine's address formed from a RAM slot base with the Thumb
# bit set — now go through ORA_FLASH_OFFSET and ORA_STAGED_FN_PTR.
# ---------------------------------------------------------------------------

HOST_CC          ?= cc
HOST_BUILD_DIR   := build-host
HOST_OBJ         := $(HOST_BUILD_DIR)/host_control_main.o
HOST_FLASH_ERASE := $(HOST_BUILD_DIR)/flash_erase.o
HOST_ALL_OBJ     := $(HOST_OBJ) $(HOST_FLASH_ERASE)

HOST_CFLAGS := -DORA_HOST_TEST=1 -DTEST_BUILD=1 -fshort-enums \
               -O1 -g -Wall -Wextra -Werror \
               -DMAJOR_VERSION=$(MAJOR_VERSION) -DMINOR_VERSION=$(MINOR_VERSION) \
               -DPATCH_VERSION=$(PATCH_VERSION) -DBUILD_VERSION=$(BUILD_VERSION) \
               $(SDRR_INCLUDE) $(ORA_INCLUDE) $(PLUGIN_INCLUDE) \
               -Iinclude \
               -MMD -MP

.PHONY: all clean distclean host clean-host

all: $(BIN)

host: $(HOST_ALL_OBJ)

$(HOST_BUILD_DIR):
	@mkdir -p $@

$(HOST_OBJ): $(SRC) | $(HOST_BUILD_DIR)
	@echo "Compiling $< for host..."
	@$(HOST_CC) $(HOST_CFLAGS) -c $< -o $@

# The erase routine is compiled for the host too, and the harness calls the
# real thing rather than the bytes the plugin staged — see ORA_STAGED_FN_PTR.
# So the sequence under test is the plugin's own, not a re-description of it.
$(HOST_FLASH_ERASE): $(FLASH_ERASE_SRC) | $(HOST_BUILD_DIR)
	@echo "Compiling $< for host..."
	@$(HOST_CC) $(HOST_CFLAGS) -c $< -o $@

clean-host:
	@rm -rf $(HOST_BUILD_DIR)

$(FLASH_ERASE_OBJ): $(FLASH_ERASE_SRC) | $(BUILD_DIR)
	@mkdir -p $(dir $@)
	@echo "Compiling $< (PIC)..."
	@$(CC) $(CFLAGS) $(FLASH_ERASE_EXTRA_CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: %.c | $(BUILD_DIR)
	@mkdir -p $(dir $@)
	@echo "Compiling $<..."
	@$(CC) $(CFLAGS) -c $< -o $@

$(ELF): $(ALL_OBJ) $(ORA_DIR)/plugin.h $(ORA_DIR)/api.h $(ORA_DIR)/system.h host_control_plugin.ld
	@echo "Linking..."
	@$(CC) $(CFLAGS) $(LDFLAGS) $(ALL_OBJ) -lc -lgcc -o $@

$(BIN): $(ELF)
	@echo "Converting ELF to binary..."
	@$(OBJCOPY) -O binary $< $@
	@$(OBJCOPY) --pad-to $$(($(PLUGIN_BASE) + 0x10000)) --gap-fill 0xFF -O binary $< $@
	@$(OBJDUMP) -s -j .plugin_header $(ELF) > $(BUILD_DIR)/$(PLUGIN_PREFIX).dis
	@$(OBJDUMP) -d -S $(ELF) >> $(BUILD_DIR)/$(PLUGIN_PREFIX).dis

$(BUILD_DIR):
	@mkdir -p $@

clean: clean-host
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)

distclean: clean

-include $(ALL_OBJ:.o=.d)