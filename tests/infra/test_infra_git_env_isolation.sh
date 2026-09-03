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
#   C  BEHAVIOURAL — a hermetic single-purpose probe performs
#      `git -C <fixture> add -A` under the hook-shaped poison, run through
#      `reify_git_env_scrub`: the write must land in the FIXTURE's own index
#      and leave the poisoned index byte-identical (the damage detector: the
#      point is that no fixture write escapes).  Hermetic rather than a
#      borrowed real member on purpose — a member's pass count is a function
#      of ambient REIFY_AUDIT_* state (measured, both green:
#      test_reify_audit_ptodo.sh reports 22 clean but 18 under the
#      REIFY_AUDIT_NO_COLD_BUILD=1 that row 1 of
#      tests/infra/run-all-ambient-vars.manifest injects into every pool
#      member), so borrowing one would red this file on a healthy scrub.
#
# Classified `intra-run-serial` in tests/infra/run-all-classification.manifest
# because it re-enters the infra harness twice over: arm F3 spawns a nested
# `bash tests/infra/run_all.sh` on a fixture INFRA_DIR, and arm G re-invokes
# tests/infra/test_host_global_unit_pinning.sh.  Either alone is enough to keep
# this file out of the parallel bucket.

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
# Set in the SOURCING shell, not exported: the lib deliberately keeps it
# shell-local so it is not injected as a ninth ambient variable into every
# run_all.sh pool member (see the lib's note beside the assignment).  Asserted
# by value-passing, never by env inheritance, so this stays true either way.
assert "B3: REIFY_GIT_ENV_SCRUB_VARS is set in the sourcing shell and non-empty" \
    bash -c '[ -n "${1:-}" ]' _ "${REIFY_GIT_ENV_SCRUB_VARS:-}"
assert "B3a: ... and NOT exported (no ninth ambient var reaches the pool members)" \
    bash -c '[ -z "${REIFY_GIT_ENV_SCRUB_VARS:-}" ]' 
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
# Arm C — BEHAVIOURAL: the helper neutralizes a hook-shaped poison, and the
# poisoned index is untouched.
#
# A hermetic probe (generated below, same shape as F3) stands in for an
# UNSANITIZED member test: it does the one thing every hermetic infra fixture
# does — `git -C <fixture> add -A`, never `cd` — under a poisoned
# GIT_INDEX_FILE, wrapped in `reify_git_env_scrub`.  Arm A already proved that
# exact vector DOES bite with the scrub off, so A and C are a controlled
# contrast varying one thing: the helper.
#
# Deliberately NOT a borrowed real member.  A member's pass count is a function
# of ambient REIFY_AUDIT_* state — measured in this lane, BOTH green:
# tests/infra/test_reify_audit_ptodo.sh reports 22 passed clean, and 18 under
# `REIFY_AUDIT_NO_COLD_BUILD=1 REIFY_PTODO_GEN_BIN=/nonexistent/gen`.
# REIFY_AUDIT_NO_COLD_BUILD=1 is row 1 of
# tests/infra/run-all-ambient-vars.manifest, injected into every run_all.sh pool
# member, so any assertion over a borrowed member's count reds the merge gate in
# the ordinary warm-lane configuration while the scrub is perfectly healthy.
# ---------------------------------------------------------------------------
echo ""
echo "--- C: the scrub neutralizes a hook-shaped poison on a hermetic probe ---"

S_POISON=""
F_WORK=""
_mk_repo S_POISON
_mk_repo F_WORK

printf 'poison-seed\n' > "$S_POISON/poison_seed.txt"
_clean_git -C "$S_POISON" add poison_seed.txt

# F's seeds are deliberately left UNSTAGED: the probe's `git add -A` below IS
# the write under test, so F's own index must still be empty when it runs.
printf 'alpha\n' > "$F_WORK/probe_alpha.txt"
printf 'beta\n' > "$F_WORK/probe_beta.txt"

