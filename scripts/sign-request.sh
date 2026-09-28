#!/usr/bin/env bash
# Answer a One ROM signing request raised on GitHub.
#
# Usage: scripts/sign-request.sh ISSUE
#
# A One ROM's owner raises a signing request with onerom hardware
# request-signature, which prints a link to the signing-request issue form
# filled in with the One ROM's Chip ID, board type, board size and
# manufacturer. This script
# reads the issue and signs it with piers.rocks's Community key through
# onerom hardware sign, which asks for the key's PIN and for confirmation.
# Once signed it posts the hardware commission command the owner runs and
# closes the issue as completed.
#
# It rejects an issue with a missing field, a Chip ID that isn't 16 hex
# digits or a manufacturer other than onerom.org. It also rejects one whose
# board type or board size onerom hardware sign refuses, which it shows by
# exiting with code 2, so board types and sizes are checked only by the CLI.
# The comment says why and asks for a new request, and the issue is closed as
# not planned. A closed issue is left alone. When onerom hardware sign fails
# in any other way or isn't confirmed nothing is posted and the issue stays
# open.
#
# Requires curl, jq and onerom, and runs where the signing server is
# reachable. It reaches GitHub through its REST API.
#
# Environment:
#   ONEROM_SIGNING_SERVER  The signing server's address, for example
#                          https://onerom-sign.internal.packom.net:8443
#   ONEROM_GITHUB_TOKEN    A fine-grained GitHub personal access token that can
#                          read and write the repository's issues.
#   ONEROM                 The onerom binary. Defaults to onerom on PATH.
set -euo pipefail

REPO="piersfinlayson/one-rom"

# The manufacturer and signing key of every community signing request.
# onerom-cli declares both, and a test there checks these copies.
MANUFACTURER="onerom.org"
KEY_ID=2

# The issue form's labels. GitHub writes each field's value beneath its label
# in an issue's body. A test in onerom-cli checks them against the form.
CHIP_ID_LABEL="Chip ID"
BOARD_LABEL="Board type"
SIZE_LABEL="Board size"
MANUFACTURER_LABEL="Manufacturer"

ONEROM="${ONEROM:-onerom}"

usage() {
    echo "usage: $0 ISSUE" >&2
    exit 2
}

