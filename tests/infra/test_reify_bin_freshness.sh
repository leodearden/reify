#!/usr/bin/env bash
# Unit tests for scripts/reify-bin-freshness.sh — the shared freshness guard
# library used by the PRD-gate tests (test_prd_gate_corpus.sh,
# test_prd_gate_objective_inheritance.sh) to refuse a verdict against a reify
# binary whose provenance is unproven — e.g. a cross-candidate leftover left
# behind in the shared _merge-verify warm lane by a sibling merge candidate
# (task #5133).
#
# Unlike scripts/reify-audit-freshness.sh (mtime vs. last crate-commit epoch),
# this guard compares a build-time HEAD-SHA sidecar (target/.reify-bin-sha)
# against HEAD at gate time — tree identity, not mtime — because a sibling
# binary built AFTER the candidate's newest source commit would be judged
# "fresh" by an mtime heuristic despite being built from the wrong tree.
#
# Tests:
#   1-2: Script exists and is sourceable
#   3:   resolve — no profile binary built → exit 2 (REIFY_BIN_NOT_BUILT)
#   4:   non-git repo_root → fail-open: reify_bin_unvouched_reason names no
#        reason, and resolve trusts the built binary
#   5:   reify_bin_unvouched_reason — sidecar SHA == HEAD → no reason
#   6:   reify_bin_unvouched_reason — sidecar SHA != HEAD (bogus) → names both
#   7:   reify_bin_unvouched_reason — sidecar absent (git repo_root) → "is absent"
#   8:   reify_bin_stamp — creates target/ + sidecar == HEAD
#   9:   round-trip — stamped repo + built bin → no reason, resolve trusts it
#   10:  reify_bin_stamp on a non-git dir → non-zero, no sidecar written
#   11:  resolve_trusted_reify_bin — explicit existing REIFY_BIN → trusted,
#        freshness bypassed entirely
#   12:  resolve_trusted_reify_bin — explicit REIFY_BIN missing path → SKIP,
#        reason mentions REIFY_BIN + missing
#   13:  resolve_trusted_reify_bin — no REIFY_BIN, no target bins → SKIP,
#        reason mentions "not built"
#   14:  resolve_trusted_reify_bin — fresh release bin → trusted, resolves
#        the release path
#   15:  resolve_trusted_reify_bin — fresh debug-only bin → trusted, resolves
#        the debug path
#   16:  resolve_trusted_reify_bin — release bin + stale sidecar → SKIP,
#        reason mentions "stale"
#   17:  resolve_trusted_reify_bin — release+debug both fresh → resolves
#        release (precedence)
#   18:  verify.sh merge-tier plan — reify-cli release pre-build + .reify-bin-sha
#        stamp lines both present and ordered BEFORE run_all.sh
#   19:  behavioral wiring — test_prd_gate_corpus.sh and
#        test_prd_gate_objective_inheritance.sh, run as black-box subprocesses
#        with REIFY_BIN pointing at a missing file, SKIP cleanly (exit 0,
#        output cites REIFY_BIN) instead of falling through to a
#        HARNESS_ERROR/FAIL
#
# Check 18 note (task 5125): run_all.sh's gating in verify.sh moved from the
# plain --include-infra tier to a DF_VERIFY_ROLE=merge-gated conditional
# (task 5125, landed just before this task started). The plain
# `verify.sh all --scope all --include-infra --print-plan` oracle no longer
# contains a run_all.sh line at all post-5125 (see
# test_verify_failfast_order.sh Test 6, assertion e). Check 18 instead
# mirrors that same Test 6's MERGE_ALL_PLAN oracle
# (`DF_VERIFY_ROLE=merge verify.sh all --scope all --print-plan`), which is
# where the sibling reify-audit pre-build/run_all.sh pairing now lives.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FRESHNESS_LIB="$REPO_ROOT/scripts/reify-bin-freshness.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== reify-bin-freshness.sh unit tests (task #5133) ==="

# Temp fixtures — cleaned up on EXIT.
TMPDIR_BINFRESH=$(mktemp -d /tmp/test-bin-freshness-XXXXXX)
trap 'rm -rf "$TMPDIR_BINFRESH"' EXIT

