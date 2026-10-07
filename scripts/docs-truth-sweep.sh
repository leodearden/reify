#!/usr/bin/env bash
# docs-truth-sweep.sh — the deployed entrypoint of the docs-truth sweep. It
# binds scripts/docs-truth-sweep.py to THIS checkout: its freshness-guarded
# release reify-audit, the escalation endpoint its .mcp.json declares, and its
# data/audit-runs/ state file. reify-docs-truth-sweep.service runs it bare, so
# this file is the single owner of those flags; extra arguments (e.g.
# --dry-run) pass through to the sweep. Rationale: docs/notes/docs-truth-sweep.md.
#
# Exit codes are the sweep's own, plus 125 when no fresh, runnable
# reify-audit can be had (nothing was checked or raised).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${REIFY_TEST_REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
BIN="$PROJECT_ROOT/target/release/reify-audit"

# shellcheck source=scripts/reify-audit-freshness.sh
source "$SCRIPT_DIR/reify-audit-freshness.sh"

guard_rc=0
reify_audit_guard "$BIN" rebuild "$PROJECT_ROOT" || guard_rc=$?
if [ "$guard_rc" -ne 0 ] || [ ! -x "$BIN" ]; then
    echo "docs-truth-sweep: no fresh, runnable reify-audit at $BIN (freshness guard rc=$guard_rc); nothing was checked or raised" >&2
    exit 125
fi

exec python3 "$SCRIPT_DIR/docs-truth-sweep.py" \
    --reify-audit "$BIN" \
    --project-root "$PROJECT_ROOT" \
    --mcp-config "$PROJECT_ROOT/.mcp.json" \
    --state-file "$PROJECT_ROOT/data/audit-runs/docs-truth-sweep.json" \
    "$@"
