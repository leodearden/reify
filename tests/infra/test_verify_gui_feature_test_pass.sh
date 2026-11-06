#!/usr/bin/env bash
# verify.sh's TEST plan EXECUTES the gui-feature-gated suite.
#
# scripts/verify.sh's add_test_passes emits a gui-feature TEST pass,
# `_gui_feat_cmd`: `cargo nextest run -p reify-gui --features gui`, or the
# `cargo test` fallback when cargo-nextest is absent. That pass EXECUTES the
# #[cfg(feature = "gui")] tests. It is distinct from build_plan's lint-side
# "gui-feature compile-check" (`cargo check -p reify-gui --features gui --tests`),
# which runs nothing. This member pins the test pass's shape, its placement in
# the plan, and its narrowing on the affected-crate axis.
#
# The INV-FEA-1 static gate (gui/src-tauri/src/engine.rs's
# MorphRegistration::Enabled arm) lives in
# tests/infra/test_compute_trampoline_registration_wired.sh.
#
# HOST-INDEPENDENCE.  Every match below uses `cargo (test|nextest run)`, never
# the bare literal `cargo nextest run`.  verify.sh falls back to `cargo test`
# when cargo-nextest is absent from PATH and both arms of emit_nextest_pass's
# if/else are shape-identical; tests/infra/test_verify_nextest_absent_suites.sh
# holds the canonical statement of that idiom.  A literal-only grep here would
# match nothing — and therefore pass vacuously — on a nextest-less host.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"
[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

echo "=== verify.sh gui-feature test-pass tests ==="

# Root for the hermetic throwaway-repo fixtures (BR_FIX, ST_FIX, IN_FIX).
FIXTURE_ROOT="$(mktemp -d)"
cleanup_fixture_root() { rm -rf "$FIXTURE_ROOT"; }
trap cleanup_fixture_root EXIT

# The canonical verify.sh plans: the --profile both test plan every block below
# reads, the lint-only plan (b9) and the offline-role plan (b9) negate over.
#
# Captured through capture_print_plan rather than a bare `$(… | grep -v '^#')`.
# `set -o pipefail` catches a verify.sh that EXITS non-zero, but not the failure
# class plan_capture_lib.sh documents explicitly: a capture TRUNCATED under load
# while still exiting 0. (b8) and (b9) are `! grep`s, so a short capture would
# satisfy them silently. The completeness assertions in (b0) are what convert
# that into a visible RED.
#
# TEST_PLAN_FULL stays un-stripped because (b7) needs the semaphore
# ACQUIRE/RELEASE marker lines, which are comments that plan_strip_comments
# removes.
TEST_PLAN_FULL=""
LINT_PLAN_FULL=""
OFFLINE_PLAN_FULL=""
capture_print_plan TEST_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan || true
capture_print_plan LINT_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan || true
capture_print_plan OFFLINE_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    env DF_VERIFY_ROLE=offline bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan || true
TEST_PLAN_SEGS="$(plan_strip_comments "$TEST_PLAN_FULL")"
LINT_PLAN_SEGS="$(plan_strip_comments "$LINT_PLAN_FULL")"
OFFLINE_PLAN_SEGS="$(plan_strip_comments "$OFFLINE_PLAN_FULL")"
export TEST_PLAN_FULL TEST_PLAN_SEGS LINT_PLAN_SEGS OFFLINE_PLAN_SEGS

# -- (b0): the canonical captures are COMPLETE ----------------------------------
# Asserted BEFORE anything consumes them, so a truncated capture fails HERE with
# a name that says so rather than as a confusing cascade of negative assertions
# that all "passed".
echo ""
echo "--- (b0): the canonical verify.sh plan captures are complete ---"
assert "b0: test plan capture is complete (structural markers present)" \
    plan_capture_complete "$TEST_PLAN_FULL"
assert "b0: lint plan capture is complete (structural markers present)" \
    plan_capture_complete "$LINT_PLAN_FULL"
assert "b0: offline-role plan capture is complete (structural markers present)" \
    plan_capture_complete "$OFFLINE_PLAN_FULL"
# Positive controls: a complete-but-empty plan would still satisfy every `! grep`
# below, so pin that each capture actually carries command lines.
assert "b0: test plan carries at least one 'cargo (test|nextest run)' line" \
    bash -c 'printf "%s\n" "$TEST_PLAN_SEGS" | grep -qE "cargo (test|nextest run)"'
assert "b0: lint plan carries at least one command line" \
    bash -c '[ -n "$LINT_PLAN_SEGS" ]'
# POSITIVE CONTROL for (b9)'s offline negation. Without it, an offline plan that
# was empty for ANY reason — a verify.sh regression, a role-parsing change — would
# satisfy "has no gui-feature pass" while proving nothing. The offline lane runs
# the heavy #[ignore] partition, so it must still carry a real test invocation.
assert "b0: the offline-role plan still carries a 'cargo (test|nextest run)' line" \
    bash -c 'printf "%s\n" "$OFFLINE_PLAN_SEGS" | grep -qE "cargo (test|nextest run)"'

# _gui_feature_pass_lines <plan> — the gui-feature TEST-EXECUTION lines of
# <plan>: a `cargo test` / `cargo nextest run` line carrying BOTH `-p reify-gui`
# AND `--features gui`.  The COMBINATION is the discriminator: the pre-existing
# lint-side `cargo check -p reify-gui --features gui --tests` (verify.sh
# build_plan's "gui-feature compile-check" block) carries `--features gui`
# too, so `--features gui` alone would be satisfied by a compile-check that
# executes nothing.
_gui_feature_pass_lines() {
    printf '%s\n' "$1" \
        | grep -E 'cargo (test|nextest run)' \
        | grep -F -- '-p reify-gui' \
        | grep -F -- '--features gui' || true
}
GUI_PASS="$(_gui_feature_pass_lines "$TEST_PLAN_SEGS")"
export GUI_PASS

# -- (b1): a gui-feature TEST-EXECUTION pass exists in the test plan ------------
echo ""
echo "--- (b1): the test plan runs the gui-feature suite (not merely compile-checks it) ---"
assert "test plan has a 'cargo (test|nextest run)' line with BOTH -p reify-gui and --features gui" \
    bash -c '[ -n "$GUI_PASS" ]'
# Every `! grep` on $GUI_PASS below is paired with `[ -n "$GUI_PASS" ]`.
# _gui_feature_pass_lines ends in `|| true`, so GUI_PASS is the empty string
# when the pass is not emitted AT ALL — and a negation over the empty string is
# vacuously true. `assert` does not abort the suite, so without the pairing a
# single failing (b1) would be followed by three GREEN false negatives claiming
# properties of a pass that does not exist.
assert "the matched gui-feature pass is NOT a 'cargo check' compile-check" \
    bash -c '[ -n "$GUI_PASS" ] && ! printf "%s\n" "$GUI_PASS" | grep -q "cargo check"'

# -- (b6): emitted EXACTLY ONCE under --profile both ---------------------------
# Emitting from inside the per-profile loop would yield two passes for the same
# feature-gated suite (debug + release) at ~20m cold build each, for no added
# coverage: `--features gui` is a feature axis, not a profile axis.
echo ""
echo "--- (b6): exactly one gui-feature pass under --profile both ---"
assert "gui-feature pass appears exactly once in the --profile both test plan" \
    bash -c '[ "$(printf "%s\n" "$GUI_PASS" | grep -c .)" -eq 1 ]'

# -- (b2): sidecar-placeholder prefix and the Cargo.toml existence guard --------
# ensure-gui-sidecar-placeholder.sh is empirically required, not defensive:
# gui/src-tauri/build.rs's tauri_build::build() validates bundle.externalBin and
# panics when gui/src-tauri/sidecar/reify-sidecar-<triple> is absent from disk.
echo ""
echo "--- (b2): guarded by 'if test -f gui/src-tauri/Cargo.toml' and prefixed by the sidecar placeholder ---"
assert "gui-feature pass is guarded by 'if test -f gui/src-tauri/Cargo.toml'" \
    bash -c 'printf "%s\n" "$GUI_PASS" | grep -qF "if test -f gui/src-tauri/Cargo.toml"'
assert "gui-feature pass runs ./scripts/ensure-gui-sidecar-placeholder.sh BEFORE the cargo invocation" \
    bash -c 'printf "%s\n" "$GUI_PASS" | grep -qE "ensure-gui-sidecar-placeholder\.sh.*cargo (test|nextest run)"'

# -- (b3): the invocation carries its OWN scoped timeout -----------------------
echo ""
echo "--- (b3): timeout --kill-after=60 <dur> wraps the gui-feature invocation ---"
# Exact-shape pattern.  The only wildcard is [^&|;]* — a CARGO_PRIO nice/ionice
# prefix contains none of those three characters, so the pattern physically
# cannot span an &&/||/; clause boundary the way a greedy '.*' would.
GUI_TIMEOUT_PATTERN='timeout --kill-after=60 [0-9]+[smhd]? [^&|;]*cargo (test|nextest run) -p reify-gui --features gui'
assert "gui-feature pass is wrapped by its own 'timeout --kill-after=60 <dur>'" \
    bash -c 'printf "%s\n" "$GUI_PASS" | grep -qE "$0"' "$GUI_TIMEOUT_PATTERN"

# Synthetic-negatives (the two-negative TIMEOUT_PATTERN idiom of
# test_compute_trampoline_registration_wired.sh's (d)): the pattern must reject a
# timeout sitting on a DIFFERENT &&-clause than the cargo invocation, and an
# untimed invocation carrying no timeout at all.
assert "GUI_TIMEOUT_PATTERN rejects: timeout on a different &&-clause than the gui-feature pass" \
    bash -c '! echo "if test -f gui/src-tauri/Cargo.toml; then timeout --kill-after=60 45m cargo clippy --workspace && cargo nextest run -p reify-gui --features gui; fi" | grep -qE "$0"' \
    "$GUI_TIMEOUT_PATTERN"
assert "GUI_TIMEOUT_PATTERN rejects: an untimed gui-feature invocation" \
    bash -c '! echo "./scripts/ensure-gui-sidecar-placeholder.sh && cargo nextest run -p reify-gui --features gui" | grep -qE "$0"' \
    "$GUI_TIMEOUT_PATTERN"

# -- (b4): FD-close ' 9<&-' -----------------------------------------------------
# FD 9 is the held semaphore slot.  tests/infra/test_verify_semaphore_wiring.sh
# (1k) fails when ANY `cargo (test|nextest run)` plan line lacks the trailing
# token, on both the task plan and the --profile both plan.  verify.sh appends
# it at exactly ONE site (`add "$cmd 9<&-"` inside emit_nextest_pass), so a pass
# emitted by a direct `add` does NOT inherit it and must carry it explicitly.
# Asserted here so this task owns the requirement instead of discovering it as
# a foreign RED in semaphore_wiring.
echo ""
echo "--- (b4): the gui-feature pass carries the trailing FD-close ' 9<&-' ---"
assert "gui-feature pass carries trailing ' 9<&-' (FD-close for children)" \
    bash -c 'printf "%s\n" "$GUI_PASS" | grep -qF " 9<&-"'

# -- (b5): concurrency-cap config on the nextest arm ---------------------------
# scripts/gen-nextest-config.sh emits a full copy of .config/nextest.toml
# carrying the occt test-group cap, the global [profile.default] test-threads
# pool cap and the per-binary priority overrides; its header records that plain
# `--config` is a NO-OP for test-groups on nextest 0.9.136, so --config-file is
# the only mechanism that applies them.  A gui-feature pass without it runs
# UNCAPPED, and a --features gui build links tauri + webkit2gtk + OCCT inside
# the held semaphore slot — exactly the RSS wave the slot exists to bound.
#
# Genuinely runner-specific (cargo test has no --config-file), so it is guarded
# in the skip-OUTSIDE-assert form of test_occt_gated_scope.sh's PLAN_HAS_NEXTEST
# guard — an in-body `exit 0` would increment PASS while checking nothing.
echo ""
echo "--- (b5): the nextest arm carries --config-file with a non-empty path ---"
PLAN_HAS_NEXTEST="$(printf '%s\n' "$TEST_PLAN_SEGS" | grep -c 'cargo nextest run' || true)"
if [ "$PLAN_HAS_NEXTEST" -gt 0 ]; then
    assert "gui-feature nextest pass carries '--config-file'" \
        bash -c 'printf "%s\n" "$GUI_PASS" | grep -qF -- "--config-file"'
    # Non-empty ARGUMENT, not merely the flag: an unpopulated ${_cfg_path}
    # renders as a bare `--config-file ` and would satisfy a flag-only grep
    # while silently degrading the pass to an uncapped run.
    assert "gui-feature nextest pass's --config-file argument is non-empty" \
        bash -c 'printf "%s\n" "$GUI_PASS" | grep -qE -- "--config-file [^[:space:];&|]+"'
else
    echo "  SKIP: (b5) --config-file — the plan has no 'cargo nextest run' line on"
    echo "        this host (nextest=0) and the cargo-test fallback has no"
    echo "        --config-file, so the property is genuinely runner-specific."
fi

# -- (b7): emitted INSIDE the semaphore bracket --------------------------------
# verify.sh's comment above `add "@@SEMAPHORE_ACQUIRE@@"` in add_test_passes
# records that MEMORY is the binding constraint on this host and the held
# slot's whole-block serialization is the only implicit bound on concurrent
# RSS-heavy link waves.  Independently re-asserted by
# test_verify_semaphore_wiring.sh (1o).
echo ""
echo "--- (b7): the gui-feature pass falls BETWEEN the ACQUIRE and RELEASE markers ---"
assert "gui-feature pass index is between the semaphore ACQUIRE and RELEASE markers" \
    bash -c '
        set -e
        ACQ_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -n "^#.*test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        GUI_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -nE "cargo (test|nextest run)" | grep -F -- "-p reify-gui" | grep -F -- "--features gui" | head -1 | cut -d: -f1)
        # LOAD-BEARING. `bash -c` does NOT inherit set -e from the caller, so
        # without the explicit `|| exit 1` this line was dead code: an empty
        # index fell through to the comparison below and only failed because
        # `[ "" -gt "5" ]` raises an integer-expression error. That made the RED
        # path accidental — a future edit defaulting GUI_IDX to a number would
        # have turned it silently GREEN.
        [ -n "$ACQ_IDX" ] && [ -n "$REL_IDX" ] && [ -n "$GUI_IDX" ] || exit 1
        [ "$GUI_IDX" -gt "$ACQ_IDX" ] && [ "$GUI_IDX" -lt "$REL_IDX" ]
    '

# -- (b8): no -E filterset narrowing the suite ---------------------------------
# Running the whole `-p reify-gui --features gui` suite is what makes ALL
# gui-gated code execute (gui_feature_tests, gui_tests, the gui-gated #[test]
# fns in event_bus_tests.rs / claude_bridge.rs, and the wholly gui-gated
# debug_server / event_bus modules).  An enumerated name filter would silently
# drift away from that set.
echo ""
echo "--- (b8): the gui-feature pass carries no -E filterset ---"
assert "gui-feature pass carries no ' -E ' filterset" \
    bash -c '[ -n "$GUI_PASS" ] && ! printf "%s\n" "$GUI_PASS" | grep -qF -- " -E "'

# -- (b9): placement — test-side only, and never on the offline lane -----------
# DF_VERIFY_ROLE=offline runs the heavy #[ignore] partition only.
echo ""
echo "--- (b9): absent from the lint-only plan and from the offline role's plan ---"
assert "lint plan has no 'cargo (test|nextest run)' line with -p reify-gui and --features gui" \
    bash -c '! printf "%s\n" "$LINT_PLAN_SEGS" | grep -E "cargo (test|nextest run)" | grep -F -- "-p reify-gui" | grep -qF -- "--features gui"'
assert "DF_VERIFY_ROLE=offline plan has no gui-feature TEST-EXECUTION pass" \
    bash -c '! printf "%s\n" "$OFFLINE_PLAN_SEGS" | grep -E "cargo (test|nextest run)" | grep -F -- "-p reify-gui" | grep -qF -- "--features gui"'

# -- (b10): survives --scope branch for a gui/src-tauri change -----------------
# decide_scope classifies gui/src-tauri/* as rust=1, so RUN_RUST=1 and the test
# passes are emitted.  Narrowing the gui-feature pass out of exactly the diffs
# most likely to break it would make the guard useless where it matters most.
# Hermetic throwaway-repo fixture (the plan_capture_lib.sh idiom from
# test_verify_throughput.sh::plan_for_shape) — never runs real cargo.
echo ""
echo "--- (b10): still emitted for --scope branch when the change is under gui/src-tauri/ ---"

# _make_plan_fixture <dir> — build a hermetic throwaway repo that can run
# `verify.sh --print-plan` without a cargo workspace, and leave it on a `main`
# branch whose base commit holds exactly the scripts verify.sh sources.
#
# ONE copy of the bootstrap, called by all three fixtures below (b10's BR_FIX,
# b13's ST_FIX, b15's IN_FIX) — they differ only in what they stage or commit on
# TOP of this base, which stays at each call site. SPOT: the `.sh` half of the
# copy list is drift-guarded by assert_source_closure_copied, but the DATA
# dependencies (occt-touching-crates.txt, release-sensitive-crates.txt,
# .config/nextest.toml) and the Python module occt-scope-lib.sh runs
# (reify_path_attribution.py) are not, so a new one added to verify.sh in three
# places would fail opaquely in whichever copy was missed.
_make_plan_fixture() {
    local _dir="$1" _f
    mkdir -p "$_dir/scripts" "$_dir/.config"
    for _f in verify.sh occt-scope-lib.sh occt-touching-crates.txt release-scope-lib.sh \
              release-sensitive-crates.txt affected-crates-lib.sh lib_test_semaphore.sh \
              lib_slot_acquire.sh lib_event_log.sh lib_clock_stop.sh cpu-admit.sh \
              lib_proc_reaper.sh \
              lib_git_env_scrub.sh lib_main_checkout.sh reify_path_attribution.py \
              gen-nextest-config.sh heavy-test-filter-lib.sh; do
        cp "$REPO_ROOT/scripts/$_f" "$_dir/scripts/$_f"
    done
    cp "$REPO_ROOT/.config/nextest.toml" "$_dir/.config/nextest.toml"
    chmod +x "$_dir/scripts/verify.sh"
    # Preflight: fail loudly if verify.sh's TRANSITIVE source closure gained a lib
    # that this copy list misses — otherwise the callers' 2>/dev/null swallows it
    # and their assertions fail opaquely (task #5154).
    assert_source_closure_copied "$REPO_ROOT/scripts" "$_dir/scripts" verify.sh || return 1
    git -C "$_dir" init -q
    git -C "$_dir" config user.email test@invalid.local
    git -C "$_dir" config user.name test
    git -C "$_dir" add scripts .config
    git -C "$_dir" commit -q -m base
    git -C "$_dir" branch -M main
}

BR_FIX="$FIXTURE_ROOT/branch-fixture"
_make_plan_fixture "$BR_FIX" || exit 1
git -C "$BR_FIX" checkout -q -b task-branch
mkdir -p "$BR_FIX/gui/src-tauri/src"
printf 'fn main() {}\n' > "$BR_FIX/gui/src-tauri/src/engine.rs"
git -C "$BR_FIX" add gui
git -C "$BR_FIX" commit -q -m "touch gui/src-tauri"

BR_PLAN_OUT=""
capture_print_plan BR_PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && exec bash scripts/verify.sh test --profile debug --scope branch --include-infra --print-plan 2>/dev/null' \
    _ "$BR_FIX" || true
export BR_PLAN_OUT

assert "b10: branch-scope plan capture is complete (structural markers present)" \
    plan_capture_complete "$BR_PLAN_OUT"
assert "b10: gui/src-tauri branch-scope plan STILL emits the gui-feature test pass" \
    bash -c 'printf "%s\n" "$BR_PLAN_OUT" | grep -E "cargo (test|nextest run)" | grep -F -- "-p reify-gui" | grep -qF -- "--features gui"'

# -- (b11): NARROWED on the affected-crate axis --------------------------------
# b10 above exercises the NARROW_ACTIVE=0 path only (the workspace-less fixture
# makes affected_crates() return the ALL sentinel), so it cannot distinguish
# "emitted because the change reaches reify-gui" from "emitted unconditionally".
# REIFY_AFFECTED_CRATES_OVERRIDE drives the narrowed path hermetically, the same
# knob test_verify_throughput.sh's plan_for_shape_narrowed uses.
#
# A `--features gui` build is a distinct feature-unification of the dependency
# graph — it shares artifacts with no other pass and costs 20m42s cold / ~137s
# warm on its own — so a branch plan narrowed to reify-doc must NOT pay it.
# Conversely a plan whose affected closure reaches reify-gui must still emit it,
# or the pass would be narrowed out of exactly the diffs that can break it.
echo ""
echo "--- (b11): emitted iff the affected-crate closure reaches reify-gui ---"

# _narrowed_gui_plan <out_var> <override> — capture a branch-scope plan whose
# AFFECTED is exactly <override>, retried until structurally complete.
#
# The capture is asserted complete SEPARATELY from the count, because the
# headline assertion here is `N_DOC -eq 0` — a NEGATIVE. The former shape piped
# verify.sh straight into `grep -cF … || true` with stderr discarded, which
# returned 0 both when the pass was correctly narrowed away AND when verify.sh
# failed outright, printed nothing, or changed its plan format. That made the
# exact behaviour this assertion exists to pin vacuously satisfiable by a broken
# fixture. Now the plan text is proven to exist first, and the count is derived
# from that proven text.
_narrowed_gui_plan() {
    capture_print_plan "$1" "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'cd "$1" && REIFY_AFFECTED_CRATES_OVERRIDE="$2" exec bash scripts/verify.sh test --profile debug --scope branch --include-infra --print-plan' \
        _ "$BR_FIX" "$2" || true
}

# _gui_pass_count <plan-text> — gui-feature passes in a captured plan.
_gui_pass_count() {
    printf '%s\n' "$1" \
        | grep -E 'cargo (test|nextest run)' \
        | grep -F -- '-p reify-gui' \
        | grep -cF -- '--features gui' || true
}

P_DOC=""; P_GUI=""; P_MIXED=""; P_MANY=""
_narrowed_gui_plan P_DOC   "reify-doc"
_narrowed_gui_plan P_GUI   "reify-gui"
_narrowed_gui_plan P_MIXED "reify-doc reify-gui"
_narrowed_gui_plan P_MANY  "reify-gui reify-eval reify-mesh-morph"

for _pair in "P_DOC:reify-doc" "P_GUI:reify-gui" "P_MIXED:reify-doc reify-gui" "P_MANY:reify-gui reify-eval reify-mesh-morph"; do
    _var="${_pair%%:*}"
    assert "b11: narrowed plan capture for AFFECTED='${_pair#*:}' is complete" \
        plan_capture_complete "${!_var}"
done

N_DOC="$(_gui_pass_count "$P_DOC")"
N_GUI="$(_gui_pass_count "$P_GUI")"
N_MIXED="$(_gui_pass_count "$P_MIXED")"
N_MANY="$(_gui_pass_count "$P_MANY")"

# Discriminating positive control for the N_DOC==0 negation: the reify-doc plan
# must still be a REAL narrowed test plan carrying `-p reify-doc`, so "0
# gui-feature passes" cannot be satisfied by an override-parsing regression that
# empties every narrowed plan.
assert "b11: the reify-doc-narrowed plan is a real narrowed plan (carries -p reify-doc)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "-p reify-doc"' _ "$P_DOC"
assert "b11: a branch plan narrowed to reify-doc does NOT emit the gui-feature pass (got $N_DOC)" \
    test "$N_DOC" -eq 0
assert "b11: a branch plan whose affected set contains reify-gui DOES emit it (got $N_GUI)" \
    test "$N_GUI" -eq 1
assert "b11: reify-gui anywhere in a multi-crate affected set is enough (got $N_MIXED)" \
    test "$N_MIXED" -eq 1
assert "b11: still emitted EXACTLY ONCE, not once per affected crate (got $N_MANY)" \
    test "$N_MANY" -eq 1

# -- (b13): narrowed on --scope staged too, not just --scope branch ------------
# b11 above drives the narrowed path through `--scope branch`, the only scope
# where narrowing ACTIVATES by default.  b13 pins that the gui-feature pass is
# ALSO narrowed on `--scope staged` WITHOUT `--narrow` — byte-for-byte what
# hooks/project-checks execs on every commit — because the emission condition
# reads SCOPE/AFFECTED_CLOSURE rather than inferring the merge gate from
# NARROW_ACTIVE=0, which holds on that tier too.  Why that inference was wrong
# and what it cost: the "NARROWED on the same affected-crate axis" bullet in
# scripts/verify.sh's add_test_passes.  Not restated here.
echo ""
echo "--- (b13): narrowed on --scope staged (no --narrow) as well as --scope branch ---"

# ST_FIX — a SECOND hermetic throwaway repo, from the shared _make_plan_fixture
# bootstrap.  BR_FIX is deliberately NOT reused: its change is COMMITTED on
# task-branch, so its staged diff is empty.  `--scope staged` reads `git diff --cached`, and an empty staged
# diff yields RUN_RUST=0 and zero test passes, which would make every assertion
# below vacuously green.  The reify-doc file is therefore left in the INDEX,
# uncommitted.
ST_FIX="$FIXTURE_ROOT/staged-fixture"
_make_plan_fixture "$ST_FIX" || exit 1
# STAGED, never committed — this is what `git diff --cached` must see.
mkdir -p "$ST_FIX/crates/reify-doc/src"
printf 'pub fn touched() {}\n' > "$ST_FIX/crates/reify-doc/src/lib.rs"
git -C "$ST_FIX" add crates

# _staged_gui_plan <out_var> <override> — capture the plan for exactly the shape
# hooks/project-checks execs: action `all` (not `test`), `--scope staged`, and NO
# `--narrow`.  Mirrors _narrowed_gui_plan's capture_print_plan + `|| true`
# discipline so the capture is proven complete before any count is derived.
_staged_gui_plan() {
    capture_print_plan "$1" "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'cd "$1" && REIFY_AFFECTED_CRATES_OVERRIDE="$2" exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan' \
        _ "$ST_FIX" "$2" || true
}

P_S_DOC=""; P_S_GUI=""
_staged_gui_plan P_S_DOC "reify-doc"
_staged_gui_plan P_S_GUI "reify-gui"

# Capture integrity asserted SEPARATELY from the counts, for the reason b11's
# header spells out: the headline assertion is a NEGATIVE and must not be
# satisfiable by a fixture that produced nothing.
assert "b13: staged-scope plan capture for closure='reify-doc' is complete" \
    plan_capture_complete "$P_S_DOC"
assert "b13: staged-scope plan capture for closure='reify-gui' is complete" \
    plan_capture_complete "$P_S_GUI"

S_DOC="$(_gui_pass_count "$P_S_DOC")"
S_GUI="$(_gui_pass_count "$P_S_GUI")"

# Anti-vacuity positive control, doubling as the B9-default coupling guard: a
# staged plan WITHOUT --narrow must still be a full-workspace plan.  Its
# presence proves RUN_RUST=1 (so test passes were emitted at all), and it pins
# that computing the closure for scope=staged does NOT activate narrowing —
# clippy and the workspace nextest pass keep --workspace, exactly as
# tests/infra/test_verify_scope.sh's B9-default scenario requires.
assert "b13: the staged plan is a real full-workspace plan (carries 'cargo clippy --workspace')" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "cargo clippy --workspace"' _ "$P_S_DOC"
assert "b13: a --scope staged plan (no --narrow) whose closure is reify-doc does NOT emit the gui-feature pass (got $S_DOC)" \
    test "$S_DOC" -eq 0
assert "b13: a --scope staged plan whose closure contains reify-gui DOES emit it (got $S_GUI)" \
    test "$S_GUI" -eq 1

# -- (b14): a malformed or absent closure fails WIDE on every scope ------------
# A malformed REIFY_AFFECTED_CRATES_OVERRIDE must never narrow the pass away —
# a typo'd operator knob becoming a silent coverage gap is strictly worse than
# an over-broad plan.  The shapes that violate it, and why each one does, are
# enumerated once at the normalization loop in scripts/verify.sh's
# add_test_passes; this block is the executable half of that list.
#
# Assertion → invariant map:
#   W_STAGED / W_BRANCH  whitespace-only override fails wide on BOTH scopes
#   N_STAGED             an UNCOMPUTABLE closure (ALL sentinel) still fails wide
#                        …and its header assertion pins that the closure was
#                        genuinely computed on the staged tier, which no count
#                        assertion can distinguish
#   G_STAGED / P_STAGED  a glob-bearing / non-crate-name override fails wide
#   A_DOC                scope=all is unconditional BY CONTRACT and cannot be
#                        defeated by a stray narrow override
echo ""
echo "--- (b14): a malformed/absent closure fails WIDE on every scope ---"

# _staged_gui_plan_noenv <out_var> — as _staged_gui_plan but with NO
# REIFY_AFFECTED_CRATES_OVERRIDE in the child env (`env -u` so a leaked export in
# the parent cannot make this case silently identical to the override cases).
# The workspace-less fixture makes `cargo metadata` fail, so affected_crates()
# returns the ALL sentinel (affected-crates-lib.sh, C5).
_staged_gui_plan_noenv() {
    capture_print_plan "$1" "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        env -u REIFY_AFFECTED_CRATES_OVERRIDE \
        bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan' \
        _ "$ST_FIX" || true
}

P_W_STAGED=""; P_W_BRANCH=""; P_N_STAGED=""; P_A_DOC=""
_staged_gui_plan   P_W_STAGED "   "
_narrowed_gui_plan P_W_BRANCH "   "
_staged_gui_plan_noenv P_N_STAGED
capture_print_plan P_A_DOC "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && REIFY_AFFECTED_CRATES_OVERRIDE="$2" exec bash scripts/verify.sh all --profile debug --scope all --include-infra --print-plan' \
    _ "$ST_FIX" "reify-doc" || true

assert "b14: whitespace-override staged-scope plan capture is complete" \
    plan_capture_complete "$P_W_STAGED"
W_STAGED="$(_gui_pass_count "$P_W_STAGED")"
assert "b14: a whitespace-only affected-crates override fails WIDE on --scope staged (got $W_STAGED)" \
    test "$W_STAGED" -eq 1

assert "b14: whitespace-override branch-scope plan capture is complete" \
    plan_capture_complete "$P_W_BRANCH"
W_BRANCH="$(_gui_pass_count "$P_W_BRANCH")"
assert "b14: a whitespace-only affected-crates override fails WIDE on --scope branch too (got $W_BRANCH)" \
    test "$W_BRANCH" -eq 1

assert "b14: no-override staged-scope plan capture is complete" \
    plan_capture_complete "$P_N_STAGED"
N_STAGED="$(_gui_pass_count "$P_N_STAGED")"
assert "b14: closure unavailable (ALL sentinel) on --scope staged still emits — C5 fail-wide (got $N_STAGED)" \
    test "$N_STAGED" -eq 1
# The count alone cannot tell "the closure was COMPUTED and came back ALL" apart
# from "the closure was never computed on this tier at all": both take verify.sh's
# FAIL-WIDE arm and both yield 1.  Every other staged case here is driven through
# REIFY_AFFECTED_CRATES_OVERRIDE, which short-circuits the
# CHANGED_FILES_RAW -> affected_crates() branch entirely, so this is the ONLY
# capture that exercises that wiring on the staged tier — an implementation that
# restricted it to scope=branch would print `closure=` here and still pass every
# count assertion above.  Asserting the header's exact `affected= closure=ALL`
# tail therefore pins two things at once: the staged-tier wiring, and the
# APPEND-ONLY shape of the narrowing header (test_verify_scope.sh greps
# `NARROW_ACTIVE=0 affected=ALL` as an unanchored substring and
# plan_capture_lib.sh matches `NARROW_ACTIVE=([0-9]+)`; both survive a trailing
# append and neither survives a reordering).
assert "b14: the staged closure is actually COMPUTED (header carries closure=ALL, not an empty closure)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "NARROW_ACTIVE=0 affected= closure=ALL"' _ "$P_N_STAGED"

# Glob-bearing override: the split is an UNQUOTED expansion, so with pathname
# expansion live `*` expands against the fixture CWD into its directory entries
# (`crates scripts`) rather than collapsing — neither is reify-gui, so the
# MEMBERSHIP arm silently narrowed the pass AWAY.  Measured at 0 emitted passes with
# `closure=*` in the header before the `set -f` + crate-name-grammar guard.  A
# malformed knob must fail WIDE on this shape exactly as it does on "   ".
P_G_STAGED=""
_staged_gui_plan P_G_STAGED '*'
assert "b14: glob-override staged-scope plan capture is complete" \
    plan_capture_complete "$P_G_STAGED"
G_STAGED="$(_gui_pass_count "$P_G_STAGED")"
assert "b14: a glob-bearing affected-crates override fails WIDE, never globs against the CWD (got $G_STAGED)" \
    test "$G_STAGED" -eq 1

# A token that cannot BE a cargo package name (here a path fragment) means the
# knob is malformed, not that the closure excludes reify-gui — verify.sh's
# FAIL-WIDE arm, not its MEMBERSHIP one.  (Arms are cited by NAME, never by
# ordinal: this suite's ordinals went stale once when task 6268 inserted
# COMPUTED-EMPTY ahead of the membership test.)
P_P_STAGED=""
_staged_gui_plan P_P_STAGED 'crates/reify-doc'
assert "b14: path-fragment override staged-scope plan capture is complete" \
    plan_capture_complete "$P_P_STAGED"
P_STAGED="$(_gui_pass_count "$P_P_STAGED")"
assert "b14: an override token outside cargo's package-name grammar fails WIDE (got $P_STAGED)" \
    test "$P_STAGED" -eq 1

assert "b14: scope=all plan capture is complete" \
    plan_capture_complete "$P_A_DOC"
A_DOC="$(_gui_pass_count "$P_A_DOC")"
assert "b14: the merge gate (scope=all) emits it unconditionally, even under a reify-doc-only override (got $A_DOC)" \
    test "$A_DOC" -eq 1

# -- (b15): a COMPUTED-EMPTY closure narrows the pass away ---------------------
# b14's fail-wide family covers every closure the run could NOT derive.  This is
# the opposite shape and the one b14 deliberately cannot reach: a closure the run
# DID derive, from this run's own changed-file list, which came back holding no
# crate at all.  A tests/infra-only diff is that shape and is extremely common
# here — affected-crates-lib.sh maps tests/infra/* to no crate, while
# decide_scope's conservative `*)` catch-all still sets RUN_RUST=1 — so before
# task 6268 it paid the `--features gui` tauri + webkit2gtk + OCCT
# feature-unification link (20m42s cold / ~137s warm, shared with no other pass)
# for a diff whose closure provably contains zero crates.
#
# IN_FIX — a THIRD hermetic throwaway repo, from the shared _make_plan_fixture
# bootstrap, then finished with b13's ST_FIX idiom for the same reason: the
# changed file is left in the INDEX, never committed, so `git diff --cached` is
# non-empty and no assertion below can go vacuously green.
# The staged path is deliberately `tests/infra/foo.sh` and NOT a `test_*.sh`
# name — that keeps it clear of the run-all classification-manifest drift guard,
# which is a plan LINE and so never executes under --print-plan, but which a
# future reader should not have to re-derive.
#
# The capture runs with NO REIFY_AFFECTED_CRATES_OVERRIDE (`env -u`, b14's
# discipline): an override is a substitute for the derivation and never carries
# the from-diff licence, so this is the only way to reach a genuinely
# COMPUTED-EMPTY closure.  It never shells out to `cargo metadata` — the
# tests/infra-only file list short-circuits at affected_crates()'s
# `${#direct[@]} -eq 0` early return, BEFORE _reverse_closure — which is exactly
# why the shape is reproducible in this workspace-less fixture.
echo ""
echo "--- (b15): a COMPUTED-EMPTY affected-crate closure narrows the pass away ---"

IN_FIX="$FIXTURE_ROOT/infra-only-fixture"
_make_plan_fixture "$IN_FIX" || exit 1
# STAGED, never committed — what `git diff --cached` must see.
mkdir -p "$IN_FIX/tests/infra"
printf '#!/usr/bin/env bash\necho placeholder\n' > "$IN_FIX/tests/infra/foo.sh"
git -C "$IN_FIX" add tests

P_IN_STAGED=""
capture_print_plan P_IN_STAGED "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    env -u REIFY_AFFECTED_CRATES_OVERRIDE \
    bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan' \
    _ "$IN_FIX" || true

assert "b15: computed-empty staged-scope plan capture is complete" \
    plan_capture_complete "$P_IN_STAGED"

# Anti-vacuity positive control, doubling as the decision that the OTHER passes
# do NOT follow: a computed-empty closure keeps NARROW_ACTIVE=0, so cargo check /
# clippy / the workspace nextest pass stay at --workspace.  Its presence also
# proves RUN_RUST=1, i.e. that test passes were emitted at all — without it a
# gui-feature count of 0 would be satisfied by a fixture that produced no test
# passes whatsoever.
assert "b15: the computed-empty staged plan is a real full-workspace plan (carries 'cargo clippy --workspace')" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "cargo clippy --workspace"' _ "$P_IN_STAGED"

# The DISCRIMINATING assertion, and the reason the narrowing header needs a
# from-diff field at all: no count assertion can tell "the closure was computed
# and came back EMPTY" apart from "the closure was never computed on this tier",
# because both leave `closure=` empty.  That is the same vacuity b14's
# `affected= closure=ALL` header assertion closes for the ALL case — without
# this, a b15 count of 0 would be satisfied by an implementation that simply
# stopped computing the closure on the staged tier.  Asserting the exact tail
# also pins the APPEND-ONLY shape of the header (test_verify_scope.sh greps
# `NARROW_ACTIVE=0 affected=ALL` unanchored and plan_capture_lib.sh matches
# `NARROW_ACTIVE=([0-9]+)`; both survive a trailing append, neither survives a
# reordering).
assert "b15: the staged closure is COMPUTED and EMPTY (header carries closure= with the from-diff licence)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "NARROW_ACTIVE=0 affected= closure= from_diff=1"' _ "$P_IN_STAGED"

# The headline.  A closure the run derived from its own diff and which holds no
# crate at all proves reify-gui is unaffected, so the feature-unification link
# must not be paid — the mirror of b13's S_DOC, reached without an override.
IN_STAGED="$(_gui_pass_count "$P_IN_STAGED")"
assert "b15: a COMPUTED-EMPTY closure on --scope staged does NOT emit the gui-feature pass (got $IN_STAGED)" \
    test "$IN_STAGED" -eq 0

# -- (b12): --test-threads reaches the gui-feature pass ------------------------
# `verify.sh --test-threads=N` caps test-execution parallelism.  The cargo-test
# fallback arm always honoured it; the nextest arm did not, so an explicit
# --test-threads capped every workspace pass but left THIS pass — a tauri +
# webkit2gtk + OCCT link inside the held semaphore slot — uncapped.
echo ""
echo "--- (b12): an explicit --test-threads=N reaches the gui-feature pass ---"
TT_GUI_PASS="$(_gui_feature_pass_lines "$(bash "$REPO_ROOT/scripts/verify.sh" test --profile debug --scope all --include-infra --test-threads=3 --print-plan 2>/dev/null)")"
export TT_GUI_PASS
assert "gui-feature pass carries --test-threads=3 when verify.sh is given --test-threads=3" \
    bash -c 'printf "%s\n" "$TT_GUI_PASS" | grep -qF -- "--test-threads=3"'

# Default (flag unset) must stay byte-identical to before this amendment.  On
# the nextest arm that means NO --test-threads fragment at all; the cargo-test
# fallback arm always emits `-- --test-threads=1` (its OCCT serialization
# guard), so the assertion is genuinely runner-specific and is guarded in the
# skip-OUTSIDE-assert form (test_occt_gated_scope.sh's PLAN_HAS_NEXTEST guard).
if [ "$PLAN_HAS_NEXTEST" -gt 0 ]; then
    assert "gui-feature nextest pass carries NO --test-threads fragment when the flag is unset" \
        bash -c '[ -n "$GUI_PASS" ] && ! printf "%s\n" "$GUI_PASS" | grep -qF -- "--test-threads"'
else
    echo "  SKIP: (b12) default-shape — the cargo-test fallback arm always carries"
    echo "        '-- --test-threads=1' (its OCCT serialization guard), so 'no"
    echo "        --test-threads by default' is a nextest-arm-only property."
fi

test_summary