# make_fixture_repo <dir> — a real git repo with one commit, so the guard has a
# genuine HEAD to compare against (mirrors test_reify_audit_freshness.sh's
# fixture).  Prints that HEAD sha, and nothing else on stdout.  A && chain,
# because bash drops `set -e` inside $(...): a failed step must fail the call.
make_fixture_repo() {
    local dir="$1"
    mkdir -p "$dir" &&
        git -C "$dir" init -q &&
        touch "$dir/placeholder" &&
        git -C "$dir" add placeholder &&
        git -C "$dir" \
            -c user.name="Test" \
            -c user.email="test@test.com" \
            commit -qm "init" >/dev/null 2>&1 &&
        git -C "$dir" rev-parse HEAD
}

# place_binary <path> [mode] — an empty file standing in for a reify binary;
# executable unless a mode says otherwise.
place_binary() {
    mkdir -p "$(dirname "$1")"
    touch "$1"
    chmod "${2:-755}" "$1"
}

# run_captured <cmd...> — run <cmd>, recording its exit status in RUN_RC and its
# stdout and stderr in RUN_OUT and RUN_ERR (trailing newlines trimmed).
#
# REIFY_BIN and BASH_ENV are removed first.  An explicit REIFY_BIN is trusted
# outright, which would turn every discovery check into a REIFY_BIN check;
# bash runs BASH_ENV before the script, and what it prints would join stdout.
# A per-run variable goes in as an inner `env NAME=value <cmd>`.  The output
# goes through files, so stdout, stderr and the status stay apart, and
# `|| RUN_RC=$?` keeps the suite's `set -e` from tripping on a non-zero verdict.
run_captured() {
    RUN_RC=0
    env -u REIFY_BIN -u BASH_ENV "$@" \
        >"$TMPDIR_BINFRESH/run.stdout" 2>"$TMPDIR_BINFRESH/run.stderr" || RUN_RC=$?
    RUN_OUT=$(<"$TMPDIR_BINFRESH/run.stdout")
    RUN_ERR=$(<"$TMPDIR_BINFRESH/run.stderr")
}

# run_resolve <args...> — the executed form: bash reify-bin-freshness.sh <args>.
run_resolve() {
    run_captured bash "$FRESHNESS_LIB" "$@"
}

# run_unvouched_reason <repo_root> — the sourced reify_bin_unvouched_reason.  A
# sourcing failure is a non-zero RUN_RC, never an empty "vouched" answer.
run_unvouched_reason() {
    run_captured bash -c 'source "$1" && reify_bin_unvouched_reason "$2"' \
        _ "$FRESHNESS_LIB" "$1"
}

# text_contains <haystack> <needle> — an assert command: succeeds when haystack
# holds needle, and prints both when it does not (assert replays only what its
# command printed).
text_contains() {
    case "$1" in
        *"$2"*) return 0 ;;
    esac
    printf 'expected to contain: %s\nactual: %s\n' "$2" "$1"
    return 1
}

GIT_REPO="$TMPDIR_BINFRESH/repo"
HEAD_SHA=$(make_fixture_repo "$GIT_REPO")

# A non-git directory, for the fail-open case.  Its binary must EXIST, so that
# `resolve` has a candidate to vouch for: the fail-open is then what trusts it,
# not a "not built" refusal standing in front of it.
NON_GIT_DIR="$TMPDIR_BINFRESH/nongit"
NON_GIT_BIN="$NON_GIT_DIR/target/release/reify"
place_binary "$NON_GIT_BIN"

# A committed repo with no target/ at all: no binary and no sidecar.
RESOLVE_EMPTY_REPO="$TMPDIR_BINFRESH/resolve-empty"
make_fixture_repo "$RESOLVE_EMPTY_REPO" >/dev/null

# ==============================================================================
# Check 1: freshness lib exists
# ==============================================================================
echo ""
echo "--- Check 1: reify-bin-freshness.sh exists ---"

assert "scripts/reify-bin-freshness.sh exists" \
    test -f "$FRESHNESS_LIB"

# ==============================================================================
# Check 2: freshness lib is sourceable
# ==============================================================================
echo ""
echo "--- Check 2: reify-bin-freshness.sh is sourceable ---"

assert "reify-bin-freshness.sh can be sourced without error" \
    bash -c "source '$FRESHNESS_LIB'"

# ==============================================================================
# Check 3: no profile binary built → `resolve` exits 2 (REIFY_BIN_NOT_BUILT)
#          and its line says so
# ==============================================================================
echo ""
echo "--- Check 3: resolve reports a repo with no built binary as not built ---"

