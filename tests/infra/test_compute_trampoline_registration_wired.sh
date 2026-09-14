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
#     instead of delegating — in src/, in benches/, in examples/, in a build.rs,
#     and inside a DEFINITION file outside the bundler body;
# and that it does NOT false-positive on the two shapes that are legitimate:
# a #[cfg(test)] module body, and a rustdoc mention of a bundle half.
#
# The h2b/h2c cases pin the ASYMMETRY hazard specifically: the positive
# (delegation/variant) pass and the negative (fourth-bundler) pass must read the
# same production-code view, so a #[cfg(test)] module, a rustdoc line, a block
# comment or a string literal naming MorphRegistration::Enabled( can never stand
# in for the production arm and make the variant pin vacuous.
#
# Mirrors tests/infra/test_nan_safe_ordering_guard_wired.sh (task 5093).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

echo "=== check-compute-trampoline-registration.sh wiring tests ==="

GATE="$REPO_ROOT/scripts/check-compute-trampoline-registration.sh"

# _plan_segs <full-plan> — the COMMAND lines of a captured plan (comments out).
# Kept separate from the capture so completeness is asserted on the FULL text:
# plan_capture_complete keys on two structural COMMENT markers, which a
# pre-stripped capture no longer carries.
_plan_segs() { printf '%s\n' "$1" | grep -v '^#' || true; }

# The orchestrator runs scripts/verify.sh, so wiring is asserted against the
# verify.sh plans. --include-infra so the lint-side infra leaf appears;
# --scope all for the full plan; env/comment lines stripped via `_plan_segs`.
#
# Captured through capture_print_plan rather than a bare `$(… | grep -v '^#')`.
# `set -o pipefail` catches a verify.sh that EXITS non-zero, but not the failure
# class plan_capture_lib.sh documents explicitly: a capture TRUNCATED under load
# while still exiting 0. Every NEGATIVE assertion below — (c)'s WARNING check,
# (f), (b8)'s "no -E filterset", (b9)'s lint/offline checks — is a `! grep`, so
# a short capture would satisfy it silently. The completeness assertions in (a0)
# are what convert that into a visible RED.
LINT_PLAN_FULL=""
TEST_PLAN_FULL=""
capture_print_plan LINT_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan || true
capture_print_plan TEST_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan || true
LINT_PLAN_SEGS="$(_plan_segs "$LINT_PLAN_FULL")"
TEST_PLAN_SEGS="$(_plan_segs "$TEST_PLAN_FULL")"
export LINT_PLAN_SEGS TEST_PLAN_SEGS LINT_PLAN_FULL TEST_PLAN_FULL

# -- (a0): the canonical captures are COMPLETE ----------------------------------
# Asserted BEFORE anything consumes them, so a truncated capture fails HERE with
# a name that says so rather than as a confusing cascade of negative assertions
# that all "passed".
echo ""
echo "--- (a0): the canonical verify.sh plan captures are complete ---"
assert "a0: lint plan capture is complete (structural markers present)" \
    plan_capture_complete "$LINT_PLAN_FULL"
assert "a0: test plan capture is complete (structural markers present)" \
    plan_capture_complete "$TEST_PLAN_FULL"
# Positive controls: a complete-but-empty plan would still satisfy every `! grep`
# below, so pin that each capture actually carries command lines.
assert "a0: lint plan carries at least one command line" \
    bash -c '[ -n "$LINT_PLAN_SEGS" ]'
assert "a0: test plan carries at least one 'cargo (test|nextest run)' line" \
    bash -c 'printf "%s\n" "$TEST_PLAN_SEGS" | grep -qE "cargo (test|nextest run)"'

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
    # The two DEFINITION files, in their real shape: mod.rs defines one half AND
    # carries the bundler body that legitimately calls both; shell_extract
    # defines the other. Present in the BASELINE so h0 is also the positive
    # control for the scoped (non-wholesale) definition-file exemption, and so
    # h7 below can flip exactly one thing.
    mkdir -p "$FIX/crates/reify-eval/src/compute_targets"
    cat > "$FIX/crates/reify-eval/src/compute_targets/mod.rs" <<'RS'
pub fn register_compute_fns(engine: &mut crate::Engine) {
    engine.register_compute_fn("solver::elastic_static", elastic_static as crate::ComputeFn);
}

