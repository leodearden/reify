#!/usr/bin/env bash
# tests/infra/test_infra_git_env_isolation.sh
#
# Regression pin for task #7106: the infra-test EXECUTION SITES must strip the
# git repository environment before spawning a member test.
#
# THE DEFECT THIS PINS (measured on git 2.43.0, the version setup-dev.sh's
# Ubuntu 24.04 floor ships).  git exports GIT_INDEX_FILE into the environment
# of `pre-commit` and `pre-merge-commit` — probed directly with an env-dumping
# hook, its value is the RELATIVE string `.git/index`, and GIT_DIR /
# GIT_WORK_TREE / GIT_COMMON_DIR are NOT exported.  A relative GIT_INDEX_FILE
# resolves against CWD, and hooks/project-checks runs verify.sh with
# CWD=REPO_ROOT, so `.git/index` names the REAL index.  Hermetic fixtures under
# tests/infra/ build throwaway repos with `git -C "$FIX"` and never `cd`, and
# GIT_INDEX_FILE OUTRANKS `git -C` — so an unscrubbed member's
# `git -C "$FIX" add -A` writes the FIXTURE's file list into the REAL index.
# Measured blast radius on a real lane: the index went 480763 -> 4827 bytes and
# `git diff --cached --stat` then read "4117 files changed, 2010075 deletions".
#
# SAFETY (task #7106 pre-1).  Every hostile-env arm below targets a throwaway
# `mktemp -d` scratch repo.  This file must NEVER point GIT_INDEX_FILE (or any
# sibling) at a real reify checkout: the published repro is itself destructive,
# and with GIT_DIR added it writes `bare = true` into the SHARED common-dir
# config that all ~253 linked worktrees read (esc-7106-1).
#
# ARMS
#   A  CONTROL / anti-vacuity, asserted FIRST — prove the poison actually bites
#      on THIS git.  If a future release stopped honouring GIT_INDEX_FILE over
#      `git -C`, arm C would pass while testing nothing.  Same shape as
#      tests/infra/test_host_global_unit_pinning.sh's A6a.
#   B  The shared runner helper exists and exposes its documented API.
#   C  BEHAVIOURAL — a member test run through `reify_git_env_scrub` under a
#      hook-shaped hostile env is fully green AND leaves the poisoned index
#      byte-identical (the damage detector: the point is that no fixture write
#      escapes).
#
# Classified `intra-run-serial` in tests/infra/run-all-classification.manifest,
# matching test_reify_audit_ptodo.sh's own row — this file re-invokes that
# member, so it inherits its bucket.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/scripts/lib_portable.sh" ] || { echo "ERROR: lib_portable.sh not found at $REPO_ROOT/scripts/lib_portable.sh"; exit 1; }
# shellcheck source=scripts/lib_portable.sh
source "$REPO_ROOT/scripts/lib_portable.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

SCRUB_LIB="$REPO_ROOT/scripts/lib_git_env_scrub.sh"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"

# _clean_git <git args...> — git for FIXTURE CONSTRUCTION AND INSPECTION only,
# with the ambient repository environment stripped.  Deliberately hand-written
# rather than routed through the lib under test: a fixture builder that used the
# subject's own helper could not distinguish "the helper works" from "the helper
# is a no-op in an already-clean environment".  Same precedent and same reason as
# tests/infra/test_host_global_unit_pinning.sh:85-88's _fixture_git.
_clean_git() {
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE git "$@"
}

# _mk_repo VARNAME — mint a throwaway git repo, registering it for cleanup and
# writing its path to the named variable.  MAIN-SHELL ONLY: the _TMPDIRS append
# is silently discarded inside a command-substitution subshell (leaking the
# tree), which is why this sets a global instead of echoing.
_mk_repo() {
    local _var="$1" d
    d="$(mktemp -d)"
    _TMPDIRS+=("$d")
    _clean_git -C "$d" init -q
    _clean_git -C "$d" config user.email "test@test.com"
    _clean_git -C "$d" config user.name "Test"
    printf -v "$_var" '%s' "$d"
}