run_resolve resolve "$RESOLVE_EMPTY_REPO"

assert "resolve: no binary built → exit 2 (REIFY_BIN_NOT_BUILT)" \
    test "$RUN_RC" -eq 2

assert "resolve: no binary built → the line says 'not built'" \
    text_contains "$RUN_OUT" 'not built'

# ==============================================================================
# Check 4: a repo_root in which git finds no repository fails OPEN —
#          reify_bin_unvouched_reason names no reason, and `resolve` trusts the
#          binary it finds although there is no sidecar
# ==============================================================================
echo ""
echo "--- Check 4: a non-git repo_root fails open ---"

run_unvouched_reason "$NON_GIT_DIR"

assert "reify_bin_unvouched_reason: non-git repo_root → exit 0" \
    test "$RUN_RC" -eq 0

assert "reify_bin_unvouched_reason: non-git repo_root → no reason (fail-open)" \
    test -z "$RUN_OUT"

run_resolve resolve "$NON_GIT_DIR"

assert "resolve: non-git repo_root, binary built, no sidecar → exit 0 (trusted)" \
    test "$RUN_RC" -eq 0

assert "resolve: non-git repo_root → the built binary" \
    test "$RUN_OUT" = "$NON_GIT_BIN"

# ==============================================================================
# Check 5: sidecar SHA == HEAD → the sidecar vouches: no reason
# ==============================================================================
echo ""
echo "--- Check 5: a sidecar naming HEAD vouches for the build ---"

mkdir -p "$GIT_REPO/target"
printf '%s\n' "$HEAD_SHA" > "$GIT_REPO/target/.reify-bin-sha"

run_unvouched_reason "$GIT_REPO"

assert "reify_bin_unvouched_reason: sidecar SHA == HEAD → exit 0" \
    test "$RUN_RC" -eq 0

assert "reify_bin_unvouched_reason: sidecar SHA == HEAD → no reason" \
    test -z "$RUN_OUT"

# ==============================================================================
# Check 6: sidecar SHA != HEAD → a reason naming both SHAs
# ==============================================================================
echo ""
echo "--- Check 6: a sidecar that does not name HEAD is unvouched, naming both SHAs ---"

printf '%s\n' "0000000000000000000000000000000000000000" > "$GIT_REPO/target/.reify-bin-sha"

run_unvouched_reason "$GIT_REPO"

assert "reify_bin_unvouched_reason: sidecar SHA != HEAD (bogus SHA) → a reason" \
    test -n "$RUN_OUT"

assert "reify_bin_unvouched_reason: sidecar SHA != HEAD → the reason names the recorded SHA" \
    text_contains "$RUN_OUT" '0000000000000000000000000000000000000000'

assert "reify_bin_unvouched_reason: sidecar SHA != HEAD → the reason names HEAD" \
    text_contains "$RUN_OUT" "$HEAD_SHA"

# ==============================================================================
# Check 7: sidecar absent (git repo_root) → a reason saying so
# ==============================================================================
echo ""
echo "--- Check 7: an absent sidecar is unvouched ---"

rm -f "$GIT_REPO/target/.reify-bin-sha"

run_unvouched_reason "$GIT_REPO"

assert "reify_bin_unvouched_reason: sidecar absent → a reason" \
    test -n "$RUN_OUT"

assert "reify_bin_unvouched_reason: sidecar absent → the reason says 'is absent' (unproven provenance)" \
    text_contains "$RUN_OUT" 'is absent'

# ==============================================================================
# Check 8: reify_bin_stamp — creates target/.reify-bin-sha == HEAD (and target/ itself)
# ==============================================================================
echo ""
echo "--- Check 8: reify_bin_stamp creates the sidecar with HEAD's SHA ---"

STAMP_REPO="$TMPDIR_BINFRESH/stamp-repo"
STAMP_HEAD_SHA=$(make_fixture_repo "$STAMP_REPO")

# target/ does not exist yet in this fixture — reify_bin_stamp must create it.
assert "target/ does not yet exist in the stamp fixture (precondition)" \
    bash -c "[ ! -d '$STAMP_REPO/target' ]"

assert "reify_bin_stamp succeeds (creates target/ + sidecar)" \
    bash -c "source '$FRESHNESS_LIB' && reify_bin_stamp '$STAMP_REPO'"

assert "reify_bin_stamp creates target/.reify-bin-sha" \
    test -f "$STAMP_REPO/target/.reify-bin-sha"