impl crate::Engine {
    pub fn register_production_compute_fns(&mut self, morph: MorphRegistration) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
        match morph {
            MorphRegistration::Enabled(f) => f(self),
            MorphRegistration::Unavailable { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn double_registration_panics() {
        let mut engine = crate::Engine::new();
        super::register_compute_fns(&mut engine);
    }
}
RS
    cat > "$FIX/crates/reify-eval/src/shell_extract_compute.rs" <<'RS'
pub fn register_shell_extract_compute_fns(engine: &mut Engine) {
    engine.register_compute_fn("shell-extract::extract", shell_extract_compute_fn as ComputeFn);
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

# h2b — THE VACUOUS-PIN HAZARD.  Same production flip as h2, but a #[cfg(test)]
# module in the SAME file still names MorphRegistration::Enabled(.  A file-wide
# presence check would be satisfied by that mention and exit 0, leaving hazard
# (2) unguarded exactly where it is most likely to arise: engine.rs already
# carries three inline #[cfg(test)] modules, and this task adds execution
# coverage for the registration, so a unit test naming the variant is a
# probable next edit.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_the_enabled_variant() {
        let _ = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    }
}
RS
stage
assert "h2b: a #[cfg(test)] mention of MorphRegistration::Enabled( does NOT satisfy the production variant pin" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# h2c — the same vacuity through the other three non-code channels: a rustdoc
# line, a /* */ block comment, and a Rust string literal.  The real tree already
# carries the string-literal shape at crates/reify-cli/src/main.rs:4390
# ("register_compute_trampolines must pass MorphRegistration::Enabled, ").
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
/// Must pass `MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer)`.
/* MorphRegistration::Enabled( — historical note, not code */
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    debug_assert!(false, "from_engine must pass MorphRegistration::Enabled( under the gui feature");
    engine.register_production_compute_fns(morph);
}
RS
stage
assert "h2c: rustdoc / block-comment / string-literal mentions of the variant do NOT satisfy the pin" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

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

# h7 — the DEFINITION-file exemption is SCOPED, not wholesale.  Exempting
# crates/reify-eval/src/compute_targets/mod.rs as a file would make the single
# most likely home for a fourth hand-rolled bundle exempt by construction: the
# file already defines one half and legitimately calls both from the bundler
# body, so "just add it here" is the path of least resistance.  Only the
# definition lines and the `fn register_production_compute_fns(` body are
# allowed; a half-call anywhere else in the file is a violation.
write_baseline
cat >> "$FIX/crates/reify-eval/src/compute_targets/mod.rs" <<'RS'

pub fn build_scratch_engine() -> crate::Engine {
    let mut engine = crate::Engine::new();
    register_compute_fns(&mut engine);
    crate::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "h7: gate FLAGS a hand-rolled bundle added to a DEFINITION file outside the bundler body" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "h7: stderr names compute_targets/mod.rs as file:line" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-eval/src/compute_targets/mod\.rs:[0-9]+'"

# h8 — SCOPE_PATHSPECS reaches the non-src production Rust surface.  benches,
# examples and build.rs are real build inputs; a bundle hand-rolled in one of
# them is the same hazard (3) as one in src/, and h3 (which only exercises
# crates/reify-foo/src/lib.rs) would not notice if the pathspecs regressed to
# src-only.
for _sub in benches/bundle.rs examples/smoke.rs build.rs; do
    write_baseline
    mkdir -p "$FIX/crates/reify-foo/$(dirname "$_sub")"
    cat > "$FIX/crates/reify-foo/$_sub" <<'RS'
fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
    stage
    assert "h8: gate FLAGS a hand-rolled bundle in crates/reify-foo/$_sub" \
        _exits_with 1 bash "$GATE" --repo-root "$FIX"
done

# h8b — gui/src-tauri/build.rs is in scope too (it is a real build input that
# runs tauri_build::build()).
write_baseline
cat > "$FIX/gui/src-tauri/build.rs" <<'RS'
fn main() {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
}
RS
stage
assert "h8b: gate FLAGS a hand-rolled bundle in gui/src-tauri/build.rs" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# h6 — usage / not-a-work-tree errors are exit 2, distinct from exit 1.
NONGIT="$DET_TMP/nongit"
mkdir -p "$NONGIT"
# Both assertions are paired with a stderr grep: exit 2 is the shared code for
# usage errors, a non-work-tree, an empty scan set and an awk failure, so the
# code alone cannot tell which cause fired.
assert "h6: an unknown flag exits 2" \
    _exits_with 2 bash "$GATE" --bogus-flag
assert "h6: stderr names the unknown argument (not some other exit-2 cause)" \
    bash -c "bash '$GATE' --bogus-flag 2>&1 >/dev/null | grep -q 'unknown argument'"
assert "h6: --repo-root pointing at a non-git dir exits 2" \
    _exits_with 2 bash "$GATE" --repo-root "$NONGIT"
assert "h6: stderr names the non-work-tree (not some other exit-2 cause)" \
    bash -c "bash '$GATE' --repo-root '$NONGIT' 2>&1 >/dev/null | grep -q 'not a git work tree'"

# ===========================================================================
# hD — BRACE-DEPTH DRIFT in the shared production-code view.
#
# The `#[cfg(test)] mod` skipper and the definition-file `in_bundler` skipper
# are both driven by `depth`.  `depth` must therefore be counted from the
# PRODUCTION-CODE view — never from the raw line — or a brace that exists only
# inside a comment, a string literal, a char literal or a raw string silently
# moves it.  Both drift directions defeat the gate:
#
#   POSITIVE drift (a stray `{`) over-extends `in_test`, so real production
#   code after the test module is `next`ed and the fourth-bundler pass goes
#   BLIND — hazard (3) unguarded (hD2/hD3a/hD4/hD5a/hD7).
#   NEGATIVE drift (a stray `}`) clears `in_test` EARLY, so #[cfg(test)] code
#   becomes visible to `_code_has` and the per-site VARIANT pin goes VACUOUS —
#   hazard (2) unguarded (hD1/hD3b/hD5b).  This is the h2b scenario the file
#   already claims to pin; h2b passes only because its fixture happens to be
#   brace-balanced.
#
# Every shape below is drawn from the LIVE scan set, not invented: unbalanced
# brace char literals at crates/reify-kernel-occt/src/lib.rs:4930,4948,9167 and
# crates/reify-test-support/src/orphan_audit.rs:778 (four today, drifting the
# gate's own view NEGATIVE by 3 and 1); raw strings in 277 scan-set files
# (crates/reify-compiler/src/expr.rs:7389, crates/reify-lsp/src/hover.rs:460,
# crates/reify-compiler/src/geometry.rs:3146); a `#[cfg(test)]`-carrying string
# at crates/reify-audit/src/jcodemunch_client.rs:1299; and the
# `starts_with("//") {` shape at crates/reify-audit/src/ptodo.rs:90,111,
# p2_consumer_stub.rs:54,137 and pdssentinel.rs:141.
#
# Same fixture harness as the h-block above (write_baseline / stage /
# _exits_with): one file overwritten per case, EXACT exit codes only.
# ===========================================================================
echo ""
echo "--- (hD): brace-depth drift from comments, strings and char literals ---"

# hD1 — VARIANT PIN GOES VACUOUS via a stray `}` in a test-assertion string.
# Same production flip as h2b, but the test module's first assertion carries
# `"expected }"`, which decrements depth below test_base and releases the
# skipper one line early — so the SECOND test's MorphRegistration::Enabled(
# becomes visible and satisfies the production pin.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_an_unclosed_block() {
        let msg = render_error();
        assert!(msg.contains("expected }"));
    }

    #[test]
    fn names_the_enabled_variant() {
        let _ = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    }
}
RS
stage
assert "hD1: a '}' inside a test-assertion STRING does not release the cfg(test) skipper (variant pin stays live)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD1: stderr names the engine.rs VARIANT-PIN violation (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qF 'gui/src-tauri/src/engine.rs: does not pass the required'"

