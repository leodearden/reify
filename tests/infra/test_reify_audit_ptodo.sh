#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo.sh
#
# Infra gate for the PTODO detector (task e / #4557):
#   (a) RATCHET  — live ptodo-baseline-gen fingerprints must be a subset of
#                  the committed crates/reify-audit/ptodo-baseline.txt
#                  (live - baseline = empty).
#   (b) SCENARIO 13 (hermetic) — a git-tracked code file carrying a fresh
#                  untracked marker produces fingerprints absent from an empty
#                  baseline, proving the ratchet fires red on new violations.
#
# Design invariant (PRD 6.6): fingerprint derivation lives ONLY in the
# ptodo-baseline-gen binary (the same ptodo::fingerprint path the ratchet uses).
# No fingerprint re-derivation happens in this bash file.
#
# SELF-MATCH SAFETY: this file must not contain any literal marker tokens that
# the PTODO structural lane sweeps for.  Marker text in scenario (b) is
# assembled from a shell variable at runtime so the written fixture carries a
# real token while this .sh source stays clean.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Graceful skip when required tools are absent.
for _tool in git cargo comm sort; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== PTODO detector infra gate ==="

# -----------------------------------------------------------------------
# Resolve ptodo-baseline-gen binary (ride freshness guard).
# The freshness guard rebuilds target/release/reify-audit (and all crate
# bins, incl. ptodo-baseline-gen) when the binary predates the last
# crates/reify-audit commit.
# -----------------------------------------------------------------------
REIFY_AUDIT_BIN="$REPO_ROOT/target/release/reify-audit"
GEN="$REPO_ROOT/target/release/ptodo-baseline-gen"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild "$REPO_ROOT" || true

# If the guard skipped or the gen binary is still absent, build explicitly.
if [ ! -x "$GEN" ]; then
    echo "ptodo-baseline-gen not found after freshness guard; building..." >&2
    cargo build --release -q -p reify-audit 2>/dev/null
fi

if [ ! -x "$GEN" ]; then
    echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen unavailable — skipping" >&2
    exit 0
fi

BASELINE="$REPO_ROOT/crates/reify-audit/ptodo-baseline.txt"

# -----------------------------------------------------------------------
# (a) RATCHET: live fingerprints (degraded-structural) must be a subset
#     of the committed baseline.
#     The committed baseline was generated WITH the task DB (a superset of
#     structural + liveness findings), so a structural-only live set is
#     guaranteed to be a subset of the baseline when the tree is clean.
#     comm -23 <(sorted live) <(sorted baseline) = lines in live NOT in baseline.
# -----------------------------------------------------------------------
echo ""
echo "--- (a) Ratchet: live fingerprints subset of committed baseline ---"

# Single EXIT trap covers all temp paths (ratchet + scenario).  Registering
# two separate traps would silently replace the first with the second, leaking
# LIVE_TMP on exit.
LIVE_TMP=""
FIX=""
FIX_LIVE=""
cleanup_all() {
    [ -n "$LIVE_TMP" ] && rm -f "$LIVE_TMP"
    [ -n "$FIX" ]      && rm -rf "$FIX"
    [ -n "$FIX_LIVE" ] && rm -f "$FIX_LIVE"
}
trap cleanup_all EXIT
LIVE_TMP="$(mktemp)"

# Run the generator in degraded-structural mode (no task DB).
# Stderr may emit a breadcrumb about missing DB — that is expected and ignored.
# Do NOT use || true here: a non-zero exit from the generator signals a broken
# detector binary, not a missing DB.  The generator exits 0 regardless of
# finding count; a non-zero exit is an infrastructure failure that must go red.
env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$REPO_ROOT" >"$LIVE_TMP" 2>/dev/null

# comm -23 requires both inputs sorted; the generator sorts internally but
# sort -u here is defensive.
NEW_IN_LIVE="$(comm -23 <(sort -u "$LIVE_TMP") <(sort -u "$BASELINE"))"

assert "live fingerprints are a subset of committed baseline (no ratchet regression)" \
    bash -c '[ -z "$1" ]' -- "$NEW_IN_LIVE"

# -----------------------------------------------------------------------
# (b) SCENARIO 13 (hermetic): a fresh untracked marker in a temp git
#     repo produces fingerprints absent from an empty baseline, proving
#     the gate would go red on a new violation.
#
#     SELF-MATCH SAFETY: the marker token is assembled from a variable
#     so this .sh source never contains a literal form.
# -----------------------------------------------------------------------
echo ""
echo "--- (b) Scenario 13: hermetic fixture detects fresh untracked marker ---"

FIX="$(mktemp -d)"
FIX_LIVE="$(mktemp)"

git -C "$FIX" init -q
mkdir -p "$FIX/src"

# Assemble the marker token at runtime so this source file never contains
# a literal swept token (the written fixture file carries the real marker).
M="TODO"
printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
git -C "$FIX" add -A

# Run the generator on the hermetic fixture in degraded-structural mode.
env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$FIX" >"$FIX_LIVE" 2>/dev/null || true

# The fixture live set must contain at least one untracked line for fresh.rs.
UNTRACKED_LINE="$(grep 'src/fresh.rs' "$FIX_LIVE" | grep ':: untracked ::' || true)"
assert "fixture live output contains an ':: untracked ::' line for src/fresh.rs" \
    bash -c '[ -n "$1" ]' -- "$UNTRACKED_LINE"

# comm -23 against an empty baseline must be non-empty (ratchet goes red).
NEW_IN_FIXTURE="$(comm -23 <(sort -u "$FIX_LIVE") /dev/null)"
assert "fixture live fingerprints are NOT in empty baseline (gate fires red)" \
    bash -c '[ -n "$1" ]' -- "$NEW_IN_FIXTURE"

# -----------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------
test_summary