_C_IDX="$S_POISON/.git/index"
_C_IDX_SHA_BEFORE=""
_C_IDX_SIZE_BEFORE=""
if [ -f "$_C_IDX" ]; then
    _C_IDX_SHA_BEFORE="$(_sha_of "$_C_IDX")"
    _C_IDX_SIZE_BEFORE="$(wc -c < "$_C_IDX" | tr -d ' ')"
fi
assert "C1: poisoned-index snapshot precondition — S's index exists and is non-empty" \
    bash -c '[ -n "$1" ] && [ "${2:-0}" -gt 0 ]' _ "$_C_IDX_SHA_BEFORE" "$_C_IDX_SIZE_BEFORE"

# The probe — a single-purpose stand-in for an UNSANITIZED member test, doing
# the one thing every hermetic infra fixture does: `git -C <fixture> add -A`
# with no `cd`.  It calls BARE `git`, never `_clean_git`, on purpose — a probe
# that pre-scrubbed its own environment would leave the helper nothing to
# neutralize and the arm would certify nothing.  Same generator shape as F3.
C_FIX="$(mktemp -d)"
_TMPDIRS+=("$C_FIX")
C_REPORT="$C_FIX/probe-report.txt"
{
    printf '#!/usr/bin/env bash\n'
    printf 'git -C %q add -A\n' "$F_WORK"
    printf '{\n'
    printf '    printf "PROBE_RAN\\n"\n'
    printf '    git -C %q ls-files --cached\n' "$F_WORK"
    printf '} > %q 2>&1\n' "$C_REPORT"
    printf 'exit 0\n'
} > "$C_FIX/git_env_probe.sh"
chmod +x "$C_FIX/git_env_probe.sh"

# The hook-shaped env: git 2.43 exports GIT_INDEX_FILE and ONLY GIT_INDEX_FILE
# into pre-commit / pre-merge-commit (measured).  Exported in a SUBSHELL, never
# as a `VAR=v func` prefix — bash keeps such an assignment set after a SHELL
# FUNCTION returns, which would silently poison every later arm in this file.
if _has_fn reify_git_env_scrub; then
    (
        export GIT_INDEX_FILE="$_C_IDX"
        reify_git_env_scrub bash "$C_FIX/git_env_probe.sh"
    ) || true
fi

_C_REPORT_TEXT=""
[ -f "$C_REPORT" ] && _C_REPORT_TEXT="$(cat "$C_REPORT")"

assert "C2: the probe actually ran through the scrub (guard is not vacuous)" \
    bash -c 'printf "%s\n" "$1" | grep -qx "PROBE_RAN"' _ "$_C_REPORT_TEXT"
# Non-vacuity floor for C4/C5, and the replacement for the old pass-count
# assertion: it proves the git write under test GENUINELY HAPPENED, so a
# byte-identical S index below reads as "the write was redirected correctly"
# rather than "no write was ever attempted".  A probe that silently no-op'd
# would otherwise satisfy C4 and C5 vacuously.
assert "C3: ... and its \`git add -A\` really staged F's seeded files (got: ${_C_REPORT_TEXT//$'\n'/ | })" \
    bash -c 'printf "%s\n" "$1" | grep -qx "probe_alpha.txt" && printf "%s\n" "$1" | grep -qx "probe_beta.txt"' \
        _ "$_C_REPORT_TEXT"

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