# hD2 — FOURTH-BUNDLER DETECTION GOES BLIND via a stray `{` in a test string.
# Same body as h3 (which the gate flags), with a #[cfg(test)] module ABOVE it
# whose assertion carries `"{ open brace in a string"`.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn renders_an_open_brace() {
        assert_eq!(render(), "{ open brace in a string");
    }
}

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD2: a '{' inside a test-assertion STRING does not hide the production fourth bundler below it" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD2: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hD3a — CHAR LITERAL, positive drift.  String blanking does not cover char
# literals, so `'{'` inflates depth exactly like the string brace in hD2.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
#[cfg(test)]
mod tests {
    const OPEN: char = '{';

    #[test]
    fn notes_the_delimiter() {
        assert_eq!(OPEN, '{');
    }
}

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD3a: a '{' CHAR LITERAL does not hide the production fourth bundler below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD3a: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hD3b — CHAR LITERAL, negative drift, in the shape the live scan set already
# carries (`rest.find([',', '}'])`).  Asserted through the POSITIVE pass because
# that is where an early skipper release is silent: the production arm is
# flipped to Unavailable and only the test module names Enabled(.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_delimiter() {
        let rest = "a,b";
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        assert!(end > 0);
    }

    #[test]
    fn names_the_enabled_variant() {
        let _ = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    }
}
RS
stage
assert "hD3b: a '}' CHAR LITERAL does not release the cfg(test) skipper (variant pin stays live)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD3b: stderr names the engine.rs VARIANT-PIN violation (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qF 'gui/src-tauri/src/engine.rs: does not pass the required'"

# hD4 — MULTI-LINE RAW STRING.  `r#"…"#` spans lines and honours no escapes, so
# its body must be blanked with cross-line state; an unbalanced `{` inside one
# is the live shape at crates/reify-compiler/src/expr.rs:7389.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_an_unclosed_structure() {
        let source = r#"pub structure Bolt {
    diameter: 5mm
"#;
        assert!(parse(source).is_err());
    }
}

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD4: a '{' inside a multi-line RAW STRING does not hide the production fourth bundler" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD4: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hD5a — BLOCK COMMENT, positive drift (`/* { */`).
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn documents_the_open_brace() {
        /* { */
        assert!(true);
    }
}

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD5a: a '{' inside a /* */ BLOCK COMMENT does not hide the production fourth bundler" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD5a: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hD5b — BLOCK COMMENT, negative drift (`/* } */`), asserted through the
# positive pass for the same reason as hD3b.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}

#[cfg(test)]
mod tests {
    #[test]
    fn documents_the_close_brace() {
        /* } */
        assert!(true);
    }

