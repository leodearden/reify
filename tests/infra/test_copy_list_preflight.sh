#!/usr/bin/env bash
# Infrastructure test for task 5154.
#
# Hermetic unit test for the shared transitive-closure copy-list preflight
# helper (tests/infra/copy_list_preflight_lib.sh), plus behavioral WIRING
# assertions (appended by later steps) proving each of the three verify.sh
# sandbox meta-tests (test_verify_throughput.sh, test_verify_scope.sh,
# test_scope_boundary.sh) actually consults it at fixture-build time.
#
# Two halves:
#   1. SYNTHETIC closure/drift grammar coverage — a throwaway "srepo/scripts"
#      tree exercises every source-line idiom the real scripts/ tree uses
#      (direct $SCRIPT_DIR/, nested-quote $(dirname BASH_SOURCE)/, a bare
#      $VAR_DIR/ idiom), the false-match decoys task 4525 already guards
#      against (comment lines, `# shellcheck source=`, a string literal that
#      merely mentions the token), the documented pure-variable-indirection
#      limitation (skipped, not resolved), and a back-edge cycle.
#   2. REAL-CLOSURE non-vacuity check against the live $REPO_ROOT/scripts
#      tree: the derived closure of verify.sh must contain lib_slot_acquire.sh
#      even though verify.sh never sources it directly (only transitively,
#      via lib_test_semaphore.sh) — proving the recursive helper protects a
#      lib the OLD direct-only grep (test_verify_throughput.sh's inline
#      preflight, pre-task-5154) would miss.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== copy_list_preflight_lib.sh: transitive closure + drift-detection ==="

# ---------------------------------------------------------------------------
# Checker helpers (predicates run directly by assert(); exit code is the
# PASS/FAIL signal, stdout/stderr is captured and dumped by assert() on FAIL).
# ---------------------------------------------------------------------------

# closure_equals DIR ENTRY EXPECTED_NEWLINE_LIST — exact-match check.
closure_equals() {
    local dir="$1" entry="$2" expected="$3" got
    got="$(derive_source_closure "$dir" "$entry")"
    if [ "$got" != "$expected" ]; then
        echo "expected:"
        printf '%s\n' "$expected"
        echo "got:"
        printf '%s\n' "$got"
        return 1
    fi
}

# closure_has DIR ENTRY NEEDLE — true iff NEEDLE is an exact line of the closure.
closure_has() {
    local dir="$1" entry="$2" needle="$3"
    derive_source_closure "$dir" "$entry" | grep -qxF "$needle"
}

# closure_lacks DIR ENTRY NEEDLE — true iff NEEDLE is NOT a line of the closure.
closure_lacks() {
    local dir="$1" entry="$2" needle="$3"
    ! derive_source_closure "$dir" "$entry" | grep -qxF "$needle"
}

# closure_terminates DIR ENTRY — proves the walk halts despite a back-edge.
closure_terminates() {
    local dir="$1" entry="$2"
    timeout 10 bash -c 'source "$1"; derive_source_closure "$2" "$3" >/dev/null' \
        _ "$SCRIPT_DIR/copy_list_preflight_lib.sh" "$dir" "$entry"
}

# drift_detected REPO_SCRIPTS FIXTURE_SCRIPTS ENTRY NEEDLE... — true iff
# assert_source_closure_copied returns non-zero AND its stderr names every
# NEEDLE (as well as the "copy-list drift: missing" phrase).
drift_detected() {
    local repo_dir="$1" fixture_dir="$2" entry="$3"
    shift 3
    local out rc needle
    out="$(assert_source_closure_copied "$repo_dir" "$fixture_dir" "$entry" 2>&1)"
    rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "expected non-zero return; got 0. output:"
        printf '%s\n' "$out"
        return 1
    fi
    case "$out" in
        *"copy-list drift: missing"*) ;;
        *)
            echo "missing 'copy-list drift: missing' phrase. output:"
            printf '%s\n' "$out"
            return 1
            ;;
    esac
    for needle in "$@"; do
        case "$out" in
            *"$needle"*) ;;
            *)
                echo "missing expected needle '$needle'. output:"
                printf '%s\n' "$out"
                return 1
                ;;
        esac
    done
}

