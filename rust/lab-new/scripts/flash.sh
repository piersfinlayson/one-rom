#!/bin/bash
# Flash a One ROM reader board.
#
# Usage: flash.sh [board]
#
# board: a full board name (e.g. fire-28-c, fire-32-a)

set -e

if [ $# -gt 1 ]; then
    echo "Usage: $0 [board]"
    echo ""
    echo "  board  <full board name>"
    echo ""
    echo "Examples:"
    echo "  $0"
    echo "  $0 fire-28-a"
    exit 1
fi
BOARD="$1"
echo "Board:   $BOARD"

# --- Build env var array and flash ---

ENV_VARS=("BOARD=$BOARD")

env "${ENV_VARS[@]}" cargo build --release
picotool load -t elf ../target/thumbv8m.main-none-eabihf/release/onerom-lab-fire
picotool reboot