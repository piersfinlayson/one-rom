#!/usr/bin/env bash
# Used by piers.rocks to program, test and commission a One ROM.
#
# Usage: scripts/commission.sh [-y] --board BOARD --size SIZE --batch BATCH
#                              --rev REV
#
# Shows the values to be programmed and asks for confirmation. -y skips it.
#
# Operation:
# - Programs a test ROM image and the host-control plugin.
# - Cycles the RGB LED.
# - Commissions the board.
#
# The CLI accepts batch and revision strings of up to 16 characters.
#
# This script enforces piers.rocks's convention of:
# - a 10 character batch string (order date YYYYMMDDXX where XX starts at 01 to
#   disambiguate multiple orders in a single day).
# - a 1 or 2 character revision (B, C2, etc).
#
# Set in the environment or in ~/.config/onerom/commission.env
# ($XDG_CONFIG_HOME/onerom/ where set). The environment overrides the file.
#   ONEROM_SIGNING_SERVER  The signing server's https address.
#   ONEROM_KEY_ID          The signing key's ID.
#   ONEROM_MANUFACTURER    The manufacturer written to OTP.
#   ONEROM_PIN             The signing key's PIN.
#   ONEROM                 The onerom binary. Defaults to onerom on PATH.
set -euo pipefail

usage() {
    echo "usage: $0 [-y] --board BOARD --size SIZE --batch BATCH --rev REV" >&2
    exit 2
}

CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/onerom/commission.env"
if [ -f "$CONFIG" ]; then
    environment=$(declare -p ONEROM_SIGNING_SERVER ONEROM_KEY_ID \
        ONEROM_MANUFACTURER ONEROM_PIN ONEROM 2>/dev/null || true)
    # shellcheck source=/dev/null
    . "$CONFIG"
    # The environment's values replace the file's.
    eval "$environment"
fi
for var in ONEROM_SIGNING_SERVER ONEROM_KEY_ID ONEROM_MANUFACTURER \
    ONEROM_PIN; do
    if [ -z "${!var:-}" ]; then
        echo "$var is not set" >&2
        exit 1
    fi
done
ONEROM="${ONEROM:-onerom}"

board="" size="" batch="" rev="" yes=false
while [ $# -gt 0 ]; do
    case "$1" in
        -y | --yes) yes=true; shift; continue ;;
    esac
    [ $# -ge 2 ] || usage
    case "$1" in
        --board) board="$2" ;;
        --size) size="$2" ;;
        --batch) batch="$2" ;;
        --rev) rev="$2" ;;
        *) usage ;;
    esac
    shift 2
done
[ -n "$board" ] && [ -n "$size" ] && [ -n "$batch" ] && [ -n "$rev" ] || usage

if [ ${#batch} -ne 10 ]; then
    echo "batch must be 10 characters" >&2
    exit 1
fi
if [ ${#rev} -gt 2 ]; then
    echo "revision must be 1 or 2 characters" >&2
    exit 1
fi

# A board's name contains its pin count, e.g. fire-24-f.
pins=$(echo "$board" | cut -d- -f2)
case "$pins" in
    24) rom_file=rand_4KB.rom type=2732 ;;
    28) rom_file=rand_64KB.rom type=27C512 ;;
    32) rom_file=rand_512KB.rom type=27C040 ;;
    40) rom_file=rand_512KB.rom type=27C400 ;;
    *)
        echo "can't find a 24, 28, 32 or 40 pin count in board $board" >&2
        exit 1
        ;;
esac
rom="$(cd "$(dirname "$0")/.." && pwd)/images/test/$rom_file"

program=("$ONEROM" program --plugin usb --plugin host-control
    --slot "file=$rom,type=$type" --board "$board" --verify -u)
rgb=("$ONEROM" control rgb cycle)
commission=("$ONEROM" hardware commission --size "$size" --board "$board"
    --batch "$batch" --revision "$rev" --key-id "$ONEROM_KEY_ID"
    --manufacturer "$ONEROM_MANUFACTURER" --pin "$ONEROM_PIN"
    --signer "$ONEROM_SIGNING_SERVER" --inspect-otp --validate --yes)

printf '%-14s%s\n' \
    Board "$board" \
    Size "$size" \
    Batch "$batch" \
    Revision "$rev" \
    Manufacturer "$ONEROM_MANUFACTURER" \
    "Key ID" "$ONEROM_KEY_ID" \
    Signer "$ONEROM_SIGNING_SERVER" \
    "Test ROM" "$rom_file, type $type"
if [ "$yes" = false ]; then
    echo
    read -r -p "Proceed? [y/N] " answer
    [ "$answer" = y ] || exit 1
fi

printf '\nProgramming\n'
"${program[@]}"
printf '\nCycling RGB LED\n'
"${rgb[@]}"
printf '\nCommissioning\n'
"${commission[@]}"