    #[test]
    fn names_the_enabled_variant() {
        let _ = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    }
}
RS
stage
assert "hD5b: a '}' inside a /* */ BLOCK COMMENT does not release the cfg(test) skipper" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD5b: stderr names the engine.rs VARIANT-PIN violation (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qF 'gui/src-tauri/src/engine.rs: does not pass the required'"

# hD6 — MUST STAY GREEN: no new drift in the FALSE-RED direction.  This is the
# case that forbids "just strip comments first, then count": a `//` inside a
# STRING is not a comment, and truncating there loses the `{` that follows it.
# Eleven lines in the live 562-file scan set change their brace balance under
# that naive truncation (crates/reify-audit/src/ptodo.rs:90,111 among them), so
# a comment-strip-first ordering trades the reported drift for a new one — here,
# releasing the skipper early and FALSE-REDing a legitimate test-module call.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn skips_comment_lines() {
        if "// not a comment".trim_start().starts_with("//") {
            assert!(true);
        }
        if "//x".starts_with("//") {
            assert!(true);
        }
        let mut engine = reify_eval::Engine::new();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        reify_eval::register_shell_extract_compute_fns(&mut engine);
    }
}
RS
stage
assert "hD6: a '//' inside a STRING does not truncate the '{' after it (no false RED on a cfg(test) call)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD7 — `#[cfg(test)]` and `mod … {` INSIDE A STRING must not arm the skipper.
# Live shape: crates/reify-audit/src/jcodemunch_client.rs:1278 and :1455 both
# carry a `"#[cfg(test)]"` source-fragment string inside an array of Rust source
# lines.  Here the fragment array sits at module scope, so test_base is 0 and
# the wrongly-armed skipper never releases — every production line below it,
# including both half-calls, is swallowed.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub const SCAFFOLD: [&str; 2] = [
    "#[cfg(test)]",
    "mod tests {",
];

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD7: a '#[cfg(test)] mod … {' inside a STRING does not arm the test-module skipper" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD7: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hD8 — POSITIVE CONTROL for the whole hD block, so it cannot be satisfied by
# the gate simply becoming unconditionally RED: every hostile shape above, in
# ONE test module, with the half-calls left legitimately INSIDE it and
# engine.rs's production arm correct.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn nothing() {}

#[cfg(test)]
mod tests {
    #[test]
    fn every_hostile_shape_at_once() {
        assert_eq!(render(), "{ open brace in a string");
        let close = '}';
        let source = r#"pub structure Bolt {
    diameter: 5mm
"#;
        /* } */
        /* { */
        if "// not a comment".starts_with("//") {
            assert!(source.is_empty() || close == '}');
        }
        let mut engine = reify_eval::Engine::new();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        reify_eval::register_shell_extract_compute_fns(&mut engine);
    }
}
RS
stage
assert "hD8: every hostile shape at once, half-calls inside the cfg(test) module — gate stays GREEN" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD9 / hD10 — BRACE ON THE LINE *AFTER* THE `mod` KEYWORD.
#
# Every cfg(test) fixture above (h2b, h4, hD1, hD3b, hD5b, hD8) happens to write
# `mod tests {` with the brace on the SAME line, so none of them actually
# exercises the other legal placement:
#
#     #[cfg(test)]
#     mod tests
#     {
#
# That is ordinary Rust, and CLAUDE.md records this repo has NO rustfmt gate
# (`cargo fmt --all --check` reports ~5913 hunks across 685 of 1755 tracked .rs
# files), so it is a shape tracked source really can carry — not a hypothetical.
# Both directions were MEASURED against a throwaway `git init` fixture on this
# tip before these assertions were written, and both are wrong today.
#
# hD9 — the VACUOUS-PIN direction (measured: exit 0, want 1).  Identical in
# intent to h2b — production arm flipped to Unavailable, only a #[cfg(test)]
# module names MorphRegistration::Enabled( — but with the brace on the next
# line the skipper never arms, the test module's mention becomes visible to the
# positive pass, and the per-site variant pin goes VACUOUS.  So hazard (2) /
# esc-2962-66 is unguarded for legal brace placement, and h2b's claim to pin it
# holds only for the same-line spelling.
write_baseline
cat > "$FIX/gui/src-tauri/src/engine.rs" <<'RS'
fn from_engine(engine: &mut reify_eval::Engine) {
    let morph = reify_eval::MorphRegistration::Unavailable { reason: "gui feature off" };
    engine.register_production_compute_fns(morph);
}

