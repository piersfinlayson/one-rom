#!/bin/bash
set -e

# Copies the built base firmware to the mounted output directory. Run from the
# repo root (i.e. after `cd one-rom`), following a `make` build.
cp firmware/build/onerom-rp235x.* /home/build/output/