assert "reify_bin_stamp sidecar contents (trimmed) equal HEAD" \
    bash -c "[ \"\$(cat '$STAMP_REPO/target/.reify-bin-sha')\" = '$STAMP_HEAD_SHA' ]"

# ==============================================================================
# Check 9: round-trip — after stamping, a built binary is vouched for:
#          reify_bin_unvouched_reason names no reason, and `resolve` trusts it
# ==============================================================================
echo ""
echo "--- Check 9: round-trip — stamped repo + built binary → trusted ---"

STAMP_BIN="$STAMP_REPO/target/release/reify"
place_binary "$STAMP_BIN"

run_unvouched_reason "$STAMP_REPO"

assert "reify_bin_unvouched_reason: stamped repo → exit 0" \
    test "$RUN_RC" -eq 0

assert "reify_bin_unvouched_reason: stamped repo → no reason after the reify_bin_stamp round-trip" \
    test -z "$RUN_OUT"

run_resolve resolve "$STAMP_REPO"

assert "resolve: stamped repo + built binary → exit 0 (trusted)" \
    test "$RUN_RC" -eq 0

assert "resolve: stamped repo + built binary → the built binary" \
    test "$RUN_OUT" = "$STAMP_BIN"

# ==============================================================================
# Check 10: reify_bin_stamp on a non-git dir → non-zero, no sidecar written
# ==============================================================================
echo ""
echo "--- Check 10: reify_bin_stamp on a non-git dir fails without writing a sidecar ---"

STAMP_NON_GIT_DIR="$TMPDIR_BINFRESH/stamp-nongit"
mkdir -p "$STAMP_NON_GIT_DIR"

assert "reify_bin_stamp on a non-git dir returns non-zero" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_bin_stamp '$STAMP_NON_GIT_DIR'"

assert "reify_bin_stamp on a non-git dir writes no sidecar" \
    bash -c "[ ! -f '$STAMP_NON_GIT_DIR/target/.reify-bin-sha' ]"

# ==============================================================================
# Check 11: resolve_trusted_reify_bin — explicit existing REIFY_BIN → trusted
#           (exit 0), REIFY_BIN_RESOLVED == that path, freshness bypassed
#           entirely (RESOLVE_EMPTY_REPO has NO target/ bins or sidecar at all —
#           if resolve fell through to auto-discovery it would find nothing).
# ==============================================================================
echo ""
echo "--- Check 11: resolve_trusted_reify_bin trusts an explicit existing REIFY_BIN ---"

EXPLICIT_BIN="$TMPDIR_BINFRESH/explicit-reify-bin"
place_binary "$EXPLICIT_BIN"

assert "resolve_trusted_reify_bin: explicit existing REIFY_BIN → trusted (exit 0)" \
    env REIFY_BIN="$EXPLICIT_BIN" bash -c "source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: explicit REIFY_BIN → REIFY_BIN_RESOLVED equals that path" \
    env REIFY_BIN="$EXPLICIT_BIN" bash -c "source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$EXPLICIT_BIN' ]"

# ==============================================================================
# Check 12: resolve_trusted_reify_bin — explicit REIFY_BIN pointing at a
#           missing path → SKIP (non-zero), reason mentions REIFY_BIN + missing.
# ==============================================================================
echo ""
echo "--- Check 12: resolve_trusted_reify_bin rejects a missing explicit REIFY_BIN ---"

MISSING_BIN="$TMPDIR_BINFRESH/does-not-exist-reify-bin"

assert "resolve_trusted_reify_bin: missing explicit REIFY_BIN → non-zero (SKIP)" \
    env REIFY_BIN="$MISSING_BIN" bash -c "source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: missing explicit REIFY_BIN → reason mentions REIFY_BIN and missing" \
    env REIFY_BIN="$MISSING_BIN" bash -c "source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -q 'REIFY_BIN' && echo \"\$REIFY_BIN_SKIP_REASON\" | grep -qi 'missing'"

# ==============================================================================
# Check 13: resolve_trusted_reify_bin — REIFY_BIN unset, no target bins → SKIP,
#           reason mentions 'not built'.
# ==============================================================================
echo ""
echo "--- Check 13: resolve_trusted_reify_bin SKIPs when nothing is built ---"

assert "resolve_trusted_reify_bin: no REIFY_BIN, no target bins → non-zero (SKIP)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: no REIFY_BIN, no target bins → reason mentions 'not built'" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -q 'not built'"