[ $# -eq 1 ] || usage
ISSUE=$1
[[ "$ISSUE" =~ ^[0-9]+$ ]] || usage

for tool in curl jq "$ONEROM"; do
    command -v "$tool" >/dev/null || { echo "$tool not found." >&2; exit 1; }
done
# An address onerom hardware sign refuses would look like a refused request,
# so a mistake here must not reach it.
[[ "${ONEROM_SIGNING_SERVER:-}" =~ ^https:// ]] || {
    echo "ONEROM_SIGNING_SERVER must be set to the signing server's https address." >&2
    exit 1
}
[ -n "${ONEROM_GITHUB_TOKEN:-}" ] || {
    echo "ONEROM_GITHUB_TOKEN must be set to a GitHub token that can write issues." >&2
    exit 1
}

# Calls GitHub's REST API for the issue. $1 is the method, $2 the path after
# the issue's address and $3, where given, the JSON body. It prints the
# response and fails on an HTTP error.
github() {
    local args=(-sS --fail-with-body -X "$1"
        -H "Accept: application/vnd.github+json"
        -H "Authorization: Bearer $ONEROM_GITHUB_TOKEN"
        -H "X-GitHub-Api-Version: 2022-11-28")
    [ $# -lt 3 ] || args+=(--data "$3")
    curl "${args[@]}" "https://api.github.com/repos/$REPO/issues/$ISSUE$2"
}

# Posts $1 as a comment on the issue, then closes it as $2, completed or
# not_planned.
answer() {
    github POST /comments "$(jq -n --arg body "$1" '{body: $body}')" >/dev/null
    github PATCH "" "$(jq -n --arg reason "$2" '{state: "closed", state_reason: $reason}')" >/dev/null
}

issue=$(github GET "")
if jq -e .pull_request <<<"$issue" >/dev/null; then
    echo "$ISSUE is a pull request, not an issue." >&2
    exit 1
fi
state=$(jq -r .state <<<"$issue")
title=$(jq -r .title <<<"$issue")
body=$(jq -r '.body // ""' <<<"$issue" | tr -d '\r')

echo "Issue $ISSUE: $title"
if [ "$state" != "open" ]; then
    echo "Issue $ISSUE is closed." >&2
    exit 1
fi

# The value beneath the heading "### $1" in the issue's body, without the
# spaces around it. Empty where the heading is missing or GitHub wrote
# "_No response_" for an empty field.
field() {
    awk -v heading="### $1" '
        $0 == heading { found = 1; next }
        found && /^### / { exit }
        found && NF { sub(/^[ \t]+/, ""); sub(/[ \t]+$/, ""); print; exit }
    ' <<<"$body" | sed 's/^_No response_$//'
}

chip_id=$(field "$CHIP_ID_LABEL")
board=$(field "$BOARD_LABEL")
size=$(field "$SIZE_LABEL")
manufacturer=$(field "$MANUFACTURER_LABEL")

# Why the request can't be signed, a line each.
reasons=()
if [ -z "$chip_id" ]; then
    reasons+=("$CHIP_ID_LABEL is missing.")
elif ! [[ "$chip_id" =~ ^[0-9A-Fa-f]{16}$ ]]; then
    reasons+=("$CHIP_ID_LABEL isn't 16 hex digits.")
fi
[ -n "$board" ] || reasons+=("$BOARD_LABEL is missing.")
[ -n "$size" ] || reasons+=("$SIZE_LABEL is missing.")
if [ -z "$manufacturer" ]; then
    reasons+=("$MANUFACTURER_LABEL is missing.")
elif [ "$manufacturer" != "$MANUFACTURER" ]; then
    reasons+=("$MANUFACTURER_LABEL isn't $MANUFACTURER.")
fi

# Closes the issue as not planned with a comment giving each of its
# arguments as a reason.
reject() {
    local comment="This signing request cannot be signed:"
    local reason
    for reason in "$@"; do
        comment+=$'\n'"- $reason"
    done
    comment+=$'\n\n'"Raise a new request with \`onerom hardware request-signature\`."
    printf '%s\n' "$comment"
    answer "$comment" not_planned
    echo "Rejected issue $ISSUE."
    exit 0
}

[ ${#reasons[@]} -eq 0 ] || reject "${reasons[@]}"

# hardware sign shows the values and asks for the PIN and for confirmation on
# the terminal. Its stdout carries only the JSON. It exits with code 2 where
# it refuses a value on its command line, and the only values the issue
# supplies that the checks above don't cover are the board type and size.
# The member doesn't see the CLI's message, which is written for someone who
# ran it.
status=0
signed=$("$ONEROM" hardware sign --chip-id "$chip_id" --board "$board" --size "$size" \
    --manufacturer "$MANUFACTURER" --signer "$ONEROM_SIGNING_SERVER" --key-id "$KEY_ID" --json) ||
    status=$?
case $status in
    0) ;;
    2) reject "$BOARD_LABEL or $SIZE_LABEL is invalid." ;;
    *)
        echo "onerom hardware sign failed. Issue $ISSUE is unchanged." >&2
        exit 1
        ;;
esac
# Nothing is printed where the signing isn't confirmed.
if ! command=$(jq -er .command <<<"$signed" 2>/dev/null); then
    echo "Not signed. Issue $ISSUE is unchanged." >&2
    exit 1
fi

comment="Signed."$'\n\n'"To commission this One ROM run:"$'\n\n```\n'"$command"$'\n```'
printf '%s\n' "$comment"
answer "$comment" completed
echo "Answered and closed issue $ISSUE."