# ---------------------------------------------------------------------------
# Arm C-PRESERVE (C6-C10) — the REVERSE direction: the scrub must not remove
# TOO MUCH.
#
# Every other arm here, and arm D's drift guard explicitly ("the only direction
# of error this permits is OVER-scrubbing"), proves only that the scrub removes
# ENOUGH. Nothing proved it removes no more than that — so widening
# REIFY_GIT_ENV_SCRUB_VARS into the config/trace/identity vars, or replacing the
# enumeration with a wholesale `GIT_*` clear, would leave arms A-G, D3 and F2
# all green while ~103 infra members silently lost their committer identity or
# config isolation. That failure would surface as unrelated flakes in whichever
# member relies on the inherited value, which is the worst possible place to
# discover it.
#
# The preserved set is DERIVED from the same Rust source arm D reads —
# specifically the "why an enumerated set" paragraph of REPO_REDIRECT_VARS's
# doc, which is the workspace's ONE statement of which GIT_* vars are
# deliberately left alone and why (GIT_CONFIG_* = harness isolation, GIT_TRACE*
# = debuggability, GIT_AUTHOR_*/GIT_COMMITTER_* = commit determinism). A
# `GIT_FOO_*` glob in that prose is expanded to the concrete `GIT_FOO_NAME`.
# Deriving rather than hardcoding is the point: a hand-written list here could
# drift into enforcing a rationale the Rust source no longer makes.
# ---------------------------------------------------------------------------
echo ""
echo "--- C-PRESERVE: the scrub leaves the non-redirect git vars alone ---"

_CP_VARS=""
if [ -f "$GIT_ENV_RS" ]; then
    _CP_VARS="$(sed -n '/Why an enumerated set/,/left untouched\./p' "$GIT_ENV_RS" \
        | grep -o '`GIT_[A-Z_]*\*\?`' | tr -d '`' \
        | sed -e 's/\*$//' \
        | grep -vx 'GIT_\?' \
        | sed -e 's/_$/_NAME/' \
        | sort -u || true)"
fi
# Pipeline order matters: the trailing `*` is stripped FIRST, then the bare
# `GIT_` left by the paragraph's `GIT_*` strawman (the wholesale clear it argues
# AGAINST — not a variable, and not exportable) is dropped, and only then is a
# genuine `GIT_FOO_` glob stem expanded to `GIT_FOO_NAME`. Expanding before the
# drop would silently mint a bogus `GIT_NAME` and assert against a variable no
# rationale ever named.

assert "C6: preserved-var set derived from git_env.rs is NON-EMPTY (guard is not vacuous)" \
    test -n "$_CP_VARS"
assert "C7: ... and contains GIT_CONFIG_GLOBAL (the parse read the right paragraph)" \
    bash -c 'printf "%s\n" "$1" | grep -qx "GIT_CONFIG_GLOBAL"' _ "$_CP_VARS"

# Static half: the two sets must not overlap. This is the direct, greppable
# statement of "no redirect var is claimed as preserved, and no preserved var
# has been pulled into the scrub list" — it reds the instant someone widens
# REIFY_GIT_ENV_SCRUB_VARS into the identity/config/trace class.
_CP_OVERLAP=""
while IFS= read -r _cp_var; do
    [ -n "$_cp_var" ] || continue
    if _in_scrub_list "$_cp_var"; then
        _CP_OVERLAP="${_CP_OVERLAP:+$_CP_OVERLAP }$_cp_var"
    fi
done <<< "$_CP_VARS"
assert "C8: no preserved var appears in REIFY_GIT_ENV_SCRUB_VARS (overlap: ${_CP_OVERLAP:-<none>})" \
    test -z "$_CP_OVERLAP"

# Behavioural half: the same hermetic-probe shape as arm C, run through the real
# helper with BOTH the hook poison and a sentinel value per preserved var in
# scope. The probe runs no git at all, so a bogus GIT_CONFIG_GLOBAL /
# GIT_TRACE sentinel is inert.
CP_FIX="$(mktemp -d)"
_TMPDIRS+=("$CP_FIX")
CP_REPORT="$CP_FIX/preserve-report.txt"
_CP_VARS_ONELINE="$(printf '%s\n' "$_CP_VARS" | tr '\n' ' ')"
{
    printf '#!/usr/bin/env bash\n'
    printf 'exec > %q 2>&1\n' "$CP_REPORT"
    printf 'for _v in %s; do\n' "$_CP_VARS_ONELINE"
    printf '    printf "PRESERVED %%s=%%s\\n" "$_v" "${!_v-<UNSET>}"\n'
    printf 'done\n'
    printf 'for _v in %s; do\n' "$REIFY_GIT_ENV_SCRUB_VARS"
    printf '    [ -n "${!_v:-}" ] && printf "LEAK %%s\\n" "$_v"\n'
    printf 'done\n'
    printf 'printf "PROBE_RAN\\n"\n'
    printf 'exit 0\n'
} > "$CP_FIX/preserve_probe.sh"
chmod +x "$CP_FIX/preserve_probe.sh"

