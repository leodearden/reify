#!/usr/bin/env bash
# Infrastructure test for task 5076 (INV-FEA-1 drift guard).
#
# Validates scripts/check-compute-trampoline-registration.sh: that it exists and
# executes, that it passes clean on the current tree, and — the task's
# user-observable signal (PRD docs/prds/compute-fea-hardening.md task A5, G2(c))
# — that it actually DETECTS drift rather than merely being present:
#   * a known engine-construction site that stopped delegating to
#     Engine::register_production_compute_fns;
#   * gui/src-tauri/src/engine.rs's #[cfg(feature = "gui")] arm flipped from
#     MorphRegistration::Enabled to Unavailable (the esc-2962-66 class — it
#     compiles clean and silently un-registers the mesh-morph producer);
#   * a FOURTH production site that hand-rolls the bundle from its halves
#     instead of delegating;
# and that it does NOT false-positive on the two shapes that are legitimate:
# a #[cfg(test)] module body, and a rustdoc mention of a bundle half.
#
# Mirrors tests/infra/test_nan_safe_ordering_guard_wired.sh (task 5093).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== check-compute-trampoline-registration.sh wiring tests ==="

GATE="$REPO_ROOT/scripts/check-compute-trampoline-registration.sh"

# The orchestrator runs scripts/verify.sh, so wiring is asserted against the
# verify.sh plans. --include-infra so the lint-side infra leaf appears;
# --scope all for the full plan; env/comment lines stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan | grep -v '^#')"
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS TEST_PLAN_SEGS

# _exits_with <want> <cmd...> — assert an EXACT exit code, not merely non-zero.
# The gate's contract distinguishes 1 (violation found) from 2 (usage / not a
# work tree); a bare `! cmd` would conflate them, so a gate that crashed with a
# usage error on every invocation would pass every detection assertion below.
_exits_with() {
    local want="$1"; shift
    local rc=0
    "$@" || rc=$?
    [ "$rc" -eq "$want" ] && return 0
    echo "expected exit $want, got $rc from: $*"
    return 1
}

# -- (a): script is referenced in the lint plan ---------------------------------
echo ""
echo "--- (a): scripts/check-compute-trampoline-registration.sh is in the lint plan ---"
assert "lint plan contains 'scripts/check-compute-trampoline-registration.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'scripts/check-compute-trampoline-registration.sh'"

# -- (b): if-test-f guard is used -----------------------------------------------
echo ""
echo "--- (b): if test -f guard is used in the lint plan ---"
assert "lint plan contains 'if test -f scripts/check-compute-trampoline-registration.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'if test -f scripts/check-compute-trampoline-registration.sh'"

# -- (c): WARNING echo is present for the guard-skip branch ---------------------
echo ""
echo "--- (c): WARNING echo for guard-skip branch in the lint plan ---"
assert "lint plan has WARNING echo for check-compute-trampoline-registration.sh skip" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'WARNING.*check-compute-trampoline-registration'"

# -- (d): invocation is wrapped with its OWN scoped timeout ---------------------
echo ""
echo "--- (d): timeout --kill-after=60 wraps the invocation in the lint plan ---"
# Exact-shape pattern: it cannot span &&-separated clauses because every segment
# between 'timeout --kill-after=60' and the script name is a short anchored class
# with no greedy '.*'.
TIMEOUT_PATTERN='timeout --kill-after=60 [0-9]+m bash scripts/check-compute-trampoline-registration\.sh'
assert "lint plan wraps check-compute-trampoline-registration.sh with 'timeout --kill-after=60'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE '$TIMEOUT_PATTERN'"

# Synthetic-negatives: the tight pattern must reject BOTH a 'timeout' sitting on
# a different &&-clause than the script, and a path-only mention with no timeout
# at all. A greedy '.*' regex would silently pass the first; a bare
# grep -q '<script name>' would silently pass the second.
assert "TIMEOUT_PATTERN rejects: timeout on a different clause than the script" \
    bash -c "! echo 'lint_command: timeout --kill-after=60 30m cargo clippy && bash scripts/check-compute-trampoline-registration.sh' | grep -qE '$TIMEOUT_PATTERN'"
