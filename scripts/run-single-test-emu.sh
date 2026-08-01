#!/usr/bin/env bash
# Run a single One ROM Emulator test.
#
# Usage: scripts/run-single-test-emu.sh [options] <board> <image> <chip-type> [<size-handling>]
#
# Arguments:
#   board           Board identifier, e.g. fire-24-a, fire-28-a, fire-40-b
#   image           ROM image path, e.g. images/test/rand_8KB.rom
#   chip-type       ROM chip type, e.g. 28C16, 2364, 27C400
#   size-handling   Optional: truncate (default: none)
#
# Options:
#   --cs1 <active_low|active_high>   CS1 polarity
#   --cs2 <active_low|active_high>   CS2 polarity
#   --cs3 <active_low|active_high>   CS3 polarity
#   --force-16-bit                   Force 16-bit mode (40-pin boards)
#   --transform <spec>               Image transforms, in the CLI's --slot
#                                    notation, e.g. swap_bytes or
#                                    deinterleave:1/2/2+swap_bytes
#   -h, --help                       Show this help and exit
#
# Examples:
#   scripts/run-single-test-emu.sh fire-24-a images/test/rand_8KB.rom 28C16 truncate
#   scripts/run-single-test-emu.sh fire-24-a images/test/rand_8KB.rom 2364 --cs1 active_low
#   scripts/run-single-test-emu.sh fire-28-a images/test/rand_64KB.rom 23512 --cs1 active_low --cs2 active_high
#   scripts/run-single-test-emu.sh fire-40-a images/test/rand_512KB.rom 27C400 --force-16-bit
#   scripts/run-single-test-emu.sh fire-24-e images/test/rand_8KB.rom 2364 --cs1 active_low --transform swap_bytes

_reproduce_cmd() {
    local board=$1 image=$2 chip_type=$3 size_handling=$4
    local cs1=$5 cs2=$6 cs3=$7 force_16_bit=$8 transform=$9

    local cmd="scripts/run-single-test-emu.sh"
    [ -n "$cs1" ]               && cmd+=" --cs1 $cs1"
    [ -n "$cs2" ]               && cmd+=" --cs2 $cs2"
    [ -n "$cs3" ]               && cmd+=" --cs3 $cs3"
    [ "$force_16_bit" = "true" ] && cmd+=" --force-16-bit"
    [ -n "$transform" ]         && cmd+=" --transform $transform"
    cmd+=" $board $image $chip_type"
    [ "$size_handling" != "none" ] && cmd+=" $size_handling"
    echo "$cmd"
    echo "  You can also preface the command env variables, such as \`ONEROM_LOG=1 RUST_LOG=debug\`"
}

_normalize_cs() {
    case $1 in
        0) echo "active_low"  ;;
        1) echo "active_high" ;;
        *) echo "$1"          ;;
    esac
}

# Translate the CLI's textual transform notation (`swap_bytes`,
# `deinterleave:<offset>/<stride>[/<bytes>]`, joined with `+`) into the JSON
# array a config file carries.  The two notations are deliberately different -
# see docs/CLI-MANUAL.md - so the harness accepts the one a user would type.
_transform_json() {
    local spec=$1
    local out="" first=1 part

    local IFS='+'
    for part in $spec; do
        local json
        case $part in
            swap_bytes)
                json='"swap_bytes"'
                ;;
            deinterleave:*)
                local params=${part#deinterleave:}
                local offset stride bytes
                IFS='/' read -r offset stride bytes <<< "$params"
                [ -z "$bytes" ] && bytes=1
                json="{\"deinterleave\":{\"offset\":$offset,\"stride\":$stride,\"bytes\":$bytes}}"
                ;;
            *)
                echo "Unknown transform '$part'" >&2
                exit 1
                ;;
        esac
        [ $first -eq 0 ] && out+=","
        out+="$json"
        first=0
    done

    echo "[$out]"
}

_run_single_test() {
    local board=$1
    local image=$2
    local chip_type=$3
    local size_handling=${4:-none}
    local cs1; cs1=$(_normalize_cs "${5:-}")
    local cs2; cs2=$(_normalize_cs "${6:-}")
    local cs3; cs3=$(_normalize_cs "${7:-}")
    local force_16_bit=${8:-false}
    local transform=${9:-}

    local chip="{\"type\":\"$chip_type\",\"file\":\"$image\""
    [ "$size_handling" != "none" ] && chip+=",\"size_handling\":\"$size_handling\""
    [ -n "$cs1" ] && chip+=",\"cs1\":\"$cs1\""
    [ -n "$cs2" ] && chip+=",\"cs2\":\"$cs2\""
    [ -n "$cs3" ] && chip+=",\"cs3\":\"$cs3\""
    [ -n "$transform" ] && chip+=",\"transform\":$(_transform_json "$transform")"
    chip+="}"

    local chip_set="{\"type\":\"single\",\"chips\":[$chip]"
    [ "$force_16_bit" = "true" ] && chip_set+=",\"firmware_overrides\":{\"fire\":{\"force_16_bit\":true}}"
    chip_set+="}"

    local tmp
    tmp=$(mktemp /tmp/onerom-test-XXXXXX)
    printf '{"version":1,"description":"PIO test","chip_sets":[%s]}\n' "$chip_set" > "$tmp"

    local desc="board=$board image=$image type=$chip_type"
    [ "$size_handling" != "none" ] && desc+=" size_handling=$size_handling"
    [ -n "$cs1" ] && desc+=" cs1=$cs1"
    [ -n "$cs2" ] && desc+=" cs2=$cs2"
    [ -n "$cs3" ] && desc+=" cs3=$cs3"
    [ "$force_16_bit" = "true" ] && desc+=" force_16_bit=true"
    [ -n "$transform" ] && desc+=" transform=$transform"
    echo ""
    echo "Testing: $desc"

    if env BOARD="$board" CONFIG="$tmp" make test-emu; then
        rm -f "$tmp"
    else
        local saved="/tmp/onerom-last-failure.json"
        cp "$tmp" "$saved"
        rm -f "$tmp"
        echo "FAILED: $desc"
        echo "Config:     $saved"
        echo "Reproduce:  $(_reproduce_cmd "$board" "$image" "$chip_type" "$size_handling" "$cs1" "$cs2" "$cs3" "$force_16_bit")"
        return 1
    fi
}

_usage() {
    sed -n '/^# /{ s/^# \?//; p; }' "$0"
}

# Only parse args and run when executed directly, not sourced.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    cs1="" cs2="" cs3=""
    force_16_bit=false
    transform=""

    positional=()
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)      _usage; exit 0 ;;
            --cs1)          cs1=$2;  shift 2 ;;
            --cs2)          cs2=$2;  shift 2 ;;
            --cs3)          cs3=$2;  shift 2 ;;
            --force-16-bit) force_16_bit=true; shift ;;
            --transform)    transform=$2; shift 2 ;;
            -*)             echo "Unknown option: $1" >&2; _usage; exit 1 ;;
            *)              positional+=("$1"); shift ;;
        esac
    done

    if [[ ${#positional[@]} -lt 3 || ${#positional[@]} -gt 4 ]]; then
        echo "Error: expected board, image, chip-type, and optional size-handling" >&2
        _usage
        exit 1
    fi

    board=${positional[0]} image=${positional[1]} chip_type=${positional[2]} size_handling=${positional[3]:-none}

    _run_single_test "$board" "$image" "$chip_type" "$size_handling" \
        "$cs1" "$cs2" "$cs3" "$force_16_bit" "$transform"
fi