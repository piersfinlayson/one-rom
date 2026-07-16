#####################################################################
# One ROM Emulator (including PIO and plugin API) tests
#####################################################################
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$SCRIPT_DIR/../scripts/run-single-test-emu.sh"

cs_logic() {
    [ "$1" -eq 0 ] && echo "active_low" || echo "active_high"
}

parse_base_config() {
    local base_config=$1
    CHIP_TYPE=""
    SIZE_HANDLING="none"
    CONFIG_CS1=""
    CONFIG_CS2=""
    CONFIG_CS3=""

    local part
    local IFS=','
    for part in $base_config; do
        case "$part" in
            type=*)  CHIP_TYPE="${part#type=}" ;;
            trunc)   SIZE_HANDLING="truncate" ;;
            cs1=0)   CONFIG_CS1="active_low" ;;
            cs1=1)   CONFIG_CS1="active_high" ;;
            cs2=0)   CONFIG_CS2="active_low" ;;
            cs2=1)   CONFIG_CS2="active_high" ;;
            cs3=0)   CONFIG_CS3="active_low" ;;
            cs3=1)   CONFIG_CS3="active_high" ;;
        esac
    done
}

run_test() {
    local board=$1
    local image=$2
    local base_config=$3
    local num_cs=$4

    parse_base_config "$base_config"

    for cs1 in 0 1; do
        if [ $num_cs -lt 2 ]; then
            _run_single_test "$board" "$image" "$CHIP_TYPE" "$SIZE_HANDLING" \
                "$(cs_logic $cs1)" "" ""
            continue
        fi
        for cs2 in 0 1; do
            if [ $num_cs -lt 3 ]; then
                _run_single_test "$board" "$image" "$CHIP_TYPE" "$SIZE_HANDLING" \
                    "$(cs_logic $cs1)" "$(cs_logic $cs2)" ""
                continue
            fi
            for cs3 in 0 1; do
                _run_single_test "$board" "$image" "$CHIP_TYPE" "$SIZE_HANDLING" \
                    "$(cs_logic $cs1)" "$(cs_logic $cs2)" "$(cs_logic $cs3)"
            done
        done
    done
}

run_no_cs() {
    local board=$1
    local image=$2
    local base_config=$3
    local force_16_bit=${4:-false}

    parse_base_config "$base_config"
    _run_single_test "$board" "$image" "$CHIP_TYPE" "$SIZE_HANDLING" \
        "$CONFIG_CS1" "$CONFIG_CS2" "$CONFIG_CS3" "$force_16_bit"
}

run_config() {
    local board=$1
    local config=$2
 
    echo ""
    echo "Testing: board=$board config=$config"
    env BOARD="$board" CONFIG="$config" make test-emu || {
        echo "FAILED: board=$board config=$config"
        echo "Reproduce:  env BOARD=$board CONFIG=$config make test-emu"
        exit 1
    }
}

run_config_api() {
    local board=$1
    local config=$2
 
    echo ""
    echo "Testing: board=$board config=$config"
    env BOARD="$board" CONFIG="$config" make test-api || {
        echo "FAILED: board=$board config=$config"
        echo "Reproduce:  env BOARD=$board CONFIG=$config make test-api"
        exit 1
    }
}

test_24_all_rom_types() {
    local board=${1:-fire-24-e}

    # Deliberately truncate one, to test that function
    run_test   $board images/test/rand_4KB.rom   trunc,type=2316  3
    run_test   $board images/test/rand_4KB.rom   type=2332  2
    run_test   $board images/test/rand_8KB.rom   type=2364  1
    run_no_cs  $board images/test/rand_0.5KB.rom type=2704
    run_no_cs  $board images/test/rand_1KB.rom   type=2708
    run_no_cs  $board images/test/rand_2KB.rom   type=2716
    run_no_cs  $board images/test/rand_4KB.rom   type=2732
    run_no_cs  $board images/test/rand_2KB.rom   type=28C16
    run_no_cs  $board images/test/rand_0.5KB.rom type=HM7641
}

