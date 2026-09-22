#!/usr/bin/env bash
#
# Program FLASH_DEVINFO and FLASH_DEVINFO_ENABLE in OTP, declaring the external
# flash on chip select 1.
#
# Defaults are for a fire-40-a - RP2354B with a 2MB W25Q16JVSS on GPIO 47.

set -uo pipefail

DEVINFO=${DEVINFO:-0x99AF}            # CS1_SIZE 9, CS0_SIZE 9, D8H 1, CS1_GPIO 47
DEVINFO_RAW=${DEVINFO_RAW:-0x3a99af}  # DEVINFO with its six ECC parity bits
ENABLE_ROW=${ENABLE_ROW:-0x20}        # BOOT_FLAGS0 with FLASH_DEVINFO_ENABLE set
EXT_BASE=0x11000000
EXT_END=0x11001000
EXT_LEN=$((EXT_END - EXT_BASE))          # follows the range above
ENABLE_RAW=$(printf '0x%06x' "$((ENABLE_ROW))")  # the row as picotool prints it

step=0
out=''
rc=0

# Colour only when a terminal is reading.
if [[ -t 1 ]]; then
    BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'; OFF=$'\033[0m'
else
    BOLD=''; RED=''; GREEN=''; OFF=''
fi
RULE=$(printf '%.0s-' {1..72})

# Tool output is indented behind a bar so it never reads as something the
# script said.
next() {
    step=$((step + 1))
    printf '\n%s%s\n  Step %d.  %s\n%s%s\n' "$BOLD" "$RULE" "$step" "$*" "$RULE" "$OFF"
}
info() { printf '  %s\n' "$*"; }
ok()   { printf '  %sPASS  %s%s\n' "$GREEN" "$*" "$OFF"; }
die()  { printf '\n  %sABORT  %s%s\n\n' "$RED" "$*" "$OFF" >&2; exit 1; }
say()  { printf '\n%s%s\n  %s\n%s%s\n' "$BOLD" "$RULE" "$*" "$RULE" "$OFF"; }

# Run a command, showing it and everything it printed.  Sets $out and $rc
# rather than exiting, so each caller decides what a failure means.
capture() {
    printf '\n  $ %s\n' "$*"
    out=$("$@" 2>&1)
    rc=$?
    [[ -n $out ]] && sed 's/^/  | /' <<<"$out"
    return 0
}

# Run a command that has to succeed.
run() {
    capture "$@"
    [[ $rc -eq 0 ]] || die "$1 failed with status $rc"
}

# Read the given OTP rows and check every one holds $1.  picotool's output
# reaches the terminal in full before any of it is parsed, so the check doesn't
# hide anything.
expect_rows() {
    local want=$1 pairs count row value
    shift
    capture picotool otp get -r -n "$@"
    [[ $rc -eq 0 ]] || die "picotool otp get failed with status $rc"
    grep -q 'WARNING' <<<"$out" && die "picotool reported a warning"

    pairs=$(awk '/^ROW/ { row = $2; sub(/:$/, "", row) }
                 /^ *VALUE / { print row, $2 }' <<<"$out")
    count=$(grep -c . <<<"$pairs")
    [[ $count -eq $# ]] || die "expected $# rows, parsed $count"
    while read -r row value; do
        [[ $value == "$want" ]] || die "row $row reads $value, expected $want"
    done <<<"$pairs"
    if [[ $count -eq 1 ]]; then ok "row reads $want"; else ok "$count rows read $want"; fi
}

# Find the one One ROM to work on, and read its state.
scan_device() {
    capture onerom scan -u
    [[ $rc -eq 0 ]] || die "onerom scan failed with status $rc"
    devices=$(grep 'State:' <<<"$out")
    found=$(grep -c . <<<"$devices")
    [[ $found -ne 0 ]] || die "no One ROM found"
    [[ $found -eq 1 ]] || die "$found One ROMs connected"
    state=$(sed -n 's/.*State: *\([A-Za-z]*\).*/\1/p' <<<"$devices")
}

confirm() {
    local reply=''
    read -r -p "$(printf '\n  %s%s [y/N]%s ' "$BOLD" "$1" "$OFF")" reply || true
    [[ $reply == y || $reply == Y ]] || { printf '  Aborted.\n\n'; exit 1; }
}

# Where the final read lands.  Set OUT to keep it.  picotool checks the output
# file's extension before it looks for a device, so a bare mktemp name fails.
if [[ -n ${OUT:-} ]]; then
    out_dir=''
else
    out_dir=$(mktemp -d -t onerom-extflash) || { echo "cannot make a temporary directory" >&2; exit 1; }
    OUT="$out_dir/ext.bin"
fi
trap 'if [[ -n $out_dir ]]; then rm -rf "$out_dir"; fi' EXIT

printf '%s%s\n' "$BOLD" "$RULE"
cat <<BANNER
  program-otp.sh

  Programs this One ROM's OTP with the external flash's size and chip select
  pin.  Once programmed the external flash is accessible at $EXT_BASE.

  row  0x054                 FLASH_DEVINFO = $DEVINFO
  rows 0x048, 0x049, 0x04a   BOOT_FLAGS0 and its two redundant copies,
                             FLASH_DEVINFO_ENABLE (bit 5) set

  Neither write can be undone.  Procedure: docs/wip/EXTERNAL-FLASH.md
BANNER
printf '%s%s\n' "$RULE" "$OFF"

next "check which device is connected"
info "There must be a single stopped One ROM connected."
scan_device
if [[ $state != Stopped ]]; then
    confirm "Device is $state.  Reboot it into BOOTSEL?"
    run onerom reboot --stopped
    scan_device
    [[ $state == Stopped ]] || die "device is still $state"
fi
confirm "Proceed?"

next "check the external flash is unreachable"
capture picotool save -r "$EXT_BASE" "$EXT_END" "$OUT"
if [[ $rc -eq 0 ]]; then
    info "External flash is already accessible.  Reading OTP to find out why."
    capture picotool otp get -r -n 0x054 0x048
    die "external flash already accessible - see the OTP rows above"
fi
grep -qi 'permission failure' <<<"$out" ||
    die "expected a permission failure, got status $rc"
ok "correctly refused"

next "check every row is unwritten"
expect_rows 0x000000 0x048 0x049 0x04a 0x054 0x055

next "write FLASH_DEVINFO"
confirm "Write $DEVINFO to row 0x054?  This is irreversible."
run picotool otp set 0x054 "$DEVINFO"

next "verify FLASH_DEVINFO"
expect_rows "$DEVINFO_RAW" 0x054

# A reboot is not a power cycle, so this does not re-sense the fuses from cold.
# It is here because it costs nothing ahead of the write that arms them.
next "reboot"
run onerom reboot --stopped

next "verify FLASH_DEVINFO again"
expect_rows "$DEVINFO_RAW" 0x054

next "set FLASH_DEVINFO_ENABLE"
confirm "Set FLASH_DEVINFO_ENABLE, writing rows 0x048, 0x049 and 0x04a?  Irreversible.  After this write a bad 0x054 or 0x055 means the chip will permanently fail to boot."
run picotool otp set -s 0x048 "$ENABLE_ROW"

next "verify BOOT_FLAGS0 and its two copies"
expect_rows "$ENABLE_RAW" 0x048 0x049 0x04a

next "reboot to pick up the new OTP"
run onerom reboot --stopped

next "read the external flash"
rm -f "$OUT"
run picotool save -r "$EXT_BASE" "$EXT_END" "$OUT"
[[ -f $OUT ]] || die "$OUT was not written"
size=$(wc -c <"$OUT" | tr -d "[:space:]")
[[ $size -eq $EXT_LEN ]] || die "saved $size bytes, expected $EXT_LEN"

ok "saved $size bytes"
say "Done.  External flash accessible at $EXT_BASE."
