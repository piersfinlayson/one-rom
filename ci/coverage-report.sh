#!/usr/bin/env bash
# Turn coverage tracefiles into figures, a line list, or a pass/fail gate.
#
# Usage: ci/coverage-report.sh [mode] [tracefile ...]
#
#   (no mode)          per-file table, grouped by component, with totals
#   --uncovered [PATH] the lines nothing reached, optionally for one file
#   --check            fail if any file is below its baseline floor
#   --raise            raise the baseline to today's figures
#
# Tracefiles default to every build/coverage/*.info, which is every
# (board, config) captured so far - so the same command gives one variant or
# all of them combined, depending on what you pass.
#
# Line coverage only.  Function records are present in a tracefile and are
# ignored here, and branch records are not even captured - ci/coverage-run.sh
# does not ask lcov for them.  That is deliberate: one number per file that
# anyone can reason about beats a richer one nobody reads.  It does mean this
# cannot see inside a switch, so an untested arm of a macro-generated dispatch
# reads as covered.
#
# Merging happens here rather than in lcov: for line coverage the rule is that
# a line is covered if any run covered it, which is a union, and doing it here
# keeps this script working with whatever lcov the machine has.  ci/coverage-run.sh
# is the only thing that needs lcov at all.
#
# The two figures serve different ends and both are needed.  The table is a
# ratchet - it exists so coverage cannot quietly go backwards, and so something
# new cannot be added with no test.  The line list is how you improve it: what
# is left after every tester, on every variant, is either a test nobody has
# written, a path the harness cannot reach, or something that genuinely needs
# real hardware, and only reading the lines tells you which.
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="$ROOT/ci/coverage-baseline.txt"
CAMPAIGN="$ROOT/ci/coverage-campaign.txt"
OUT_DIR="$ROOT/build/coverage"
GROUP_MAP="$ROOT/ci/coverage-groups.txt"
EXCLUDE="$ROOT/ci/coverage-exclude.txt"

# shellcheck source=ci/coverage-lib.sh
. "$ROOT/ci/coverage-lib.sh"

MODE="table"
FILTER=""
case "${1:-}" in
    --uncovered) MODE="uncovered"; shift
                 case "${1:-}" in *.c|*.h) FILTER="$1"; shift;; esac ;;
    --check)     MODE="check"; shift ;;
    --raise)     MODE="raise"; shift ;;
    --*)         echo "unknown option '$1'" >&2; exit 2 ;;
esac

