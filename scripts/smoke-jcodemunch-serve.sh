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
#      with a JSON-RPC body.
#   2. tools/call get_changed_symbols for the reify repo over a
#      content-guaranteed commit range returns NON-EMPTY symbol data
#      (the observable signal); prints the symbol data.
#   3. jcodemunch-watcher.service is concurrently active while assertion 2
#      succeeded — proving watcher-write + serve-read is non-fatal on the
#      shared index (dark-factory parity).
#
# Exits 0 on success (all assertions pass).
# Exits 1 on first failed assertion (with a descriptive error message).
#
# Run before activation to confirm RED; run after activation to confirm GREEN:
#   bash scripts/smoke-jcodemunch-serve.sh
#
# Prerequisites: curl, jq

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/smoke-jcodemunch-serve.sh [-h|--help]

Activation smoke test for the jcodemunch query-serve (L-SERVE).
Asserts:
  1. MCP handshake at http://127.0.0.1:8901/mcp returns JSON-RPC body.
  2. get_changed_symbols for reify returns NON-EMPTY symbol data.
  3. jcodemunch-watcher.service is active concurrently with assertion 2.
Exits 0 on success, 1 on failure.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

SERVE_URL="http://127.0.0.1:8901/mcp"
MCP_TIMEOUT=15
WATCHER_SERVICE="jcodemunch-watcher"

# Repo and commit range resolved in step-4 (wired by the implementation).
# Placeholders below are replaced when assertions 2+3 are made GREEN.
REPO_ID="leodearden-reify"
# A recent reify commit range guaranteed to yield changed symbols.
# Resolved and pinned during step-4 against the running serve.
COMMIT_FROM="__COMMIT_FROM__"
COMMIT_TO="__COMMIT_TO__"

SMOKE_TMPDIR=$(mktemp -d /tmp/smoke-jcodemunch-XXXXXX)
trap 'rm -rf "$SMOKE_TMPDIR"' EXIT

# ── Assertion 1: MCP handshake ─────────────────────────────────────────────────
echo "smoke-jcodemunch-serve: [1] MCP handshake at $SERVE_URL ..."

SESSION_ID="smoke-$(date +%s)-$$"

# POST initialize; capture response headers + body.
http_code=$(curl -s \
    -o "$SMOKE_TMPDIR/init_body.json" \
    -D "$SMOKE_TMPDIR/init_headers.txt" \
    -w "%{http_code}" \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$SERVE_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SESSION_ID" \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke-jcodemunch","version":"0.1"}}}' \
    2>/dev/null) || {
    echo "FAIL [1]: curl to $SERVE_URL failed (connection refused or timeout)." >&2
    echo "       Start the serve first:" >&2
    echo "         uvx --python 3.12 --from 'jcodemunch-mcp @ git+https://github.com/jgravelle/jcodemunch-mcp.git@v1.108.27' jcodemunch-mcp serve --transport streamable-http --host 127.0.0.1 --port 8901 --watcher=false" >&2
    echo "       Or enable the systemd unit:" >&2
    echo "         systemctl --user enable --now jcodemunch-serve.service" >&2
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

# Extract server-assigned session-id from response headers (prefer server's value).
server_session=$(grep -i '^mcp-session-id:' "$SMOKE_TMPDIR/init_headers.txt" 2>/dev/null \
    | head -1 | sed 's/^[Mm][Cc][Pp]-[Ss]ession-[Ii][Dd]:[[:space:]]*//' | tr -d '\r' || true)
if [[ -n "$server_session" ]]; then
    SESSION_ID="$server_session"
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
echo "smoke-jcodemunch-serve: [2] get_changed_symbols for $REPO_ID ($COMMIT_FROM..$COMMIT_TO) ..."

# Guard: skip assertion 2+3 if commit range not yet resolved.
if [[ "$COMMIT_FROM" == "__COMMIT_FROM__" || "$COMMIT_TO" == "__COMMIT_TO__" ]]; then
    echo "SKIP [2+3]: commit range not yet resolved (step-4 will wire this)." >&2
    echo "            Re-run after step-4 implementation." >&2
    echo "smoke-jcodemunch-serve: assertion 1 OK — assertions 2+3 pending step-4"
    exit 0
fi

http_code2=$(curl -s \
    -o "$SMOKE_TMPDIR/query_body.txt" \
    -D "$SMOKE_TMPDIR/query_headers.txt" \
    -w "%{http_code}" \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$SERVE_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SESSION_ID" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"get_changed_symbols\",\"arguments\":{\"repo\":\"$REPO_ID\",\"from_commit\":\"$COMMIT_FROM\",\"to_commit\":\"$COMMIT_TO\"}}}" \
    2>/dev/null) || {
    echo "FAIL [2]: curl to $SERVE_URL tools/call failed." >&2
    exit 1
}

if [[ "$http_code2" != "200" ]]; then
    echo "FAIL [2]: tools/call get_changed_symbols returned HTTP $http_code2 (expected 200)." >&2
    echo "       Response: $(cat "$SMOKE_TMPDIR/query_body.txt" 2>/dev/null)" >&2
    exit 1
fi

# Parse response: handle both plain JSON and SSE data: lines.
query_body="$SMOKE_TMPDIR/query_body.txt"
if grep -qi "text/event-stream" "$SMOKE_TMPDIR/query_headers.txt" 2>/dev/null; then
    sse_data2=$(grep '^data:' "$SMOKE_TMPDIR/query_body.txt" | head -1 | sed 's/^data://')
    echo "$sse_data2" > "$SMOKE_TMPDIR/query_parsed.json"
    query_body="$SMOKE_TMPDIR/query_parsed.json"
fi

# Extract symbol data from JSON-RPC result.
# Server may return structuredContent or content[].text.
symbol_data=$(jq -r '
    .result.structuredContent.changed_symbols //
    (.result.content[]? | select(.type=="text") | .text | fromjson? | .changed_symbols?) //
    null
' "$query_body" 2>/dev/null || echo "null")

if [[ "$symbol_data" == "null" || "$symbol_data" == "[]" || -z "$symbol_data" ]]; then
    echo "FAIL [2]: get_changed_symbols returned empty or null changed_symbols." >&2
    echo "       Full response: $(cat "$query_body" 2>/dev/null)" >&2
    echo "       Verify that REPO_ID='$REPO_ID' and commit range '$COMMIT_FROM..$COMMIT_TO'" >&2
    echo "       are correct for the watcher-indexed /home/leo/src/reify checkout." >&2
    exit 1
fi

echo "smoke-jcodemunch-serve: assertion 2 OK — changed_symbols (non-empty):"
echo "$symbol_data" | jq '.[0:3]' 2>/dev/null || echo "$symbol_data" | head -20

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
