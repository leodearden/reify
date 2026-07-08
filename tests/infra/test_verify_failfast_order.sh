#!/usr/bin/env bash
# Infrastructure test for task 4448.
# Validates fail-fast ordering in verify.sh build_plan():
#
#   (1) Incident/#4446 deliverable: `DF_VERIFY_ROLE=merge verify.sh test --scope all`
#       has `npm run typecheck` BEFORE `./scripts/verify.sh psi-gate` (the expensive pole).
#   (2) `verify.sh all --scope all --include-infra` has npm run typecheck AND infra
#       checks BEFORE the psi-gate.
#   (3) Preservation: plan still contains all expected components (content unchanged).
#
# The bounded node||cargo overlap assertions (step-3) will be appended to this file
# once the fail-fast reorder lands (step-2).
#
# Oracle: verify.sh --print-plan (hermetic, never runs cargo/npm).
# Index technique mirrors test_release_mode_in_test_command.sh:60-63.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh fail-fast ordering tests (task 4448) ==="

# Capture plan outputs. Strip comment lines (^#).
# DF_VERIFY_ROLE=merge test --scope all defaults to --profile both (both debug + release passes).
MERGE_TEST_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
ALL_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --include-infra --print-plan | grep -v '^#')"
PLAIN_TEST_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
# task 5125: merge-tier capture — action=all, role=merge, NO --include-infra
# (mirrors hooks/pre-merge-commit:39 exactly). The wholesale run_all.sh pool
# now lives here (gated on DF_VERIFY_ROLE=merge), not on the role=task
# INCLUDE_INFRA tier.
MERGE_ALL_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
export MERGE_TEST_PLAN ALL_PLAN PLAIN_TEST_PLAN MERGE_ALL_PLAN

# ===========================================================================
# Test 1: #4446 deliverable — merge-role test plan: npm typecheck BEFORE psi-gate
# ===========================================================================
echo ""
echo "--- Test 1: merge-role test/scope=all: npm run typecheck ordered BEFORE psi-gate ---"

assert "merge test plan: npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$MERGE_TEST_PLAN"

# plain task-role test plan: same ordering guarantee
assert "plain test plan (task role): npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$PLAIN_TEST_PLAN"

# ===========================================================================
# Test 2: all --scope all --include-infra: npm AND infra BEFORE psi-gate
# ===========================================================================
echo ""
echo "--- Test 2: all/scope=all/include-infra: npm + infra ordered BEFORE psi-gate ---"

assert "all plan: npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$ALL_PLAN"

