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

# shellcheck source=ci/coverage-lib.sh
. "$ROOT/ci/coverage-lib.sh"

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

coverage_require_lcov "$ROOT"

# The compiler is pinned for the same reason lcov is: it owns the numbers.  A
# different gcc inlines differently and attributes lines differently, so a floor
# raised from one machine is not a floor another can meet.  gcov has to be the
# matching one - it reads a counter file only from its own version of gcc - and
# lcov is told which rather than left to find "gcov" on PATH, which is the
# distribution's and belongs to a different compiler.
#
# HOST_CC as well as CC, because the plugins' host builds take their compiler
# from that one - see HOST_CC in plugins/*/*/Makefile and in the testers' build
# scripts.  Left at its default it would be the distribution's cc, putting one
# compiler's counters and another's in the same capture.
CC_VERSION=$(tr -d '[:space:]' < "$ROOT/ci/c-compiler-version")
export CC="gcc-$CC_VERSION"
export HOST_CC="$CC"
GCOV="gcov-$CC_VERSION"

# rustc links the tester binaries, and its default driver is the distribution's
# cc.  The instrumented objects are the pinned compiler's, and the -lgcov the
# build scripts ask for has to be that compiler's runtime too - resolved by the
# driver, so the driver is the one to change.  Left alone the link succeeds
# against the wrong libgcov and the run writes no counters at all, which reads
# as "nothing was instrumented" rather than as a mismatch.
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C linker=$CC"
for t in "$CC" "$GCOV"; do
    command -v "$t" >/dev/null || {
        echo "$t not found on PATH - install it with ci/install-c-compiler.sh." >&2
        exit 1
    }
done

[ -f "$TESTER_LIST" ] || { echo "missing $TESTER_LIST" >&2; exit 1; }

mkdir -p "$OUT"

# Where the instrumented objects - and so the counter files - end up.  Not a
# maintained list: the firmware always, plus whichever plugin directories the
# build created.  nullglob is what makes an unmatched pattern disappear rather
# than reach a command as a literal, which matters on the first run when none
# of these exist yet.
gcov_dirs() {
    local d
    for d in "$ROOT/firmware/build-test-cov" "$ROOT"/plugins/*/*/build-host-cov \
             "$ROOT"/build/c-tests-cov/*; do
        [ -d "$d" ] && printf '%s\n' "$d"
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

# ONEROM_LOG=1 because the firmware's own boot logging is code under test.
# Every tester reads it and defaults it off, which leaves BOOT_LOGGING_EN false
# for the whole run - so main.c never calls log_init() or log_roms(), and all
# of firmware/src/log.c measures as unreached.  The output goes to the tester's
# log beside the tracefile.
for t in $TESTERS; do
    target=$(target_for "$t")
    [ -n "$target" ] || { echo "unknown tester '$t' - see $TESTER_LIST" >&2; exit 2; }
    printf '    %-8s ' "$t"
    if COVERAGE_FW=1 COVERAGE_PLUGIN=1 ONEROM_LOG=1 BOARD="$BOARD" CONFIG="$CONFIG" \
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
if ! lcov --capture --gcov-tool "$GCOV" $capture_args \
        --output-file "$OUT/$tag.raw" >/dev/null; then
    echo >&2
    echo "lcov failed to capture $tag - no tracefile written." >&2
    echo "An 'unreachable' error above means a line the source marks as" >&2
    echo "unreachable was reached: the code, the marker, or both are wrong." >&2
    exit 1
fi

# Paths as they are written everywhere else.  sed over the SF: lines rather
# than lcov --substitute, since the manifest is prepended in the same pass.
out="$OUT/$tag.info"
{ coverage_src_manifest "$ROOT"; sed "s#^SF:$ROOT/#SF:#" "$OUT/$tag.raw"; } > "$out"
rm -f "$OUT/$tag.raw"

echo "    -> $out"
