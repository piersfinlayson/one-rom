#!/bin/bash
# Flash a One ROM reader board.
#
# Usage: flash.sh <board_or_pins> <chip_type> [--cs1 high|low] [--cs2 high|low] [--cs3 high|low]
#
# board_or_pins: pin count shorthand (24, 28, 32, 40) or a full board name
#                (e.g. fire-28-c, fire-32-a).  Pin counts map to the default
#                board for that socket size.
# chip_type:     chip to read, name or alias (e.g. 2364, 27512, 27c400, 2332)
# --cs1/2/3:     active level for configurable CS lines on mask ROMs (required
#                when the chip has Configurable CS lines, ignored otherwise)

set -e

if [ $# -lt 2 ]; then
    echo "Usage: $0 <board_or_pins> <chip_type> [--cs1 high|low] [--cs2 high|low] [--cs3 high|low]"
    echo ""
    echo "  board_or_pins  24 | 28 | 32 | 40 | <full board name>"
    echo "  chip_type      e.g. 2364, 27512, 27c040, 27c400, 2332, 2316"
    echo ""
    echo "Examples:"
    echo "  $0 32 27c040"
    echo "  $0 fire-28-c 23256 --cs1 high --cs2 low"
    echo "  $0 24 2316 --cs1 low --cs2 high --cs3 high"
    exit 1
fi

BOARD_OR_PINS=$1
CHIP=$2
shift 2

CS1=""
CS2=""
CS3=""

while [ $# -gt 0 ]; do
    case $1 in
        --cs1) CS1=$2; shift 2 ;;
        --cs2) CS2=$2; shift 2 ;;
        --cs3) CS3=$2; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

# --- Resolve pin count shorthand to default board name ---

case $BOARD_OR_PINS in
    24) BOARD="fire-24-e" ;;
    28) BOARD="fire-28-a" ;;
    32) BOARD="fire-32-a" ;;
    40) BOARD="fire-40-a" ;;
    *)  BOARD="$BOARD_OR_PINS" ;;
esac

# --- Determine MCU variant ---
# fire-32-*, fire-40-*, and fire-28-c onwards are RP2350B (48 GPIOs).
# All earlier boards are RP2350A (30 GPIOs).
# Update this if new rp2350b fire-28 revisions are introduced.
# For now use RP235XB in all cases, see Cargo.toml for details.
case $BOARD in
    fire-28-a|fire-28-b)
        FEATURE="rp2350b"
        ;;
    fire-40-*|fire-32-*|fire-28*)
        FEATURE="rp2350b"
        ;;
    *)
        FEATURE="rp2350b"
        ;;
esac

# --- Report what we're doing ---

echo "Board:   $BOARD ($FEATURE)"
echo "Chip:    $CHIP"
[ -n "$CS1" ] && echo "CS1:     $CS1"
[ -n "$CS2" ] && echo "CS2:     $CS2"
[ -n "$CS3" ] && echo "CS3:     $CS3"

# --- Build env var array and flash ---

ENV_VARS=("BOARD=$BOARD" "CHIP_TYPE=$CHIP")
[ -n "$CS1" ] && ENV_VARS+=("CS1=$CS1")
[ -n "$CS2" ] && ENV_VARS+=("CS2=$CS2")
[ -n "$CS3" ] && ENV_VARS+=("CS3=$CS3")

env "${ENV_VARS[@]}" cargo run --no-default-features --features "$FEATURE" --release