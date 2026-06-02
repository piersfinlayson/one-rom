#####################################################################
# PIO tests
#
# Uses epio PIO emulator to verify correct PIO behaviour
#####################################################################
set -e

run_test() {
    local hw_rev=$1
    local image=$2
    local base_config=$3
    local num_cs=$4
    local extra_flags=${5:-}

    for cs1 in 0 1; do
        if [ $num_cs -lt 2 ]; then
            local cmd="HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS=\"$extra_flags\" ROM_CONFIGS=\"file=$image,$base_config,cs1=$cs1\" make test-pio"
            echo "$cmd"
            env HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS="$extra_flags" \
                ROM_CONFIGS="file=$image,$base_config,cs1=$cs1" make test-pio > /dev/null || \
                { echo "FAILED: $cmd"; exit 1; }
            continue
        fi
        for cs2 in 0 1; do
            if [ $num_cs -lt 3 ]; then
                local cmd="HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS=\"$extra_flags\" ROM_CONFIGS=\"file=$image,$base_config,cs1=$cs1,cs2=$cs2\" make test-pio"
                echo "$cmd"
                env HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS="$extra_flags" \
                    ROM_CONFIGS="file=$image,$base_config,cs1=$cs1,cs2=$cs2" make test-pio > /dev/null || \
                    { echo "FAILED: $cmd"; exit 1; }
                continue
            fi
            for cs3 in 0 1; do
                local cmd="HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS=\"$extra_flags\" ROM_CONFIGS=\"file=$image,$base_config,cs1=$cs1,cs2=$cs2,cs3=$cs3\" make test-pio"
                echo "$cmd"
                env HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS="$extra_flags" \
                    ROM_CONFIGS="file=$image,$base_config,cs1=$cs1,cs2=$cs2,cs3=$cs3" make test-pio > /dev/null || \
                    { echo "FAILED: $cmd"; exit 1; }
            done
        done
    done
}

run_no_cs() {
    local hw_rev=$1
    local image=$2
    local base_config=$3
    local extra_flags=${4:-}

    local cmd="HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS=\"$extra_flags\" ROM_CONFIGS=\"file=$image,$base_config\" make test-pio"
    echo "$cmd"
    env HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS="$extra_flags" \
        ROM_CONFIGS="file=$image,$base_config" make test-pio > /dev/null || \
        { echo "FAILED: $cmd"; exit 1; }
}

test_24_all_rom_types() {
    local hw_rev=${1:-fire-24-e}
    local extra_flags=${2:-}

    run_test   $hw_rev images/test/rand_8KB.rom trunc,type=2316 3 "$extra_flags"
    run_test   $hw_rev images/test/rand_8KB.rom trunc,type=2332 2 "$extra_flags"
    run_test   $hw_rev images/test/rand_8KB.rom type=2364       1 "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2704   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2708   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2716   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2732   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=28C16   "$extra_flags"
}

test_28_all_rom_types() {
    local hw_rev=${1:-fire-28-a}
    local extra_flags=${2:-}

    run_test   $hw_rev images/test/rand_64KB.rom trunc,type=23128 3 "$extra_flags"
    run_test   $hw_rev images/test/rand_64KB.rom trunc,type=23256 2 "$extra_flags"
    run_test   $hw_rev images/test/rand_64KB.rom type=23512       2 "$extra_flags"
    run_test   $hw_rev images/test/rand_128KB.rom type=231024      1 "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom trunc,type=2764    "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom trunc,type=27128   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom trunc,type=27256   "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom type=27512         "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom trunc,type=28C64    "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_64KB.rom trunc,type=28C256   "$extra_flags"

    # Supported as of 0.6.9
    run_test   $hw_rev images/test/rand_8KB.rom type=2364         1 "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2704    "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2708    "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2716    "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_8KB.rom trunc,type=2732    "$extra_flags"

    # Supported as of 0.6.11
    run_test   $hw_rev images/test/rand_64KB.rom type=23QL512 1 "$extra_flags"

    # Supported as of 0.6.12
    run_test   $hw_rev images/test/rand_64KB.rom trunc,type=23QL384 1 "$extra_flags"
}