# ---------------------------------------------------------------------------
# Build a synthetic authoritative "scripts/" tree covering every grammar case.
#
#   verify.sh --(direct $SCRIPT_DIR/)--> liba.sh, libb.sh
#   liba.sh   --(nested-quote $(dirname BASH_SOURCE) idiom)--> libc.sh
#   libc.sh   --(bare $VAR_DIR/ idiom, depth-3)--> libd.sh
#   libc.sh   --(pure-variable indirection; documented limitation)--> libe.sh   [NOT resolved]
#   libd.sh   --(back-edge, cycle-safety)--> liba.sh
#   libb.sh   is a leaf (no sources)
#
# Plus decoys inside verify.sh that a naive "mentions the token" grep would
# false-match on (task 4525 class): a comment-only source line, a
# `# shellcheck source=` directive, and an echoed string literal.
# ---------------------------------------------------------------------------
SREPO="$(mktemp -d)"
_TMPDIRS+=("$SREPO")
mkdir -p "$SREPO/scripts"

cat > "$SREPO/scripts/verify.sh" <<'VERIFY_EOF'
#!/usr/bin/env bash
# synthetic verify.sh for copy-list-preflight closure tests (task 5154)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   source "$SCRIPT_DIR/comment_only.sh"
# shellcheck source=scripts/sc_only.sh
echo 'source "$X/echo_decoy.sh"'
source "$SCRIPT_DIR/liba.sh"
source "$SCRIPT_DIR/libb.sh"
VERIFY_EOF

cat > "$SREPO/scripts/liba.sh" <<'LIBA_EOF'
#!/usr/bin/env bash
# synthetic liba.sh — sources libc.sh via the nested-quote BASH_SOURCE idiom.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/libc.sh"
LIBA_EOF

cat > "$SREPO/scripts/libb.sh" <<'LIBB_EOF'
#!/usr/bin/env bash
# synthetic libb.sh — leaf, no further sources.
LIBB_EOF

cat > "$SREPO/scripts/libc.sh" <<'LIBC_EOF'
#!/usr/bin/env bash
# synthetic libc.sh — sources libd.sh via a bare $VAR_DIR idiom (depth-3),
# plus a pure-variable indirection to libe.sh that must NOT be resolved
# (documented limitation: pure-variable `source "$var"` lines are not
# statically parsed — the filename lives on a separate assignment line/
# statement that does not itself start with source/.).
LIBC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$LIBC_DIR/libd.sh"
_p="$LIBC_DIR/libe.sh"; source "$_p"
LIBC_EOF

cat > "$SREPO/scripts/libd.sh" <<'LIBD_EOF'
#!/usr/bin/env bash
# synthetic libd.sh — back-edge to liba.sh (cycle-safety guard).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/liba.sh"
LIBD_EOF

cat > "$SREPO/scripts/libe.sh" <<'LIBE_EOF'
#!/usr/bin/env bash
# synthetic libe.sh — reachable only via a pure-variable indirection from
# libc.sh; must NOT appear in the derived closure (documented limitation).
LIBE_EOF

# Partial fixture: only liba.sh + libb.sh copied — simulates copy-list drift
# (libc.sh and libd.sh missing).
FIXTURE_PARTIAL="$(mktemp -d)"
_TMPDIRS+=("$FIXTURE_PARTIAL")
mkdir -p "$FIXTURE_PARTIAL/scripts"
cp "$SREPO/scripts/verify.sh" "$FIXTURE_PARTIAL/scripts/verify.sh"
cp "$SREPO/scripts/liba.sh" "$FIXTURE_PARTIAL/scripts/liba.sh"
cp "$SREPO/scripts/libb.sh" "$FIXTURE_PARTIAL/scripts/libb.sh"

# Full fixture: every real closure member copied (libe.sh intentionally
# excluded — it is never part of the closure, so it must not be required).
FIXTURE_FULL="$(mktemp -d)"
_TMPDIRS+=("$FIXTURE_FULL")
mkdir -p "$FIXTURE_FULL/scripts"
cp "$SREPO/scripts/verify.sh" "$FIXTURE_FULL/scripts/verify.sh"
cp "$SREPO/scripts/liba.sh" "$FIXTURE_FULL/scripts/liba.sh"
cp "$SREPO/scripts/libb.sh" "$FIXTURE_FULL/scripts/libb.sh"
cp "$SREPO/scripts/libc.sh" "$FIXTURE_FULL/scripts/libc.sh"
cp "$SREPO/scripts/libd.sh" "$FIXTURE_FULL/scripts/libd.sh"