_sha_of() { portable_sha256 "$1" | awk '{print $1}'; }

echo "=== infra git-env isolation tests (task #7106) ==="

# ---------------------------------------------------------------------------
# Arm A — CONTROL: the poison really does redirect a raw `git -C`.
#
# S is the "real repo" stand-in; F is the "hermetic fixture" stand-in.  The
# poison names S's index while `git -C` names F — exactly the production shape,
# with a throwaway repo standing in for the checkout.
# ---------------------------------------------------------------------------
echo ""
echo "--- A: CONTROL — GIT_INDEX_FILE outranks \`git -C\` on this git ---"

S_CTL=""
F_CTL=""
_mk_repo S_CTL
_mk_repo F_CTL

printf 'seed\n' > "$S_CTL/seed.txt"
_clean_git -C "$S_CTL" add seed.txt
printf 'fixture-only\n' > "$F_CTL/fixture_probe.txt"

assert "A0: precondition — S's index lists its own seed.txt before the poison" \
    bash -c 'printf "%s\n" "$1" | grep -qx "seed.txt"' \
        _ "$(_clean_git -C "$S_CTL" ls-files --cached)"

GIT_INDEX_FILE="$S_CTL/.git/index" git -C "$F_CTL" add -A || true

_A_S_STAGED="$(_clean_git -C "$S_CTL" ls-files --cached || true)"
_A_F_STAGED="$(_clean_git -C "$F_CTL" ls-files --cached || true)"

assert "A1: poisoned \`git -C \$F add -A\` wrote F's file list into S's index" \
    bash -c 'printf "%s\n" "$1" | grep -qx "fixture_probe.txt"' _ "$_A_S_STAGED"
assert "A2: ... REPLACING S's own content (seed.txt gone — the measured damage shape)" \
    bash -c '! printf "%s\n" "$1" | grep -qx "seed.txt"' _ "$_A_S_STAGED"
assert "A3: ... while F's own index was never written (the fixture repo is the victim's alibi)" \
    test -z "$_A_F_STAGED"

# ---------------------------------------------------------------------------
# Arm B — the shared runner helper and its API.
# ---------------------------------------------------------------------------
echo ""
echo "--- B: scripts/lib_git_env_scrub.sh exposes the runner API ---"

assert "B1: scripts/lib_git_env_scrub.sh exists" test -f "$SCRUB_LIB"

_SCRUB_LIB_OK=0
if [ -f "$SCRUB_LIB" ]; then
    # shellcheck source=scripts/lib_git_env_scrub.sh
    source "$SCRUB_LIB" && _SCRUB_LIB_OK=1
fi

assert "B2: sourcing lib_git_env_scrub.sh succeeds" test "$_SCRUB_LIB_OK" -eq 1
assert "B3: REIFY_GIT_ENV_SCRUB_VARS is exported and non-empty" \
    bash -c '[ -n "${1:-}" ]' _ "${REIFY_GIT_ENV_SCRUB_VARS:-}"
# Asserted against THIS shell, not a `bash -c` subprocess: a sourced function is
# invisible to a child, so a `bash -c 'declare -F ...'` probe would report
# "missing" even once the lib lands.
_has_fn() { declare -F "$1" >/dev/null 2>&1; }
assert "B4: reify_git_env_scrub is defined in the sourcing shell" _has_fn reify_git_env_scrub
assert "B5: reify_git_env_scrub_prefix is defined in the sourcing shell" _has_fn reify_git_env_scrub_prefix

# ---------------------------------------------------------------------------
# Arm D — DRIFT GUARD: REIFY_GIT_ENV_SCRUB_VARS is a SUPERSET of the
# workspace's canonical Rust answer, crates/reify-test-support/src/git_env.rs's
# REPO_REDIRECT_VARS.
#
# DERIVED from that source on every infra run, never a second hardcoded copy —
# the house PT-DRIFT / PG-DRIFT pattern (tests/infra/test_verify_scope.sh
# re-derives is_swept_ext and the prd-gate coupled set the same way).
#
# ONE-WAY BY DESIGN, exactly like PT-DRIFT: this fires when the RUST set gains a
# variable the bash list lacks.  The reverse — bash carrying an extra var the
# Rust set later drops — is left free, so the only direction of error this
# permits is OVER-scrubbing, never a silent coverage hole.
# ---------------------------------------------------------------------------
echo ""
echo "--- D: bash scrub list is a superset of Rust's REPO_REDIRECT_VARS (derive-from-source) ---"

