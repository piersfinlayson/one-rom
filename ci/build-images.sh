#!/usr/bin/env bash
set -e

VERSION=$1
DEST_PREFIX=$2
DEST_VERSION=v$VERSION

if [ -z "$VERSION" ] || [ -z "$DEST_PREFIX" ]; then
    echo "Usage: $0 <version> <destination_prefix>"
    echo "  - example: $0 0.5.2 ../one-rom-images"
    exit 1
fi

FIRE_BOARDS=(fire-24-a fire-24-usb-b fire-24-c fire-24-d fire-24-e fire-24-f fire-28-a fire-28-b fire-28-c fire-32-a fire-32-b fire-40-a fire-40-b)
ICE_BOARDS=(ice-24-d ice-24-e ice-24-f ice-24-g ice-24-usb-h ice-24-i ice-24-j ice-28-a)
STM32_MCUS=(f401rb f401rc f401re f405rg f411rc f411re f446rc f446re)

# Board-specific extra args (applied to all MCUs for that board)
declare -A BOARD_EXTRA_ARGS=(
    ["ice-24-d"]="STATUS_LED=0"
)

# Board+MCU-specific extra args (if you need them)
#
# Currently unused
#
#declare -A BOARD_MCU_EXTRA_ARGS=(
#    ["ice-24-d+f401re"]="STATUS_LED=0 SOME_OTHER=1"
#)
declare -A BOARD_MCU_EXTRA_ARGS=()

json_boards=""

# Build fire boards with RP2350
for board in "${FIRE_BOARDS[@]}"; do
    echo "Building $board with rp2350..."
    
    # Get extra args
    extra_args="${BOARD_MCU_EXTRA_ARGS[$board+rp2350]:-${BOARD_EXTRA_ARGS[$board]:-}}"
    
    eval "EXCLUDE_METADATA=1 ROM_CONFIGS= HW_REV=$board MCU=rp2350 $extra_args make"
    
    dest_dir="$DEST_PREFIX/$DEST_VERSION/$board/rp2350"
    mkdir -p "$dest_dir"
    cp "sdrr/build/sdrr-rp2350.bin" "$dest_dir/firmware.bin"

    # Add to JSON
    json_boards+="
                {
                    \"name\": \"$board\",
                    \"mcus\": [
                        {\"name\": \"rp2350\"}
                    ]
                },"
done

# For ice boards
for board in "${ICE_BOARDS[@]}"; do
    json_mcus=""
    for mcu in "${STM32_MCUS[@]}"; do
        echo "Building $board with $mcu..."
        
        # Get extra args (board+mcu specific takes precedence over board-only)
        extra_args="${BOARD_MCU_EXTRA_ARGS[$board+$mcu]:-${BOARD_EXTRA_ARGS[$board]:-}}"
        
        eval "EXCLUDE_METADATA=1 ROM_CONFIGS= HW_REV=$board MCU=$mcu $extra_args make"
        
        dest_dir="$DEST_PREFIX/$DEST_VERSION/$board/$mcu"
        mkdir -p "$dest_dir"
        cp "sdrr/build/sdrr-stm32$mcu.bin" "$dest_dir/firmware.bin"

        # Add MCU to list
        json_mcus+="
                        {\"name\": \"$mcu\"},"
    done

    # Remove trailing comma from mcus
    json_mcus="${json_mcus%,}"
    
    # Add board to JSON
    json_boards+="
                {
                    \"name\": \"$board\",
                    \"mcus\": [$json_mcus
                    ]
                },"
done

# Remove trailing comma from boards
json_boards="${json_boards%,}"

# Write JSON file
cat > "/tmp/releases.json" <<EOF
        {
            "version": "$VERSION",
            "path": "$DEST_VERSION",
            "boards": [$json_boards
            ]
        }
EOF

echo "JSON manifest fragment written to /tmp/releases.json"