assert "all plan: check_event_inventory.sh index < psi-gate index" \
    bash -c '
        INFRA_IDX=$(printf "%s\n" "$1" | grep -n "check_event_inventory\.sh" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$INFRA_IDX" ] && [ -n "$PSI_IDX" ] && [ "$INFRA_IDX" -lt "$PSI_IDX" ]
    ' _ "$ALL_PLAN"

# ===========================================================================
# Test 3: Preservation — plan still contains all expected components
# ===========================================================================
echo ""
echo "--- Test 3: preservation — all expected components still present ---"

assert "merge test plan: contains check-manifold-deps.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "check-manifold-deps\.sh"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: contains tree-sitter-generate.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "tree-sitter-generate\.sh"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: contains npm ci && npm run typecheck && npm test (gui chain intact)" \
    bash -c 'printf "%s\n" "$1" | grep -q "npm ci && npm run typecheck && npm test"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: gated OCCT pass ABSENT (task 4451: OCCT folded into nextest pool)" \
    bash -c '! printf "%s\n" "$1" | grep -q "cargo-test-occt-gated\.sh"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: nextest --workspace pass present, no --exclude (task 4451: OCCT in pool)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo nextest run --workspace" && ! printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace.*--exclude"' _ "$MERGE_TEST_PLAN"

assert "all plan: contains cargo clippy (rust lint gate for action=all)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy"' _ "$ALL_PLAN"

assert "all plan: contains check-manifold-deps.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "check-manifold-deps\.sh"' _ "$ALL_PLAN"

assert "all plan: contains tree-sitter-generate.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "tree-sitter-generate\.sh"' _ "$ALL_PLAN"

assert "all plan: contains npm ci && npm run typecheck && npm test (gui chain intact)" \
    bash -c 'printf "%s\n" "$1" | grep -q "npm ci && npm run typecheck && npm test"' _ "$ALL_PLAN"

assert "all plan: gated OCCT pass ABSENT (task 4451: OCCT folded into nextest pool)" \
    bash -c '! printf "%s\n" "$1" | grep -q "cargo-test-occt-gated\.sh"' _ "$ALL_PLAN"

assert "all plan: nextest --workspace pass present, no --exclude (task 4451: OCCT in pool)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo nextest run --workspace" && ! printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace.*--exclude"' _ "$ALL_PLAN"

# ===========================================================================
# Test 4: bounded overlap — action=all: node lane is BACKGROUNDED and joined
# before the pole (step-3 RED; FAIL until step-4 impl adds backgrounding)
# ===========================================================================
echo ""
echo "--- Test 4: action=all: node lane backgrounded + joined before psi-gate ---"

# (a) exactly one backgrounded npm line (carries & + _VERIFY_NODE_BG_PID=$!)
assert "all plan: exactly one backgrounded node line (bg start + PID capture)" \
    bash -c '
        CNT=$(printf "%s\n" "$1" | grep -cE "npm run typecheck.*&[[:space:]]*_VERIFY_NODE_BG_PID=\\\$!" || true)
        [ "$CNT" -eq 1 ]
    ' _ "$ALL_PLAN"

# (b) a later wait line matching ^wait .*_VERIFY_NODE_BG_PID
assert "all plan: wait for _VERIFY_NODE_BG_PID line present" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^wait .*_VERIFY_NODE_BG_PID"' _ "$ALL_PLAN"

# (c) overlap positioning: bg-node < clippy < wait < psi-gate
assert "all plan: bg node index < cargo clippy index" \
    bash -c '
        BG_IDX=$(printf "%s\n" "$1" | grep -n "_VERIFY_NODE_BG_PID=" | head -1 | cut -d: -f1)
        CLIPPY_IDX=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        [ -n "$BG_IDX" ] && [ -n "$CLIPPY_IDX" ] && [ "$BG_IDX" -lt "$CLIPPY_IDX" ]
    ' _ "$ALL_PLAN"

assert "all plan: cargo clippy index < wait index" \
    bash -c '
        CLIPPY_IDX=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        WAIT_IDX=$(printf "%s\n" "$1" | grep -nE "^wait .*_VERIFY_NODE_BG_PID" | head -1 | cut -d: -f1)
        [ -n "$CLIPPY_IDX" ] && [ -n "$WAIT_IDX" ] && [ "$CLIPPY_IDX" -lt "$WAIT_IDX" ]
    ' _ "$ALL_PLAN"

assert "all plan: wait index < psi-gate index" \
    bash -c '
        WAIT_IDX=$(printf "%s\n" "$1" | grep -nE "^wait .*_VERIFY_NODE_BG_PID" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$WAIT_IDX" ] && [ -n "$PSI_IDX" ] && [ "$WAIT_IDX" -lt "$PSI_IDX" ]
    ' _ "$ALL_PLAN"

# (d) f2 regression lock: no 'nice.*ensure-gui-sidecar-placeholder' on one line
# (copied verbatim from test_verify_gui_feature_check.sh:146-148)
assert "all plan: f2 ensure-gui-sidecar-placeholder NOT preceded by nice/ionice" \
    bash -c '! printf "%s\n" "$1" | grep -qE "nice -n.*ensure-gui-sidecar-placeholder"' _ "$ALL_PLAN"

# (e) role-prio regression lock: only cargo lines carry the nice/ionice prefix
# (copied verbatim from test_verify_role_prio.sh:107-109, with task role)
ROLE_ALL_PLAN="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
export ROLE_ALL_PLAN
assert "all plan (task role): only cargo lines carry nice/ionice prefix (node/wait lines clean)" \
    bash -c '! printf "%s\n" "$1" | grep -F "nice -n 15 ionice -c 2 -n 7 " | grep -vq "cargo"' _ "$ROLE_ALL_PLAN"

# ===========================================================================
# Test 5: no backgrounding for action=test — plain lines, no overlap
# (step-3 RED; FAIL until step-4 lands; but (f)/(g) will pass after step-2)
# ===========================================================================
echo ""
echo "--- Test 5: action=test: node lane NOT backgrounded (plain lines only) ---"

# (f) no _VERIFY_NODE_BG_PID sentinel in plain test plan
assert "plain test plan: _VERIFY_NODE_BG_PID NOT present (no backgrounding for test)" \
    bash -c '! printf "%s\n" "$1" | grep -q "_VERIFY_NODE_BG_PID"' _ "$PLAIN_TEST_PLAN"

# (g) no ^wait line in plain test plan
assert "plain test plan: no ^wait line (no join for test)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "^wait "' _ "$PLAIN_TEST_PLAN"

# (h) npm run typecheck still present and still before psi-gate (ordering holds without bgd)
# — re-asserts Test 1's guarantee from the plain-test-plan perspective
assert "plain test plan: npm run typecheck present and before psi-gate" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$PLAIN_TEST_PLAN"

# ===========================================================================
# Test 6: task #4624 — reify-audit release pre-step ordered BEFORE run_all.sh
#          and run_all.sh line carries REIFY_AUDIT_NO_COLD_BUILD=1
#
# task 5125: the pre-step + run_all.sh pairing MOVED to the merge tier
# (DF_VERIFY_ROLE=merge) — the wholesale pool suite no longer runs on the
# per-task INCLUDE_INFRA tier (fixes M-way shared-pool contention). Assertions
# (a)-(d) now read MERGE_ALL_PLAN instead of ALL_PLAN; (e) is a new drift
# guard confirming the role=task ALL_PLAN no longer carries the wholesale
# suite.
#
# Hermetic: --print-plan never runs cargo.
# RED until step-3 (task 5125) restructures build_plan's role gating.
# ===========================================================================
echo ""
echo "--- Test 6 (#4624/5125): reify-audit pre-step before run_all.sh + env token (merge tier) ---"

# (a) pre-step line is present in the merge-all-plan (contains `cargo build
#     --release` and `-p reify-audit`)
assert "merge-all plan: reify-audit release pre-step line present (cargo build --release -p reify-audit)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo build --release" && printf "%s\n" "$1" | grep "cargo build --release" | grep -q "\-p reify-audit"' _ "$MERGE_ALL_PLAN"

# (b) pre-step index < run_all.sh line index
assert "merge-all plan: reify-audit pre-step index < run_all.sh index (pre-step ordered before suite)" \
    bash -c '
        PRE_IDX=$(printf "%s\n" "$1" | grep -n "cargo build --release" | grep "\-p reify-audit" | head -1 | cut -d: -f1)
        RUN_IDX=$(printf "%s\n" "$1" | grep -n "run_all\.sh" | head -1 | cut -d: -f1)
        [ -n "$PRE_IDX" ] && [ -n "$RUN_IDX" ] && [ "$PRE_IDX" -lt "$RUN_IDX" ]
    ' _ "$MERGE_ALL_PLAN"

# (c) pre-step line does NOT contain `run_all.sh` (it is a separate plan line)
#     and does NOT contain `20m` (its own bounded timeout must differ from the
#     run_all wall, so we know it is the pre-step, not the walled line)
assert "merge-all plan: reify-audit pre-step line is separate from run_all.sh (no 'run_all.sh' on the cargo line)" \
    bash -c '
        PRE_LINE=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-audit" | head -1)
        echo "$PRE_LINE" | grep -qv "run_all\.sh"
    ' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: reify-audit pre-step line does not contain '20m' (timeout distinct from run_all wall)" \
    bash -c '
        PRE_LINE=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-audit" | head -1)
        echo "$PRE_LINE" | grep -qv "20m"
    ' _ "$MERGE_ALL_PLAN"

# (d) run_all.sh plan line carries REIFY_AUDIT_NO_COLD_BUILD=1 (backstop armed)
assert "merge-all plan: run_all.sh line carries REIFY_AUDIT_NO_COLD_BUILD=1" \
    bash -c 'printf "%s\n" "$1" | grep "run_all\.sh" | grep -q "REIFY_AUDIT_NO_COLD_BUILD=1"' _ "$MERGE_ALL_PLAN"

# (e) task 5125: role=task ALL_PLAN (--include-infra, no DF_VERIFY_ROLE) no
#     longer carries the wholesale suite — it moved to the merge tier above.
assert "all plan (role=task): LACKS run_all.sh (wholesale suite moved to merge tier, task 5125)" \
    bash -c '! printf "%s\n" "$1" | grep -q "run_all\.sh"' _ "$ALL_PLAN"

# (f)-(k) task 5139: assert the generated PLAN STRING only, via the hermetic
# --print-plan oracle (amend review, reviewer_comprehensive test_coverage) —
# they confirm verify.sh emits `2>&1` / drops `-q` in the right places, not
# that stderr text actually reaches the archived stdout log end-to-end. An
# e2e check would need to drive the real cargo/run_all pipeline (heavy,
# non-hermetic) for no added fidelity on this pure string-shape change — see
# design_decisions in .task/plan.json ("plan-shape via --print-plan").
#
# (f)-(g) run_all.sh stderr is not captured into the archived
# attempt-N.test-*.log — merge it into the already-captured stdout stream
# (2>&1) while preserving the Summary/FAILED classifier-marker contract.
# (f) RED until step-2: run_all.sh line merges stderr into stdout.
assert "merge-all plan: run_all.sh line merges stderr into captured stdout (2>&1)" \
    bash -c 'printf "%s\n" "$1" | grep "run_all\.sh" | grep -q "2>&1"' _ "$MERGE_ALL_PLAN"

# (g) contract guard (green before and after): stdout is never redirected
# away, so run_all's Summary/FAILED classifier markers stay classifier-visible.
assert "merge-all plan: run_all.sh line keeps stdout (no 1>&2 / >/dev/null)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "run_all\.sh" | head -1)
        echo "$L" | grep -qv "1>&2" && echo "$L" | grep -qv ">/dev/null"
    ' _ "$MERGE_ALL_PLAN"

# (h)-(i) task 5139: the two release pre-builds run cargo build --release -q,
# which swallows all compiler diagnostics — the 06-27/28 failure cluster and
# esc-5077-1 archived with no usable evidence. Drop -q so diagnostics land in
# the archived log.
assert "merge-all plan: reify-audit pre-step drops -q (compiler diagnostics archived)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-audit" | head -1)
        [ -n "$L" ] && echo "$L" | grep -qvE "[[:space:]]-q([[:space:]]|;|$)"
    ' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: reify-cli pre-step present and drops -q" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-cli" | head -1)
        [ -n "$L" ] && echo "$L" | grep -qvE "[[:space:]]-q([[:space:]]|;|$)"
    ' _ "$MERGE_ALL_PLAN"

# (j)-(k) task 5139 (review fix, reviewer_comprehensive robustness_error_handling):
# dropping -q alone does NOT archive compiler diagnostics — cargo writes ALL of
# its output (Compiling/Finished progress AND rustc error/warning diagnostics)
# to stderr, never stdout, and DF captures verify.sh's stdout stream only.
# Merge stderr into stdout on both pre-build lines, mirroring the run_all.sh
# fix in (f) above.
assert "merge-all plan: reify-audit pre-step merges stderr into captured stdout (2>&1)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-audit" | head -1)
        [ -n "$L" ] && echo "$L" | grep -q "2>&1"
    ' _ "$MERGE_ALL_PLAN"

