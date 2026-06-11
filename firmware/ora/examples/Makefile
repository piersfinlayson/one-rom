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
#   make                        - build all examples as user plugins
#   make PLUGIN_TYPE=SYSTEM     - build all examples as system plugins
#   make clean                  - remove all build artifacts

PLUGIN_MK := ../plugin.mk

EXAMPLES := $(patsubst %/plugin_main.c,%,$(wildcard */plugin_main.c))

.PHONY: help all clean distclean $(EXAMPLES)

all: $(EXAMPLES)

$(EXAMPLES):
	@echo "Building example plugin in $@..."
	@$(MAKE) -f $(PLUGIN_MK) \
		SRC=$(CURDIR)/$@/plugin_main.c \
		BUILD_DIR=$(CURDIR)/$@/build \
		ORA_INCLUDE=$(CURDIR)/.. \
		EXTRA_C_FLAGS="-I $(CURDIR)/../../include -I $(CURDIR)/../../apio/include" \
		$(if $(PLUGIN_TYPE),PLUGIN_TYPE=$(PLUGIN_TYPE),)

clean:
	@echo "Cleaning all build artifacts..."
	@for d in $(EXAMPLES); do \
		$(MAKE) -f $(PLUGIN_MK) BUILD_DIR=$(CURDIR)/$$d/build clean; \
	done

distclean: clean

help:
	@echo "Usage:"
	@echo "  make                        - build all examples as user plugins"
	@echo "  make PLUGIN_TYPE=SYSTEM     - build all examples as system plugins"
	@echo "  make clean                  - remove all build artifacts"