if [ $# -gt 0 ]; then
    TRACEFILES="$*"
else
    TRACEFILES=$(ls "$OUT_DIR"/*.info 2>/dev/null || true)
fi
[ -n "$TRACEFILES" ] || { echo "no tracefiles - run ci/coverage-run.sh first" >&2; exit 1; }

# The floors belong to a set of runs, not to a codebase.  A floor raised from
# eight runs cannot be met by one - fewer runs reach fewer lines - so a check
# against a different set fails and reads as a regression when nothing
# regressed.  Comparing what is present against ci/coverage-campaign.txt is
# what stops that.
#
# Only when reporting on the whole set.  Naming tracefiles explicitly is how
# you look at one board, and that is a legitimate thing to want.
if [ $# -eq 0 ] && [ -f "$CAMPAIGN" ]; then
    missing=""
    while read -r board config; do
        [ -n "$board" ] || continue
        want="$OUT_DIR/$board--$(basename "$config" .json).info"
        [ -f "$want" ] || missing="$missing  $(basename "$want")\n"
    done <<EOF
$(grep -vE '^\s*(#|$)' "$CAMPAIGN")
EOF
    if [ -n "$missing" ]; then
        echo "The campaign in $CAMPAIGN is not fully captured - missing:" >&2
        printf "%b" "$missing" >&2
        echo "Run ci/coverage-campaign.sh, or name tracefiles explicitly to" >&2
        echo "report on a subset." >&2
        exit 1
    fi
fi

# Every tracefile carries a manifest of the source it describes, one #SRC:
# line per file with its hash.  Merging is a union, so a tracefile taken
# before an edit can only add coverage - it would credit lines that may no
# longer exist, and say nothing about it.
#
# Git state cannot catch that.  The tool is run before committing, so the
# commit does not move between the edit and the check.  Hashing the source
# does catch it.
#
# Refuse rather than quietly skip: skipping silently is the same failure in
# the other direction.
now=$(coverage_src_manifest "$ROOT")

for tf in $TRACEFILES; do
    was=$(grep '^#SRC:' "$tf" || true)
    [ -n "$was" ] || {
        echo "$(basename "$tf") has no source manifest - captured by an older" >&2
        echo "ci/coverage-run.sh.  Re-run it." >&2
        exit 1
    }
    if [ "$was" != "$now" ]; then
        echo "Source has changed since $(basename "$tf") was captured:" >&2
        diff <(printf '%s\n' "$was") <(printf '%s\n' "$now") |
            sed -n 's/^[<>] #SRC:\([^ ]*\).*/  \1/p' | sort -u | head -20 >&2
        echo "Re-run ci/coverage-run.sh - these figures describe different code." >&2
        exit 1
    fi
done

dest=/dev/stdout
if [ "$MODE" = raise ]; then
    dest="$(mktemp)"
    trap 'rm -f "$dest"' EXIT
fi

# shellcheck disable=SC2086
awk -v mode="$MODE" -v filter="$FILTER" -v groups="$GROUP_MAP" -v exclude="$EXCLUDE" \
    -v baseline="$BASELINE" -v root="$ROOT" '
function name_of(path,   i) {
    for (i = 1; i <= ngroup; i++)
        if (index(path, gpath[i]) == 1) return gname[i]
    ungrouped[path] = 1
    return "Ungrouped"
}
function n_ungrouped(   k, n) { for (k in ungrouped) n++; return n + 0 }
function excluded(path,   i) {
    for (i = 1; i <= nexcl; i++)
        if (index(path, excl[i]) == 1) return 1
    return 0
}
function pct(h, t) { return t ? 100.0 * h / t : 0 }

BEGIN {
    while ((getline line < groups) > 0) {
        if (line ~ /^ *#/ || line ~ /^ *$/) continue
        n = split(line, fld, /[ \t]+/)
        ngroup++; gpath[ngroup] = fld[1]
        gname[ngroup] = fld[2]
        for (i = 3; i <= n; i++) gname[ngroup] = gname[ngroup] " " fld[i]
    }
    while ((getline line < exclude) > 0) {
        if (line ~ /^ *#/ || line ~ /^ *$/) continue
        nexcl++; excl[nexcl] = line
    }
    while ((getline line < baseline) > 0) {
        if (line ~ /^ *#/ || line ~ /^ *$/) continue
        split(line, fld, /[ \t]+/); floor_of[fld[1]] = fld[2] + 0
    }
}

# Union the tracefiles: a line is covered if any run covered it.
/^SF:/  { sf = substr($0, 4); skip = excluded(sf); next }
skip    { next }
/^DA:/  {
    split(substr($0, 4), d, ",")
    key = sf SUBSEP d[1]
    if (!(key in seen)) { seen[key] = 1; files[sf] = 1; total[sf]++ }
    if (d[2] + 0 > 0 && !(key in covered)) { covered[key] = 1; hit[sf]++ }
    next
}

END {
    if (mode == "uncovered") {
        for (k in seen) {
            split(k, kp, SUBSEP)
            if (k in covered) continue
            if (filter != "" && index(kp[1], filter) == 0) continue
            miss[kp[1]] = miss[kp[1]] " " kp[2]
        }
        nout = 0
        for (f in miss) { out[++nout] = f }
        for (i = 1; i <= nout; i++)
            for (j = i + 1; j <= nout; j++)
                if (out[j] < out[i]) { t = out[i]; out[i] = out[j]; out[j] = t }
        for (i = 1; i <= nout; i++) {
            f = out[i]
            n = split(miss[f], ln, " ")
            for (a = 1; a <= n; a++) for (b = a + 1; b <= n; b++)
                if (ln[b] + 0 < ln[a] + 0) { t = ln[a]; ln[a] = ln[b]; ln[b] = t }
            printf "%s  (%d uncovered of %d)\n", f, total[f] - hit[f], total[f]
            line = "   "
            for (a = 1; a <= n; a++) {
                if (ln[a] == "") continue
                if (length(line) > 70) { print line; line = "   " }
                line = line " " ln[a]
            }
            if (line != "   ") print line
        }
        exit 0
    }

    nf = 0
    for (f in files) { flist[++nf] = f }
    for (i = 1; i <= nf; i++)
        for (j = i + 1; j <= nf; j++) {
            a = name_of(flist[i]) SUBSEP flist[i]; b = name_of(flist[j]) SUBSEP flist[j]
            if (b < a) { t = flist[i]; flist[i] = flist[j]; flist[j] = t }
        }

    if (mode == "raise") {
        print "# Per-file coverage floors.  A file may not drop below its floor."
        print "# Raised by ci/coverage-report.sh --raise.  Never lowered by hand"
        print "# without saying why in the commit."
        for (i = 1; i <= nf; i++) {
            f = flist[i]; rate = pct(hit[f], total[f])
            fl = (f in floor_of && floor_of[f] > rate) ? floor_of[f] : rate
            printf "%-52s %.1f\n", f, fl
        }
        exit 0
    }

    if (mode == "check") {
        bad = 0
        for (i = 1; i <= nf; i++) {
            f = flist[i]; rate = pct(hit[f], total[f])
            if (!(f in floor_of)) {
                printf "NEW    %-48s %5.1f%%  (no floor yet)\n", f, rate
                bad = 1; continue
            }
            if (rate + 0.05 < floor_of[f]) {
                printf "DROP   %-48s %5.1f%%  floor %.1f%%\n", f, rate, floor_of[f]
                bad = 1
            } else if (rate > floor_of[f] + 0.05) {
                printf "RAISE  %-48s %5.1f%%  floor %.1f%%\n", f, rate, floor_of[f]
                raisable++
            }
        }
        if (n_ungrouped()) {
            printf "\n%d file(s) match no entry in %s - the group map is stale\n", n_ungrouped(), groups
            for (u in ungrouped) print "   " u
            bad = 1
        }
        if (bad) { print "\ncoverage check FAILED"; exit 1 }
        if (raisable)
            printf "\ncoverage check passed - %d floor(s) can go up, run --raise\n", raisable
        else
            print "coverage check passed"
        exit 0
    }

    printf "%-52s %7s %7s %7s %7s\n", "File", "Lines", "Hit", "Rate", "Floor"
    printf "%-52s %7s %7s %7s %7s\n", "----", "-----", "---", "----", "-----"
    prev = ""
    for (i = 1; i <= nf; i++) {
        f = flist[i]; g = name_of(f)
        if (g != prev && prev != "") {
            printf "%-52s %7d %7d %6.1f%%\n\n", prev, gt, gh, pct(gh, gt)
            gt = 0; gh = 0
        }
        prev = g; gt += total[f]; gh += hit[f]
        at += total[f]; ah += hit[f]
        # A file with no floor is new, and --check is what fails on it.  Here
        # it just reads as having none.
        rate = pct(hit[f], total[f])
        if (f in floor_of) {
            fl = sprintf("%6.1f%%", floor_of[f])
            # Named, not left to the reader to spot by comparing two numbers.
            # BELOW is the failure the gate reports, and the headroom above a
            # floor is what --raise would take up.
            if (rate + 0.05 < floor_of[f])        mark = "  BELOW"
            else if (rate > floor_of[f] + 0.05)   mark = sprintf("  +%.1f", rate - floor_of[f])
            else                                  mark = ""
        } else {
            fl = "      -"
            mark = "  NEW"
        }
        printf "  %-50s %7d %7d %6.1f%% %s%s\n", f, total[f], hit[f], rate, fl, mark
    }
    if (prev != "") printf "%-52s %7d %7d %6.1f%%\n", prev, gt, gh, pct(gh, gt)
    printf "\n%-52s %7d %7d %6.1f%%\n", "ALL", at, ah, pct(ah, at)

    if (n_ungrouped()) {
        printf "\n%d file(s) match no entry in %s - the group map is stale,\n", n_ungrouped(), groups
        printf "so they are counted in ALL but belong to no component:\n"
        for (u in ungrouped) printf "   %s\n", u
        exit 1
    }
}
' $TRACEFILES > "$dest"

# --raise writes the baseline itself.  Leaving it to a shell redirect is how I
# truncated the file with a pipe while testing, and a floor nobody wrote down
# is a floor that does not exist.
if [ "$MODE" = raise ]; then
    if [ -f "$BASELINE" ] && diff -q "$BASELINE" "$dest" >/dev/null 2>&1; then
        echo "No floor moved - $BASELINE is already current."
    else
        [ -f "$BASELINE" ] && diff "$BASELINE" "$dest" |
            sed -n 's/^> \(.*\)/  raised: \1/p'
        cp "$dest" "$BASELINE"
        echo "Wrote $BASELINE"
    fi
fi
