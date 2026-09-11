#!/usr/bin/env bash
# scripts/smoke-jcodemunch-serve.sh
#
# Activation smoke test for the jcodemunch query-serve (L-SERVE).
#
# Design: docs/architecture-audit/jcodemunch-serve-activation.md
#         docs/prds/reify-audit-p1-jcodemunch-substrate.md §8 (leaf 1)
#
# Asserts:
#   1. MCP handshake (initialize → notifications/initialized) over
#      streamable-HTTP at http://127.0.0.1:8901/mcp returns HTTP 200
#      with a JSON-RPC body and a server-assigned Mcp-Session-Id header.
#   2. tools/call get_changed_symbols for the reify repo over a
#      content-guaranteed commit range returns NON-EMPTY symbol data
#      (the observable signal); prints the symbol data.
#   3. jcodemunch-watcher.service is concurrently active while assertion 2
#      succeeded — proving watcher-write + serve-read is non-fatal on the
#      shared index (dark-factory parity).
#
# Exits 0 on success (all assertions pass).
# Exits 1 on first failed assertion (with a descriptive error message).
# Exits 2 on CLI misuse.
#
# There is no persistent serve unit to activate against any more; bring one up
# for the duration of the run with the lifecycle wrapper, and tell this script
# which index identity that serve answers for:
#   bash scripts/with-jcodemunch-serve.sh --port 8901 -- \
#       bash scripts/smoke-jcodemunch-serve.sh --repo local/reify-4ae45bbd
#
# Prerequisites: curl, jq

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/smoke-jcodemunch-serve.sh [--repo <id>] [-h|--help]

Activation smoke test for the jcodemunch query-serve (L-SERVE).
Asserts:
  1. MCP handshake at http://127.0.0.1:8901/mcp returns JSON-RPC body
     and a server-assigned Mcp-Session-Id header.
  2. get_changed_symbols for the queried repo returns NON-EMPTY symbol data.
  3. jcodemunch-watcher.service is active concurrently with assertion 2.

Options:
  --repo <id>   Index identity to query (default: leodearden/reify).
                Pass local/reify-4ae45bbd against a serve spawned by
                scripts/with-jcodemunch-serve.sh — see the note at REPO_ID.

Exits 0 on success, 1 on a failed assertion, 2 on CLI misuse.
USAGE
}

cli_error() { echo "$(basename "$0"): $*" >&2; exit 2; }

