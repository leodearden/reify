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
# Arm F — RUNNER CONTRACT (run_all.sh): EVERY member spawn carries the scrub.
#
# A COUNTING assertion, deliberately not a list of today's six line numbers
# (1384/1449/1856/1893/1924/2005), which rot on the next edit above them: a NEW
# unscrubbed spawn site added later must fail this guard.
#
# Line continuations are folded first, so a spawn whose `reify_git_env_scrub`
# sits on the preceding physical line (the `env -u REIFY_RUN_ALL_MEMBER_SUBSET`
# site does exactly that) is counted as the one logical statement it is.
# ---------------------------------------------------------------------------
echo ""
echo "--- F: every run_all.sh member spawn carries the scrub ---"

RUN_ALL="$SCRIPT_DIR/run_all.sh"
_F_SPAWN_FLOOR=6

_F_LOGICAL="$(sed -e :a -e '/\\$/N; s/\\\n//; ta' "$RUN_ALL")"
_F_SPAWNS="$(printf '%s\n' "$_F_LOGICAL" \
    | grep -E 'bash "\$INFRA_DIR/|bash "\$test_file"' || true)"
_F_TOTAL="$(printf '%s\n' "$_F_SPAWNS" | grep -c . || true)"
_F_SCRUBBED="$(printf '%s\n' "$_F_SPAWNS" | grep -c 'reify_git_env_scrub' || true)"

# Floor, not equality: run_all.sh gaining a seventh spawn site must not red this
# arm, but losing the ability to SEE the spawn sites (a reshaped invocation the
# pattern no longer matches) must.
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
