#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_budget_skip.sh
#
# Meta-test for tasks #4624 / #5962: budget-safe PTODO skip path.
#
# Invokes tests/infra/test_reify_audit_ptodo.sh as a subprocess under a
# controlled environment to pin TWO orthogonal properties of the budget-safe
# path — one about cost, one about honesty:
#
#   COST (#4624)    REIFY_AUDIT_NO_COLD_BUILD=1 must NOT invoke `cargo build`.
#                   The knob exists so a budget-constrained caller can run the
#                   infra suite without paying for a cold detector build.
#
#   HONESTY (#5962, esc-5405-7)
#                   With the binary ABSENT, that skip leaves EVERY scenario
#                   unexecuted.  Exiting 0 there would be a hard gate reporting
#                   green having asserted nothing, so the script must refuse —
#                   loudly, exit 1.  Declining to do the work is legitimate;
#                   declining to do the work and calling it a pass is not.
#
# The two are easy to conflate and must be proven together: satisfying COST by
# skipping is only sound while HONESTY stops the skip from being laundered into
# a pass, and satisfying HONESTY by rebuilding would violate COST.
#
# Design:
#   - A shim `cargo` on PATH writes a marker file when invoked.
#   - REIFY_AUDIT_BIN is set to a nonexistent path, so the freshness guard sees
#     an ABSENT binary (rc 75) — the zero-scenario partition, not the
#     present-but-stale one.
#   - REIFY_AUDIT_NO_COLD_BUILD=1 arms the budget-safe skip mode.
#
# Assertions:
#   (1) test_reify_audit_ptodo.sh exits 1 — an ABSENT binary is refused, not
#       laundered into a pass
#   (2) its combined output contains the specific refusal diagnostic, so an
#       unrelated nonzero exit cannot satisfy (1)
#   (3) its combined output contains the budget-safe skip message
#   (4) the shim cargo marker was NOT created (no cold build attempted)
#
# Together (1)+(2) prove the gate became LOUD, while (3)+(4) prove it did so
# WITHOUT reintroducing the cold build #4624 removed.
#
# PARTITION NOTE: this covers only the ABSENT-binary case.  The PRESENT-but-
# stale case remains a graceful exit-0 skip — scenarios (c)+(d)+(e) still run
# against the stale binary, so the run does assert something — and is covered
# by tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh, which passes a
# stale COPY and still expects exit 0.  Do not "unify" the two expectations.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO budget-safe skip meta-test (tasks #4624 / #5962) ==="

# Graceful skip when bash or the PTODO test script are absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_budget_skip.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# Setup: temp dir with shim cargo that writes a marker file when invoked.
# The shim exits 0 (like a real cargo) but never creates any binary — so the
# freshness guard will always re-see an absent binary after the shim runs.
# ---------------------------------------------------------------------------
BS_META_TMPDIR=$(mktemp -d /tmp/test-ptodo-budget-skip-XXXXXX)
trap 'rm -rf "$BS_META_TMPDIR"' EXIT

BS_MARKER="$BS_META_TMPDIR/cargo-was-invoked"
FAKE_BIN_PATH="$BS_META_TMPDIR/nonexistent-reify-audit-$$"

# Shim cargo: writes marker to prove invocation, then exits 0.
cat > "$BS_META_TMPDIR/cargo" <<EOF
#!/usr/bin/env bash
# Shim cargo for budget-safe skip meta-test — writes marker and exits 0.
touch '$BS_MARKER'
exit 0
EOF
chmod +x "$BS_META_TMPDIR/cargo"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<nonexistent>  — overrides the binary path (after impl)
#   REIFY_AUDIT_NO_COLD_BUILD=1    — arms the budget-safe skip
#   PATH=<shimdir>:$PATH           — shim cargo intercepts any cargo invocation
#
# Capture combined stdout+stderr for skip-message assertion.
# Use set +e so we can inspect the exit code independently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh under budget-safe env ---"

BS_OUTPUT_FILE="$BS_META_TMPDIR/ptodo-output"
set +e
REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_FILE" 2>&1
BS_EXIT=$?
set -e

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) Script must exit 1.  REIFY_AUDIT_BIN points at a nonexistent path, so the
#     freshness guard returns 75 and every scenario below its `-x` guards is
#     skipped — the run asserts NOTHING.  Exiting 0 there would let a hard gate
#     report green on zero executed assertions, which is exactly the false-green
#     that #4733's residual left open (esc-5405-7, closed by #5962).  Skipping
#     the work is fine; skipping it and claiming a pass is not.
assert "test_reify_audit_ptodo.sh exits 1 — a budget-safe skip of an ABSENT binary is a hard failure, not a graceful skip" \
    bash -c "[ '$BS_EXIT' -eq 1 ]"

# (2) ...and it must exit 1 for THAT reason.  A bare exit-code check is satisfied
#     by any unrelated failure — a `set -e` abort, a crash, a genuinely failing
#     scenario assertion, or a future regression that breaks the script early —
#     which would leave this meta-test green while the behaviour it exists to pin
#     silently disappeared.  Same false-green class as the bug being fixed.
#     Fixed-string match on the floor's own refusal diagnostic ties the assertion
#     to the specific code path, mirroring the rationale for (3) below.
assert "exit 1 came from the no-silent-green floor (refusal diagnostic present)" \
    bash -c "grep -qF 'refusing to report green for a hard gate that asserted nothing' '$BS_OUTPUT_FILE'"

# (3) Combined output must contain the specific budget-safe skip message emitted
#     by test_reify_audit_ptodo.sh when rc==75 (impl-ptodo-skip step, line ~70).
#     Use a fixed-string match to pin the assertion to the exact code path and
#     prevent an unrelated tool-absent skip (e.g. comm/sort not on PATH → "skipping")
#     from producing a false-green while the REIFY_AUDIT_NO_COLD_BUILD logic was
#     never exercised.
assert "output contains the budget-safe skip message (REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe))" \
    bash -c "grep -qF 'REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)' '$BS_OUTPUT_FILE'"

# (4) Shim cargo must NOT have been invoked — marker file must be absent.
#     A present marker proves a cold build was attempted, violating the budget-safe contract.
assert "shim cargo NOT invoked — no cold build attempted (marker file absent)" \
    bash -c "[ ! -f '$BS_MARKER' ]"

test_summary