assert "TIMEOUT_PATTERN rejects: a path-only reference carrying no timeout" \
    bash -c "! echo 'lint_command: cat scripts/check-compute-trampoline-registration.sh' | grep -qE '$TIMEOUT_PATTERN'"

# -- (e): script exists and is executable on disk -------------------------------
echo ""
echo "--- (e): scripts/check-compute-trampoline-registration.sh exists and is executable ---"
assert "scripts/check-compute-trampoline-registration.sh exists" \
    test -f "$GATE"
assert "scripts/check-compute-trampoline-registration.sh is executable" \
    test -x "$GATE"

# -- (f): script is NOT in the test plan (placement in lint only) ---------------
echo ""
echo "--- (f): scripts/check-compute-trampoline-registration.sh is NOT in the test plan ---"
assert "test plan does NOT reference scripts/check-compute-trampoline-registration.sh" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'scripts/check-compute-trampoline-registration.sh'"

# -- (g): script runs clean (exit 0) against the current tree -------------------
echo ""
echo "--- (g): check-compute-trampoline-registration.sh exits 0 on the current tree ---"
assert "bash scripts/check-compute-trampoline-registration.sh --repo-root REPO_ROOT exits 0" \
    bash "$GATE" --repo-root "$REPO_ROOT"
assert "bash scripts/check-compute-trampoline-registration.sh exits 0 with CWD=repo root (mirrors lint_command)" \
    bash -c "cd '$REPO_ROOT' && bash scripts/check-compute-trampoline-registration.sh"

# -- (h): DETECTION — the task's user-observable signal (PRD A5 G2(c)) ----------
# Proven against a throwaway `git init` fixture rather than the real tree, so the
# assertions are about the GATE's behaviour and cannot be invalidated by an
# unrelated edit to a real source file. The gate's source set is `git ls-files`-
# hermetic, so every mutation below must be re-staged to become visible.
echo ""
echo "--- (h): gate DETECTS drift and honors the documented exemptions ---"

DET_TMP="$(mktemp -d)"
cleanup_det() { rm -rf "$DET_TMP"; }
trap cleanup_det EXIT

FIX="$DET_TMP/fixture"
mkdir -p "$FIX"
git -C "$FIX" init -q
git -C "$FIX" config user.email test@invalid.local
git -C "$FIX" config user.name test

# write_baseline — the CLEAN tree: all three known engine-construction sites
# present, each delegating and carrying its required MorphRegistration variant,
# and no fourth hand-rolled bundler anywhere.
write_baseline() {
    rm -rf "${FIX:?}/crates" "${FIX:?}/gui"
    mkdir -p "$FIX/crates/reify-cli/src" "$FIX/gui/src-tauri/src" "$FIX/crates/reify-eval/src"
    cat > "$FIX/crates/reify-cli/src/main.rs" <<'RS'
fn register_compute_trampolines(engine: &mut reify_eval::Engine) {
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Enabled(
        reify_mesh_morph::register_morph_producer,
    ));
}
RS
    cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    #[cfg(feature = "gui")]
    let morph =
        reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    #[cfg(not(feature = "gui"))]
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}
RS
    cat > "$FIX/crates/reify-eval/src/test_runner.rs" <<'RS'
fn engine_for_tests(engine: &mut crate::Engine) {
    engine.register_production_compute_fns(crate::compute_targets::MorphRegistration::Unavailable {
        reason: "reify-mesh-morph is not a dependency of reify-eval",
    });
}
RS
}

stage() { git -C "$FIX" add -A; }

# h0 — POSITIVE CONTROL. Without this the h-block would be vacuous: a gate that
# exited 1 unconditionally would satisfy h1/h2/h3/h6 and only h4/h5 would object.
write_baseline
stage
assert "h0: gate exits 0 on a clean three-site fixture (positive control)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# h1 — a known site stopped delegating to the bundler.
write_baseline
cat > "$FIX/crates/reify-cli/src/main.rs" <<'RS'
fn register_compute_trampolines(engine: &mut reify_eval::Engine) {
    let _morph = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
}
RS
stage
assert "h1: gate FLAGS a known site whose register_production_compute_fns( call was removed" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "h1: stderr names the offending site path" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -q 'crates/reify-cli/src/main.rs'"