REPO_ID_ARG=""
while [[ $# -gt 0 ]]; do
    # Normalise --repo=X into --repo X, so the value is validated in one place.
    if [[ "$1" == --repo=* ]]; then set -- --repo "${1#--repo=}" "${@:2}"; fi
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        --repo)
            # A valueless --repo must NOT fall back to the default: that would
            # silently query the husk identity the note below warns about.
            [[ $# -ge 2 && -n "$2" ]] || cli_error "--repo requires an index identity"
            REPO_ID_ARG="$2"
            shift 2
            ;;
        *)
            cli_error "unexpected argument: $1"
            ;;
    esac
done

SERVE_URL="http://127.0.0.1:8901/mcp"
MCP_TIMEOUT=15
# The QUERY budget is deliberately an order of magnitude above the handshake's,
# and must not be collapsed back into it.  The retired long-lived serve unit
# (deliberately unnamed here: it no longer ships) kept an in-process cache warm
# ACROSS smoke runs, so a query came back in 3-7 s and could safely share
# MCP_TIMEOUT.  scripts/with-jcodemunch-serve.sh spawns a FRESH serve per
# invocation and this script issues exactly ONE query per serve, so that single
# query is now ALWAYS the cold one: measured 34-100 s across four observations
# on two hosts.  The larger number costs a dead port nothing, because it is only
# ever paid by a serve that already ACCEPTED the connection — a refused
# connection fails instantly at the handshake below, which keeps MCP_TIMEOUT.
QUERY_TIMEOUT="${SMOKE_QUERY_TIMEOUT:-180}"
WATCHER_SERVICE="jcodemunch-watcher"

# Resolved during L-SERVE spike (task 4102, step-4) against the running serve.
# Repo identifier: the leodearden/reify index in ~/.code-index (schema v16).
# Source root used by the index for git ops: /home/leo/src/reify-analysis-spec-coverage
# (or /home/leo/src/reify once the watcher re-indexes the canonical checkout).
# Commit range: 3 commits ending at the index HEAD 27b212c (Merge task/3773 into main).
# Both commits exist in the canonical /home/leo/src/reify git history.
#
# THE DEFAULT IS THE HUSK — PASS --repo AGAINST ANY SERVE YOU BRING UP TODAY.
# `leodearden/reify` is what jcodemunch resolves for this checkout when nothing
# forces the identity lever: the pinned release ships `git_root_identity: True`,
# so a serve left to itself answers for the git remote. Every reify-owned
# invocation now forces JCODEMUNCH_GIT_ROOT_IDENTITY=0 instead —
# scripts/jcodemunch-index-reify.sh when indexing, scripts/with-jcodemunch-serve.sh
# when spawning a serve — so a serve brought up the sanctioned way answers for the
# per-path `local/reify-4ae45bbd` and holds NOTHING under the name below.
# reify-audit already derives the per-path identity rather than defaulting
# (task 6108, landed). This default survives only because the SINCE/UNTIL range
# above was resolved against the husk's index HEAD and has never been re-resolved
# against the per-path index — re-resolving it is what would let the default flip.
REPO_ID="${REPO_ID_ARG:-leodearden/reify}"
SINCE_SHA="00f56f1a20be3a66a0797663506280be4db9ccf3"
UNTIL_SHA="27b212c61cfe86bf57055d769921805e34d8b467"

SMOKE_TMPDIR=$(mktemp -d /tmp/smoke-jcodemunch-XXXXXX)
trap 'rm -rf "$SMOKE_TMPDIR"' EXIT

# ── Assertion 1: MCP handshake ─────────────────────────────────────────────────
echo "smoke-jcodemunch-serve: [1] MCP handshake at $SERVE_URL ..."

# POST initialize WITHOUT mcp-session-id — server assigns one and returns it
# in the response Mcp-Session-Id header (streamable-HTTP MCP protocol).
http_code=$(curl -s \
    -o "$SMOKE_TMPDIR/init_body.json" \
    -D "$SMOKE_TMPDIR/init_headers.txt" \
    -w "%{http_code}" \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$SERVE_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke-jcodemunch","version":"0.1"}}}' \
    2>/dev/null) || {
    echo "FAIL [1]: curl to $SERVE_URL failed (connection refused or timeout)." >&2
    echo "       There is no persistent serve unit any more.  The wrapper spawns" >&2
    echo "       one for the duration of a command and tears it down on exit:" >&2
    echo "         bash scripts/with-jcodemunch-serve.sh --port 8901 -- \\" >&2
    echo "             bash scripts/smoke-jcodemunch-serve.sh --repo local/reify-4ae45bbd" >&2
    echo "       --repo is not optional there: the wrapper forces" >&2
    echo "       JCODEMUNCH_GIT_ROOT_IDENTITY=0, so its serve answers for the per-path" >&2
    echo "       local/reify-<hash> index that scripts/jcodemunch-index-reify.sh" >&2
    echo "       maintains, not for the default husk — which would clear assertion 1" >&2
    echo "       and then fail assertion 2 with an empty result." >&2
    echo "       The wrapper owns the pinned jcodemunch version — do not pin one here." >&2
    echo "       See: docs/architecture-audit/jcodemunch-serve-activation.md" >&2
    exit 1
}

if [[ "$http_code" != "200" ]]; then
    echo "FAIL [1]: $SERVE_URL returned HTTP $http_code (expected 200)." >&2
    echo "       Response body: $(cat "$SMOKE_TMPDIR/init_body.json" 2>/dev/null)" >&2
    exit 1
fi

# Handle SSE response: extract the data: line if content-type is text/event-stream.
init_body="$SMOKE_TMPDIR/init_body.json"
if grep -qi "text/event-stream" "$SMOKE_TMPDIR/init_headers.txt" 2>/dev/null; then
    sse_data=$(grep '^data:' "$SMOKE_TMPDIR/init_body.json" | head -1 | sed 's/^data://')
    echo "$sse_data" > "$SMOKE_TMPDIR/init_parsed.json"
    init_body="$SMOKE_TMPDIR/init_parsed.json"
fi

if ! grep -q '"jsonrpc"' "$init_body" 2>/dev/null; then
    echo "FAIL [1]: $SERVE_URL initialize response has no JSON-RPC body." >&2
    echo "       Response: $(cat "$init_body" 2>/dev/null)" >&2
    exit 1
fi

# Extract server-assigned session-id from response headers. grep -i already
# anchors to the right header line case-insensitively (HTTP header names are
# case-insensitive per RFC 7230); strip everything up to the first colon so
# any casing of "Mcp-Session-Id" is handled, not just the canonical form.
SESSION_ID=$(grep -i '^mcp-session-id:' "$SMOKE_TMPDIR/init_headers.txt" 2>/dev/null \
    | head -1 | sed -E 's/^[^:]*:[[:space:]]*//' | tr -d '\r' || true)
if [[ -z "$SESSION_ID" ]]; then
    echo "FAIL [1]: initialize response carried no Mcp-Session-Id header (jcodemunch serve is expected to assign one; see docs/prds/jcodemunch-substrate-restoration.capability-manifest.md server-assigned-session-id-is-the-contract)." >&2
    echo "       Response headers: $(cat "$SMOKE_TMPDIR/init_headers.txt" 2>/dev/null)" >&2
    exit 1
fi

# POST notifications/initialized (fire-and-forget; server may return 202 or 200).
curl -s \
    -o /dev/null \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$SERVE_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SESSION_ID" \
    -d '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' \
    2>/dev/null || true

echo "smoke-jcodemunch-serve: assertion 1 OK (HTTP 200, JSON-RPC body)"

# ── Assertion 2: get_changed_symbols returns NON-EMPTY symbol data ─────────────
echo "smoke-jcodemunch-serve: [2] get_changed_symbols for $REPO_ID ($SINCE_SHA..$UNTIL_SHA) ..."

http_code2=$(curl -s \
    -o "$SMOKE_TMPDIR/query_body.txt" \
    -D "$SMOKE_TMPDIR/query_headers.txt" \
    -w "%{http_code}" \
    --max-time "$QUERY_TIMEOUT" \
    -X POST "$SERVE_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SESSION_ID" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"get_changed_symbols\",\"arguments\":{\"repo\":\"$REPO_ID\",\"since_sha\":\"$SINCE_SHA\",\"until_sha\":\"$UNTIL_SHA\"}}}" \
    2>/dev/null) || {
    echo "FAIL [2]: curl to $SERVE_URL tools/call failed (${QUERY_TIMEOUT}s budget)." >&2
    echo "       The handshake above already succeeded, so the serve is up and" >&2
    echo "       this is most likely the budget running out.  Every serve the" >&2
    echo "       wrapper spawns is cold, and a cold query has measured 34-100 s." >&2
    echo "       Buy more time for one run without editing this file:" >&2
    echo "         SMOKE_QUERY_TIMEOUT=360 bash scripts/with-jcodemunch-serve.sh --port 8901 \\" >&2
    echo "             -- bash scripts/smoke-jcodemunch-serve.sh --repo local/reify-4ae45bbd" >&2
    exit 1
}

if [[ "$http_code2" != "200" ]]; then
    echo "FAIL [2]: tools/call get_changed_symbols returned HTTP $http_code2 (expected 200)." >&2
    echo "       Response: $(cat "$SMOKE_TMPDIR/query_body.txt" 2>/dev/null)" >&2
    exit 1
fi

# Parse response: handle SSE (data: prefix) and plain JSON.
# SSE body: "event: message\ndata: {...}\n..."
# Plain JSON body: "{...}"
# Extract the JSON payload into a file, then check it.
query_raw="$SMOKE_TMPDIR/query_body.txt"
query_json="$SMOKE_TMPDIR/query_json.json"
if grep -q '^data:' "$query_raw" 2>/dev/null; then
    grep '^data:' "$query_raw" | head -1 | sed 's/^data:[[:space:]]*//' > "$query_json"
else
    cp "$query_raw" "$query_json"
fi

# Check for JSON-RPC error in the response body.
if jq -e '.result.content[]? | select(.type=="text") | .text | test("\"error\"")' \
        "$query_json" >/dev/null 2>&1; then
    err_text=$(jq -r '(.result.content[]? | select(.type=="text") | .text)' "$query_json" 2>/dev/null | head -c 300)
    echo "FAIL [2]: get_changed_symbols returned an error: $err_text" >&2
    exit 1
fi

# Non-emptiness check. The server returns result.content[0].text which is:
#   (a) MUNCH-encoded ("#MUNCH/1 ..." header) — presence = non-empty symbol data.
#   (b) Plain JSON with .changed_symbols / .added_symbols / .removed_symbols arrays.
# Write the text field to a temp file for inspection.
jq -r '(.result.content[]? | select(.type=="text") | .text) // empty' \
    "$query_json" > "$SMOKE_TMPDIR/result_text.txt" 2>/dev/null || true

result_text_file="$SMOKE_TMPDIR/result_text.txt"

if grep -q '^#MUNCH/1' "$result_text_file" 2>/dev/null; then
    # MUNCH-encoded: presence of the header confirms non-empty symbol data.
    munch_header=$(head -1 "$result_text_file")
    echo "smoke-jcodemunch-serve: assertion 2 OK — non-empty MUNCH-encoded symbol data:"
    echo "  $munch_header"
    # Print first few @N= reference definitions as the observable signal.
    grep '^@' "$result_text_file" | head -5 || true
elif [[ -s "$result_text_file" ]]; then
    # Plain JSON path — check symbol count.
    symbol_count=$(jq -r '
        (.changed_symbols | length) + (.added_symbols | length) + (.removed_symbols | length)
    ' "$result_text_file" 2>/dev/null || echo "0")
    if [[ "$symbol_count" -gt 0 ]]; then
        echo "smoke-jcodemunch-serve: assertion 2 OK — $symbol_count symbols:"
        jq '.changed_symbols[0:2], .added_symbols[0:1]' "$result_text_file" 2>/dev/null || true
    else
        echo "FAIL [2]: get_changed_symbols returned empty symbol data." >&2
        echo "       Full response (first 400 chars): $(head -c 400 "$query_json" 2>/dev/null)" >&2
        echo "       Verify REPO_ID='$REPO_ID', SINCE_SHA='$SINCE_SHA', UNTIL_SHA='$UNTIL_SHA'" >&2
        echo "       An empty result here is most often an IDENTITY mismatch, not a data" >&2
        echo "       gap: a serve from scripts/with-jcodemunch-serve.sh answers for the" >&2
        echo "       per-path local/reify-<hash> index.  Re-run with" >&2
        echo "       --repo local/reify-4ae45bbd." >&2
        exit 1
    fi
else
    echo "FAIL [2]: get_changed_symbols returned no result text." >&2
    echo "       Full response (first 400 chars): $(head -c 400 "$query_json" 2>/dev/null)" >&2
    exit 1
fi

# ── Assertion 3: watcher is concurrently active ────────────────────────────────
echo "smoke-jcodemunch-serve: [3] jcodemunch-watcher.service active concurrently ..."

if ! systemctl --user is-active "$WATCHER_SERVICE.service" >/dev/null 2>&1; then
    echo "FAIL [3]: jcodemunch-watcher.service is not active." >&2
    echo "       Watcher-write + serve-read concurrency could not be verified." >&2
    echo "       Start watcher: systemctl --user start jcodemunch-watcher.service" >&2
    exit 1
fi

echo "smoke-jcodemunch-serve: assertion 3 OK (watcher active while serve answered query)"

# ── All assertions passed ──────────────────────────────────────────────────────
echo "smoke-jcodemunch-serve: OK  serve=$SERVE_URL  repo=$REPO_ID  watcher=active"