GIT_ENV_RS="$REPO_ROOT/crates/reify-test-support/src/git_env.rs"

assert "D0: crates/reify-test-support/src/git_env.rs exists (the derivation's source)" \
    test -f "$GIT_ENV_RS"

# Comment lines are stripped BEFORE the string capture so a future comment
# inside the literal that happens to quote a GIT_* name cannot inflate the
# derived set.
_D_RUST_VARS=""
if [ -f "$GIT_ENV_RS" ]; then
    _D_RUST_VARS="$(sed -n '/^pub const REPO_REDIRECT_VARS/,/^\];/p' "$GIT_ENV_RS" \
        | grep -v '^[[:space:]]*//' \
        | grep -o '"GIT_[A-Z_]*"' | tr -d '"' | sort -u || true)"
fi

# Non-vacuity FIRST: a renamed constant, a reshaped literal or a broken sed
# range must fail LOUDLY here rather than silently comparing against the empty
# set — which every containment assertion below would satisfy trivially.
assert "D1: derived REPO_REDIRECT_VARS set is NON-EMPTY (guard is not vacuous)" \
    test -n "$_D_RUST_VARS"
assert "D2: derived set contains GIT_DIR (the parse really read the constant, not some other literal)" \
    bash -c 'printf "%s\n" "$1" | grep -qx "GIT_DIR"' _ "$_D_RUST_VARS"

_in_scrub_list() {
    local _needle="$1" _v
    for _v in ${REIFY_GIT_ENV_SCRUB_VARS:-}; do
        [ "$_v" = "$_needle" ] && return 0
    done
    return 1
}

while IFS= read -r _d_var; do
    [ -n "$_d_var" ] || continue
    assert "D3: $_d_var (REPO_REDIRECT_VARS) is present in REIFY_GIT_ENV_SCRUB_VARS" \
        _in_scrub_list "$_d_var"
done <<< "$_D_RUST_VARS"

# ---------------------------------------------------------------------------
# Arm C — BEHAVIOURAL: a member test run through the scrub under a hook-shaped
# hostile env is green, and the poisoned index is untouched.
#
# The expected result is DERIVED from a clean-env baseline captured live in this
# same run, not pinned to a literal subtest count that would rot the moment
# test_reify_audit_ptodo.sh gains an arm.  C0 is the anti-vacuity floor on that
# baseline: a RATCHET_SKIP / missing-binary degradation reports a different,
# smaller count (see that file's rc-75 / rc-125 partition), and a skipped
# baseline compared against a skipped scrubbed run would agree vacuously.
# ---------------------------------------------------------------------------
echo ""
echo "--- C: a member test survives a hook-shaped hostile env through the scrub ---"

# Floor, not equality: adding subtests to test_reify_audit_ptodo.sh must not red
# this file, but a degraded/skipped run (which reports far fewer) must.
_C_PASS_FLOOR=22

_C_BASE_OUT=""
_C_BASE_RC=0
_C_BASE_OUT="$(bash "$PTODO_TEST" 2>&1)" || _C_BASE_RC=$?
_C_BASE_LINE="$(printf '%s\n' "$_C_BASE_OUT" \
    | grep -E '^Results: [0-9]+ passed, [0-9]+ failed$' | tail -1 || true)"
_C_BASE_PASSED="$(printf '%s\n' "$_C_BASE_LINE" | sed -n 's/^Results: \([0-9]*\) passed.*/\1/p')"

assert "C0a: clean-env baseline of test_reify_audit_ptodo.sh exits 0" \
    test "$_C_BASE_RC" -eq 0