test_32pin() {
    local hw_rev=${1:-fire-32-a}
    local extra_flags=${2:-}

    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C010,trunc "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C020,trunc "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C040       "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C301,trunc "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C080,cs1=0 "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C080,cs1=1 "$extra_flags"
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=28C512,trunc "$extra_flags"

    # Supported as of 0.6.13
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=23C1010,trunc "$extra_flags"

    # Not supported on fire-32-a:
    if [ "$hw_rev" = "fire-32-a" ]; then
        return
    fi
    run_no_cs  $hw_rev images/test/rand_512KB.rom type=SST39SF040    "$extra_flags"
}

test_40pin() {
    local hw_rev=${1:-fire-40-a}
    local extra_flags=${2:-}

    run_no_cs  $hw_rev images/test/rand_512KB.rom type=27C400 "$extra_flags"
}

run_config() {
    local hw_rev=$1
    local config=$2
    local extra_flags=${3:-}

    local cmd="HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS=\"$extra_flags\" CONFIG=\"$config\" make test-pio"
    echo "$cmd"
    env HW_REV=$hw_rev MCU=rp2350 EXTRA_C_FLAGS="$extra_flags" \
        CONFIG="$config" make test-pio > /dev/null || \
        { echo "FAILED: $cmd"; exit 1; }
}

test_config() {
    local hw_rev=${1:-fire-24-a}
    local config=$2
    local extra_flags=${3:-}

    run_config $hw_rev "$config" "$extra_flags"
}

test_24_config() {
    local config=$1

    test_config fire-24-a "$config"
    test_config fire-24-b "$config"
    test_config fire-24-c "$config"
    test_config fire-24-d "$config"
    test_config fire-24-e "$config"
}

test_24_config_c_onwards() {
    local config=$1

    test_config fire-24-c "$config"
    test_config fire-24-d "$config"
    test_config fire-24-e "$config"
}

test_28_config() {
    local config=$1

    test_config fire-28-a "$config"
}

test_32_config() {
    local config=$1

    test_config fire-32-a "$config"
}

# Test every ROM type on every Fire 24 hardware revision.  This tests a single
# ROM image/set
test_24_all_rom_types fire-24-a
test_24_all_rom_types fire-24-b
test_24_all_rom_types fire-24-c
test_24_all_rom_types fire-24-d
test_24_all_rom_types fire-24-e

# Test every ROM type on the first Fire 28 hardware revision.  This tests a
# single ROM image/set
test_28_all_rom_types fire-28-a
test_28_all_rom_types fire-28-b
test_28_all_rom_types fire-28-c

test_32pin fire-32-a
test_32pin fire-32-b

test_40pin fire-40-a
test_40pin fire-40-a -DFORCE_16_BIT

# Test specific ROM configurations on all Fire 24 hardware revisions.
test_24_config old-config/pet-4-40-50.mk
test_24_config old-config/test/24-random-27xx.mk

# Test multi-ROM sets on revisions C+.  A/B do not support multi-ROM sets with
# PIO support due to a lack of contiguity between CS and X pins.
test_24_config_c_onwards old-config/test/set-2-images.mk
test_24_config_c_onwards old-config/test/set-3-images.mk

# Test banked switched ROM configurations on all Fire 24 hardware revisions.
# All hardware revisions support bank switched ROMs with PIO support.
test_24_config old-config/bank-c64-char.mk

# Test specific ROM configurations on all Fire 28 hardware revisions.
test_28_config old-config/28-c64c.mk
test_28_config old-config/28-1541ii.mk

# Test specific ROM configurations on all Fire 32 hardware revisions.
test_32_config old-config/test/32-random-27c080.mk
test_32_config old-config/test/32-random-27c301.mk
test_32_config old-config/test/32-random-27c0x0.mk

# Test specific ROM configurations on all Fire 40 hardware revisions.
test_config fire-40-a old-config/test/40-random.mk