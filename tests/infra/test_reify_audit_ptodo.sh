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
#   (c) EXIT-CODE HARD GATE (task η, #4559) — structural untracked High
#                  finding → reify-audit exits non-zero.  Runs whenever the
#                  binary is PRESENT, independent of staleness/RATCHET_SKIP.
#   (d) ORPHANED-CITE HARD GATE (task #4733) — a cite to a done task is
#                  classified orphaned→High → reify-audit exits non-zero.
#                  Hermetic (sqlite3 seeded tasks.db + --tasks-file []).
#                  Runs whenever binary is PRESENT and sqlite3 is available.
#
# Design invariant (PRD 6.6): fingerprint derivation lives ONLY in the
# ptodo-baseline-gen binary (the same ptodo::fingerprint path the ratchet uses).
# No fingerprint re-derivation happens in this bash file.
#
# Budget-safe restructure (task #4733):
#   Scenarios (a)+(b) are precision-sensitive (require a FRESH gen binary to
#   emit correct fingerprints) and are wrapped in a RATCHET_SKIP guard.
#   Scenarios (c)+(d) are STABLE across the warm-lane staleness window — a
#   present-but-stale binary emits High findings correctly — so they run
#   whenever REIFY_AUDIT_BIN is executable, regardless of RATCHET_SKIP.
#   This prevents the whole-test-skip bug where a stale warm-lane binary
#   caused the PTODO hard gate to be silently bypassed (incident 2026-06-22/23).
#
# SELF-MATCH SAFETY: this file must not contain any literal marker tokens that
# the PTODO structural lane sweeps for.  Marker text in scenarios (b)/(c)/(d)
# is assembled from shell variables at runtime so the written fixture carries a
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
for _tool in git cargo comm sort sqlite3; do
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
#
# Testability seam (task #4624): REIFY_AUDIT_BIN and REIFY_PTODO_GEN_BIN can
# be overridden by environment variables for hermetic meta-tests that need to
# exercise the budget-safe skip path without a real binary on disk.
# -----------------------------------------------------------------------
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-$REPO_ROOT/target/release/reify-audit}"
GEN="${REIFY_PTODO_GEN_BIN:-$REPO_ROOT/target/release/ptodo-baseline-gen}"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

# Use rebuild-budget-safe mode (task #4624): when REIFY_AUDIT_NO_COLD_BUILD=1
# and the binary is absent/stale, the guard returns 75 (EX_TEMPFAIL skip
# sentinel) instead of invoking `cargo build` inside the 20m run_all.sh wall.
#
# Task #4733 restructure: map 75 → RATCHET_SKIP=1 rather than exit 0.
# This lets scenarios (c)+(d) (the High-severity hard gate) still execute
# when the binary is PRESENT (only stale), while keeping the graceful-skip
# contract for scenarios (a)+(b) (the gen-driven ratchet, which is
# precision-sensitive and genuinely needs a fresh binary).
#
# A truly ABSENT binary cannot run anything; when REIFY_AUDIT_BIN is not
# executable, (c)+(d) skip gracefully with a one-line note.  That case is
# already defended upstream by verify.sh:1049's positive assertion that
# hard-aborts the plan if the pre-build produced no binary.
RATCHET_SKIP=0

set +e
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild-budget-safe "$REPO_ROOT" 2>&1
_guard_rc=$?
set -e

if [ "$_guard_rc" -eq 75 ]; then
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
    RATCHET_SKIP=1
fi

# If ratchet not yet skipped, ensure GEN is available.
# GEN checks are inside RATCHET_SKIP==0 because they only affect the ratchet
# path; the (c)+(d) hard gate runs from REIFY_AUDIT_BIN, not GEN.
if [ "${RATCHET_SKIP}" = "0" ]; then
    if [ ! -x "$GEN" ]; then
        if [ "${REIFY_AUDIT_NO_COLD_BUILD:-0}" = "1" ]; then
            echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen absent and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
            RATCHET_SKIP=1
        else
            echo "ptodo-baseline-gen not found after freshness guard; building..." >&2
            cargo build --release -q -p reify-audit 2>/dev/null
        fi
    fi

    if [ ! -x "$GEN" ]; then
        echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen unavailable — skipping ratchet" >&2
        RATCHET_SKIP=1
    fi
fi

BASELINE="$REPO_ROOT/crates/reify-audit/ptodo-baseline.txt"

