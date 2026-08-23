#!/usr/bin/env bash
# Run ci/coverage-run.sh inside a Linux container, from any host.
#
# Usage: ci/coverage-docker.sh <board> <config> [tester ...]
#        ci/coverage-docker.sh --campaign
#
# Coverage needs GNU gcc and libgcov, so ci/coverage-run.sh refuses to run
# anywhere but Linux.  This is how a developer on another platform gets the same
# figures: the same script, in the container ci/docker builds, with the tree
# copied in rather than mounted.
#
# Copied rather than mounted for two reasons.  Cargo keys its fingerprints on
# the host it built for, so a Linux build sharing rust/target with a macOS one
# makes each of them rebuild the whole workspace every time they alternate.  And
# a run that cannot write to the tree cannot leave anything behind in it.
#
# Tracefiles come back to build/coverage, which is where ci/coverage-report.sh
# looks for them.  Report separately, on the host - it reads tracefiles and
# needs no toolchain.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${COVERAGE_IMAGE:-onerom-build:latest}"
NAME="onerom-cov-run-$$"
WORK=/home/build/work

usage() {
    echo "usage: $0 <board> <config> [tester ...]" >&2
    echo "       $0 --campaign" >&2
    exit 2
}

[ $# -ge 1 ] || usage
[ "$1" = "--campaign" ] || [ $# -ge 2 ] || usage

command -v docker >/dev/null || { echo "docker not found on PATH." >&2; exit 1; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || {
    echo "Image $IMAGE not found - build it with ci/docker/build.sh." >&2
    exit 1
}

cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$NAME" -w /home/build "$IMAGE" sleep infinity >/dev/null
docker exec "$NAME" mkdir -p "$WORK"

# What to copy in.  git ls-files rather than a list of excludes: an exclude
# pattern for build output also matches rust/config/build and its siblings,
# which are build scripts and are source.
#
# apio and epio are their own repositories and so are not tracked here, but the
# firmware does not build without them.  Their build output is left behind: a
# host-built libepio.a copied into the container is the wrong architecture, and
# make sees a library newer than its sources and does not rebuild it, so the
# link fails on every epio symbol.
echo "=== copying the tree into $NAME"
{
    git -C "$ROOT" ls-files
    for d in firmware/apio firmware/epio; do
        [ -d "$ROOT/$d" ] || continue
        find "$d" -type f -not -path '*/.git/*' -not -path '*/build/*'
    done
} | COPYFILE_DISABLE=1 tar -C "$ROOT" -cf - -T - | docker exec -i "$NAME" tar -xf - -C "$WORK"

if [ "$1" = "--campaign" ]; then
    docker exec "$NAME" bash -lc "cd $WORK && ci/coverage-campaign.sh"
else
    docker exec "$NAME" bash -lc "cd $WORK && ci/coverage-run.sh $*"
fi

echo "=== collecting tracefiles"
mkdir -p "$ROOT/build/coverage"
docker exec "$NAME" bash -lc "cd $WORK && tar -cf - build/coverage" |
    tar -xf - -C "$ROOT"

ls "$ROOT"/build/coverage/*.info 2>/dev/null || true
echo "Report with: ci/coverage-report.sh"
