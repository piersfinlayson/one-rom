# One ROM Test Makefile
# For native test builds only
MAKEFLAGS += --no-builtin-rules --no-builtin-variables

include epio/wasm/exports.mk

COLOUR_YELLOW := $(shell echo -e '\033[33m')
COLOUR_RESET := $(shell echo -e '\033[0m')

# Output directory from sdrr-gen
GEN_OUTPUT_DIR ?= output
OUTPUT_DIR := ../$(GEN_OUTPUT_DIR)

# Include generated config
ifneq ($(wildcard $(OUTPUT_DIR)/generated.mk),)
  include $(OUTPUT_DIR)/generated.mk
else
  $(error sdrr-gen generated.mk not found. Run sdrr-gen first.)
endif

# WASM build configuration
LENS_BUILD_DIR := build-lens
WASM_CC := emcc
WASM_BIN := $(LENS_BUILD_DIR)/onerom-lens.html

# Source files
SRCS := src/constants.c src/main.c src/rom_impl.c src/test.c src/utils.c \
        src/vector.c src/stm32f4.c src/rp235x.c src/piodma/pio.c \
        src/piodma/piorom.c src/piodma/pioram.c src/piodma/dma.c \
        test/stub_rp235x.c test/test_log.c \
		lens/src/lens_main.c

# WASM object files (same sources, different build dir)
WASM_OBJS := $(patsubst src/%.c,$(LENS_BUILD_DIR)/%.o,$(filter src/%,$(SRCS)))
WASM_OBJS += $(patsubst test/%.c,$(LENS_BUILD_DIR)/%.o,$(filter test/%,$(SRCS)))
WASM_OBJS += $(patsubst lens/src/%.c,$(LENS_BUILD_DIR)/%.o,$(filter lens/src/%,$(SRCS)))
WASM_ROMS_OBJ := $(LENS_BUILD_DIR)/roms.o
WASM_SDRR_CONFIG_OBJ := $(LENS_BUILD_DIR)/sdrr_config.o

# Generated files
ROMS_SRC := $(OUTPUT_DIR)/roms.c
ROMS_OBJ := $(BUILD_DIR)/roms.o
SDRR_CONFIG_SRC := $(OUTPUT_DIR)/sdrr_config.c
SDRR_CONFIG_OBJ := $(BUILD_DIR)/sdrr_config.o

# Web files
WEB_FILES := lens/web/index.html lens/web/logic-analyzer.js lens/web/style.css

VERSION_MAJOR := 0
VERSION_MINOR := 6
VERSION_PATCH := 4
BUILD_NUMBER := 0
GIT_COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# WASM-specific flags (no sanitizer, add EPIO_WASM define)
WASM_CFLAGS := -DAPIO_EMULATION=1 -DTEST_BUILD=1 -DONEROM_LENS \
			$(EXTRA_C_FLAGS) -I include -I $(OUTPUT_DIR) -I include/test \
			-I apio/include -I epio/include -I ora \
			-DSDRR_VERSION_MAJOR=$(VERSION_MAJOR) -DSDRR_VERSION_MINOR=$(VERSION_MINOR) \
			-DSDRR_VERSION_PATCH=$(VERSION_PATCH) -DSDRR_BUILD_NUMBER=$(BUILD_NUMBER) \
			-DSDRR_GIT_COMMIT=\"$(GIT_COMMIT)\" \
			-DBOOT_LOGGING=1 -DDEBUG_LOGGING=1 \
			-g -O0 -Wall -Wextra -Werror -ffunction-sections -fdata-sections \
			-MMD -MP -fshort-enums

ONEROM_WASM_EXPORTS := "_onerom_init","_onerom_drive_pins","_onerom_release_pins",\
"_onerom_read_data","_onerom_get_addr_pin","_onerom_get_data_pin",\
"_onerom_get_cs1_pin","_onerom_get_cs2_pin","_onerom_get_cs3_pin",\
"_onerom_get_x1_pin","_onerom_get_x2_pin","_onerom_get_pio_disassembly",\
"_onerom_lens_get_rom_size","_onerom_lens_get_num_data_bits",\
"_onerom_lens_get_num_addr_bits","_onerom_lens_get_rom_type",\
"_onerom_drive_addr","_onerom_get_ce_pin","_onerom_get_oe_pin"\
,"_onerom_get_byte_pin"

