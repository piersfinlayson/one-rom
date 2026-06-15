# sdrr-gen Output directory
GEN_OUTPUT_DIR ?= output
OUTPUT_DIR := ../$(GEN_OUTPUT_DIR)

CC = gcc

# Note the output -D'__attribute__(x)=' is to neutralise section headers in
# the auto-generated source files, which macOS doesn't like.
CFLAGS = $(EXTRA_C_FLAGS) -DTEST_BUILD -Werror -Wall -Wextra -Wimplicit-fallthrough -g -I include -I../sdrr/include -I../output -I../output -D'__attribute__(x)=' -fshort-enums
LDFLAGS = -lcurl -lzip -ljson-c
TARGET = image-test
BUILD_DIR = build

SRCS = src/main.c src/check-roms.c src/roms-config.c src/query-roms.c src/json-config.c
OBJS := $(addprefix $(BUILD_DIR)/,$(notdir $(SRCS:.c=.o)))
ROMS_SRC := $(OUTPUT_DIR)/roms.c
ROMS_OBJ := $(BUILD_DIR)/roms.o

# macOS homebrew support
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    BREW_PREFIX := $(shell brew --prefix)
    CFLAGS += -I$(BREW_PREFIX)/include
    LDFLAGS += -L$(BREW_PREFIX)/lib
endif

# Convenience target
all: $(BUILD_DIR)/$(TARGET)

$(BUILD_DIR)/%.o: src/%.c include/roms-test.h include/json-config.h $(OUTPUT_DIR)/roms.h $(OUTPUT_DIR)/sdrr_config.h | $(BUILD_DIR)
	@echo "- Compiling test/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

# roms.o is built from roms.c from a different directory
$(ROMS_OBJ): $(ROMS_SRC) ../Makefile | $(BUILD_DIR)
	@echo "- Compiling test/$<"
	@$(CC) $(CFLAGS) -c $< -o $@

# Create build directory and build target
$(BUILD_DIR)/$(TARGET): $(OBJS) $(ROMS_OBJ) | $(BUILD_DIR)
	@echo "- Linking object files to create test binary -> test/$@"
	@$(CC) $(CFLAGS) -o $@ $(OBJS) $(ROMS_OBJ) $(LDFLAGS)

# Create build directory
$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

# Clean up
clean:
	rm -rf $(BUILD_DIR)

# Run tests
test: $(BUILD_DIR)/$(TARGET)
	@echo "- Running tests..."
	@./$(BUILD_DIR)/$(TARGET)

# Force rebuild if output files change
../output/roms.c ../output/roms.h:
	@echo "ROM files missing - run sdrr-gen first"
	@false

.PHONY: clean
