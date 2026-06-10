#!/usr/bin/env bash
# Infrastructure test for task 4378.
# Drift catcher for the gui-feature compile-check added to scripts/verify.sh.
#
# Drives verify.sh via --print-plan (hermetic: never builds anything) and
# asserts the `cargo check -p reify-gui --features gui --tests` gate is:
#   (a) PRESENT for a Rust change (gui/src-tauri/src/main.rs) under lint/all;
#   (b) ABSENT  for a frontend-only change (gui/src/foo.ts — RUN_RUST=0);
#   (c) ABSENT  for a docs-only change (docs/x.md — RUN_RUST=0);
#   (d) ABSENT  for action=test (DO_LINT=0) even with a staged Rust file;
#   (e) PRESENT for action=lint with a staged Rust file;
#   (f) the cargo check line carries the nice/ionice role prefix while the
#       ensure-gui-sidecar-placeholder.sh && portion does NOT.
#
# Reuses the make_fixture / plan_has / plan_lacks pattern from
# test_verify_scope.sh (isolated git repo + three scripts, --print-plan oracle).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify.sh gui-feature compile-check wiring tests ==="

# ---------------------------------------------------------------------------
# Fixture setup
# ---------------------------------------------------------------------------
# make_fixture VARNAME — create an isolated git repo seeded with the scripts
# verify.sh needs (same pattern as test_verify_scope.sh).
make_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh"                   "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh"           "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt"    "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh"        "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh"       "$dir/scripts/affected-crates-lib.sh"
    cp "$REPO_ROOT/scripts/lib_test_semaphore.sh"        "$dir/scripts/lib_test_semaphore.sh"
    cp "$REPO_ROOT/scripts/gen-nextest-config.sh"        "$dir/scripts/gen-nextest-config.sh"
    mkdir -p "$dir/.config"
    cp "$REPO_ROOT/.config/nextest.toml"                 "$dir/.config/nextest.toml"
    chmod +x "$dir/scripts/verify.sh"
    # Preflight: fail loudly if verify.sh sources a lib that was not copied to the
    # fixture.  Without this check a new 'source "$SCRIPT_DIR/foo.sh"' line in
    # verify.sh would surface only as an opaque "scripts/foo.sh not found" startup
    # error on every plan invocation (the pre-existing affected-crates-lib.sh gap).
    while IFS= read -r _lib; do
        [ -f "$dir/scripts/$_lib" ] || {
            echo "ERROR: make_fixture: '$_lib' is source'd by verify.sh" \
                 "but was not copied to the fixture." >&2
            echo "       Fix: add cp \"\$REPO_ROOT/scripts/$_lib\" \"\$dir/scripts/$_lib\"" \
                 "in make_fixture." >&2
            exit 1
        }
    done < <(grep -E 'source "\$SCRIPT_DIR/' "$dir/scripts/verify.sh" \
                 | sed -n 's|.*source "\$SCRIPT_DIR/\([^"]*\)".*|\1|p' || true)
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    printf -v "$_var" '%s' "$dir"
}

FIX=""
make_fixture FIX

# plan_for ACTION <file...> — stage the given files, capture the verify.sh plan
# for `<ACTION> --scope staged --profile debug`, then clean up.
# Output is written to PLAN_OUT.
PLAN_OUT=""
plan_for() {
    local action="$1"; shift
    local f
    for f in "$@"; do
        mkdir -p "$FIX/$(dirname "$f")"
        printf 'x\n' > "$FIX/$f"
        git -C "$FIX" add "$f"
    done
    PLAN_OUT="$(cd "$FIX" && bash scripts/verify.sh "$action" --profile debug --scope staged --print-plan)"
    git -C "$FIX" reset -q -- . 2>/dev/null || true
    for f in "$@"; do rm -f "$FIX/$f"; done
}

# Convenience predicates over PLAN_OUT.
plan_has()   { printf '%s\n' "$PLAN_OUT" | grep -qE "$1"; }
plan_lacks() { ! printf '%s\n' "$PLAN_OUT" | grep -qE "$1"; }

