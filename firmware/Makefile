# One ROM firmware Makefile

# To build, use the top-level /Makefile, which generates the required files and
# configuration to build One ROM, then calls this Makefile.

COLOUR_YELLOW := $(shell echo -e '\033[33m')
COLOUR_RESET := $(shell echo -e '\033[0m')
COLOUR_RED := $(shell echo -e '\033[31m')

# Build directory
BUILD_DIR := build

# This directory
FIRMWARE_DIR := firmware

# Version of apio to build against.  Remove the apio/ directory after changing
# this, so it is re-cloned.
APIO_VERSION ?= v0.2.0

OBJCOPY_EXTRA = --set-section-flags .metadata=contents,readonly -R .metadata -R .rom_data -R .padding_to_rom_data
$(info - $(COLOUR_YELLOW)Excluding metadata and ROM images from binary$(COLOUR_RESET))

# Toolchain
#
# The version is pinned in ci/arm-toolchain-version, and ci/install-arm-toolchain.sh
# installs it and creates this version-less symlink, so CI, the build container
# and a developer's machine all compile the firmware with the same compiler.
# Set TOOLCHAIN to override - the installer prints the directory to use.
TOOLCHAIN ?= /opt/arm-toolchain/bin
CC := $(TOOLCHAIN)/arm-none-eabi-gcc
OBJDUMP := $(TOOLCHAIN)/arm-none-eabi-objdump
OBJCOPY := $(TOOLCHAIN)/arm-none-eabi-objcopy
SIZE := $(TOOLCHAIN)/arm-none-eabi-size

# Source files
SRCS := src/constants.c src/globals.c src/log.c \
		src/main.c src/plugin.c src/utils.c \
		src/vector.c src/rp235x.c src/piodma/pio.c \
		src/piodma/piorom2.c src/piodma/pioram.c src/piodma/dma.c \
		src/piodma/pioplugin.c
OBJS := $(patsubst src/%,$(BUILD_DIR)/%,$(SRCS:.c=.o))
SEGGER_SRCS := segger-rtt/RTT/SEGGER_RTT.c segger-rtt/RTT/SEGGER_RTT_printf.c
SEGGER_OBJS := $(BUILD_DIR)/SEGGER_RTT.o $(BUILD_DIR)/SEGGER_RTT_printf.o

# Output files
BIN_PREFIX ?= onerom-rp235x
ELF := $(BUILD_DIR)/$(BIN_PREFIX).elf
BIN := $(BUILD_DIR)/$(BIN_PREFIX).bin
MAP := $(BUILD_DIR)/$(BIN_PREFIX).map
DISASM := $(BUILD_DIR)/$(BIN_PREFIX).dis

# Linker script selection
LDSCRIPT=link/linker.ld

# Flags
COMMON_FLAGS := -mthumb -mfloat-abi=hard -ffast-math -fshort-enums
COMMON_FLAGS += -mcpu=cortex-m33 -mfpu=fpv5-sp-d16

SEGGER_RTT_FLAGS := -DBUFFER_SIZE_UP=3584
CFLAGS := $(EXTRA_C_FLAGS) $(COMMON_FLAGS) $(SEGGER_RTT_FLAGS) -Werror \
          -I include -I segger-rtt/RTT -I segger-rtt/Config \
		  -I apio/include -I epio/include -I ora -I generated \
		  -g -nostdlib -O3 -ffunction-sections -fomit-frame-pointer -fdata-sections \
		  -Wall -Wextra -MMD -MP -fstack-usage \
		  -DONEROM_VERSION_MAJOR=$(VERSION_MAJOR) -DONEROM_VERSION_MINOR=$(VERSION_MINOR) \
		  -DONEROM_VERSION_PATCH=$(VERSION_PATCH) -DONEROM_BUILD_NUMBER=$(BUILD_NUMBER) \
		  -DONEROM_GIT_COMMIT=\"$(GIT_COMMIT)\" 
LDFLAGS := $(COMMON_FLAGS) -Werror -Llink -nostdlib -specs=nosys.specs -specs=nano.specs -Wl,--no-gc-sections -Wl,--fatal-warnings -Wl,-Map=$(MAP)

ifeq ($(PLUGIN_LOGGING), 1)
	CFLAGS += -DPLUGIN_LOGGING=1
endif
ifeq ($(DEBUG_LOGGING), 1)
	CFLAGS += -DDEBUG_LOGGING=1
endif

# Phony targets
.PHONY: all clean clean-segger clean-apio-src segger segger-rtt apio size flash run

# Default target
all: $(BIN) size

# Create build directory
$(BUILD_DIR):
	mkdir -p $@

# Clone the  repo
segger-rtt:
	@if [ ! -d "$@" ]; then \
		git clone https://github.com/piersfinlayson/segger-rtt.git; \
	fi

apio:
	@if [ ! -d "$@" ]; then \
		git -c advice.detachedHead=false clone --quiet --depth 1 --branch $(APIO_VERSION) https://github.com/piersfinlayson/apio.git; \
	fi

# sdrr object files
$(BUILD_DIR)/%.o: src/%.c ../Makefile | $(BUILD_DIR) segger-rtt apio
	@mkdir -p $(@D)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

# Explicit rules for SEGGER files
$(BUILD_DIR)/SEGGER_RTT.o: segger-rtt/RTT/SEGGER_RTT.c ../Makefile  | $(BUILD_DIR)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/SEGGER_RTT_printf.o: segger-rtt/RTT/SEGGER_RTT_printf.c ../Makefile | $(BUILD_DIR)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: apio/src/%.c ../Makefile | $(BUILD_DIR)
	@mkdir -p $(@D)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

segger-rtt/RTT/SEGGER_RTT.c segger-rtt/RTT/SEGGER_RTT_printf.c: segger-rtt

# Link
$(ELF): $(OBJS) $(SEGGER_OBJS) $(LDSCRIPT) | $(BUILD_DIR)
	@echo "- Linking object files to create ELF -> $(FIRMWARE_DIR)/$@"
	@$(CC) $(LDFLAGS) -T $(LDSCRIPT) $(OBJS) $(SEGGER_OBJS) -o $@

# Generate binary
$(BIN): $(ELF)
	@echo "- Generating binary from $(FIRMWARE_DIR)/$< -> $(FIRMWARE_DIR)/$@"
	@$(OBJCOPY) -O binary $(OBJCOPY_EXTRA) --gap-fill=0xff $< $@
	@$(OBJDUMP) -D -m arm -S -t -h $< > $(DISASM)
	@echo "----"
	@echo "Build complete:"

# Size information
size: $(ELF)
	@$(SIZE) $(ELF)
	@ls -ltr $(ELF) 
	@ls -ltr $(BIN) 

# Clean segger
clean-segger-src:
	rm -rf segger-rtt/

clean-apio-src:
	rm -rf apio/

# Clean build
clean-build:
	rm -fr $(BUILD_DIR)

# Clean
clean: clean-build clean-segger-src clean-apio-src
	rm generated/* 2>/dev/null || true

# Include dependency files
-include $(shell find $(BUILD_DIR) -name '*.d' 2>/dev/null)