# ==============================================================================
# Check 14: resolve_trusted_reify_bin — release bin + sidecar == HEAD → trusted,
#           resolves the release path.
# ==============================================================================
echo ""
echo "--- Check 14: resolve_trusted_reify_bin resolves a fresh release bin ---"

RESOLVE_REPO_D="$TMPDIR_BINFRESH/resolve-d"
D_HEAD_SHA=$(make_fixture_repo "$RESOLVE_REPO_D")

D_RELEASE_BIN="$RESOLVE_REPO_D/target/release/reify"
place_binary "$D_RELEASE_BIN"
printf '%s\n' "$D_HEAD_SHA" > "$RESOLVE_REPO_D/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: fresh release bin → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_D'"

assert "resolve_trusted_reify_bin: fresh release bin → REIFY_BIN_RESOLVED is the release path" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_D' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$D_RELEASE_BIN' ]"

# ==============================================================================
# Check 15: resolve_trusted_reify_bin — debug bin only + sidecar == HEAD →
#           trusted, resolves the debug path.
# ==============================================================================
echo ""
echo "--- Check 15: resolve_trusted_reify_bin resolves a fresh debug-only bin ---"

RESOLVE_REPO_E="$TMPDIR_BINFRESH/resolve-e"
E_HEAD_SHA=$(make_fixture_repo "$RESOLVE_REPO_E")

E_DEBUG_BIN="$RESOLVE_REPO_E/target/debug/reify"
place_binary "$E_DEBUG_BIN"
printf '%s\n' "$E_HEAD_SHA" > "$RESOLVE_REPO_E/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: fresh debug-only bin → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_E'"

assert "resolve_trusted_reify_bin: fresh debug-only bin → REIFY_BIN_RESOLVED is the debug path" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_E' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$E_DEBUG_BIN' ]"

# ==============================================================================
# Check 16: resolve_trusted_reify_bin — release bin + sidecar MISMATCH → SKIP,
#           reason mentions 'stale'.
# ==============================================================================
echo ""
echo "--- Check 16: resolve_trusted_reify_bin SKIPs a stale release bin ---"

RESOLVE_REPO_F="$TMPDIR_BINFRESH/resolve-f"
make_fixture_repo "$RESOLVE_REPO_F" >/dev/null

F_RELEASE_BIN="$RESOLVE_REPO_F/target/release/reify"
place_binary "$F_RELEASE_BIN"
printf '%s\n' "0000000000000000000000000000000000000000" > "$RESOLVE_REPO_F/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: release bin + sidecar mismatch → non-zero (SKIP)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_REPO_F'"

assert "resolve_trusted_reify_bin: release bin + sidecar mismatch → reason mentions 'stale'" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_REPO_F' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -qi 'stale'"

# ==============================================================================
# Check 17: resolve_trusted_reify_bin — both release+debug present and fresh →
#           resolves release (precedence).
# ==============================================================================
echo ""
echo "--- Check 17: resolve_trusted_reify_bin prefers release over debug ---"

RESOLVE_REPO_G="$TMPDIR_BINFRESH/resolve-g"
G_HEAD_SHA=$(make_fixture_repo "$RESOLVE_REPO_G")

G_RELEASE_BIN="$RESOLVE_REPO_G/target/release/reify"
G_DEBUG_BIN="$RESOLVE_REPO_G/target/debug/reify"
place_binary "$G_RELEASE_BIN"
place_binary "$G_DEBUG_BIN"
printf '%s\n' "$G_HEAD_SHA" > "$RESOLVE_REPO_G/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: both bins present+fresh → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_G'"

assert "resolve_trusted_reify_bin: both bins present+fresh → resolves release (precedence over debug)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_G' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$G_RELEASE_BIN' ]"

# ==============================================================================
# Check 18: verify.sh plan-shape — reify-cli release pre-build + .reify-bin-sha
#           stamp lines present and ordered BEFORE run_all.sh, in the
#           merge-tier plan (DF_VERIFY_ROLE=merge — see the file-header note
#           above on why this oracle, not the plain --include-infra one, is
#           used post-task-5125). Hermetic: --print-plan never runs cargo.
# ==============================================================================
echo ""
echo "--- Check 18: verify.sh merge-tier plan orders the reify-cli pre-build + stamp before run_all.sh ---"

MERGE_ALL_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"

