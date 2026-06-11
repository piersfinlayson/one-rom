# One ROM Test Makefile
# For native test builds only
MAKEFLAGS += --no-builtin-rules --no-builtin-variables

CC := gcc
COLOUR_YELLOW := $(shell echo -e '\033[33m')
COLOUR_RESET := $(shell echo -e '\033[0m')

BUILD_DIR := build-test
BIN := $(BUILD_DIR)/onerom-test

# Output directory from sdrr-gen
GEN_OUTPUT_DIR ?= output
OUTPUT_DIR := ../$(GEN_OUTPUT_DIR)

# Include generated config
ifneq ($(wildcard $(OUTPUT_DIR)/generated.mk),)
  include $(OUTPUT_DIR)/generated.mk
else
  $(error sdrr-gen generated.mk not found. Run sdrr-gen first.)
endif

# Source files
SRCS := src/constants.c src/main.c src/rom_impl.c src/test.c src/utils.c \
        src/vector.c src/stm32f4.c src/rp235x.c src/piodma/pio.c \
        src/piodma/piorom.c src/piodma/pioram.c src/piodma/dma.c \
        src/plugin.c \
        test/stub_rp235x.c test/test_main.c test/test_log.c \
        test/test_image.c test/test_gpio.c
OBJS := $(patsubst src/%.c,$(BUILD_DIR)/%.o,$(filter src/%,$(SRCS)))
OBJS += $(patsubst test/%.c,$(BUILD_DIR)/%.o,$(filter test/%,$(SRCS)))

# Generated files
ROMS_SRC := $(OUTPUT_DIR)/roms.c
ROMS_OBJ := $(BUILD_DIR)/roms.o
SDRR_CONFIG_SRC := $(OUTPUT_DIR)/sdrr_config.c
SDRR_CONFIG_OBJ := $(BUILD_DIR)/sdrr_config.o

GIT_COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Compile flags:
# - fsanitize=address -fno-omit-frame-pointer for debug builds
# - fshort-enums to ensure enums the same size as in firmware
CFLAGS := -DAPIO_EMULATION=1 -DTEST_BUILD=1 \
			$(EXTRA_C_FLAGS) -I include -I $(OUTPUT_DIR) -I include/test \
			-I apio/include -I epio/include -I ora \
			-DSDRR_VERSION_MAJOR=$(VERSION_MAJOR) -DSDRR_VERSION_MINOR=$(VERSION_MINOR) \
			-DSDRR_VERSION_PATCH=$(VERSION_PATCH) -DSDRR_BUILD_NUMBER=$(BUILD_NUMBER) \
			-DSDRR_GIT_COMMIT=\"$(GIT_COMMIT)\" \
			-DBOOT_LOGGING=1 -DDEBUG_LOGGING=1 \
			-g -O0 -Wall -Wextra -Werror -ffunction-sections -fdata-sections \
			-MMD -MP -fshort-enums 
#			-fsanitize=address -fno-omit-frame-pointer

# Linker flags:
# - fsanitize=address for debug builds
# - segalign 0x80000 to allow 512KB alignment (for ROM RAM table)
# - no_fixup_chains to make the 512KB alignement work on macOS
# - no_pie to avoid position independent executable which breaks alignment on macOS
LDFLAGS := 
#			-g -fsanitize=address 

# Targets
.PHONY: all clean run debug clean-apio-src apio clean-epio-src epio-src epio

all: $(BIN)
	@echo "Running One ROM test\n-----"
	@$(BIN)

apio:
	@if [ ! -d "apio" ]; then \
		git clone https://github.com/piersfinlayson/apio.git; \
	fi

epio-src:
	@if [ ! -d "epio" ]; then \
		git clone https://github.com/piersfinlayson/epio.git; \
	fi

epio: epio-src
	@$(MAKE) -C epio

$(BUILD_DIR):
	@mkdir -p $@

$(BUILD_DIR)/%.o: src/%.c | $(BUILD_DIR) apio
	@mkdir -p $(@D)
	@echo "- Compiling $<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: test/%.c | $(BUILD_DIR) epio
	@mkdir -p $(@D)
	@echo "- Compiling $<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(ROMS_OBJ): $(ROMS_SRC) | $(BUILD_DIR)
	@echo "- Compiling $(ROMS_SRC)"
	@$(CC) $(CFLAGS) -c $< -o $@

$(SDRR_CONFIG_OBJ): $(SDRR_CONFIG_SRC) | $(BUILD_DIR)
	@echo "- Compiling $(SDRR_CONFIG_SRC)"
	@$(CC) $(CFLAGS) -c $< -o $@

$(BIN): $(OBJS) $(ROMS_OBJ) $(SDRR_CONFIG_OBJ) | epio
	@echo "- Linking test"
	@$(CC) $(LDFLAGS) $^ -L epio/build -lepio -o $@

clean-apio-src:
	@rm -rf apio/

clean-epio-src:
	@rm -rf epio/

clean: clean-apio-src clean-epio-src
	@rm -rf $(BUILD_DIR)

-include $(OBJS:.o=.d) $(ROMS_OBJ:.o=.d) $(SDRR_CONFIG_OBJ:.o=.d)