assert "merge-all plan: reify-cli pre-step merges stderr into captured stdout (2>&1)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "cargo build --release" | grep "\-p reify-cli" | head -1)
        [ -n "$L" ] && echo "$L" | grep -q "2>&1"
    ' _ "$MERGE_ALL_PLAN"

# (l)-(m) task 5139 (amendment review, reviewer_comprehensive
# robustness_error_handling): the reify-audit binary-missing guard (task
# #4624's positive assertion — Cargo.toml present but pre-build produced no
# binary -> abort loudly) emitted its ERROR(#4624) diagnostic to stderr-only
# (echo ... >&2; false). When that guard fires, the explanation was dropped
# from the archived stdout log — reproducing the exact "archived with no
# usable evidence" gap 5139 closes elsewhere for the cargo/run_all lines.
# Merge its stderr into the captured stdout stream too, same as (f)/(j)/(k).
assert "merge-all plan: reify-audit binary-missing guard (#4624) merges stderr into captured stdout (2>&1)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "ERROR(#4624)" | head -1)
        [ -n "$L" ] && echo "$L" | grep -q "2>&1"
    ' _ "$MERGE_ALL_PLAN"

# (m) contract guard (green before and after): the guard's abort semantics
# (non-zero exit via `false`) must survive the stderr-capture fix — the fix
# is a stream redirect, not a logic change.
assert "merge-all plan: reify-audit binary-missing guard (#4624) still aborts (false present, non-zero exit preserved)" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep "ERROR(#4624)" | head -1)
        [ -n "$L" ] && echo "$L" | grep -q "false"
    ' _ "$MERGE_ALL_PLAN"

test_summary
