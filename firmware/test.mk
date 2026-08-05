# One ROM Test Makefile
# For native test builds only
MAKEFLAGS += --no-builtin-rules --no-builtin-variables

# Versions of apio and epio to build against.  Remove the apio/ or epio/
# directory after changing these, so it is re-cloned.  Note that epio pins its
# own apio version for its internal build, in epio/Makefile.
APIO_VERSION ?= v0.2.0
EPIO_VERSION ?= v0.2.0

# Build mode.  WASM=1 cross-compiles the library to WebAssembly with the
# Emscripten toolchain (for One ROM Lens); the default is a native host build.
# In WASM mode we compile with emcc, link against epio's wasm build, and archive
# with emar (llvm-ar) — the host libtool/ar cannot handle wasm objects.  Callers
# should also pass a distinct BUILD_DIR (e.g. build-wasm) so native and wasm
# objects do not clash.
WASM ?= 0
ifeq ($(WASM),1)
  CC := emcc
  EPIO_MAKE_TARGET := wasm
  EPIO_LIB := epio/build/wasm/libepio.a
  AR_EXTRACT := emar x
  AR_COMBINE := emar rcs
else
  CC := gcc
  EPIO_MAKE_TARGET :=
  EPIO_LIB := epio/build/libepio.a
  ifeq ($(shell uname -s),Darwin)
    AR_EXTRACT := ar x
    AR_COMBINE := libtool -static -o
  else
    AR_EXTRACT := ar x
    AR_COMBINE := ar rcs
  endif
endif

COLOUR_YELLOW := $(shell echo -e '\033[33m')
COLOUR_RESET := $(shell echo -e '\033[0m')

BUILD_DIR := build-test
LIB := $(BUILD_DIR)/libonerom-test.a

# Source files
SRCS := src/constants.c src/globals.c src/log.c \
		src/main.c src/plugin.c src/utils.c \
		src/vector.c src/rp235x.c src/piodma/pio.c \
		src/piodma/piorom2.c src/piodma/pioram.c src/piodma/dma.c \
		src/piodma/pioplugin.c \
        test/stub_rp235x.c test/ffi.c \
		generated/gen-config.c
OBJS := $(patsubst src/%.c,$(BUILD_DIR)/%.o,$(filter src/%,$(SRCS)))
OBJS += $(patsubst test/%.c,$(BUILD_DIR)/%.o,$(filter test/%,$(SRCS)))
OBJS += $(patsubst generated/%.c,$(BUILD_DIR)/%.o,$(filter generated/%,$(SRCS)))

GIT_COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Compile flags:
# - fsanitize=address -fno-omit-frame-pointer for debug builds
# - fshort-enums to ensure enums the same size as in firmware
CFLAGS := -DAPIO_EMULATION=1 -DTEST_BUILD=1 \
			$(EXTRA_C_FLAGS) -I include -I generated -I include/test \
			-I apio/include -I epio/include -I ora \
			-DONEROM_VERSION_MAJOR=$(VERSION_MAJOR) -DONEROM_VERSION_MINOR=$(VERSION_MINOR) \
			-DONEROM_VERSION_PATCH=$(VERSION_PATCH) -DONEROM_BUILD_NUMBER=$(BUILD_NUMBER) \
			-DONEROM_GIT_COMMIT=\"$(GIT_COMMIT)\" \
			-DBOOT_LOGGING=1 -DDEBUG_LOGGING=1 -DPLUGIN_LOGGING=1 \
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
.PHONY: all clean run debug clean-apio-src apio clean-epio-src epio-src clean-test epio

all: $(LIB)

apio:
	@if [ ! -d "apio" ]; then \
		git -c advice.detachedHead=false clone --quiet --depth 1 --branch $(APIO_VERSION) https://github.com/piersfinlayson/apio.git; \
	fi

epio-src:
	@if [ ! -d "epio" ]; then \
		git -c advice.detachedHead=false clone --quiet --depth 1 --branch $(EPIO_VERSION) https://github.com/piersfinlayson/epio.git; \
	fi

epio: epio-src
	@$(MAKE) -C epio $(EPIO_MAKE_TARGET)

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

$(BUILD_DIR)/%.o: generated/%.c | $(BUILD_DIR)
	@mkdir -p $(@D)
	@echo "- Compiling $<"
	@$(CC) $(CFLAGS) -c $< -o $@

$(LIB): $(OBJS) | epio
	@echo "- Archiving library"
	@mkdir -p $(BUILD_DIR)/epio-objs
	@cd $(BUILD_DIR)/epio-objs && $(AR_EXTRACT) $(CURDIR)/$(EPIO_LIB)
	@$(AR_COMBINE) $@ $^ $(BUILD_DIR)/epio-objs/*.o

clean-apio-src:
	@rm -rf apio/

clean-epio-src:
	@rm -rf epio/

clean-test:
	@rm -rf $(BUILD_DIR)

clean: clean-test clean-apio-src clean-epio-src

-include $(OBJS:.o=.d)
