#!/usr/bin/env bash
# Answer a One ROM signing request raised on GitHub.
#
# Usage: scripts/sign-request.sh [--pin PIN] ISSUE
#
# A One ROM's owner raises a signing request with onerom hardware
# request-signature, which prints a link to the signing-request issue form
# filled in with the One ROM's Chip ID, board type, board size and
# manufacturer. This script
# reads the issue and signs it with piers.rocks's Community key through
# onerom hardware sign, which prompts for the key's PIN unless --pin is used,
# and for confirmation.
# Once signed it posts the hardware commission command the owner runs and
# the hardware validate command that checks the result, then closes the issue
# as completed.
#
# An issue is rejected where a field is missing, the Chip ID isn't 16 hex
# digits or the manufacturer isn't onerom.org. It is also rejected where
# onerom hardware sign exits with code 2 for an invalid board type or board
# size, so board types and sizes are checked only by the CLI. The comment
# lists the reasons and how to raise a new request, and the issue is closed as
# not planned. A closed issue is left alone. When onerom hardware sign fails
# in any other way or isn't confirmed nothing is posted and the issue stays
# open.
#
# Requires curl, jq and onerom, and runs where the signing server is
# reachable. It reaches GitHub through its REST API.
#
# Set in the environment or in ~/.config/onerom/sign-request.env
# ($XDG_CONFIG_HOME/onerom/ where set). The environment overrides the file.
#   ONEROM_SIGNING_SERVER  The signing server's https address.
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

usage() {
    echo "usage: $0 [--pin PIN] ISSUE" >&2
    exit 2
}

PIN=
ISSUE=
while [ $# -gt 0 ]; do
    case $1 in
        --pin)
            # An empty PIN makes onerom hardware sign exit with code 2, which
            # closes the issue as an invalid board type or size.
            [ -n "${2:-}" ] || usage
            PIN=$2
            shift 2
            ;;
        *)
            [ -z "$ISSUE" ] || usage
            ISSUE=$1
            shift
            ;;
    esac
done
[[ "$ISSUE" =~ ^[0-9]+$ ]] || usage

CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/onerom/sign-request.env"
if [ -f "$CONFIG" ]; then
    environment=$(declare -p ONEROM_SIGNING_SERVER ONEROM_GITHUB_TOKEN ONEROM 2>/dev/null || true)
    # shellcheck source=/dev/null
    . "$CONFIG"
    # The environment's values replace the file's.
    eval "$environment"
fi

ONEROM="${ONEROM:-onerom}"

for tool in curl jq "$ONEROM"; do
    command -v "$tool" >/dev/null || { echo "$tool not found." >&2; exit 1; }
