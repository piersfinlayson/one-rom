# Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
#
# MIT License
#
# Tests Makefile
#
# Builds all One ROM plugin API hardware tests.  Each test lives in its own
# subdirectory containing a plugin_main.c source file.
#
# Usage:
#   make                        - build all tests as user plugins
#   make PLUGIN_TYPE=SYSTEM     - build all tests as system plugins
#   make clean                  - remove all build artifacts

PLUGIN_MK := ../plugin.mk

TESTS := $(patsubst %/plugin_main.c,%,$(wildcard */plugin_main.c))

.PHONY: help all clean distclean $(TESTS)

all: $(TESTS)

$(TESTS):
	@echo "Building test plugin in $@..."
	@$(MAKE) -f $(PLUGIN_MK) \
		SRC=$(CURDIR)/$@/plugin_main.c \
		BUILD_DIR=$(CURDIR)/$@/build \
		ORA_INCLUDE=$(CURDIR)/.. \
		EXTRA_C_FLAGS="-I $(CURDIR)/../../include -I $(CURDIR)/../../apio/include" \
		$(if $(PLUGIN_TYPE),PLUGIN_TYPE=$(PLUGIN_TYPE),)

clean:
	@echo "Cleaning all build artifacts..."
	@for d in $(TESTS); do \
		$(MAKE) -f $(PLUGIN_MK) BUILD_DIR=$(CURDIR)/$$d/build clean; \
	done

distclean: clean

help:
	@echo "Usage:"
	@echo "  make                        - build all tests as user plugins"
	@echo "  make PLUGIN_TYPE=SYSTEM     - build all tests as system plugins"
	@echo "  make clean                  - remove all build artifacts"
