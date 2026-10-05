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

ELF=../target/thumbv8m.main-none-eabihf/release/onerom-lab-fire

# A running Lab, or One ROM with the USB plugin.
RUNNING=(--vid 0x1209 --pid 0xf542)

# A commissioned board's bootloader.  A board that hasn't been commissioned
# appears as a stock RP2350, which picotool finds without arguments.
COMMISSIONED=(--vid 0x1209 --pid 0xf540)

# --- Build ---

ENV_VARS=("BOARD=$BOARD")

env "${ENV_VARS[@]}" cargo build --release

# --- Reboot into the bootloader and flash ---

if picotool reboot -u "${RUNNING[@]}" > /dev/null 2>&1; then
    echo "Rebooted the running board into its bootloader"
fi

# The bootloader takes a moment to appear after a reboot.
FOUND=
for _ in $(seq 1 10); do
    if picotool info "${COMMISSIONED[@]}" > /dev/null 2>&1; then
        FOUND=commissioned
        break
    fi
    if picotool info > /dev/null 2>&1; then
        FOUND=stock
        break
    fi
    sleep 0.5
done

case "$FOUND" in
    commissioned) BOOTLOADER=("${COMMISSIONED[@]}") ;;
    stock) BOOTLOADER=() ;;
    *)
        echo "No board found in its bootloader.  Hold BOOTSEL while connecting it."
        exit 1
        ;;
esac

picotool load "$ELF" -t elf "${BOOTLOADER[@]}"
picotool reboot "${BOOTLOADER[@]}"