# -----------------------------------------------------------------------
# Single EXIT trap covers all temp paths.  Registering two separate traps
# would silently replace the first with the second, leaking temps on exit.
# -----------------------------------------------------------------------
LIVE_TMP=""
FIX=""         # dirty fixture (scenario b/c): git repo with untracked marker
FIX_LIVE=""
FIX2=""        # scenario (c) clean-fixture temp dir
FIX2_RUNS=""   # scenario (c)/(d) empty runs-db file
FIX_D=""       # scenario (d) orphaned-cite fixture temp dir
cleanup_all() {
    # Use "|| true" to ensure each line exits 0 even when the variable is empty
    # ([ -n "" ] && rm exits 1 from the short-circuit, which would propagate as
    # the trap's exit code and override the script's exit status).
    [ -n "$LIVE_TMP"  ] && rm -f  "$LIVE_TMP"  || true
    [ -n "$FIX"       ] && rm -rf "$FIX"        || true
    [ -n "$FIX_LIVE"  ] && rm -f  "$FIX_LIVE"  || true
    [ -n "$FIX2"      ] && rm -rf "$FIX2"       || true
    [ -n "$FIX2_RUNS" ] && rm -f  "$FIX2_RUNS" || true
    [ -n "$FIX_D"     ] && rm -rf "$FIX_D"      || true
}
trap cleanup_all EXIT

# -----------------------------------------------------------------------
# (a)+(b) RATCHET and HERMETIC FIXTURE — gen-driven, precision-sensitive.
# Wrapped in RATCHET_SKIP==0 guard (task #4733).
# -----------------------------------------------------------------------
if [ "${RATCHET_SKIP}" = "0" ] && [ -x "$GEN" ]; then
    LIVE_TMP="$(mktemp)"

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
fi