assert "C0b: clean-env baseline emits a Results line" test -n "$_C_BASE_LINE"
assert "C0c: clean-env baseline reports 0 failed (got: ${_C_BASE_LINE:-<none>})" \
    bash -c 'printf "%s\n" "$1" | grep -q " 0 failed$"' _ "$_C_BASE_LINE"
assert "C0d: clean-env baseline passed-count >= $_C_PASS_FLOOR (not a RATCHET_SKIP/degraded run; got: ${_C_BASE_PASSED:-0})" \
    bash -c '[ -n "$1" ] && [ "$1" -ge "$2" ]' _ "${_C_BASE_PASSED:-}" "$_C_PASS_FLOOR"

S_POISON=""
_mk_repo S_POISON
printf 'poison-seed\n' > "$S_POISON/poison_seed.txt"
_clean_git -C "$S_POISON" add poison_seed.txt

_C_IDX="$S_POISON/.git/index"
_C_IDX_SHA_BEFORE=""
_C_IDX_SIZE_BEFORE=""
if [ -f "$_C_IDX" ]; then
    _C_IDX_SHA_BEFORE="$(_sha_of "$_C_IDX")"
    _C_IDX_SIZE_BEFORE="$(wc -c < "$_C_IDX" | tr -d ' ')"
fi
assert "C1: poisoned-index snapshot precondition — S's index exists and is non-empty" \
    bash -c '[ -n "$1" ] && [ "${2:-0}" -gt 0 ]' _ "$_C_IDX_SHA_BEFORE" "$_C_IDX_SIZE_BEFORE"

# The hook-shaped env: git 2.43 exports GIT_INDEX_FILE and ONLY GIT_INDEX_FILE
# into pre-commit / pre-merge-commit (measured).  Exported in a SUBSHELL, never
# as a `VAR=v func` prefix — bash keeps such an assignment set after a SHELL
# FUNCTION returns, which would silently poison every later arm in this file.
_C_SCRUBBED_OUT=""
_C_SCRUBBED_RC=0
if _has_fn reify_git_env_scrub; then
    _C_SCRUBBED_OUT="$(
        export GIT_INDEX_FILE="$_C_IDX"
        reify_git_env_scrub bash "$PTODO_TEST" 2>&1
    )" || _C_SCRUBBED_RC=$?
else
    _C_SCRUBBED_RC=127
fi
_C_SCRUBBED_LINE="$(printf '%s\n' "$_C_SCRUBBED_OUT" \
    | grep -E '^Results: [0-9]+ passed, [0-9]+ failed$' | tail -1 || true)"

assert "C2: poisoned + scrubbed run exits 0 (got rc=$_C_SCRUBBED_RC)" \
    test "$_C_SCRUBBED_RC" -eq 0
assert "C3: poisoned + scrubbed Results line is IDENTICAL to the clean-env baseline (baseline: '${_C_BASE_LINE:-<none>}', scrubbed: '${_C_SCRUBBED_LINE:-<none>}')" \
    bash -c '[ -n "$1" ] && [ "$1" = "$2" ]' _ "$_C_SCRUBBED_LINE" "$_C_BASE_LINE"

_C_IDX_SHA_AFTER=""
_C_IDX_SIZE_AFTER=""
if [ -f "$_C_IDX" ]; then
    _C_IDX_SHA_AFTER="$(_sha_of "$_C_IDX")"
    _C_IDX_SIZE_AFTER="$(wc -c < "$_C_IDX" | tr -d ' ')"
fi

assert "C4: DAMAGE DETECTOR — the poisoned index is byte-identical after the run (sha $_C_IDX_SHA_BEFORE -> ${_C_IDX_SHA_AFTER:-<gone>})" \
    bash -c '[ -n "$1" ] && [ "$1" = "$2" ]' _ "$_C_IDX_SHA_AFTER" "$_C_IDX_SHA_BEFORE"
assert "C5: ... and unchanged in size ($_C_IDX_SIZE_BEFORE -> ${_C_IDX_SIZE_AFTER:-<gone>} bytes)" \
    bash -c '[ -n "$1" ] && [ "$1" = "$2" ]' _ "$_C_IDX_SIZE_AFTER" "$_C_IDX_SIZE_BEFORE"

test_summary