#[cfg(test)]
mod tests
{
    #[test]
    fn names_the_enabled_variant() {
        let _ = reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
    }
}
RS
stage
assert "hD9: a cfg(test) module with the brace on the NEXT line does not satisfy the production variant pin" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD9: stderr names the engine.rs VARIANT-PIN violation (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qF 'gui/src-tauri/src/engine.rs: does not pass the required'"

# hD10 — the FALSE-RED direction (measured: exit 1, want 0).  The h4 body — the
# shape of all five real in-src callers — with the brace on the next line.  The
# skipper not arming makes a legitimate test-module half-call visible to the
# NEGATIVE pass, reddening a clean tree.  Paired with hD9 deliberately: a "fix"
# that only widened the skipper would satisfy hD9 while breaking this, and one
# that only narrowed it would do the reverse.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn nothing() {}

#[cfg(test)]
mod tests
{
    #[test]
    fn builds_an_engine() {
        let mut engine = reify_eval::Engine::new();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        reify_eval::register_shell_extract_compute_fns(&mut engine);
    }
}
RS
stage
assert "hD10: a legitimate cfg(test) half-call with the brace on the NEXT line is NOT flagged" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD10b — the `;` guard on the brace-on-next-line handling.  `#[cfg(test)] mod
# tests;` declares an EXTERNAL module file and opens no body here, so it must
# NOT leave the skipper armed for some unrelated later brace — otherwise the
# production fn below it would be swallowed and the fourth-bundler pass would
# go blind exactly as in hD7.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
#[cfg(test)]
mod tests;

pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hD10b: a self-terminating '#[cfg(test)] mod tests;' does not arm the skipper for a later unrelated brace" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hD10b: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# ===========================================================================
# hE — THE INLINE ESCAPE IS A *COMMENT* CONCEPT.
#
# `// trampoline-registration:allow` declares an intentional direct half-call.
# It must therefore be matched against the COMMENT text the lexer drops, not
# against raw $0 (which also carries every string and char literal on the line)
# and not against `code` (from which the comment has already been removed).
# Raw $0 is what the gate uses today, and that is under-flagging.
# ===========================================================================
echo ""
echo "--- (hE): the escape is matched against comment text, not raw \$0 ---"

# hE1 — ESCAPE-IN-STRING (measured: exit 0, want 1).  A production line that
# calls a bundle half AND merely MENTIONS the escape token inside a string
# literal is silently suppressed today: the whole line is `next`ed before the
# half-call is ever tested.  A violation that a nearby string can switch off is
# not a gate.  The mirror-image rationale is already written down in
# check-nan-safe-ordering.sh, in the comment on its `comment_tail ~
# /nan-safe:allow/` match explaining why raw $0 is wrong because a token
# "merely *appears inside a string*" (cite by text, not :NNN — line numbers
# drift, see docs/prds/compute-fea-hardening.md decision 9).
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    let _doc = "trampoline-registration:allow"; reify_eval::compute_targets::register_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hE1: a STRING literal naming the escape token does NOT suppress a real half-call on that line" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hE1: stderr still names the offending site as file:line" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hE2 — the OPPOSITE-direction regression pin.  Moving the escape check off raw
# $0 must not disable the escape mechanism outright, so a REAL trailing
# `// trampoline-registration:allow — <reason>` comment must still suppress —
# including on a line that also carries an unrelated string literal, which is
# precisely the shape that distinguishes "matched against the comment tail"
# from "matched against the lexed code" (the latter suppresses nothing at all,
# because the lexer drops the comment).  Green today; it exists so hE1's fix
# cannot be a blunt deletion of the escape.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    let _label = "bespoke scratch engine";
    reify_eval::compute_targets::register_compute_fns(&mut engine); // trampoline-registration:allow — bespoke fixture, the bundle is wrong here
    reify_eval::register_shell_extract_compute_fns(&mut engine); // trampoline-registration:allow — bespoke fixture, the bundle is wrong here
    engine
}
RS
stage
assert "hE2: a real trailing '// trampoline-registration:allow — <reason>' comment still suppresses" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hE3 — the escape does not LEAK to the next line.  The dropped comment tail is
# per-line state; if it were stashed without being reset at every entry to the
# lexer, an escaped line would silently clear the UNescaped half-call under it.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/lib.rs" <<'RS'
pub fn build_engine() -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new();
    reify_eval::compute_targets::register_compute_fns(&mut engine); // trampoline-registration:allow — bespoke fixture
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}
RS
stage
assert "hE3: an escaped line does not suppress the UNescaped half-call on the following line" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hE3: stderr names the fourth-bundler hit as file:line (not some other exit-1 cause)" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# ===========================================================================
# hG — THE DEFINITION-FILE BUNDLER SKIPPER MUST ARM ON A *DEFINITION*, NOT ON A
# DECLARATION.
#
# Inside EXEMPT_DEFINITION_FILES the body of `fn register_production_compute_fns`
# is legitimately exempt (that body IS the bundler), tracked by `pending_bundler`
# -> `in_bundler`. `pending_bundler` armed on ANY line matching the fn name,
# including one that opens no body — a trait-method declaration, an extern entry,
# a signature whose `{` is several lines down. It then stayed armed until the
# NEXT brace-opening line, and `next`ed that unrelated block out of the negative
# pass wholesale.
#
# That is a SILENT UNDER-FLAG — it fails toward GREEN — inside exactly the two
# files the gate header calls "the single most likely place for someone to add"
# a fourth hand-rolled bundle. Measured on the fixture below: the pre-fix gate
# exits 0, the fixed gate exits 1 naming mod.rs:14/:15.
# ===========================================================================
echo ""
echo "--- (hG): the bundler-body exemption arms on a definition, not a declaration ---"

# hG1 — a trait-method DECLARATION of the bundler, followed by an unrelated
# `impl` block that hand-rolls the bundle. The declaration must not exempt the
# impl block.
write_baseline
cat > "$FIX/crates/reify-eval/src/compute_targets/mod.rs" <<'RS'
pub fn register_compute_fns(engine: &mut crate::Engine) {
    engine.register_compute_fn("solver::elastic_static", elastic_static as crate::ComputeFn);
}

pub trait ComputeRegistrar {
    fn register_production_compute_fns(&mut self, morph: MorphRegistration);
}

impl crate::Engine {
    pub fn register_production_compute_fns(&mut self, morph: MorphRegistration) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
    }

    pub fn register_legacy_compute_fns(&mut self) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
    }
}
RS
stage
assert "hG1: a bundler DECLARATION does not exempt the next unrelated brace-opening block" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hG1: stderr names the hand-rolled sibling in the definition file as file:line" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-eval/src/compute_targets/mod\.rs:[0-9]+'"