# -----------------------------------------------------------------------
# (c) EXIT-CODE HARD GATE (task η, #4559): an untracked marker is
#     Severity::High → reify-audit exits NON-ZERO (exit code = High count).
#
#     Runs whenever REIFY_AUDIT_BIN is executable, independent of
#     staleness/RATCHET_SKIP (task #4733 fix).
#
#     Two hermetic cases using a dedicated dirty fixture (not $FIX from (b),
#     which may be absent when RATCHET_SKIP=1):
#       (c-dirty)  fresh.rs carries marker → non-zero exit
#       (c-clean)  repo with marker-free content → exit 0
#
#     VALIDATED DESIGN:
#       - An empty 0-byte file is an acceptable --runs-db for --pattern PTODO
#         (the CLI opens it but the PTODO lanes never read ctx.conn).
#       - env -u REIFY_PTODO_TASKS_DB prevents a stale env var from routing
#         liveness checks to an unexpected tasks DB.
#       - We test via EXIT CODE, not stream parsing (JSON goes to stderr;
#         the gate cares only about the process exit code = High-count).
#       - Uses structural High kind (untracked) which works without a
#         tasks.db; orphaned (liveness High) is exercised in scenario (d).
#
#     SELF-MATCH SAFETY: marker token assembled from $M at runtime.
# -----------------------------------------------------------------------
if [ -x "$REIFY_AUDIT_BIN" ]; then
    echo ""
    echo "--- (c) Exit-code hard gate: untracked → High → non-zero exit ---"

    # Set up the dirty fixture for (c).  When scenario (b) ran (RATCHET_SKIP=0),
    # $FIX is already a git repo with src/fresh.rs carrying the marker, so we
    # reuse it.  When RATCHET_SKIP=1 (stale binary, b skipped), $FIX is empty
    # and we create a fresh fixture so (c) is self-contained.
    M="TODO"
    if [ -z "$FIX" ]; then
        FIX="$(mktemp -d)"
        git -C "$FIX" init -q
        mkdir -p "$FIX/src"
        printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
        git -C "$FIX" add -A
    fi

    FIX2_RUNS="$(mktemp)"

    # Snapshot FAIL before scenario (c) begins.  @@HARDGATE_C_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (c) asserts — i.e. every assert
    # passed.  A broken gate (any FAIL increment) suppresses the sentinel so the
    # meta-test stays RED (fixes silent_pass_on_failure).  The token contains no
    # TODO/FIXME/HACK substring and appears only in echo lines — SELF-MATCH SAFETY.
    _fail_before_c=$FAIL

    # Guard: assert the precondition — $FIX must be set and src/fresh.rs must
    # be git-tracked.  A failed precondition is an infra failure, not a product
    # regression — fail early with a clear message.
    assert "(c-dirty) precondition: \$FIX set and src/fresh.rs git-tracked" \
        bash -c 'git -C "$1" ls-files --error-unmatch src/fresh.rs >/dev/null 2>&1' -- "$FIX"

    # (c-dirty) marker present → exactly 1 High finding → exit 1.
    # Asserting the exact code (1) distinguishes "gate fired" from "binary errored"
    # (e.g. IO misconfig exits 125, Rust panic exits 101).
    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX" \
            --runs-db "$FIX2_RUNS" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_dirty=$?
    set -e

    assert "(c-dirty) untracked marker → reify-audit exits 1 (exactly 1 High finding)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_dirty"

    # (c-clean) a clean repo has no High findings → exit 0.
    FIX2="$(mktemp -d)"
    git -C "$FIX2" init -q
    mkdir -p "$FIX2/src"
    printf '// no markers here — purely a comment\n' > "$FIX2/src/clean.rs"
    git -C "$FIX2" add -A

    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX2" \
            --runs-db "$FIX2_RUNS" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_clean=$?
    set -e

    assert "(c-clean) no markers → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_clean"

    # Emit passing-branch sentinel for scenario (c).  Gated on FAIL counter
    # unchanged — suppressed if any (c) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_c" ] && echo "@@HARDGATE_C_PASSED@@"

    # -----------------------------------------------------------------------
    # (d) ORPHANED-CITE HARD GATE (task #4733): a cite to a DONE task is
    #     classified orphaned→High → reify-audit exits NON-ZERO.
    #
    #     Hermetic recipe (mirrors crates/reify-audit/tests/cli.rs:1632-1716):
    #       - Temp git repo with a single cited marker in src/cited.rs
    #         (assembled from $M + $CITE_ID so this source never contains the
    #         literal swept form — SELF-MATCH SAFETY).
    #       - Seed <repo>/.taskmaster/tasks/tasks.db via sqlite3 with the
    #         production tasks schema + INSERT (master,4444,'done').
    #       - Write [] to a temp file for --tasks-file (bypasses MCP loader
    #         while the PTODO β liveness lane still reads the sqlite3 tasks.db).
    #       - env -u REIFY_PTODO_TASKS_DB prevents stale env from routing.
    #
    #     Two assertions:
    #       (d-orphan)  cited.rs + task done → orphaned High → exit 1
    #       (d-control) UPDATE task to pending → live cite → exit 0
    #
    #     VALIDATED DESIGN (from crates/reify-audit/tests/cli.rs §8.3):
    #       - src/cited.rs has ONLY the cited marker (no bare markers) so the
    #         structural untracked lane does NOT fire → exactly 1 High (orphaned).
    #       - The tasks.db is seeded AFTER git-add to mirror the untracked-in-
    #         worktree reality of a real merge verify.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (d) Orphaned-cite hard gate: done-task cite → orphaned → High → non-zero exit ---"

    FIX_D="$(mktemp -d)"
    git -C "$FIX_D" init -q
    mkdir -p "$FIX_D/src"

    # Assemble the cited marker token at runtime (SELF-MATCH SAFETY).
    M="TODO"
    CITE_ID="4444"
    printf '// %s(#%s): wire the orphaned-cite path\n' "$M" "$CITE_ID" > "$FIX_D/src/cited.rs"
    git -C "$FIX_D" add -A

    # Seed tasks.db AFTER the git commit (mirrors untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_D/.taskmaster/tasks"
    sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_D_TASKS_FILE="$FIX_D/tasks.json"
    printf '[]' > "$FIX_D_TASKS_FILE"

    # Snapshot FAIL before scenario (d) begins.  @@HARDGATE_D_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (d) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel — SELF-MATCH SAFETY as above.
    _fail_before_d=$FAIL

    # (d-orphan) done task → orphaned → High → exit 1.
    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_D" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_D_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_orphan=$?
    set -e

    assert "(d-orphan) orphaned cite (#${CITE_ID}) → done-task → reify-audit exits 1 (exactly 1 High)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_orphan"

    # (d-control) UPDATE task status to pending → live cite → no High → exit 0.
    sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID};"

    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_D" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_D_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_live=$?
    set -e

    assert "(d-control) pending-task cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_live"

    # Emit passing-branch sentinel for scenario (d).  Gated on FAIL counter
    # unchanged — suppressed if any (d) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_d" ] && echo "@@HARDGATE_D_PASSED@@"
else
    echo ""
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent — (c)+(d) hard gate skipped (graceful)" >&2
fi

# -----------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------
test_summary