if _has_fn reify_git_env_scrub; then
    (
        export GIT_INDEX_FILE="$_C_IDX"
        for _cp_var in $_CP_VARS_ONELINE; do
            export "$_cp_var=cp-sentinel"
        done
        reify_git_env_scrub bash "$CP_FIX/preserve_probe.sh"
    ) || true
fi

_CP_REPORT_TEXT=""
[ -f "$CP_REPORT" ] && _CP_REPORT_TEXT="$(cat "$CP_REPORT")"

assert "C9: the preserve probe ran through the scrub (guard is not vacuous)" \
    bash -c 'printf "%s\n" "$1" | grep -qx "PROBE_RAN"' _ "$_CP_REPORT_TEXT"
# Asserted per-var so a failure names the variable that was over-scrubbed.
while IFS= read -r _cp_var; do
    [ -n "$_cp_var" ] || continue
    assert "C10: $_cp_var survived the scrub with its value intact" \
        bash -c 'printf "%s\n" "$1" | grep -qx "PRESERVED $2=cp-sentinel"' \
            _ "$_CP_REPORT_TEXT" "$_cp_var"
done <<< "$_CP_VARS"
# The contrast that keeps C10 honest: the SAME invocation must still have
# stripped the redirect vars. Without this, a helper that had degenerated into a
# no-op would satisfy every C10 assertion above.
assert "C11: ... while the redirect vars were still stripped in that same run (got: ${_CP_REPORT_TEXT//$'\n'/ | })" \
    bash -c '! printf "%s\n" "$1" | grep -q "^LEAK "' _ "$_CP_REPORT_TEXT"

# ---------------------------------------------------------------------------
# Arm E — RUNNER CONTRACT (verify.sh): the selective-infra plan leaf carries the
# scrub.
#
# Derived BEHAVIOURALLY from a real `--print-plan` (hermetic by design: it runs
# nothing), not from a grep of verify.sh's source.  A staged
# tests/prd-gate/fixtures/*.ri unconditionally pulls
# tests/infra/test_reify_audit_ptodo.sh into the plan via
# select_cheap_ptodo_gate — which is precisely the overlay-sanctioned PRD-fixture
# landing path that produced the reported failure.
#
# The expected prefix is read from reify_git_env_scrub_prefix, never hand-written
# here: a literal copy would silently stop matching the moment
# REIFY_GIT_ENV_SCRUB_VARS widens, turning this pin into a lie.
# ---------------------------------------------------------------------------
echo ""
echo "--- E: verify.sh's selective-infra leaf carries the scrub prefix ---"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
# shellcheck source=tests/infra/plan_capture_lib.sh
source "$SCRIPT_DIR/plan_capture_lib.sh"

# _mk_plan_fixture VARNAME — a throwaway repo that a copied verify.sh can run
# --print-plan inside.
#
# `cp -R scripts` rather than the hand-maintained per-lib copy list the older
# --print-plan sandboxes use (test_verify_scope.sh's make_fixture and siblings):
# copying the WHOLE directory is immune to source-closure drift by construction,
# so this fixture needs no assert_source_closure_copied preflight and cannot
# become the copy-list-drift class (tasks 4525/4626/4625) that preflight exists
# to catch.  scripts/ is 2.5 MB / ~106 files, so the copy is cheap.
_mk_plan_fixture() {
    local _var="$1" d
    d="$(mktemp -d)"
    _TMPDIRS+=("$d")
    cp -R "$REPO_ROOT/scripts" "$d/scripts"
    mkdir -p "$d/.config"
    cp "$REPO_ROOT/.config/nextest.toml" "$d/.config/nextest.toml"
    _clean_git -C "$d" init -q
    _clean_git -C "$d" config user.email "test@test.com"
    _clean_git -C "$d" config user.name "Test"
    printf -v "$_var" '%s' "$d"
}