# hG2 — the OPPOSITE-direction pin. The real bundler DEFINITION, with a
# MULTI-LINE signature whose `{` lands several lines below the fn name, must
# still arm the skipper — otherwise the arming guard would have turned a real
# exemption into a FALSE RED on the two half-calls the bundler body makes.
write_baseline
cat > "$FIX/crates/reify-eval/src/compute_targets/mod.rs" <<'RS'
pub fn register_compute_fns(engine: &mut crate::Engine) {
    engine.register_compute_fn("solver::elastic_static", elastic_static as crate::ComputeFn);
}

impl crate::Engine {
    pub fn register_production_compute_fns(
        &mut self,
        morph: MorphRegistration,
    ) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
    }
}
RS
stage
assert "hG2: a bundler DEFINITION with a multi-line signature still exempts its body" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hG3 — the multi-line DECLARATION spelling: the `;` lands on its own line.
# Pins the disarm half of the guard (arming survives signature-continuation
# lines, which carry no `;`, but not the terminating `;`).
write_baseline
cat > "$FIX/crates/reify-eval/src/compute_targets/mod.rs" <<'RS'
pub fn register_compute_fns(engine: &mut crate::Engine) {
    engine.register_compute_fn("solver::elastic_static", elastic_static as crate::ComputeFn);
}

pub trait ComputeRegistrar {
    fn register_production_compute_fns(
        &mut self,
        morph: MorphRegistration,
    );
}

impl crate::Engine {
    pub fn register_production_compute_fns(&mut self, morph: MorphRegistration) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
    }

    pub fn register_legacy_compute_fns(&mut self) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
    }
}
RS
stage
assert "hG3: a MULTI-LINE bundler declaration disarms on its ';' and exempts nothing" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hG3: stderr names the hand-rolled sibling as file:line" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-eval/src/compute_targets/mod\.rs:[0-9]+'"

# ===========================================================================
# hF — THE GATE MUST NOT FAIL SILENTLY TOWARD GREEN.
#
# Three distinct ways this gate can report "clean" without having actually
# checked anything, all of which check-nan-safe-ordering.sh on main already
# guards and this copy does not:
#
#   hF1  it scanned NOTHING (SCOPE_PATHSPECS went stale) — indistinguishable,
#        from the caller's side, from having scanned everything and found it
#        clean;
#   hF2  its awk failed — and awk's own failure status is 1 for some modes,
#        which is exactly the gate's "found a violation" code, so the failure
#        is laundered into an ordinary-looking verdict;
#   hF3  its LEXER desynced mid-file — every line after the desync point that
#        matches `carried_in && code == ""` is `next`ed, and an unscanned line
#        can never be flagged.
#
# hF4 pins the one thing that must NOT come with hF3's warning.
# ===========================================================================
echo ""
echo "--- (hF): a gate that checked nothing must not report clean ---"

# hF1 — EMPTY SCAN SET must be a hard error (exit 2), not a vacuous exit 0.
# Two shapes, because the guard has to sit AFTER the `*/tests/*` filter to
# catch the second: a scope check written before the filter passes hF1b.
# Note both fixtures also have all three known sites missing, so the POSITIVE
# pass has already queued exit-1 violations — a stale scope is an
# infrastructure fault and must outrank them, which is what pins the guard's
# placement relative to the final violations check.
SCOPE_FIX="$DET_TMP/scope-empty"
mkdir -p "$SCOPE_FIX/src"
git -C "$SCOPE_FIX" init -q
git -C "$SCOPE_FIX" config user.email test@invalid.local
git -C "$SCOPE_FIX" config user.name test
printf '# not rust\n' > "$SCOPE_FIX/README.md"
printf 'fn main() {}\n' > "$SCOPE_FIX/src/main.rs"   # top-level src/, not crates/*/src/
git -C "$SCOPE_FIX" add -A
assert "hF1a: a repo where SCOPE_PATHSPECS matches NOTHING exits 2, not 0" \
    _exits_with 2 bash "$GATE" --repo-root "$SCOPE_FIX"
assert "hF1a: stderr says the scope matched nothing (not a generic failure)" \
    bash -c "bash '$GATE' --repo-root '$SCOPE_FIX' 2>&1 >/dev/null | grep -qi 'SCOPE_PATHSPECS'"

# hF1b — every pathspec match is filtered out by `*/tests/*`. Single-star git
# pathspecs are not path-boundary-aware, so crates/*/src/*.rs DOES match
# crates/reify-foo/src/tests/helper.rs; the scan set is non-empty before the
# filter and empty after it.
SCOPE_FIX2="$DET_TMP/scope-empty-after-filter"
mkdir -p "$SCOPE_FIX2/crates/reify-foo/src/tests"
git -C "$SCOPE_FIX2" init -q
git -C "$SCOPE_FIX2" config user.email test@invalid.local
git -C "$SCOPE_FIX2" config user.name test
printf 'pub fn helper() {}\n' > "$SCOPE_FIX2/crates/reify-foo/src/tests/helper.rs"
git -C "$SCOPE_FIX2" add -A
assert "hF1b: a scan set emptied by the */tests/* filter exits 2 (guard sits AFTER the filter)" \
    _exits_with 2 bash "$GATE" --repo-root "$SCOPE_FIX2"