echo ""
echo "--- derive_source_closure: synthetic grammar coverage ---"
assert "closure(verify.sh) == exactly {liba,libb,libc,libd}.sh" \
    closure_equals "$SREPO/scripts" verify.sh "$(printf 'liba.sh\nlibb.sh\nlibc.sh\nlibd.sh')"
assert "closure excludes comment-only decoy line" \
    closure_lacks "$SREPO/scripts" verify.sh comment_only.sh
assert "closure excludes shellcheck-directive decoy line" \
    closure_lacks "$SREPO/scripts" verify.sh sc_only.sh
assert "closure excludes echoed-string decoy line" \
    closure_lacks "$SREPO/scripts" verify.sh echo_decoy.sh
assert "closure excludes libe.sh (pure-variable indirection, documented limitation)" \
    closure_lacks "$SREPO/scripts" verify.sh libe.sh
assert "closure walk terminates despite libd->liba back-edge (cycle-safety)" \
    closure_terminates "$SREPO/scripts" verify.sh

echo ""
echo "--- assert_source_closure_copied: drift detection ---"
assert "drift: only {liba,libb} copied -> non-zero, names libc.sh AND libd.sh" \
    drift_detected "$SREPO/scripts" "$FIXTURE_PARTIAL/scripts" verify.sh libc.sh libd.sh
assert "no drift: full closure {liba,libb,libc,libd} copied -> zero" \
    assert_source_closure_copied "$SREPO/scripts" "$FIXTURE_FULL/scripts" verify.sh

echo ""
echo "--- derive_source_closure: REAL closure non-vacuity (live \$REPO_ROOT/scripts) ---"
# Non-vacuity discriminator: lib_slot_acquire.sh is reached ONLY transitively
# (verify.sh -> lib_test_semaphore.sh -> lib_slot_acquire.sh) — a direct-only
# grep of verify.sh (the pre-task-5154 state of test_verify_throughput.sh's
# inline preflight) would miss it entirely.
assert "real closure(verify.sh) contains TRANSITIVE lib_slot_acquire.sh (non-vacuity)" \
    closure_has "$REPO_ROOT/scripts" verify.sh lib_slot_acquire.sh
assert "real closure(verify.sh) contains direct occt-scope-lib.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh occt-scope-lib.sh
assert "real closure(verify.sh) contains direct release-scope-lib.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh release-scope-lib.sh
assert "real closure(verify.sh) contains direct affected-crates-lib.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh affected-crates-lib.sh
assert "real closure(verify.sh) contains direct lib_test_semaphore.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh lib_test_semaphore.sh
assert "real closure(verify.sh) contains direct cpu-admit.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh cpu-admit.sh
assert "real closure(verify.sh) contains direct lib_proc_reaper.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh lib_proc_reaper.sh
assert "real closure(verify.sh) contains direct heavy-test-filter-lib.sh" \
    closure_has "$REPO_ROOT/scripts" verify.sh heavy-test-filter-lib.sh

echo ""
echo "--- wiring: test_verify_throughput.sh consults the shared preflight ---"
# Behavioral proof (not a source-grep): inject a phantom lib that is never
# copied to any fixture and assert the meta-test itself exits non-zero.
# RED until step-4 wires make_branch_fixture to assert_source_closure_copied
# — today throughput.sh's own inline direct-only preflight (pre-task-5154)
# ignores this env entirely, so the suite runs to completion and exits 0.
# `timeout`-guarded so a regression here can't wedge the pool.
assert "throughput.sh preflight fires on injected copy-list drift" \
    bash -c 'REIFY_COPY_LIST_PREFLIGHT_INJECT_PHANTOM=__never_copied__.sh timeout 120 bash "$1" >/dev/null 2>&1; [ $? -ne 0 ]' \
    _ "$REPO_ROOT/tests/infra/test_verify_throughput.sh"

test_summary