# ---------------------------------------------------------------------------
# Scenario (a): Rust change under `all` → gui-feature check PRESENT
# ---------------------------------------------------------------------------
echo ""
echo "--- (a) gui/src-tauri/src/main.rs + action=all → check PRESENT ---"
plan_for all gui/src-tauri/src/main.rs
assert "(a) RUN_RUST=1 (Rust change sets rust scope)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1"' _ "$PLAN_OUT"
assert "(a) gui-feature check line present (ensure-placeholder && cargo check)" \
    plan_has 'ensure-gui-sidecar-placeholder\.sh &&.*cargo check -p reify-gui --features gui --tests'

# ---------------------------------------------------------------------------
# Scenario (b): frontend-only TS change → gui-feature check ABSENT (RUN_RUST=0)
# ---------------------------------------------------------------------------
echo ""
echo "--- (b) gui/src/foo.ts (frontend-only) → check ABSENT ---"
plan_for all gui/src/foo.ts
assert "(b) RUN_RUST=0 (frontend-only change)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0"' _ "$PLAN_OUT"
assert "(b) gui-feature check absent (RUN_RUST=0 gating)" \
    plan_lacks 'cargo check -p reify-gui --features gui'

# ---------------------------------------------------------------------------
# Scenario (c): docs-only change → gui-feature check ABSENT
# ---------------------------------------------------------------------------
echo ""
echo "--- (c) docs/x.md (docs-only) → check ABSENT ---"
plan_for all docs/x.md
assert "(c) RUN_RUST=0 (docs-only change)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0"' _ "$PLAN_OUT"
assert "(c) gui-feature check absent (docs-only)" \
    plan_lacks 'cargo check -p reify-gui --features gui'

# ---------------------------------------------------------------------------
# Scenario (d): action=test with a staged Rust file → check ABSENT (DO_LINT=0)
# ---------------------------------------------------------------------------
echo ""
echo "--- (d) action=test + staged Rust → check ABSENT (lint-side only) ---"
plan_for test gui/src-tauri/src/main.rs
assert "(d) RUN_RUST=1 (Rust change)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1"' _ "$PLAN_OUT"
assert "(d) gui-feature check absent for action=test (DO_LINT=0)" \
    plan_lacks 'cargo check -p reify-gui --features gui'

# ---------------------------------------------------------------------------
# Scenario (e): action=lint with a staged Rust file → check PRESENT
# ---------------------------------------------------------------------------
echo ""
echo "--- (e) action=lint + staged Rust → check PRESENT ---"
plan_for lint gui/src-tauri/src/main.rs
assert "(e) RUN_RUST=1 (Rust change)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1"' _ "$PLAN_OUT"
assert "(e) gui-feature check present for action=lint" \
    plan_has 'ensure-gui-sidecar-placeholder\.sh &&.*cargo check -p reify-gui --features gui --tests'

# ---------------------------------------------------------------------------
# Scenario (f): prefix-purity — cargo check line has nice/ionice; ensure
# script does NOT carry the prefix (DF_VERIFY_ROLE=task, action=all scope=all)
# ---------------------------------------------------------------------------
echo ""
echo "--- (f) prefix-purity: cargo check prefixed, ensure line clean ---"
ROLE_PLAN="$(DF_VERIFY_ROLE=task bash "$FIX/scripts/verify.sh" all --scope all --profile debug --print-plan)"
# f1: the cargo check -p reify-gui line carries the task role prefix
assert "(f) cargo check line has 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c 'printf "%s\n" "$1" | grep -qE "nice -n 15 ionice -c 2 -n 7 cargo check -p reify-gui"' \
    _ "$ROLE_PLAN"
# f2: the ensure-gui-sidecar-placeholder.sh invocation is NOT preceded by the nice/ionice prefix
assert "(f) ensure-gui-sidecar-placeholder.sh portion NOT preceded by nice/ionice" \
    bash -c '! printf "%s\n" "$1" | grep -qE "nice -n.*ensure-gui-sidecar-placeholder"' \
    _ "$ROLE_PLAN"

test_summary
