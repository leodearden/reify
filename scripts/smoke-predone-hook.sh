#!/usr/bin/env bash
# scripts/smoke-predone-hook.sh
#
# Activation smoke test for the F-infra pre-done hook (T-8).
#
# Design: docs/architecture-audit/f-infra-design.md §11
#
# Asserts that the pre-done gating loop is correctly wired on the host:
#   1. FUSED_MEMORY_PREDONE_HOOK_REIFY is set in the live fused-memory
#      service environment (systemd user unit).
#   2. The binary referenced by that env var is executable and responds
#      to --help.
#   (2.5) The env var value contains the required template tokens:
#          --task, {id}, and --pre-done (per design §11.1).
#   3. The fused-memory MCP endpoint at :8002 is responsive.
#
# Exits 0 on success (all assertions pass).
# Exits 1 on first failed assertion (with a descriptive error message).
#
# Run before activation to confirm RED; run after activation to confirm GREEN:
#   bash scripts/smoke-predone-hook.sh
#
# --- Manual MCP round-trip extension (run by hand on demand) ---
# The full gating loop can be verified by marking a phantom-done task
# 'done' via the fused-memory MCP and observing pre_done_hook_rejected:
#
#   # In a Python shell or httpx call against http://localhost:8002/mcp:
#   set_task_status(task_id="<phantom-task-id>", status="done",
#                   project_root="/home/leo/src/reify")
#   # Expected response:
#   #   {"success": false, "error": "pre_done_hook_rejected", ...}
#
# The upstream test suite (dark-factory/fused-memory/tests/test_pre_done_hook.py)
# covers the rejection-on-non-zero-exit invariant; this script covers wiring only.
# -----------------------------------------------------------------

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/smoke-predone-hook.sh [-h|--help]

Activation smoke test for the FUSED_MEMORY_PREDONE_HOOK_REIFY pre-done hook.
Asserts: (1) env var set in fused-memory service, (2) binary executable,
         (2.5) env value contains --task {id} --pre-done template tokens,
         (3) fused-memory MCP endpoint responsive,
         (4a/4b) binary round-trip with seeded fixtures (known-pass + known-fail).
Exits 0 on success, 1 on failure.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

SERVICE="fused-memory"
ENV_VAR="FUSED_MEMORY_PREDONE_HOOK_REIFY"
MCP_URL="http://localhost:8002/mcp"
MCP_TIMEOUT=5

# ── Assertion 1: env var is set in the live service environment ──────────────
echo "smoke-predone-hook: checking $ENV_VAR in $SERVICE service environment..."

service_env=$(systemctl --user show "$SERVICE" --property=Environment 2>/dev/null) || {
    echo "ERROR: systemctl --user show $SERVICE failed. Is the service known to systemd?" >&2
    exit 1
}

if ! echo "$service_env" | grep -qE "\b${ENV_VAR}="; then
    echo "FAIL: expected ${ENV_VAR} in $SERVICE service Environment= directives." >&2
    echo "      Run: systemctl --user show $SERVICE --property=Environment" >&2
    echo "      Then add: Environment=${ENV_VAR}=<path-to-binary> --task {id} --pre-done" >&2
    echo "      to /home/leo/.config/systemd/user/$SERVICE.service and reload." >&2
    exit 1
fi

# Extract the value of ENV_VAR from the Environment= output.
# systemd --property=Environment output format:
#   Environment=KEY1=VAL1 KEY2=VAL2 "KEY3=val with spaces" ...
# Values containing spaces are surrounded by double-quotes in the output.
# Try the quoted form first (handles values with spaces such as our templated hook),
# then fall back to the bare (no-space) form.
env_value=$(echo "$service_env" | grep -oE '"'"${ENV_VAR}"'=[^"]*"' | head -1 | tr -d '"' | cut -d= -f2- || true)
if [[ -z "$env_value" ]]; then
    env_value=$(echo "$service_env" | grep -oE "${ENV_VAR}=[^ ]+" | head -1 | cut -d= -f2-)
fi