# h2 — the cfg(feature = "gui") arm flipped to Unavailable. The delegation call
# is intact, so only the per-site VARIANT pin can catch this.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}
RS
stage
assert "h2: gate FLAGS gui engine.rs carrying Unavailable but no MorphRegistration::Enabled(" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "h2: stderr names gui/src-tauri/src/engine.rs" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -q 'gui/src-tauri/src/engine.rs'"

# h3 — a FOURTH production site hand-rolls the bundle from its halves.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "h3: gate FLAGS a fourth production site that hand-rolls the bundle halves" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "h3: stderr names the fourth site as file:line" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# h4 — the SAME body inside a #[cfg(test)] module is legitimate (this is the
# shape of all five real in-src callers: compute_persist.rs:529,672;
# as_printed_material.rs:542; compute_targets/mod.rs:541,583).
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn nothing() {}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_an_engine() {
        let mut engine = reify_eval::Engine::new();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        reify_eval::register_shell_extract_compute_fns(&mut engine);
    }
}
RS
stage
assert "h4: gate does NOT flag the same body inside a #[cfg(test)] module" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# h5 — the inline escape clears an intentional site.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine); // trampoline-registration:allow — bespoke fixture
    reify_eval::register_shell_extract_compute_fns(&mut engine); // trampoline-registration:allow — bespoke fixture
    engine
}
RS
stage
assert "h5: gate CLEARS the same site once annotated with // trampoline-registration:allow" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# h5b — comment-tail strip. A rustdoc MENTION of a bundle half is not a call.
# The real tree carries exactly this shape at crates/reify-mesh-morph/src/lib.rs
# :372 ("Mirrors `reify_eval::compute_targets::register_compute_fns`"), outside
# any #[cfg(test)] module — so only the //-strip keeps it green, not the awk
# cfg(test) skipper.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
/// Mirrors `reify_eval::compute_targets::register_compute_fns(&mut engine)`:
/// called once at Engine construction.
pub fn install_hook() {}
RS
stage
assert "h5b: gate does NOT flag a rustdoc mention of a bundle half (comment-tail strip)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# h6 — usage / not-a-work-tree errors are exit 2, distinct from exit 1.
NONGIT="$DET_TMP/nongit"
mkdir -p "$NONGIT"
assert "h6: an unknown flag exits 2" \
    _exits_with 2 bash "$GATE" --bogus-flag
assert "h6: --repo-root pointing at a non-git dir exits 2" \
    _exits_with 2 bash "$GATE" --repo-root "$NONGIT"

# ===========================================================================
# Part B — verify.sh's TEST plan EXECUTES the gui-feature-gated suite.
#
# The static gate above (Part A) pins gui/src-tauri/src/engine.rs's
# MorphRegistration::Enabled arm by grep.  Part B is the dynamic half: verify.sh
# must actually RUN the #[cfg(feature = "gui")] tests, which today it only
# COMPILE-checks (`cargo check -p reify-gui --features gui --tests`, the sole
# `--features` usage in the file).
#
# HOST-INDEPENDENCE.  Every match below uses `cargo (test|nextest run)`, never
# the bare literal `cargo nextest run`.  verify.sh falls back to `cargo test`
# when cargo-nextest is absent from PATH and both arms of emit_nextest_pass's
# if/else are shape-identical; tests/infra/test_verify_nextest_absent_suites.sh
# holds the canonical statement of that idiom.  A literal-only grep here would
# match nothing — and therefore pass vacuously — on a nextest-less host.
# ===========================================================================
echo ""
echo "=== Part B: the gui-feature suite is EXECUTED by verify.sh's test plan ==="

