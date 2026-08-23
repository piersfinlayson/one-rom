#!/usr/bin/env bash
# Build instrumented, run testers, and capture one tracefile.
#
# Usage: ci/coverage-run.sh <board> <config> [tester ...]
#
# Testers are named in ci/coverage-testers.txt and default to all of them.
# Coverage from every tester in one invocation accumulates into a single
# tracefile, because the counters sit next to the objects and the objects are
# not rebuilt between testers of the same board and config.
#
# One tracefile per (board, config), written to build/coverage and stamped with
# the commit it was captured at.  ci/coverage-report.sh merges them, and is
# separate so that re-reporting - or reporting at a different granularity -
# never re-runs a build.  Changing board or config rebuilds the C, which is why
# the split is there at all: coverage cannot accumulate in place across those.
#
# Nothing is written if a tester fails.  A repo where a test fails is a repo in
# a bad state, and a coverage figure taken from it means nothing.
set -e
shopt -s nullglob

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TESTER_LIST="$ROOT/ci/coverage-testers.txt"
OUT="$ROOT/build/coverage"

all_testers() { grep -vE '^\s*(#|$)' "$TESTER_LIST" | awk '{print $1}'; }
target_for()  { grep -vE '^\s*(#|$)' "$TESTER_LIST" | awk -v n="$1" '$1 == n {print $2}'; }

usage() {
    echo "usage: $0 <board> <config> [tester ...]" >&2
    echo "  testers: $(all_testers | tr '\n' ' ')(default: all)" >&2
    exit 2
}

[ $# -ge 2 ] || usage
BOARD="$1"; CONFIG="$2"; shift 2
TESTERS="${*:-$(all_testers)}"

[ "$(uname -s)" = "Linux" ] || {
    echo "Coverage runs on Linux only (found $(uname -s)) - see ci/docker." >&2
    exit 1
}

# The version the pin names is the minimum a run accepts.  Below it lcov has no
# notion of LCOV_UNREACHABLE_START and reads it as an ordinary comment, so the
# lines the source says cannot run count as unreached and the floors fail with
# nothing saying why - a silence worth a check of its own.  ci/install-lcov.sh
# installs the pinned version on a machine that has an older one.
command -v lcov >/dev/null || {
    echo "lcov not found on PATH - install it with ci/install-lcov.sh." >&2
    exit 1
}
want=$(tr -d '[:space:]v' < "$ROOT/ci/lcov-version")
have=$(lcov --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1)
[ -n "$have" ] || { echo "cannot read a version from 'lcov --version'." >&2; exit 1; }
if [ "$(printf '%s\n%s\n' "$want" "$have" | sort -V | head -1)" != "$want" ]; then
    echo "lcov $have found, $want or newer needed." >&2
    echo "Older lcov ignores the source's LCOV_UNREACHABLE markers rather than" >&2
    echo "checking them, and measures the marked lines as unreached." >&2
    echo "Install the pinned version with ci/install-lcov.sh." >&2
    exit 1
fi

SUM=$(command -v sha256sum || command -v shasum) || {
    echo "neither sha256sum nor shasum found on PATH." >&2; exit 1; }
[ -f "$TESTER_LIST" ] || { echo "missing $TESTER_LIST" >&2; exit 1; }

mkdir -p "$OUT"

# Where the instrumented objects - and so the counter files - end up.  Not a
# maintained list: the firmware always, plus whichever plugin directories the
# build created.  nullglob is what makes an unmatched pattern disappear rather
# than reach a command as a literal, which matters on the first run when none
# of these exist yet.
gcov_dirs() {
    local d
    for d in "$ROOT/firmware/build-test-cov" "$ROOT"/plugins/*/*/build-host-cov; do
        [ -d "$d" ] && printf '%s\n' "$d"
    done
}

# The source the measurement describes, one line per file.
#
# A tracefile taken before an edit describes code that is no longer there, and
# because merging is a union it can only ever add coverage - so a stale one
# inflates the figure and says nothing.  Git state cannot detect this: the
# whole point of the tool is to run it before committing, so the commit never
# moves between the edit and the check.
#
# Hashing the source does detect it, and the manifest goes inside the
# tracefile so it cannot be separated from the data it describes.  The trees
# are hashed whole rather than per component, because one run measures all
# three and an edit anywhere invalidates all of it.
src_manifest() {
    local d
    for d in "$ROOT/firmware/src" "$ROOT/firmware/include" "$ROOT"/plugins/*/*/src; do
        [ -d "$d" ] || continue
        find "$d" -type f \( -name '*.c' -o -name '*.h' \) -print0 |
            sort -z | xargs -0 "$SUM" |
            sed "s#$ROOT/##" | awk '{printf "#SRC:%s %s\n", $2, $1}'
    done
}

# Counters are deleted, not zeroed.  A counter file written by a different
# build of the same object is not stale data to be reset - gcov refuses it
# outright with "overwriting an existing profile data with a different
# checksum", and the run's coverage is lost.  Deleting is the only thing that
# survives a board or config change.
for d in $(gcov_dirs); do
    find "$d" -name '*.gcda' -delete 2>/dev/null || true
done

tag="$BOARD--$(basename "$CONFIG" .json)"
echo "=== coverage: $tag [$(echo $TESTERS)]"

for t in $TESTERS; do
    target=$(target_for "$t")
    [ -n "$target" ] || { echo "unknown tester '$t' - see $TESTER_LIST" >&2; exit 2; }
    printf '    %-8s ' "$t"
    if COVERAGE_FW=1 COVERAGE_PLUGIN=1 BOARD="$BOARD" CONFIG="$CONFIG" \
        make --no-print-directory -C "$ROOT" "$target" >"$OUT/$tag.$t.log" 2>&1; then
        echo "ok"
    else
        echo "FAILED"
        echo >&2
        echo "$target failed - see $OUT/$tag.$t.log" >&2
        echo "No tracefile written: coverage from a repo with a failing test" >&2
        echo "says nothing about the repo." >&2
        exit 1
    fi
done

dirs=$(gcov_dirs)
[ -n "$dirs" ] || { echo "no instrumented objects - nothing ran" >&2; exit 1; }

capture_args=""
for d in $dirs; do capture_args="$capture_args --directory $d"; done
# Progress to /dev/null, errors kept.  lcov fails the capture, writing no
# tracefile at all, when a line inside an LCOV_UNREACHABLE region was reached -
# it names the file and line on stderr, and that message is the whole point of
# marking the region rather than excluding it.
# shellcheck disable=SC2086
if ! lcov --capture $capture_args --output-file "$OUT/$tag.raw" >/dev/null; then
    echo >&2
    echo "lcov failed to capture $tag - no tracefile written." >&2
    echo "An 'unreachable' error above means a line the source marks as" >&2
    echo "unreachable was reached: the code, the marker, or both are wrong." >&2
    exit 1
fi

# Paths as they are written everywhere else.  sed over the SF: lines rather
# than lcov --substitute, since the manifest is prepended in the same pass.
out="$OUT/$tag.info"
{ src_manifest; sed "s#^SF:$ROOT/#SF:#" "$OUT/$tag.raw"; } > "$out"
rm -f "$OUT/$tag.raw"

echo "    -> $out"