if [[ -z "$env_value" ]]; then
    echo "FAIL: ${ENV_VAR} is set but has an empty value." >&2
    exit 1
fi

# ── Assertion 2: configured binary is executable ─────────────────────────────
echo "smoke-predone-hook: checking binary from ${ENV_VAR}..."

# Take the first token as the binary path (the rest are CLI args).
binary=$(echo "$env_value" | awk '{print $1}')

if [[ ! -x "$binary" ]]; then
    echo "FAIL: binary '$binary' is not executable (or does not exist)." >&2
    echo "      Install via: cargo install --path crates/reify-audit --root ~/.cargo --force" >&2
    exit 1
fi

if ! "$binary" --help >/dev/null 2>&1; then
    echo "FAIL: '$binary' --help failed (binary is present but crashes on --help)." >&2
    exit 1
fi

# ── Assertion 2.5: env value contains required template tokens ───────────────
echo "smoke-predone-hook: checking template tokens in ${ENV_VAR} value..."

if [[ "$env_value" != *"--task"* ]]; then
    echo "FAIL: env value missing template tokens. Expected '<binary> --task {id} --pre-done' per design §11.1; got: $env_value" >&2
    exit 1
fi

if [[ "$env_value" != *"{id}"* ]]; then
    echo "FAIL: env value missing template tokens. Expected '<binary> --task {id} --pre-done' per design §11.1; got: $env_value" >&2
    exit 1
fi

if [[ "$env_value" != *"--pre-done"* ]]; then
    echo "FAIL: env value missing template tokens. Expected '<binary> --task {id} --pre-done' per design §11.1; got: $env_value" >&2
    exit 1
fi

# ── Assertion 3: fused-memory MCP endpoint is responsive ─────────────────────
echo "smoke-predone-hook: probing MCP endpoint at $MCP_URL..."

initialize_payload='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke-test","version":"0.1"}}}'

http_response=$(curl -s -o /tmp/smoke-predone-mcp-resp.json -w "%{http_code}" \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -d "$initialize_payload" 2>/dev/null) || {
    echo "FAIL: curl to $MCP_URL failed (connection refused or timeout)." >&2
    echo "      Check: systemctl --user status $SERVICE" >&2
    exit 1
}

if [[ "$http_response" != "200" ]]; then
    echo "FAIL: fused-memory MCP endpoint at $MCP_URL returned HTTP $http_response (expected 200)." >&2
    exit 1
fi

if ! grep -q '"jsonrpc"' /tmp/smoke-predone-mcp-resp.json 2>/dev/null; then
    echo "FAIL: fused-memory MCP endpoint at $MCP_URL did not respond with a JSON-RPC body." >&2
    echo "      Response: $(cat /tmp/smoke-predone-mcp-resp.json 2>/dev/null)" >&2
    exit 1
fi

rm -f /tmp/smoke-predone-mcp-resp.json

# ── Assertion 4: binary round-trip with seeded fixtures ──────────────────────
# Tests a known-pass task (4a) and a known-fail task (4b) directly against the
# binary (not via the wrapper) to catch re-introduction of the dead
# .taskmaster/tasks/tasks.json default and output-format regressions.
#
# Design ref: task 3731 Part B; docs/architecture-audit/f-infra-design.md §11.
echo "smoke-predone-hook: running seeded fixture round-trips (assertions 4a, 4b)..."

SMOKE_TMPDIR=$(mktemp -d /tmp/smoke-predone-XXXXXX)
trap 'rm -rf "$SMOKE_TMPDIR"' EXIT

# Write tasks.json with two synthetic TaskMetadata shapes.
cat > "$SMOKE_TMPDIR/tasks.json" <<'TASKS_EOF'
[
  {
    "task_id": "smoke-pass-99991",
    "status": "pending",
    "files": [],
    "done_provenance": null,
    "title": "Smoke test known-pass task",
    "prd": null,
    "consumer_ref": null,
    "audit_foundation": null,
    "done_at": null
  },
  {
    "task_id": "smoke-fail-99992",
    "status": "done",
    "files": ["crates/reify-audit/src/lib.rs"],
    "done_provenance": {
      "kind": "manual",
      "commit": "0000000000000000000000000000000000000000",
      "note": null
    },
    "title": "Smoke test known-fail (phantom-done) task",
    "prd": null,
    "consumer_ref": null,
    "audit_foundation": null,
    "done_at": null
  }
]
TASKS_EOF