assert "merge-all plan: reify-cli release pre-build line present (cargo build --release -p reify-cli)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo build --release" && printf "%s\n" "$1" | grep "cargo build --release" | grep -q "\-p reify-cli"' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: reify-cli pre-build index < run_all.sh index (pre-step ordered before suite)" \
    bash -c '
        PRE_IDX=$(printf "%s\n" "$1" | grep -n "cargo build --release" | grep "\-p reify-cli" | head -1 | cut -d: -f1)
        RUN_IDX=$(printf "%s\n" "$1" | grep -n "run_all\.sh" | head -1 | cut -d: -f1)
        [ -n "$PRE_IDX" ] && [ -n "$RUN_IDX" ] && [ "$PRE_IDX" -lt "$RUN_IDX" ]
    ' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: .reify-bin-sha stamp line present" \
    bash -c 'printf "%s\n" "$1" | grep -q "\.reify-bin-sha"' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: .reify-bin-sha stamp index < run_all.sh index (stamp ordered before suite)" \
    bash -c '
        STAMP_IDX=$(printf "%s\n" "$1" | grep -n "\.reify-bin-sha" | head -1 | cut -d: -f1)
        RUN_IDX=$(printf "%s\n" "$1" | grep -n "run_all\.sh" | head -1 | cut -d: -f1)
        [ -n "$STAMP_IDX" ] && [ -n "$RUN_IDX" ] && [ "$STAMP_IDX" -lt "$RUN_IDX" ]
    ' _ "$MERGE_ALL_PLAN"

# ==============================================================================
# Check 19: behavioral wiring — running the (not-yet-modified) PRD gate tests
#           as black-box subprocesses with REIFY_BIN pointing at a missing
#           file must produce a clean SKIP citing the new freshness guard
#           (REIFY_BIN), not a HARNESS_ERROR/FAIL. Today both tests trust
#           REIFY_BIN blindly (no existence check), so this exercises the
#           real, unmodified gate test files end to end.
#
# set -e note: the gate tests may currently exit non-zero (e.g.
# test_prd_gate_objective_inheritance.sh FAILs today under a missing
# REIFY_BIN) — capture via `cmd || VAR=$?` (mirroring the ALPHA_EXIT idiom
# inside those same gate tests) so a non-zero exit doesn't trip this script's
# own `set -e` before the assertion runs.
# ==============================================================================
echo ""
echo "--- Check 19: PRD gate tests SKIP cleanly under a missing REIFY_BIN ---"

CORPUS_GATE_EXIT=0
CORPUS_GATE_OUT="$(REIFY_BIN=/nonexistent/reify bash "$REPO_ROOT/tests/infra/test_prd_gate_corpus.sh" 2>&1)" || CORPUS_GATE_EXIT=$?

assert "test_prd_gate_corpus.sh: REIFY_BIN=missing file → exit 0" \
    bash -c "[ '$CORPUS_GATE_EXIT' -eq 0 ]"

assert "test_prd_gate_corpus.sh: REIFY_BIN=missing file → output contains SKIP" \
    bash -c 'printf "%s\n" "$1" | grep -q "SKIP"' _ "$CORPUS_GATE_OUT"

assert "test_prd_gate_corpus.sh: REIFY_BIN=missing file → output cites REIFY_BIN (new freshness guard, distinct from the pre-existing toolchain SKIP)" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_BIN"' _ "$CORPUS_GATE_OUT"

OBJECTIVE_GATE_EXIT=0
OBJECTIVE_GATE_OUT="$(REIFY_BIN=/nonexistent/reify bash "$REPO_ROOT/tests/infra/test_prd_gate_objective_inheritance.sh" 2>&1)" || OBJECTIVE_GATE_EXIT=$?

assert "test_prd_gate_objective_inheritance.sh: REIFY_BIN=missing file → exit 0" \
    bash -c "[ '$OBJECTIVE_GATE_EXIT' -eq 0 ]"

assert "test_prd_gate_objective_inheritance.sh: REIFY_BIN=missing file → output contains SKIP" \
    bash -c 'printf "%s\n" "$1" | grep -q "SKIP"' _ "$OBJECTIVE_GATE_OUT"

assert "test_prd_gate_objective_inheritance.sh: REIFY_BIN=missing file → output cites REIFY_BIN (new freshness guard)" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_BIN"' _ "$OBJECTIVE_GATE_OUT"

# -- Summary ------------------------------------------------------------------
test_summary