# Un-stripped capture: (b7) needs the semaphore ACQUIRE/RELEASE marker lines,
# which are comments and are removed by TEST_PLAN_SEGS's `grep -v '^#'`.
TEST_PLAN_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan)"
OFFLINE_PLAN_SEGS="$(DF_VERIFY_ROLE=offline bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan | grep -v '^#')"
export TEST_PLAN_FULL OFFLINE_PLAN_SEGS

# _gui_feature_pass_lines <plan> — the gui-feature TEST-EXECUTION lines of
# <plan>: a `cargo test` / `cargo nextest run` line carrying BOTH `-p reify-gui`
# AND `--features gui`.  The COMBINATION is the discriminator: the pre-existing
# lint-side `cargo check -p reify-gui --features gui --tests` at verify.sh:2223
# carries `--features gui` too, so `--features gui` alone would be satisfied by
# a compile-check that executes nothing.
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
assert "the matched gui-feature pass is NOT a 'cargo check' compile-check" \
    bash -c '! printf "%s\n" "$GUI_PASS" | grep -q "cargo check"'

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

# Synthetic-negatives (the two-negative idiom at
# test_nan_safe_ordering_guard_wired.sh:51-58): the pattern must reject a
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
# in the skip-OUTSIDE-assert form documented at test_occt_gated_scope.sh:246-254
# — an in-body `exit 0` would increment PASS while checking nothing.
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
# verify.sh:1853-1862 records that MEMORY is the binding constraint on this host
# and the held slot's whole-block serialization is the only implicit bound on
# concurrent RSS-heavy link waves.  Independently re-asserted by
# test_verify_semaphore_wiring.sh (1o).
echo ""
echo "--- (b7): the gui-feature pass falls BETWEEN the ACQUIRE and RELEASE markers ---"
assert "gui-feature pass index is between the semaphore ACQUIRE and RELEASE markers" \
    bash -c '
        ACQ_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -n "^#.*test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        GUI_IDX=$(printf "%s\n" "$TEST_PLAN_FULL" | grep -nE "cargo (test|nextest run)" | grep -F -- "-p reify-gui" | grep -F -- "--features gui" | head -1 | cut -d: -f1)
        [ -n "$ACQ_IDX" ] && [ -n "$REL_IDX" ] && [ -n "$GUI_IDX" ]
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
    bash -c '! printf "%s\n" "$GUI_PASS" | grep -qF -- " -E "'

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
# test_verify_throughput.sh:98-113) — never runs real cargo.
echo ""
echo "--- (b10): still emitted for --scope branch when the change is under gui/src-tauri/ ---"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"
[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

BR_FIX="$DET_TMP/branch-fixture"
mkdir -p "$BR_FIX/scripts" "$BR_FIX/.config"
for _f in verify.sh occt-scope-lib.sh occt-touching-crates.txt release-scope-lib.sh \
          release-sensitive-crates.txt affected-crates-lib.sh lib_test_semaphore.sh \
          lib_slot_acquire.sh lib_clock_stop.sh cpu-admit.sh lib_proc_reaper.sh \
          gen-nextest-config.sh heavy-test-filter-lib.sh; do
    cp "$REPO_ROOT/scripts/$_f" "$BR_FIX/scripts/$_f"
done
cp "$REPO_ROOT/.config/nextest.toml" "$BR_FIX/.config/nextest.toml"
chmod +x "$BR_FIX/scripts/verify.sh"
# Preflight: fail loudly if verify.sh's TRANSITIVE source closure gained a lib
# that this copy list misses — otherwise the 2>/dev/null below swallows it and
# the branch assertion fails opaquely (task #5154).
assert_source_closure_copied "$REPO_ROOT/scripts" "$BR_FIX/scripts" verify.sh || exit 1
git -C "$BR_FIX" init -q
git -C "$BR_FIX" config user.email test@invalid.local
git -C "$BR_FIX" config user.name test
git -C "$BR_FIX" add scripts .config
git -C "$BR_FIX" commit -q -m base
git -C "$BR_FIX" branch -M main
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

test_summary
