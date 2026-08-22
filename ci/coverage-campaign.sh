#!/usr/bin/env bash
# Run every (board, config) in ci/coverage-campaign.txt.
#
# Usage: ci/coverage-campaign.sh [--keep]
#
#   --keep   leave existing tracefiles in place, so an interrupted campaign
#            can be resumed.  Without it the set is cleared first, because a
#            tracefile left over from a campaign definition that has since
#            changed is exactly what the report will refuse.
#
# This takes hours.  It is what moves the floors, not something to run in the
# edit loop - for that, run ci/coverage-run.sh against one board and config.
#
# The same script is what CI runs, so the two cannot drift into measuring
# different sets.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CAMPAIGN="$ROOT/ci/coverage-campaign.txt"
OUT="$ROOT/build/coverage"

[ -f "$CAMPAIGN" ] || { echo "missing $CAMPAIGN" >&2; exit 1; }

if [ "${1:-}" != "--keep" ]; then
    rm -f "$OUT"/*.info
fi

total=$(grep -cvE '^\s*(#|$)' "$CAMPAIGN")
n=0
grep -vE '^\s*(#|$)' "$CAMPAIGN" | while read -r board config; do
    n=$((n + 1))
    echo
    echo "########## [$n/$total] $board  $config"
    "$ROOT/ci/coverage-run.sh" "$board" "$config"
done

echo
echo "Campaign complete.  ci/coverage-report.sh for the figures."