# Emscripten linker flags
WASM_LDFLAGS := -s WASM=1 \
				-s EXPORTED_RUNTIME_METHODS='["ccall","cwrap","UTF8ToString"]' \
				-s EXPORTED_FUNCTIONS='[$(EPIO_WASM_EXPORTS)]' \
				-s EXPORTED_FUNCTIONS='[$(EPIO_WASM_EXPORTS),$(ONEROM_WASM_EXPORTS)]' \
				-s ALLOW_MEMORY_GROWTH=1

# Targets
.PHONY: all clean run debug web copy-web serve clean-apio-src apio epio-src epio clean-epio-src

all:  epio/wasm/exports.mk $(WASM_BIN) copy-web
	@echo "WASM build complete: $(WASM_BIN)"

# Copy web files to build directory
copy-web: $(WEB_FILES)
	@echo "- Copying web files to $(LENS_BUILD_DIR)"
	@cp lens/web/index.html $(LENS_BUILD_DIR)/
	@cp lens/web/logic-analyzer.js $(LENS_BUILD_DIR)/
	@cp lens/web/style.css $(LENS_BUILD_DIR)/

# Alias for copy-web
web: copy-web

# Alias for serve
run: serve

# Run web server
serve: all
	@echo "$(COLOUR_YELLOW)Starting web server on http://localhost:8000 $(COLOUR_RESET)"
	@echo "$(COLOUR_YELLOW)Open http://localhost:8000/index.html in your browser $(COLOUR_RESET)"
	@python3 -m http.server -d $(LENS_BUILD_DIR) 8000

apio:
	@if [ ! -d "$@" ]; then \
		git clone https://github.com/piersfinlayson/apio.git; \
	fi

epio-src:
	@if [ ! -d "epio" ]; then \
		git clone https://github.com/piersfinlayson/epio.git; \
	fi

epio/build/wasm/libepio.a: epio

epio:
	@$(MAKE) -C epio wasm

epio/wasm/exports.mk: | epio-src
	@$(MAKE) -C epio wasm

epio/wasm/exports.mk: epio-src

$(LENS_BUILD_DIR):
	@mkdir -p $@

$(LENS_BUILD_DIR)/%.o: src/%.c | $(LENS_BUILD_DIR) apio
	@mkdir -p $(@D)
	@echo "- Compiling WASM $<"
	@$(WASM_CC) $(WASM_CFLAGS) -c $< -o $@

$(LENS_BUILD_DIR)/%.o: test/%.c | $(LENS_BUILD_DIR) apio
	@mkdir -p $(@D)
	@echo "- Compiling WASM $<"
	@$(WASM_CC) $(WASM_CFLAGS) -c $< -o $@

$(LENS_BUILD_DIR)/%.o: lens/src/%.c | $(LENS_BUILD_DIR) apio
	@mkdir -p $(@D)
	@echo "- Compiling WASM $<"
	@$(WASM_CC) $(WASM_CFLAGS) -c $< -o $@

$(WASM_ROMS_OBJ): $(ROMS_SRC) | $(LENS_BUILD_DIR)
	@echo "- Compiling WASM $(ROMS_SRC)"
	@$(WASM_CC) $(WASM_CFLAGS) -c $< -o $@

$(WASM_SDRR_CONFIG_OBJ): $(SDRR_CONFIG_SRC) | $(LENS_BUILD_DIR)
	@echo "- Compiling WASM $(SDRR_CONFIG_SRC)"
	@$(WASM_CC) $(WASM_CFLAGS) -c $< -o $@

$(WASM_BIN): $(WASM_OBJS) $(WASM_ROMS_OBJ) $(WASM_SDRR_CONFIG_OBJ) epio/build/wasm/libepio.a
	@echo "- Linking WASM"
	@$(WASM_CC) $(WASM_LDFLAGS) $^ -L epio/build/wasm -l epio -o $@

clean-apio-src:
	@echo "- Cleaning apio source"
	@rm -rf apio/

clean-epio-src:
	@echo "- Cleaning epio source"
	@rm -rf epio/

clean: clean-apio-src clean-epio-src
	@echo "- Cleaning WASM build"
	@rm -rf $(BUILD_DIR)

-include $(WASM_OBJS:.o=.d) $(WASM_ROMS_OBJ:.o=.d) $(WASM_SDRR_CONFIG_OBJ:.o=.d)