E_FIX=""
_mk_plan_fixture E_FIX
mkdir -p "$E_FIX/tests/prd-gate/fixtures"
printf 'x\n' > "$E_FIX/tests/prd-gate/fixtures/git_env_scrub_probe.ri"
_clean_git -C "$E_FIX" add tests/prd-gate/fixtures/git_env_scrub_probe.ri

_E_PLAN=""
capture_print_plan _E_PLAN "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan 2>/dev/null' \
    _ "$E_FIX" || true

_E_LEAF="$(printf '%s\n' "$_E_PLAN" | grep 'test_reify_audit_ptodo\.sh' | head -1 || true)"
_E_PREFIX=""
if _has_fn reify_git_env_scrub_prefix; then
    _E_PREFIX="$(reify_git_env_scrub_prefix)"
fi

# Non-vacuity FIRST: without a leaf to inspect, E2/E3 would pass or fail for
# reasons that have nothing to do with the scrub.
assert "E1: staged prd-gate .ri yields a tests/infra/test_reify_audit_ptodo.sh plan leaf (guard is not vacuous)" \
    test -n "$_E_LEAF"
assert "E2: that leaf carries the scrub prefix from reify_git_env_scrub_prefix ('${_E_PREFIX:-<none>}')" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] && printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$_E_LEAF" "$_E_PREFIX"
assert "E3: ... positioned between the timeout and the bash it wraps" \
    bash -c 'printf "%s\n" "$1" | grep -qE "timeout.*env -u GIT_DIR.*bash"' _ "$_E_LEAF"

# ---------------------------------------------------------------------------
# Arm F — RUNNER CONTRACT (run_all.sh): EVERY member spawn, and every one of
# run_all.sh's OWN repo-targeting git calls, carries the scrub.
#
# COUNTING assertions, deliberately not a list of today's line numbers, which
# rot on the next edit above them: a NEW unscrubbed site added later must fail
# these guards.
#
# TWO normalizations are applied to run_all.sh's source before counting, in
# this order, and each is load-bearing:
#
#   1. FOLD LINE CONTINUATIONS — so a spawn whose `reify_git_env_scrub` sits on
#      the preceding physical line (the `env -u REIFY_RUN_ALL_MEMBER_SUBSET`
#      site does exactly that) is counted as the one logical statement it is.
#   2. DROP COMMENT LINES — a bare `grep` for the invocation shape cannot tell
#      an executable statement from prose, and run_all.sh is heavily commented,
#      including a block that discusses the member spawns directly. The first
#      comment line to quote an invocation verbatim would otherwise score as an
#      unscrubbed site and red F2/F5 on a change that touched nothing
#      executable. Anchoring instead on a leading `reify_git_env_scrub`/`bash`
#      was rejected: it would also stop matching a legitimately-reshaped spawn
#      (e.g. one inside a command substitution), turning a real coverage hole
#      into a silent pass — F1/F4's floors only catch a DROP in visible sites,
#      not a site the pattern was narrowed past.
#
# Both floors are floors, not equalities: run_all.sh gaining a seventh spawn
# site must not red these arms, but losing the ability to SEE the sites (a
# reshaped invocation the pattern no longer matches) must.
# ---------------------------------------------------------------------------
echo ""
echo "--- F: every run_all.sh member spawn and own git call carries the scrub ---"

