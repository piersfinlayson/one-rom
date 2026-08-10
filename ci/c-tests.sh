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
set -e

CC="${CC:-cc}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

CFLAGS="-std=gnu23 -g -O1 -fsanitize=address -Wall -Wextra -Werror"

failed=0

run_test() {
    local name="$1"
    shift
    echo "Building C test: $name..."
    # shellcheck disable=SC2086
    "$CC" $CFLAGS "$@" -o "$OUT/$name"
    echo "Running C test: $name..."
    if ! "$OUT/$name"; then
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