done
# onerom hardware sign exits with code 2 for an address that isn't https,
# which would look like an invalid board type or size.
[[ "${ONEROM_SIGNING_SERVER:-}" =~ ^https:// ]] || {
    echo "ONEROM_SIGNING_SERVER isn't an https address." >&2
    exit 1
}
# A CLI without hardware sign exits with code 2 as well, which would look like
# an invalid board type or size.
"$ONEROM" hardware sign --help >/dev/null 2>&1 || {
    echo "$(command -v "$ONEROM") doesn't support \`hardware sign\`." >&2
    exit 1
}
[ -n "${ONEROM_GITHUB_TOKEN:-}" ] || {
    echo "ONEROM_GITHUB_TOKEN isn't set." >&2
    exit 1
}

# Calls GitHub's REST API for the issue and prints the response. $1 is what
# the call does, for example "read issue 323", for the error message where it
# fails. $2 is the method, $3 the path after the issue's address and $4, where
# provided, the JSON body. It exits where GitHub can't be reached or returns
# an error.
github() {
    local args=(-sS -w '\n%{http_code}' -X "$2"
        -H "Accept: application/vnd.github+json"
        -H "Authorization: Bearer $ONEROM_GITHUB_TOKEN"
        -H "X-GitHub-Api-Version: 2022-11-28")
    [ $# -lt 4 ] || args+=(--data "$4")
    local response error
    # curl's own error, for example a host it couldn't resolve, goes into the
    # message rather than above it.
    error=$(mktemp)
    response=$(curl "${args[@]}" "https://api.github.com/repos/$REPO/issues/$ISSUE$3" 2>"$error") || {
        echo "Failed to $1: $(sed 's/^curl: //' "$error")" >&2
        rm -f "$error"
        exit 1
    }
    rm -f "$error"
    local status=${response##*$'\n'}
    response=${response%$'\n'*}
    if [[ "$status" != 2* ]]; then
        echo "Failed to $1: HTTP $status $(jq -r '.message // empty' <<<"$response" 2>/dev/null)" >&2
        exit 1
    fi
    printf '%s\n' "$response"
}

# Every comment the script posts ends with this line.
AUTOMATED="This is an automated message."

# Posts $1 as a comment on the issue, then closes it as $2, completed or
# not_planned. Once it's posted it prints the comment beneath a heading,
# indented, so it's clear in the terminal which text went to GitHub.
answer() {
    local comment="$1"$'\n\n'"$AUTOMATED"
    github "comment on issue $ISSUE" POST /comments \
        "$(jq -n --arg body "$comment" '{body: $body}')" >/dev/null
    echo "Posted to issue $ISSUE:"
    awk 'NF { $0 = "  " $0 } 1' <<<"$comment"
    github "close issue $ISSUE" PATCH "" \
        "$(jq -n --arg reason "$2" '{state: "closed", state_reason: $reason}')" >/dev/null
}

issue=$(github "read issue $ISSUE" GET "")
if jq -e .pull_request <<<"$issue" >/dev/null; then
    echo "$ISSUE is a pull request, not an issue." >&2
    exit 1
fi
state=$(jq -r .state <<<"$issue")
title=$(jq -r .title <<<"$issue")
body=$(jq -r '.body // ""' <<<"$issue" | tr -d '\r')

echo "Issue $ISSUE: $title"
if [ "$state" != "open" ]; then
    # GitHub records how and when an issue was closed. A rejected request is
    # closed as not planned and a signed one as completed.
    how=$(jq -r '.state_reason // empty | gsub("_"; " ")' <<<"$issue")
    when=$(jq -r '.closed_at // empty | .[0:10]' <<<"$issue")
    echo "Issue $ISSUE was closed${how:+ as $how}${when:+ on $when}." >&2
    echo "  Reopen it on GitHub to sign it." >&2
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
    local comment="This signing request cannot be fulfilled:"
    local reason
    for reason in "$@"; do
        comment+=$'\n'"- $reason"
    done
    comment+=$'\n\n'"Raise a new request with \`onerom hardware request-signature\`."
    answer "$comment" not_planned
    echo "Rejected and closed issue $ISSUE."
    exit 0
}

[ ${#reasons[@]} -eq 0 ] || reject "${reasons[@]}"

# hardware sign prints the values and prompts for confirmation on the
# terminal, and for the PIN unless --pin is used. Only the JSON goes to stdout.
# An invalid value on its command line makes it exit with code 2, and the only
# values from the issue not checked above are the board type and size. The
# member doesn't see the CLI's error message, which is written for whoever ran
# it. With --pin=PIN a PIN starting with - parses as a value, not an option.
status=0
signed=$("$ONEROM" hardware sign --chip-id "$chip_id" --board "$board" --size "$size" \
    --manufacturer "$MANUFACTURER" --signer "$ONEROM_SIGNING_SERVER" --key-id "$KEY_ID" \
    ${PIN:+"--pin=$PIN"} --json) ||
    status=$?
case $status in
    0) ;;
    2)
        echo "\`onerom hardware sign\` refused the board type or board size." >&2
        reject "$BOARD_LABEL or $SIZE_LABEL is invalid."
        ;;
    *)
        echo "\`onerom hardware sign\` failed. Issue $ISSUE is unchanged." >&2
        exit 1
        ;;
esac
# Nothing is printed where the signing isn't confirmed.
if ! command=$(jq -er .command <<<"$signed" 2>/dev/null); then
    echo "Not signed. Issue $ISSUE is unchanged." >&2
    exit 1
fi

comment="Signed."$'\n\n'"To commission this One ROM run:"$'\n\n```\n'"$command"$'\n```'
comment+=$'\n\n'"To check the One ROM is properly commissioned run:"$'\n\n```\nonerom hardware validate\n```'
answer "$comment" completed
echo "Signed and closed issue $ISSUE."