test_28_all_rom_types() {
    local board=${1:-fire-28-a}

    run_no_cs  $board images/test/rand_8KB.rom   type=28C64
    run_no_cs  $board images/test/rand_32KB.rom  type=28C256
    run_test   $board images/test/rand_64KB.rom  type=23QL512 1
    run_test   $board images/test/rand_48KB.rom  type=23QL384 1
    run_test   $board images/test/rand_16KB.rom  type=23128   3
    run_test   $board images/test/rand_32KB.rom  type=23256   2
    run_test   $board images/test/rand_64KB.rom  type=23512   2
    run_test   $board images/test/rand_128KB.rom type=231024  1
    run_no_cs  $board images/test/rand_8KB.rom   type=2764
    run_no_cs  $board images/test/rand_16KB.rom  type=27128
    run_no_cs  $board images/test/rand_32KB.rom  type=27256
    run_no_cs  $board images/test/rand_64KB.rom  type=27512
}

test_32pin() {
    local board=${1:-fire-32-a}

    run_no_cs  $board images/test/rand_128KB.rom type=27C010
    run_no_cs  $board images/test/rand_256KB.rom type=27C020
    run_no_cs  $board images/test/rand_512KB.rom type=27C040
    run_no_cs  $board images/test/rand_128KB.rom type=27C301
    run_no_cs  $board images/test/rand_512KB.rom type=27C080,cs1=0
    run_no_cs  $board images/test/rand_512KB.rom type=27C080,cs1=1
    run_no_cs  $board images/test/rand_64KB.rom  type=28C512

    # Supported as of 0.6.13
    run_no_cs  $board images/test/rand_512KB.rom type=23C1010,trunc

    # Not supported on fire-32-a:
    if [ "$board" = "fire-32-a" ]; then
        echo "Skipping SST39SF040 test on $board (not supported)"
        return
    fi
    run_no_cs  fire-32-b images/test/rand_512KB.rom type=SST39SF040
}

test_40pin() {
    local board=${1:-fire-40-a}
    local force_16_bit=${2:-false}

    run_no_cs  $board images/test/rand_512KB.rom type=27C400 "$force_16_bit"
    run_no_cs  $board images/test/rand_256KB.rom type=27C200 "$force_16_bit"
}

test_config() {
    local board=${1:-fire-24-a}
    local config=$2

    run_config $board "$config"
}

test_config_api() {
    local board=${1:-fire-24-a}
    local config=$2

    run_config_api $board "$config"
}

test_24_config() {
    local config=$1

    test_config fire-24-a "$config"
    test_config fire-24-b "$config"
    test_config fire-24-c "$config"
    test_config fire-24-d "$config"
    test_config fire-24-e "$config"
    test_config fire-24-f "$config"
}

test_24_config_api() {
    local config=$1

    test_config_api fire-24-a "$config"
    test_config_api fire-24-b "$config"
    test_config_api fire-24-c "$config"
    test_config_api fire-24-d "$config"
    test_config_api fire-24-e "$config"
    test_config_api fire-24-f "$config"
}

test_24_config_c_onwards() {
    local config=$1

    test_config fire-24-c "$config"
    test_config fire-24-d "$config"
    test_config fire-24-e "$config"
    test_config fire-24-f "$config"
}

test_28_config() {
    local config=$1

    test_config fire-28-a "$config"
    test_config fire-28-b "$config"
    test_config fire-28-c "$config"
    test_config fire-28-d "$config"
}

test_28_config_api() {
    local config=$1

    test_config_api fire-28-a "$config"
    test_config_api fire-28-b "$config"
    test_config_api fire-28-c "$config"
    test_config_api fire-28-d "$config"
}

test_28_config_c_onwards() {
    local config=$1

    test_config fire-28-c "$config"
    test_config fire-28-d "$config"
}

test_32_config() {
    local config=$1

    test_config fire-32-a "$config"
    test_config fire-32-b "$config"
}

test_32_config_api() {
    local config=$1

    test_config_api fire-32-a "$config"
    test_config_api fire-32-b "$config"
}

test_40_config() {
    local config=$1

    test_config fire-40-a "$config"
    test_config fire-40-b "$config"
}

test_40_config_api() {
    local config=$1

    test_config_api fire-40-a "$config"
    test_config_api fire-40-b "$config"
}

# Test every standard ROM type on every standard hardware revision.
# Do just one 24/28/32/40 variant now, so we fail early if any ROM types are
# broken.
test_40pin fire-40-a
test_40pin fire-40-a true
test_28_all_rom_types fire-28-a
test_24_all_rom_types fire-24-a
test_32pin fire-32-a