# Exit 2 is the SHARED code for four distinct causes (usage error, not-a-git-
# work-tree, empty scan set, awk failure), so the code alone does not pin the
# behaviour this case names. The stderr grep — the idiom hF1a already uses — is
# what distinguishes "the empty-scan guard sits after the */tests/* filter"
# from any unrelated regression that trips one of the other three.
assert "hF1b: stderr says the scope matched nothing (discriminates the four exit-2 causes)" \
    bash -c "bash '$GATE' --repo-root '$SCOPE_FIX2' 2>&1 >/dev/null | grep -qi 'SCOPE_PATHSPECS'"

# hF2 — AWK FAILURE must be exit 2, never laundered into 1.  PATH-shadow awk
# with a stub that always exits 1 and run against the CLEAN baseline: today the
# unchecked `out="$(awk …)"` assignment aborts under `set -e` carrying awk's
# own status, so the gate exits 1 on a clean tree with nothing printed — the
# same code it uses for a genuine violation.  main's rationale for the
# explicit check is at check-nan-safe-ordering.sh, in the comment above its
# `out="$(awk …)"` status check that begins "Checked explicitly (rather than
# left to `set -e`)" (cite by text, not :NNN — line numbers drift, see
# docs/prds/compute-fea-hardening.md decision 9).
#
# hF1's guard is what makes this total: with an empty scan set now impossible,
# the negative-pass loop is guaranteed to execute at least once, so a
# systematically broken awk can never reach the verdict at all.
STUB_BIN="$DET_TMP/stub-bin"
mkdir -p "$STUB_BIN"
printf '#!/bin/sh\nexit 1\n' > "$STUB_BIN/awk"
chmod +x "$STUB_BIN/awk"
write_baseline
stage
assert "hF2: a failing awk exits 2, not 1 (a clean tree must not look like a violation)" \
    _exits_with 2 env "PATH=$STUB_BIN:$PATH" bash "$GATE" --repo-root "$FIX"
assert "hF2: stderr names the awk failure and the file it was scanning" \
    bash -c "env 'PATH=$STUB_BIN:$PATH' bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -q 'awk failed while scanning'"

# hF3 — LEXER DESYNC must WARN, and must be VERDICT-NEUTRAL.
# The fixture is deliberately malformed Rust (an unterminated `/*`): a
# well-formed file that desyncs this lexer would BE the undiscovered bug the
# warning exists to surface, so the desync has to be induced directly.
#
# Verdict-neutrality matters more here than on main: this gate's scan set is
# 562 files against nan-safe's 121.  It measures clean today (every file ends
# balanced — see hF4), but the warning must be structurally incapable of
# turning a green tree red even if that stops being true.
write_baseline
mkdir -p "$FIX/crates/reify-foo/src"
cat > "$FIX/crates/reify-foo/src/unbalanced.rs" <<'RS'
pub fn documented() {}

