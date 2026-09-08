#!/usr/bin/env bash
# Check log format strings for conversions One ROM's formatter does not
# implement.
#
# The format(printf) attributes on the logging functions make the compiler
# check argument types, but they cannot catch this: standard printf supports
# %f, so GCC is perfectly happy with it, and the firmware would emit "%!f" at
# run time instead of a number.  This is the check that turns that into a build
# failure.
#
# Refused, and why:
#   f F e E g G a A   no floating point in this firmware
#   n                 writes through a pointer; a security hazard with no use
#   j t               intmax_t and ptrdiff_t modifiers, unused
#   w                 C23 bit precise forms, unused
#
# The space flag is deliberately not accepted by the pattern below.  Nothing in
# the tree uses it, and treating it as a flag would make every "50 % of" in a
# comment or message look like a conversion.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Sources whose log calls reach One ROM's formatter.  Vendored trees are
# excluded: they have their own printf, not ours.
DIRS=(
    firmware/src
    firmware/include
    firmware/ora/examples
    plugins/system/usb/src
    plugins/system/usb/include
    plugins/user/activity/src
    plugins/user/blink/src
    plugins/user/host-control/src
    plugins/user/rgb/src
)

FLAGS='[-+#0]*'
WIDTH='[0-9*]*'
PREC='(\.[0-9*]+)?'
LEN='(hh|h|ll|l|z)?'
PATTERN="%${FLAGS}${WIDTH}${PREC}${LEN}[fFeEgGaAn]|%${FLAGS}${WIDTH}${PREC}[jt][diouxX]|%${FLAGS}${WIDTH}w"

# Only lines that are logging calls; a format specifier elsewhere is not ours
# to police.
CALLS='(^|[^A-Za-z_])(LOG|ERR|DEBUG|ora_log|ora_err_log|ora_debug_log|log)[[:space:]]*\('

found=0
for d in "${DIRS[@]}"; do
    [ -d "$d" ] || continue
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        if [ "$found" -eq 0 ]; then
            echo "Unsupported conversion in a log format string:"
            echo
            found=1
        fi
        echo "  $line"
    done < <(grep -rnE --include='*.c' --include='*.h' "$CALLS" "$d" 2>/dev/null \
             | grep -vE '/old/' \
             | grep -E "$PATTERN" || true)
done

if [ "$found" -ne 0 ]; then
    echo
    echo "One ROM's formatter does not implement these; see the list in"
    echo "firmware/src/rtt.c.  At run time they log as a marker such as %!f."
    exit 1
fi

echo "Log format strings use only supported conversions"