RUN_ALL="$SCRIPT_DIR/run_all.sh"
_F_SPAWN_FLOOR=6
_F_GIT_FLOOR=7

_F_LOGICAL="$(sed -e :a -e '/\\$/N; s/\\\n//; ta' "$RUN_ALL" \
    | grep -v '^[[:blank:]]*#' || true)"
_F_SPAWNS="$(printf '%s\n' "$_F_LOGICAL" \
    | grep -E 'bash "\$INFRA_DIR/|bash "\$test_file"' || true)"
_F_TOTAL="$(printf '%s\n' "$_F_SPAWNS" | grep -c . || true)"
_F_SCRUBBED="$(printf '%s\n' "$_F_SPAWNS" | grep -c 'reify_git_env_scrub' || true)"

assert "F1: run_all.sh member-spawn sites found >= $_F_SPAWN_FLOOR (pattern still sees them; got $_F_TOTAL)" \
    bash -c '[ "${1:-0}" -ge "$2" ]' _ "$_F_TOTAL" "$_F_SPAWN_FLOOR"
assert "F2: EVERY member spawn is wrapped in reify_git_env_scrub ($_F_SCRUBBED of $_F_TOTAL)" \
    bash -c '[ "${1:-0}" -eq "${2:-0}" ]' _ "$_F_SCRUBBED" "$_F_TOTAL"
# F3 is the END-TO-END proof the two counting arms above cannot give: run the
# real run_all.sh over a one-member fixture INFRA_DIR under a hook-shaped
# hostile env, and have the member report which scrubbed variables actually
# reached it.  This subsumes a `grep` for the source line — an unsourced lib
# makes run_all.sh exit before the member runs, which F3a catches.
#
# The probe's variable list is baked from REIFY_GIT_ENV_SCRUB_VARS at generation
# time, so it widens with the list rather than pinning today's eight.
F3_FIX="$(mktemp -d)"
_TMPDIRS+=("$F3_FIX")
F3_REPORT="$F3_FIX/probe-report.txt"
{
    printf '#!/usr/bin/env bash\n'
    printf 'exec > %q 2>&1\n' "$F3_REPORT"
    printf 'for _v in %s; do\n' "$REIFY_GIT_ENV_SCRUB_VARS"
    printf '    [ -n "${!_v:-}" ] && printf "LEAK %%s=%%s\\n" "$_v" "${!_v}"\n'
    printf 'done\n'
    printf 'printf "PROBE_RAN\\n"\n'
    printf 'exit 0\n'
} > "$F3_FIX/test_git_env_probe.sh"
chmod +x "$F3_FIX/test_git_env_probe.sh"

S_RUNALL=""
_mk_repo S_RUNALL
printf 'seed\n' > "$S_RUNALL/seed.txt"
_clean_git -C "$S_RUNALL" add seed.txt

# Hook-shaped: GIT_INDEX_FILE and only GIT_INDEX_FILE, pointed at a throwaway
# repo (task #7106 pre-1 — never a real checkout).
(
    export GIT_INDEX_FILE="$S_RUNALL/.git/index"
    bash "$RUN_ALL" "$F3_FIX" >/dev/null 2>&1
) || true

_F3_REPORT_TEXT=""
[ -f "$F3_REPORT" ] && _F3_REPORT_TEXT="$(cat "$F3_REPORT")"

assert "F3a: the fixture member actually ran under run_all.sh (guard is not vacuous)" \
    bash -c 'printf "%s\n" "$1" | grep -qx "PROBE_RAN"' _ "$_F3_REPORT_TEXT"
assert "F3b: NO scrubbed variable reached the member (got: ${_F3_REPORT_TEXT//$'\n'/ | })" \
    bash -c '! printf "%s\n" "$1" | grep -q "^LEAK "' _ "$_F3_REPORT_TEXT"

