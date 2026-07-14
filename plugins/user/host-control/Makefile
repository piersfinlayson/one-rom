# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License

TOOLCHAIN ?=
CC      := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-gcc,arm-none-eabi-gcc)
OBJCOPY := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objcopy,arm-none-eabi-objcopy)
OBJDUMP := $(if $(TOOLCHAIN),$(TOOLCHAIN)/arm-none-eabi-objdump,arm-none-eabi-objdump)

MAJOR_VERSION := 0
MINOR_VERSION := 1
PATCH_VERSION := 1
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

.PHONY: all clean distclean

all: $(BIN)

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

clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)

distclean: clean

-include $(ALL_OBJ:.o=.d)