/* this block comment is never terminated, so the lexer ends the file
   still inside it and every following line lexes to nothing
RS
stage
assert "hF3a: an unbalanced file WARNs on stderr" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -q 'lexer state unbalanced'"
assert "hF3a: the WARN names the offending file" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep 'lexer state unbalanced' | grep -q 'unbalanced\.rs'"
assert "hF3a: the WARN is VERDICT-NEUTRAL — an otherwise-clean tree still exits 0" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hF3b — the other direction: the WARN must not mask or upgrade a real
# violation either.  Same unbalanced file, plus the h3 fourth bundler.
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
assert "hF3b: a real violation alongside an unbalanced file still exits 1 (not 2, not 0)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"
assert "hF3b: the violation is still reported as file:line despite the WARN" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -qE 'crates/reify-foo/src/lib\.rs:[0-9]+'"

# hF4 — NO SPURIOUS WARN FROM THE POSITIVE PASS.  `_code_has` deliberately
# early-exits on its first match (`code ~ pat { found = 1; exit }`), which
# leaves the lexer legitimately unbalanced at that point — mid-fn, depth > 0.
# The positive pass makes six such calls across the three known sites, so a
# verbatim port of main's END block would warn six times on every SUCCESSFUL
# run.  This divergence is specific to this guard: nan-safe has no early-exit
# helper, which is the concrete reason its END block cannot be copied
# byte-for-byte.
#
# Asserted on the CLEAN fixture first — every file there is balanced, so the
# ONLY possible source of a warning is the early exit — and then on the real
# tree, whose 562-file scan set measures 562/562 balanced today.  No coverage
# is lost by silencing the positive pass: all three known sites are also in
# the negative pass's scan set and are scanned there in full.
write_baseline
stage
assert "hF4: a clean fixture exits 0 (positive control for the stderr assertion below)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"
assert "hF4: _code_has's early exit produces NO 'lexer state unbalanced' warning" \
    bash -c "! bash '$GATE' --repo-root '$FIX' 2>&1 >/dev/null | grep -q 'lexer state unbalanced'"
assert "hF4: the REAL tree produces no 'lexer state unbalanced' warning either" \
    bash -c "! bash '$GATE' --repo-root '$REPO_ROOT' 2>&1 >/dev/null | grep -q 'lexer state unbalanced'"

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

# TEST_PLAN_FULL is the un-stripped capture taken at the top of this file — (b7)
# needs the semaphore ACQUIRE/RELEASE marker lines, which are comments and are
# removed by TEST_PLAN_SEGS's `_plan_segs`. Its completeness is asserted in (a0).
#
# The offline-role plan is captured the same guarded way, because (b9) negates
# it: a truncated capture would satisfy `! grep` silently.
OFFLINE_PLAN_FULL=""
capture_print_plan OFFLINE_PLAN_FULL "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    env DF_VERIFY_ROLE=offline bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan || true
OFFLINE_PLAN_SEGS="$(_plan_segs "$OFFLINE_PLAN_FULL")"
export OFFLINE_PLAN_FULL OFFLINE_PLAN_SEGS

assert "b0: offline-role plan capture is complete (structural markers present)" \
    plan_capture_complete "$OFFLINE_PLAN_FULL"
# POSITIVE CONTROL for (b9)'s negation. Without it, an offline plan that was
# empty for ANY reason — a verify.sh regression, a role-parsing change — would
# satisfy "has no gui-feature pass" while proving nothing. The offline lane runs
# the heavy #[ignore] partition, so it must still carry a real test invocation.
assert "b0: the offline-role plan still carries a 'cargo (test|nextest run)' line" \
    bash -c 'printf "%s\n" "$OFFLINE_PLAN_SEGS" | grep -qE "cargo (test|nextest run)"'

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
# test_verify_throughput.sh:98-113) — never runs real cargo.
echo ""
echo "--- (b10): still emitted for --scope branch when the change is under gui/src-tauri/ ---"

# plan_capture_lib.sh is sourced at the top of this file (the canonical captures
# in (a0) already depend on it); only the copy-list preflight is local to b10.
[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

BR_FIX="$DET_TMP/branch-fixture"
mkdir -p "$BR_FIX/scripts" "$BR_FIX/.config"
for _f in verify.sh occt-scope-lib.sh occt-touching-crates.txt release-scope-lib.sh \
          release-sensitive-crates.txt affected-crates-lib.sh lib_test_semaphore.sh \
          lib_slot_acquire.sh lib_clock_stop.sh cpu-admit.sh lib_proc_reaper.sh \
          lib_git_env_scrub.sh \
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

# ST_FIX — a SECOND hermetic throwaway repo, built with b10's idiom.  BR_FIX is
# deliberately NOT reused: its change is COMMITTED on task-branch, so its staged
# diff is empty.  `--scope staged` reads `git diff --cached`, and an empty staged
# diff yields RUN_RUST=0 and zero test passes, which would make every assertion
# below vacuously green.  The reify-doc file is therefore left in the INDEX,
# uncommitted.
ST_FIX="$DET_TMP/staged-fixture"
mkdir -p "$ST_FIX/scripts" "$ST_FIX/.config"
for _f in verify.sh occt-scope-lib.sh occt-touching-crates.txt release-scope-lib.sh \
          release-sensitive-crates.txt affected-crates-lib.sh lib_test_semaphore.sh \
          lib_slot_acquire.sh lib_clock_stop.sh cpu-admit.sh lib_proc_reaper.sh \
          lib_git_env_scrub.sh \
          gen-nextest-config.sh heavy-test-filter-lib.sh; do
    cp "$REPO_ROOT/scripts/$_f" "$ST_FIX/scripts/$_f"
done
cp "$REPO_ROOT/.config/nextest.toml" "$ST_FIX/.config/nextest.toml"
chmod +x "$ST_FIX/scripts/verify.sh"
assert_source_closure_copied "$REPO_ROOT/scripts" "$ST_FIX/scripts" verify.sh || exit 1
git -C "$ST_FIX" init -q
git -C "$ST_FIX" config user.email test@invalid.local
git -C "$ST_FIX" config user.name test
git -C "$ST_FIX" add scripts .config
git -C "$ST_FIX" commit -q -m base
git -C "$ST_FIX" branch -M main
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
# from "the closure was never computed on this tier at all": both take arm 2 and
# both yield 1.  Every other staged case here is driven through
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
# (`crates scripts`) rather than collapsing — neither is reify-gui, so arm 3
# silently narrowed the pass AWAY.  Measured at 0 emitted passes with
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
# knob is malformed, not that the closure excludes reify-gui — arm 2, not arm 3.
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
# skip-OUTSIDE-assert form (test_occt_gated_scope.sh:246-254).
if [ "$PLAN_HAS_NEXTEST" -gt 0 ]; then
    assert "gui-feature nextest pass carries NO --test-threads fragment when the flag is unset" \
        bash -c '[ -n "$GUI_PASS" ] && ! printf "%s\n" "$GUI_PASS" | grep -qF -- "--test-threads"'
else
    echo "  SKIP: (b12) default-shape — the cargo-test fallback arm always carries"
    echo "        '-- --test-threads=1' (its OCCT serialization guard), so 'no"
    echo "        --test-threads by default' is a nextest-arm-only property."
fi

test_summary