# F4/F5 — run_all.sh's OWN repo-targeting git calls, not just the members it
# spawns.  These run IN the run_all.sh process, so no member-spawn scrub reaches
# them, and the content-skip engine that makes five of them is gated on
# `_RA_INBOUND_ROLE = merge` — i.e. it executes ONLY under the hook environment
# that actually exports GIT_INDEX_FILE.  `git status` there both READS and
# (stat-cache refresh) can WRITE the index that variable names, so an unscrubbed
# one would compute its RUN/SKIP decisions from, and touch, a foreign index.
# Benign today only because CWD happens to be REPO_ROOT and the relative
# `.git/index` therefore names the same repo — not a property to rest on (under
# `git commit --only` GIT_INDEX_FILE names a TEMPORARY index; see
# crates/reify-test-support/src/git_env.rs's own analysis).
_F_GIT_CALLS="$(printf '%s\n' "$_F_LOGICAL" | grep -F 'git -C "' || true)"
_F_GIT_TOTAL="$(printf '%s\n' "$_F_GIT_CALLS" | grep -c . || true)"
_F_GIT_SCRUBBED="$(printf '%s\n' "$_F_GIT_CALLS" \
    | grep -cF 'reify_git_env_scrub git -C "' || true)"

assert "F4: run_all.sh's own \`git -C\` sites found >= $_F_GIT_FLOOR (pattern still sees them; got $_F_GIT_TOTAL)" \
    bash -c '[ "${1:-0}" -ge "$2" ]' _ "$_F_GIT_TOTAL" "$_F_GIT_FLOOR"
assert "F5: EVERY one of them is wrapped in reify_git_env_scrub ($_F_GIT_SCRUBBED of $_F_GIT_TOTAL)" \
    bash -c '[ "${1:-0}" -eq "${2:-0}" ]' _ "$_F_GIT_SCRUBBED" "$_F_GIT_TOTAL"

# ---------------------------------------------------------------------------
# Arm G — HOSTILE-ENV INVARIANT REGRESSION GUARD.
#
# tests/infra/test_host_global_unit_pinning.sh documents (at :79-83 and
# :144-148) a DELIBERATE refusal to scrub GIT_* for the code under test: its A6
# arm poisons GIT_DIR/GIT_WORK_TREE on purpose to prove
# scripts/lib_main_checkout.sh's resolver neutralizes them itself, and states
# that "a harness that pre-cleaned the environment would be certifying a posture
# production never actually has".
#
# A6 sets its poison EXPLICITLY per-command (`GIT_DIR=... GIT_WORK_TREE=... git
# -C ...` at :311-313) rather than inheriting it, so the runner scrub should not
# reach it — but that is an INFERENCE, so pin it.  If this arm ever goes red, do
# NOT weaken it: it means the runner scrub genuinely conflicts with a documented
# invariant and needs escalation.
# ---------------------------------------------------------------------------
echo ""
echo "--- G: the deliberately-hostile-env member still passes through the scrub ---"

HOST_GLOBAL_TEST="$SCRIPT_DIR/test_host_global_unit_pinning.sh"
_G_OUT=""
_G_RC=0
if _has_fn reify_git_env_scrub && [ -f "$HOST_GLOBAL_TEST" ]; then
    _G_OUT="$(reify_git_env_scrub bash "$HOST_GLOBAL_TEST" 2>&1)" || _G_RC=$?
else
    _G_RC=127
fi
_G_LINE="$(printf '%s\n' "$_G_OUT" \
    | grep -E '^Results: [0-9]+ passed, [0-9]+ failed$' | tail -1 || true)"

assert "G1: test_host_global_unit_pinning.sh exits 0 through reify_git_env_scrub (got rc=$_G_RC)" \
    test "$_G_RC" -eq 0
assert "G2: ... reporting 0 failed — its A6 self-poisoning invariant survives the runner scrub (got: ${_G_LINE:-<none>})" \
    bash -c '[ -n "$1" ] && printf "%s\n" "$1" | grep -q " 0 failed$"' _ "$_G_LINE"

test_summary
