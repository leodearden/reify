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
         (3) fused-memory MCP endpoint responsive.
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

# ── All assertions passed ─────────────────────────────────────────────────────
echo "smoke-predone-hook: OK  binary=$binary  service=active"
