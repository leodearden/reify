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

# -- (e): script exists and is executable on disk -------------------------------
echo ""
echo "--- (e): scripts/check-compute-trampoline-registration.sh exists and is executable ---"
assert "scripts/check-compute-trampoline-registration.sh exists" \
    test -f "$GATE"
assert "scripts/check-compute-trampoline-registration.sh is executable" \
    test -x "$GATE"

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

test_summary
