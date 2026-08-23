#!/usr/bin/env bash
# Host side C unit tests.
#
# These compile firmware C for the host and exercise it directly, which suits
# self-contained logic with no hardware dependency.  Tests that need the
# firmware running as a whole belong in the emulator instead - see
# ci/test-emu.sh and firmware/test.mk.
#
# Built with AddressSanitizer, so a memory error is reported precisely rather
# than showing up as an unexplained failure somewhere later.
#
# Compiled as C23, matching the Arm toolchain's default for the firmware
# proper, so the compiler has to be new enough to have it.  $CC defaults to the
# one pinned in ci/c-compiler-version, which ci/install-c-compiler.sh installs
# and which the coverage runs use as well - the -Werror below then gates on the
# same warnings wherever it runs.
#
# COVERAGE_C=1 builds instrumented into build/c-tests-cov/<name> and leaves the
# objects and their counters there for ci/coverage-run.sh to capture alongside
# the emulator testers'.  One directory per test, because gcov refuses a counter
# file written by a different build of the same object outright rather than
# overwriting it, and firmware/src/rtt.c is built here as well as by
# firmware/test.mk.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CC="${CC:-gcc-$(tr -d '[:space:]' < "$ROOT/ci/c-compiler-version")}"
command -v "$CC" >/dev/null || {
    echo "$CC not found on PATH - install it with ci/install-c-compiler.sh." >&2
    echo "To build with something else, set CC.  It has to have -std=gnu23," >&2
    echo "so GCC 14 or later, or Clang 18 or later - and the warnings it" >&2
    echo "gates on are then its own, not the ones CI fails on." >&2
    exit 1
}
COVERAGE_C="${COVERAGE_C:-0}"

CFLAGS="-std=gnu23 -g -O1 -fsanitize=address -Wall -Wextra -Werror"

if [ "$COVERAGE_C" = 1 ]; then
    COV_ROOT="$ROOT/build/c-tests-cov"
    rm -rf "$COV_ROOT"
else
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
fi

failed=0

# run_test <name> <compiler argument>...
#
# Arguments are split into sources and flags by extension, because the coverage
# build has to compile each source to a named object of its own rather than
# going straight from sources to an executable - gcov writes a counter file
# beside the object, and there is no object to write beside otherwise.
run_test() {
    local name="$1"
    shift
    local srcs=() flags=() objs=() a s obj dir bin
    for a in "$@"; do
        case "$a" in
            *.c) srcs+=("$a") ;;
            *)   flags+=("$a") ;;
        esac
    done

    echo "Building C test: $name..."
    if [ "$COVERAGE_C" = 1 ]; then
        dir="$COV_ROOT/$name"
        mkdir -p "$dir"
        bin="$dir/$name"
        for s in "${srcs[@]}"; do
            obj="$dir/$(basename "$s" .c).o"
            # shellcheck disable=SC2086
            "$CC" $CFLAGS --coverage "${flags[@]}" -c "$s" -o "$obj"
            objs+=("$obj")
        done
        # shellcheck disable=SC2086
        "$CC" $CFLAGS --coverage "${objs[@]}" -o "$bin"
    else
        bin="$TMP/$name"
        # shellcheck disable=SC2086
        "$CC" $CFLAGS "${flags[@]}" "${srcs[@]}" -o "$bin"
    fi

    echo "Running C test: $name..."
    if ! "$bin"; then
        echo "FAILED: $name"
        failed=1
    fi
}

# The RTT ring and formatter in firmware/src/rtt.c.  firmware/test/rtt/include.h
# stands in for firmware/include/include.h, so the test directory must come
# first on the include path; firmware/include follows it so that the real
# rtt.h - and the binary compatibility static asserts in it - are the ones
# under test.
run_test rtt \
    -I "$ROOT/firmware/test/rtt" \
    -I "$ROOT/firmware/include" \
    "$ROOT/firmware/src/rtt.c" \
    "$ROOT/firmware/test/rtt/test_rtt.c" \
    "$ROOT/firmware/test/rtt/test_fmt.c"

if [ "$failed" -ne 0 ]; then
    echo "C tests FAILED"
    exit 1
fi

echo "C tests passed"
