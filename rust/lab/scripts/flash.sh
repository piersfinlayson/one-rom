#!/bin/bash
# Flash a One ROM reader board.
#
# Usage: flash.sh [--serial SERIAL] [board]
#
# board:  a full board name (e.g. fire-28-c, fire-32-a)
# SERIAL: the board's USB serial number, which `onerom scan` displays.  It's
#         required where another device with the same USB ID is connected,
#         for example a One ROM with the USB plugin.

set -e

usage() {
    echo "Usage: $0 [--serial SERIAL] [board]"
    echo ""
    echo "  board   <full board name>"
    echo "  SERIAL  the board's USB serial number"
    echo ""
    echo "Examples:"
    echo "  $0"
    echo "  $0 fire-28-a"
    echo "  $0 --serial 62CD9AE3C0771A7E fire-28-a"
    exit 1
}

SER=()
BOARD=
while [ $# -gt 0 ]; do
    case $1 in
        --serial)
            [ -n "${2:-}" ] || usage
            SER=(--ser "$2")
            shift 2
            ;;
        *)
            [ -z "$BOARD" ] || usage
            BOARD=$1
            shift
            ;;
    esac
done
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

# The reboot fails for a board already in its bootloader, so its error is
# displayed only where no bootloader is found.
REBOOT_ERROR=
if REBOOT_ERROR=$(picotool reboot -u "${RUNNING[@]}" "${SER[@]}" 2>&1); then
    echo "Rebooted the running board into its bootloader"
fi

# The bootloader takes a moment to appear after a reboot.
FOUND=
for _ in $(seq 1 10); do
    if picotool info "${COMMISSIONED[@]}" "${SER[@]}" > /dev/null 2>&1; then
        FOUND=commissioned
        break
    fi
    if picotool info "${SER[@]}" > /dev/null 2>&1; then
        FOUND=stock
        break
    fi
    sleep 0.5
done

case "$FOUND" in
    commissioned) BOOTLOADER=("${COMMISSIONED[@]}" "${SER[@]}") ;;
    stock) BOOTLOADER=("${SER[@]}") ;;
    *)
        echo "No board found in its bootloader.  Hold BOOTSEL while connecting it."
        if [ -n "$REBOOT_ERROR" ]; then
            echo "Rebooting the board into its bootloader failed:"
            awk '{ print "  " $0 }' <<<"$REBOOT_ERROR"
            echo "With more than one device connected, use --serial."
        fi
        exit 1
        ;;
esac

picotool load "$ELF" -t elf "${BOOTLOADER[@]}"
picotool reboot "${BOOTLOADER[@]}"