# Create a minimal runs.db with just the events table.
if ! sqlite3 "$SMOKE_TMPDIR/runs.db" "CREATE TABLE events (task_id TEXT, event_type TEXT);" 2>/dev/null; then
    echo "FAIL (4-setup): could not create seeded runs.db via sqlite3." >&2
    echo "  Ensure sqlite3 is installed (apt install sqlite3)." >&2
    exit 1
fi

# ── Sub-assertion 4a: known-pass → expect exit 0 ─────────────────────────────
set +e
"$binary" \
    --task smoke-pass-99991 \
    --pre-done \
    --tasks-file "$SMOKE_TMPDIR/tasks.json" \
    --runs-db    "$SMOKE_TMPDIR/runs.db" \
    --project-root "$SMOKE_TMPDIR" \
    >"$SMOKE_TMPDIR/pass.stdout" \
    2>"$SMOKE_TMPDIR/pass.stderr"
pass_exit=$?
set -e

if [[ "$pass_exit" -ne 0 ]]; then
    echo "FAIL (4a): known-pass task exited $pass_exit (expected 0)." >&2
    echo "  stdout: $(cat "$SMOKE_TMPDIR/pass.stdout")" >&2
    echo "  stderr: $(cat "$SMOKE_TMPDIR/pass.stderr")" >&2
    exit 1
fi

# Extract the trailing JSON block from stderr. The binary may emit git
# diagnostic warnings before the JSON array (see crates/reify-audit/tests/
# cli.rs:73-85, which uses rfind("\n[") for the same reason). $SMOKE_TMPDIR
# is not a git repo today, so the warning path is dormant — but mirroring
# the cli.rs helper keeps 4a robust to future binary changes.
if ! awk 'BEGIN{p=0} /^\[/{p=1} p{print}' "$SMOKE_TMPDIR/pass.stderr" \
        | jq -e 'type == "array"' >/dev/null 2>&1; then
    echo "FAIL (4a): known-pass stderr trailing block is not a JSON array (output-format regression)." >&2
    echo "  stderr: $(cat "$SMOKE_TMPDIR/pass.stderr")" >&2
    exit 1
fi

echo "smoke-predone-hook: assertion 4a OK (known-pass → exit 0, stderr JSON array)"

# ── Sub-assertion 4b: known-fail → expect non-zero AND not 125 ───────────────
set +e
"$binary" \
    --task smoke-fail-99992 \
    --pre-done \
    --tasks-file "$SMOKE_TMPDIR/tasks.json" \
    --runs-db    "$SMOKE_TMPDIR/runs.db" \
    --project-root "$SMOKE_TMPDIR" \
    >"$SMOKE_TMPDIR/fail.stdout" \
    2>"$SMOKE_TMPDIR/fail.stderr"
fail_exit=$?
set -e

if [[ "$fail_exit" -eq 0 ]]; then
    echo "FAIL (4b): known-fail task exited 0 (expected non-zero High-finding count)." >&2
    echo "  stderr: $(cat "$SMOKE_TMPDIR/fail.stderr")" >&2
    exit 1
fi

if [[ "$fail_exit" -eq 125 ]]; then
    echo "FAIL (4b): known-fail task exited 125 (infrastructure error — likely missing" >&2
    echo "  --tasks-file or output-format-parser regression)." >&2
    echo "  stderr: $(cat "$SMOKE_TMPDIR/fail.stderr")" >&2
    exit 1
fi

echo "smoke-predone-hook: assertion 4b OK (known-fail → exit $fail_exit, not 125)"

# ── All assertions passed ─────────────────────────────────────────────────────
echo "smoke-predone-hook: OK  binary=$binary  service=active"
