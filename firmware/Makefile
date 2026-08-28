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
APIO_VERSION ?= v0.3.0

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
SRCS := src/constants.c src/binary_info.c src/build_date.c \
		src/globals.c src/log.c src/rtt.c \
		src/main.c src/plugin.c src/utils.c \
		src/vector.c src/rp235x.c src/piodma/pio.c \
		src/piodma/piorom2.c src/piodma/pioram.c src/piodma/dma.c \
		src/piodma/pioplugin.c src/piodma/pioled.c
OBJS := $(patsubst src/%,$(BUILD_DIR)/%,$(SRCS:.c=.o))

# Output files
BIN_PREFIX ?= onerom-rp235x
ELF := $(BUILD_DIR)/$(BIN_PREFIX).elf
BIN := $(BUILD_DIR)/$(BIN_PREFIX).bin
MAP := $(BUILD_DIR)/$(BIN_PREFIX).map
DISASM := $(BUILD_DIR)/$(BIN_PREFIX).dis

# Linker script selection
LDSCRIPT=link/linker.ld

# Flags
#
# Built soft-float, deliberately.  Nothing in the firmware does floating point
# arithmetic, but with the FPU enabled the compiler uses a double register as an
# 8-byte move - zeroing two adjacent uint64_t in setup_sel_pins was enough - and
# the first such instruction sets CONTROL.FPCA on that core for good.  From then
# on every interrupt pushes the extended exception frame, 104 bytes instead of
# 32, out of a plugin stack that is measured in hundreds.  Re-enabling
# -mfloat-abi=hard and -mfpu costs those 72 bytes on every interrupt and buys
# nothing.
COMMON_FLAGS := -mthumb -ffast-math -fshort-enums
COMMON_FLAGS += -mcpu=cortex-m33

CFLAGS := $(EXTRA_C_FLAGS) $(COMMON_FLAGS) -Werror \
          -I include \
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

# Rebuild every object when the compiler flags change.
#
# An object rule cannot depend on CFLAGS, so the flags are recorded in a stamp
# file and the objects depend on that.  The file is rewritten only when the
# flags differ, so an unchanged build does not retrigger.  This must come after
# every CFLAGS assignment above.
#
# The objects used to depend on the top level Makefile instead, which was both
# too much and too little - it rebuilt on any edit to that file, and missed a
# flag given on the command line, so PLUGIN_LOGGING=1 on a built tree silently
# produced the previous binary.
CFLAGS_STAMP := $(BUILD_DIR)/.cflags
$(shell mkdir -p $(BUILD_DIR); \
        printf '%s' '$(CFLAGS)' | cmp -s - $(CFLAGS_STAMP) || \
        printf '%s' '$(CFLAGS)' > $(CFLAGS_STAMP))

# Phony targets
.PHONY: all clean clean-apio-src apio size flash run

# Default target
all: $(BIN) size

# Create build directory
$(BUILD_DIR):
	mkdir -p $@

apio:
	@if [ ! -d "$@" ]; then \
		git -c advice.detachedHead=false clone --quiet --depth 1 --branch $(APIO_VERSION) https://github.com/piersfinlayson/apio.git; \
	fi

# The build date stamp.
#
# Depending on every other object is what makes the date honest: this is
# recompiled when, and only when, something else was, so the stamp is the
# moment the firmware was actually built rather than whenever this file last
# happened to compile.
#
# The date is given here rather than in CFLAGS on purpose.  CFLAGS is recorded
# in $(CFLAGS_STAMP) and every object depends on it, so a date in there would
# differ on every invocation and rebuild the whole firmware each time.
$(BUILD_DIR)/build_date.o: $(filter-out $(BUILD_DIR)/build_date.o,$(OBJS))
$(BUILD_DIR)/build_date.o: CFLAGS += -DONEROM_BUILD_DATE='"$(shell date -u +'%b %e %Y %H:%M:%SZ')"'

# sdrr object files
$(BUILD_DIR)/%.o: src/%.c $(CFLAGS_STAMP) | $(BUILD_DIR) apio
	@mkdir -p $(@D)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: apio/src/%.c $(CFLAGS_STAMP) | $(BUILD_DIR)
	@mkdir -p $(@D)
	@echo "- Compiling $(FIRMWARE_DIR)/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

# Link
$(ELF): $(OBJS) $(LDSCRIPT) | $(BUILD_DIR)
	@echo "- Linking object files to create ELF -> $(FIRMWARE_DIR)/$@"
	@$(CC) $(LDFLAGS) -T $(LDSCRIPT) $(OBJS) -o $@

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

clean-apio-src:
	rm -rf apio/

# Clean build
clean-build:
	rm -fr $(BUILD_DIR)

# Clean
clean: clean-build clean-apio-src
	rm generated/* 2>/dev/null || true

# Include dependency files
-include $(shell find $(BUILD_DIR) -name '*.d' 2>/dev/null)