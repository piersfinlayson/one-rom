# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License
#
# Examples Makefile
#
# Builds all One ROM plugin examples.  Each example lives in its own
# subdirectory containing a plugin_main.c source file.
#
# Usage:
#   make

PLUGIN_NAME := user/neo
PLUGIN_SRC := src/neo_main.c
PLUGIN_TYPE := USER
PLUGIN_SRC_DIR := src

BUILD_DIR := build
ORA_DIR := ../../../sdrr/ora

PLUGIN_MK := $(ORA_DIR)/plugin.mk

.PHONY: help all clean distclean build_dir $(EXAMPLES)

all: plugin

plugin: build_dir
	@echo "Building $(PLUGIN_NAME) plugin in $@..."
	@$(MAKE) -f $(PLUGIN_MK) \
		SRC=$(CURDIR)/$(PLUGIN_SRC) \
		BUILD_DIR=$(CURDIR)/$(BUILD_DIR) \
		ORA_INCLUDE=$(CURDIR)/$(ORA_DIR) \
		PLUGIN_TYPE=$(PLUGIN_TYPE)

build_dir: 
	@echo "Creating build directory $(BUILD_DIR) for $(PLUGIN_NAME) plugin..."
	@mkdir -p $(BUILD_DIR)

clean:
	@echo "Cleaning all build artifacts..."
	@rm -fr $(BUILD_DIR)

distclean: clean

help:
	@echo "Usage:"
	@echo "  make            - build plugin"
	@echo "  make distclean  - remove all build artifacts"