# Remaining 24 pin boards.
test_24_all_rom_types fire-24-b
test_24_all_rom_types fire-24-c
test_24_all_rom_types fire-24-d
test_24_all_rom_types fire-24-e
test_24_all_rom_types fire-24-f

# Remaining 28 pin boards.
test_28_all_rom_types fire-28-c # First, as B is same as A
test_28_all_rom_types fire-28-b
test_28_all_rom_types fire-28-d

# Remaining 32 pin boards.
test_32pin fire-32-b

# Remaining 40 pin boards.
test_40pin fire-40-b
test_40pin fire-40-b true

# Extended set of 24 & 28 pin ROM tests
test_24_config onerom-config/test/24-random-23xx.json
test_24_config onerom-config/test/24-random-27xx.json
test_24_config onerom-config/test/24-random-28xx.json
test_28_config onerom-config/test/28-random-23xxx.json
test_28_config onerom-config/test/28-random-23qlxxx.json
test_28_config onerom-config/test/28-random-27xxx.json
test_28_config onerom-config/test/28-random-28xxx.json

# Test specific ROM configurations on all Fire 40 hardware revisions.
test_40_config onerom-config/test/40-random.json
test_40_config onerom-config/test/40-random-force-16bit.json

# Test bank switched ROM configurations on all Fire 24 hardware revisions.
# All 24 pin hardware revisions support bank switched ROMs with PIO support.
test_24_config onerom-config/test/24-bank-23xx.json
test_24_config onerom-config/test/24-bank-27xx.json
test_24_config onerom-config/test/24-bank-28xx.json

# Test bank switched ROM configurations on fire-28-c (no X pins on earlier
# revisions)
test_config fire-28-c onerom-config/test/28-bank-23xxx.json
test_config fire-28-c onerom-config/test/28-bank-23qlxxx.json
test_config fire-28-c onerom-config/test/28-bank-27xxx.json
test_config fire-28-c onerom-config/test/28-bank-28xxx.json
test_config fire-28-d onerom-config/test/28-bank-23xxx.json
test_config fire-28-d onerom-config/test/28-bank-23qlxxx.json
test_config fire-28-d onerom-config/test/28-bank-27xxx.json
test_config fire-28-d onerom-config/test/28-bank-28xxx.json

# Test multi-chip ROM configurations on all Fire 24 hardware revisions.
test_24_config_c_onwards onerom-config/test/24-multi-2364.json
test_24_config_c_onwards onerom-config/test/24-multi-2316.json
test_28_config_c_onwards onerom-config/test/28-multi-231024.json

# Test specific ROM configurations on all Fire 24 hardware revisions.
# fire-24-c only has 2 image select jumpers so can only test the first
# 4 sets within the PET config, but does check that the firmware
# correctly wraps at that point.
test_24_config onerom-config/pet-4-40-50.json
test_24_config onerom-config/test/24-random-27xx.json

# Test specific ROM configurations on all Fire 28 hardware revisions.
test_28_config onerom-config/28-c64c.json
test_28_config onerom-config/28-1541ii.json

# Test specific ROM configurations on all Fire 32 hardware revisions.
test_32_config onerom-config/test/32-random-27c080.json
test_32_config onerom-config/test/32-random-27c301.json
test_32_config onerom-config/test/32-random-27c0x0.json
test_config fire-32-b onerom-config/test/32-random-23c1001.json

# Plugin API tests
test_24_config_api onerom-config/test/24-random-23xx.json
test_24_config_api onerom-config/test/24-random-27xx.json
test_24_config_api onerom-config/test/24-random-28xx.json
test_28_config_api onerom-config/test/28-random-23xxx.json
test_28_config_api onerom-config/test/28-random-23qlxxx.json
test_28_config_api onerom-config/test/28-random-27xxx.json
test_28_config_api onerom-config/test/28-random-28xxx.json
test_32_config_api onerom-config/test/32-random-27c080.json
test_32_config_api onerom-config/test/32-random-27c301.json
test_32_config_api onerom-config/test/32-random-27c0x0.json
test_config fire-32-b onerom-config/test/32-random-extra.json
test_40_config_api onerom-config/test/40-random.json
test_40_config_api onerom-config/test/40-random-force-16bit.json
