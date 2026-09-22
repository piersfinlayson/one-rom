#!/usr/bin/env bash
#
# Test program-otp.sh against a stubbed picotool and onerom.
#
# The stubs model one RP2354B whose OTP state lives in a file, so a case can
# start the device virgin, part-programmed or finished.
#
# This does not test the actual hardware or the real picotool and onerom binaries.
#
#     ./test-program-otp.sh            all cases
#     ./test-program-otp.sh happy      one case, showing its full output

set -uo pipefail
SCRIPT=${SCRIPT:-$(cd "$(dirname "$0")" && pwd)/program-otp.sh}
TMP=$(mktemp -d) && trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"

cat > "$TMP/bin/onerom" <<'EOF'
#!/usr/bin/env bash
if [[ $1 == scan ]]; then
  echo "Scanning ... "
  m=$SCAN
  # These two model a Running device that the reboot then stops, or loses.
  [[ $m == runthenstop && -f $RSTATE ]] && m=one
  [[ $m == runthengone && -f $RSTATE ]] && m=none
  case $m in
    none) echo "found 0 connected devices:" ;;
    two)  echo "found 2 connected devices:"
          echo "  One ROM Fire 40 A - Firmware: v0.8.0 State: Stopped Serial: AAAA"
          echo "  One ROM Fire 40 A - Firmware: v0.8.0 State: Running Serial: BBBB" ;;
    running|runthenstop|runthengone)
          echo "found 1 connected devices:"
          echo "  One ROM Fire 40 A - Firmware: v0.8.0 State: Running Serial: AAAA" ;;
    scanfail) echo "error"; exit 3 ;;
    *)    echo "found 1 connected devices:"
          echo "  One ROM Fire 40 A - Firmware: v0.8.0 State: Stopped Serial: AAAA" ;;
  esac
  exit 0
fi
touch "$RSTATE"; echo "Rebooting One ROM into stopped state"; exit 0
EOF

cat > "$TMP/bin/picotool" <<'EOF'
#!/usr/bin/env bash
. "$OTPSTATE"
case $1 in
info) echo "Program Information"; exit 0 ;;
save)
  [[ $5 == *.* ]] || { echo "ERROR: filename '$5' does not have a recognized file type (extension)"; exit 157; }
  if [[ $enable == 0x000020 || $READABLE == 1 ]]; then
      dd if=/dev/zero of="$5" bs=1 count=$SAVELEN 2>/dev/null; exit 0
  fi
  echo "ERROR: The RP2350 device returned an error: permission failure"; exit 1 ;;
otp)
  shift; sub=$1; shift
  rows=(); for a in "$@"; do [[ $a == -* ]] || rows+=("$a"); done
  if [[ $sub == get ]]; then
    for r in "${rows[@]}"; do
      case $r in
        0x048) echo "ROW 0x0048: OTP_DATA_BOOT_FLAGS0"; echo; echo "    VALUE $enable"; echo ;;
        0x049) echo "ROW 0x0049"; echo; echo "    VALUE $enable"; echo ;;
        0x04a) echo "ROW 0x004a"; echo; echo "    VALUE $enable"; echo ;;
        0x054) echo "ROW 0x0054: OTP_DATA_FLASH_DEVINFO"; echo
               [[ $WARN == 1 ]] && echo "    RAW_VALUE=0x1299af (WARNING - ECC IS INVALID)"
               echo "    VALUE $devinfo"; echo ;;
        0x055) echo "ROW 0x0055: OTP_DATA_FLASH_PARTITION_SLOT_SIZE"; echo; echo "    VALUE $slot"; echo ;;
      esac
    done
    exit 0
  fi
  case ${rows[0]} in
    0x054) sed -i.bak "s/devinfo=.*/devinfo=0x3a99af/" "$OTPSTATE" ;;
    0x048) sed -i.bak "s/enable=.*/enable=0x000020/"   "$OTPSTATE" ;;
  esac
  echo "ROW 0x0048  OLD_VALUE=0x000000"; exit 0 ;;
esac
EOF
chmod +x "$TMP/bin/onerom" "$TMP/bin/picotool"

# name | scan | starting devinfo | starting enable | starting 0x055 | readable | warn | save length | expected abort text ('' means it must finish)
CASES=(
"no device|none|0x000000|0x000000|0x000000|0|0|4096|no One ROM found"
"two devices|two|0x000000|0x000000|0x000000|0|0|4096|2 One ROMs connected"
"scan fails|scanfail|0x000000|0x000000|0x000000|0|0|4096|onerom scan failed with status 3"
"running, reboot works|runthenstop|0x000000|0x000000|0x000000|0|0|4096|"
"running, reboot loses it|runthengone|0x000000|0x000000|0x000000|0|0|4096|no One ROM found"
"running, stays running|running|0x000000|0x000000|0x000000|0|0|4096|device is still Running"
"virgin device|one|0x000000|0x000000|0x000000|0|0|4096|"
"flash readable, OTP set|one|0x3a99af|0x000020|0x000000|0|0|4096|already accessible"
"flash readable, OTP blank|one|0x000000|0x000000|0x000000|1|0|4096|already accessible"
"0x054 already written|one|0x3a99af|0x000000|0x000000|0|0|4096|row 0x0054 reads 0x3a99af"
"0x055 not blank|one|0x000000|0x000000|0x00abcd|0|0|4096|row 0x0055 reads 0x00abcd"
"picotool warns|one|0x000000|0x000000|0x000000|0|1|4096|picotool reported a warning"
"short read at the end|one|0x000000|0x000000|0x000000|0|0|2048|saved 2048 bytes"
)

pass=0; fail=0; only=${1:-}
for c in "${CASES[@]}"; do
    IFS='|' read -r name scan dev en slot readable warn savelen want <<<"$c"
    [[ -n $only && $name != *"$only"* ]] && continue
    printf 'devinfo=%s\nenable=%s\nslot=%s\n' "$dev" "$en" "$slot" > "$TMP/otp"
    rm -f "$TMP/rebooted"
    got=$(printf 'y\ny\ny\ny\n' | env PATH="$TMP/bin:$PATH" \
        SCAN="$scan" RSTATE="$TMP/rebooted" OTPSTATE="$TMP/otp" \
        READABLE="$readable" WARN="$warn" SAVELEN="$savelen" \
        "$SCRIPT" 2>&1)
    code=$?
    [[ -n $only ]] && { printf '%s\n' "$got"; }
    if [[ -z $want ]]; then
        ok=$([[ $code -eq 0 && $got == *"Done."* ]] && echo 1 || echo 0)
        expect="completes"
    else
        ok=$([[ $code -ne 0 && $got == *"$want"* ]] && echo 1 || echo 0)
        expect="aborts: $want"
    fi
    if [[ $ok == 1 ]]; then printf 'ok    %-26s %s\n' "$name" "$expect"; pass=$((pass+1))
    else printf 'FAIL  %-26s %s\n  got exit %s:\n%s\n' "$name" "$expect" "$code" "$got"; fail=$((fail+1)); fi
done
printf '\n%d passed, %d failed\n' "$pass" "$fail"
[[ $fail -eq 0 ]]
