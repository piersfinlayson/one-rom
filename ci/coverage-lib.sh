#!/usr/bin/env bash
# Shared by ci/coverage-run.sh and ci/coverage-report.sh.  Sourced, never run.
#
# What is here is what more than one of them has to agree about: what a
# tracefile says it describes, and which lcov a capture accepts.  A second copy
# of either is a copy nothing compares.

# The manifest of the source a tracefile describes, one "#SRC:<path> <hash>"
# line per file, written into the tracefile so the data and the description of
# what it measures cannot be separated.
#
# Merging tracefiles is a union, so one taken before an edit can only add
# coverage - it credits lines that may no longer exist, and says nothing about
# it.  Git state cannot detect this: the whole point of the tool is to run it
# before committing, so the commit never moves between the edit and the check.
# Hashing the source does detect it.
#
# The trees are hashed whole rather than per component, because one campaign
# measures all of them and an edit anywhere invalidates all of it.
coverage_src_manifest() {
    local root="$1" sum d
    sum=$(command -v sha256sum || command -v shasum) || {
        echo "neither sha256sum nor shasum found on PATH." >&2
        return 1
    }
    for d in "$root/firmware/src" "$root/firmware/include" "$root"/plugins/*/*/src; do
        [ -d "$d" ] || continue
        find "$d" -type f \( -name '*.c' -o -name '*.h' \) -print0 |
            sort -z | xargs -0 "$sum" |
            sed "s#$root/##" | awk '{printf "#SRC:%s %s\n", $2, $1}'
    done
}

# The version ci/lcov-version names is the minimum a capture accepts.  Below it
# lcov has no notion of LCOV_UNREACHABLE_START and reads it as an ordinary
# comment, so the lines the source says cannot run count as unreached and the
# floors fail with nothing saying why - a silence worth a check of its own.
# ci/install-lcov.sh installs the pinned version on a machine that has an older
# one.
coverage_require_lcov() {
    local root="$1" want have
    command -v lcov >/dev/null || {
        echo "lcov not found on PATH - install it with ci/install-lcov.sh." >&2
        return 1
    }
    want=$(tr -d '[:space:]v' < "$root/ci/lcov-version")
    have=$(lcov --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1)
    [ -n "$have" ] || {
        echo "cannot read a version from 'lcov --version'." >&2
        return 1
    }
    if [ "$(printf '%s\n%s\n' "$want" "$have" | sort -V | head -1)" != "$want" ]; then
        echo "lcov $have found, $want or newer needed." >&2
        echo "Older lcov ignores the source's LCOV_UNREACHABLE markers rather than" >&2
        echo "checking them, and measures the marked lines as unreached." >&2
        echo "Install the pinned version with ci/install-lcov.sh." >&2
        return 1
    fi
}
