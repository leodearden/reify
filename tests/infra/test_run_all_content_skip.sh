#!/usr/bin/env bash
# tests/infra/test_run_all_content_skip.sh — gate test for the merge-tier
# content-addressed per-member SKIP engine in tests/infra/run_all.sh
# (task 5273, merge-gate-riders PRD §4 rider γ).
#
# The skip engine drops a drift-guard pool member from the merge-tier run
# when its declared tracked-file closure (run-all-skip-closures.manifest) is
# byte-identical (git tree compare) to its last-executed-green main sha, so
# an unchanged member is not re-run every merge. Ships PRODUCTION-INERT:
# active ONLY when REIFY_RUN_ALL_CONTENT_SKIP=1 AND the inbound role is
# `merge` AND REIFY_RUN_ALL_SKIP_STATE names a state file. Every fixture here
# constructs its own hermetic git repo + closures manifest + state ledger and
# drives run_all.sh with those three keys set; no fixture ever touches real
# repo state.
#
# All facets (SKIP/RUN decision lines, backstop, fail-open storm-escape, inert
# gating, state-ledger update, under-declaration drift-guard) live in this one
# file — one `pool` row in run-all-classification.manifest, shared fixture
# builders, mirroring how test_run_all.sh houses many sub-cases.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_ALL="$SCRIPT_DIR/run_all.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== run_all.sh content-addressed skip engine tests (task 5273) ==="

# ---------------------------------------------------------------------------
# Shared fixture helpers.
# ---------------------------------------------------------------------------

# out_has HAYSTACK NEEDLE / out_lacks HAYSTACK NEEDLE — fork-free bash-native
# substring predicates (mirrors test_run_all.sh's `[[ == *substr* ]]` idiom;
# avoids the grep-under-load flakiness documented at test_run_all.sh:90-98).
out_has()   { [[ "$1" == *"$2"* ]]; }
out_lacks() { [[ "$1" != *"$2"* ]]; }

# git_init_fixture DIR — hermetic throwaway git repo (isolated identity, no
# GPG/hooks). The fixture dir is its own toplevel, so the skip engine's
# `git -C "$INFRA_DIR" rev-parse --show-toplevel` resolves to DIR and its
# repo-relative pathspecs match the files placed at DIR root.
git_init_fixture() {
    local dir="$1"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" config commit.gpgsign false
    git -C "$dir" config core.hooksPath /dev/null
}

# mk_member DIR NAME [RC] — write an executable mock member that exits RC
# (default 0).
mk_member() {
    local dir="$1" name="$2" rc="${3:-0}"
    printf '#!/usr/bin/env bash\nexit %s\n' "$rc" > "$dir/$name"
    chmod +x "$dir/$name"
}

# run_skip STATE CLOSURES DIR [ROLE] — invoke run_all.sh on fixture DIR with
# the content-skip engine keyed active (ROLE default `merge`). Captures
# combined stdout+stderr into the global RUN_OUT and the exit code into
# RUN_RC. Extra `KEY=VALUE` env-prefix tokens may be passed via the global
# RUN_SKIP_ENV array (e.g. backstop-threshold overrides).
run_skip() {
    local state="$1" closures="$2" dir="$3" role="${4:-merge}"
    RUN_RC=0
    RUN_OUT="$(
        DF_VERIFY_ROLE="$role" \
        REIFY_RUN_ALL_CONTENT_SKIP=1 \
        REIFY_RUN_ALL_SKIP_STATE="$state" \
        RUN_ALL_SKIP_CLOSURES_MANIFEST="$closures" \
        "${RUN_SKIP_ENV[@]+${RUN_SKIP_ENV[@]}}" \
        bash "$RUN_ALL" "$dir" 2>&1
    )" || RUN_RC=$?
}
RUN_OUT=""
RUN_RC=0
RUN_SKIP_ENV=()

# ===========================================================================
# Section 1 (step-1): SKIP (content-clean) happy path.
#   A mapped member whose closure is byte-identical between its green sha and
#   HEAD (green_sha == HEAD, clean worktree) must be SKIPPED: a
#   `SKIP (content-clean): <m> green=<sha>` line and NO `--- Running: <m> ---`.
#   RED until the skip engine (step-2) exists.
# ===========================================================================
echo ""
echo "--- Section 1 (step-1): SKIP (content-clean) happy path ---"

S1_DIR="$(mktemp -d)"; _TMPDIRS+=("$S1_DIR")
git_init_fixture "$S1_DIR"
mk_member "$S1_DIR" test_alpha.sh 0
printf 'alpha closure payload\n' > "$S1_DIR/alpha_dep.txt"
git -C "$S1_DIR" add -A
git -C "$S1_DIR" commit -q -m "base"
S1_GREEN="$(git -C "$S1_DIR" rev-parse HEAD)"

# Fixture closures manifest + state ledger live OUTSIDE any declared closure
# (underscore-prefixed, never test_*.sh, created post-commit) so they can
# never be mistaken for a member or a closure-path delta.
S1_CLOSURES="$S1_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S1_CLOSURES"
S1_STATE="$S1_DIR/_meta_state.ledger"
S1_NOW="$(date +%s)"
{
    printf '__MERGES__ 3\n'
    printf 'test_alpha.sh %s %s 3\n' "$S1_GREEN" "$S1_NOW"
} > "$S1_STATE"

run_skip "$S1_STATE" "$S1_CLOSURES" "$S1_DIR"

assert "S1: emits SKIP (content-clean) for the byte-identical mapped member" \
    out_has "$RUN_OUT" "SKIP (content-clean): test_alpha.sh green=$S1_GREEN"

assert "S1: skipped member is NOT executed (no '--- Running: test_alpha.sh ---')" \
    out_lacks "$RUN_OUT" "--- Running: test_alpha.sh ---"

assert "S1: suite still exits 0 when the only member is skipped" \
    test "$RUN_RC" -eq 0

# ===========================================================================
# Section 2 (step-3): RUN (delta) — content changed, member must run.
#   A mapped member whose closure changed between green and the run must RUN,
#   with a `RUN (delta): <m> touched=<path>` line and a `--- Running: <m> ---`.
#   RED until step-4: step-2 keeps a non-clean member in the list (so it is
#   already EXECUTED) but emits no RUN decision line yet.
# ===========================================================================

# -- 2a: committed delta (green sha < HEAD; a declared closure file changed) --
echo ""
echo "--- Section 2a (step-3): RUN (delta), committed change to a closure file ---"

S2A_DIR="$(mktemp -d)"; _TMPDIRS+=("$S2A_DIR")
git_init_fixture "$S2A_DIR"
mk_member "$S2A_DIR" test_alpha.sh 0
printf 'v1\n' > "$S2A_DIR/alpha_dep.txt"
git -C "$S2A_DIR" add -A
git -C "$S2A_DIR" commit -q -m "base"
S2A_GREEN="$(git -C "$S2A_DIR" rev-parse HEAD)"
printf 'v2 (changed)\n' > "$S2A_DIR/alpha_dep.txt"
git -C "$S2A_DIR" add -A
git -C "$S2A_DIR" commit -q -m "touch closure"
S2A_CLOSURES="$S2A_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S2A_CLOSURES"
S2A_STATE="$S2A_DIR/_meta_state.ledger"
{ printf '__MERGES__ 2\n'; printf 'test_alpha.sh %s %s 2\n' "$S2A_GREEN" "$(date +%s)"; } > "$S2A_STATE"

run_skip "$S2A_STATE" "$S2A_CLOSURES" "$S2A_DIR"

assert "S2a: emits RUN (delta) for the committed closure change" \
    out_has "$RUN_OUT" "RUN (delta): test_alpha.sh"
assert "S2a: RUN (delta) names the touched closure path" \
    out_has "$RUN_OUT" "touched=alpha_dep.txt"
assert "S2a: the delta member IS executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# -- 2b: worktree-dirty delta (green..HEAD clean, uncommitted closure edit) ---
echo ""
echo "--- Section 2b (step-3): RUN (delta), uncommitted (worktree) change to a closure file ---"

S2B_DIR="$(mktemp -d)"; _TMPDIRS+=("$S2B_DIR")
git_init_fixture "$S2B_DIR"
mk_member "$S2B_DIR" test_alpha.sh 0
printf 'v1\n' > "$S2B_DIR/alpha_dep.txt"
git -C "$S2B_DIR" add -A
git -C "$S2B_DIR" commit -q -m "base"
S2B_GREEN="$(git -C "$S2B_DIR" rev-parse HEAD)"   # green == HEAD (committed-clean)
printf 'dirty (uncommitted)\n' >> "$S2B_DIR/alpha_dep.txt"   # leave uncommitted
S2B_CLOSURES="$S2B_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S2B_CLOSURES"
S2B_STATE="$S2B_DIR/_meta_state.ledger"
{ printf '__MERGES__ 2\n'; printf 'test_alpha.sh %s %s 2\n' "$S2B_GREEN" "$(date +%s)"; } > "$S2B_STATE"

run_skip "$S2B_STATE" "$S2B_CLOSURES" "$S2B_DIR"

assert "S2b: emits RUN (delta) for the uncommitted (worktree) closure change" \
    out_has "$RUN_OUT" "RUN (delta): test_alpha.sh"
assert "S2b: the worktree-dirty member IS executed (not skipped)" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# -- 2c: own-file change (implicit closure member) always runs (contract K3) --
echo ""
echo "--- Section 2c (step-3): RUN (delta), the member's own file changed ---"

S2C_DIR="$(mktemp -d)"; _TMPDIRS+=("$S2C_DIR")
git_init_fixture "$S2C_DIR"
printf '#!/usr/bin/env bash\nexit 0\n' > "$S2C_DIR/test_alpha.sh"
chmod +x "$S2C_DIR/test_alpha.sh"
printf 'stable\n' > "$S2C_DIR/alpha_dep.txt"
git -C "$S2C_DIR" add -A
git -C "$S2C_DIR" commit -q -m "base"
S2C_GREEN="$(git -C "$S2C_DIR" rev-parse HEAD)"
# change ONLY the member's own file (declared closure alpha_dep.txt untouched);
# the own file is an IMPLICIT closure member, so this must still force a run.
printf '#!/usr/bin/env bash\n# revised body\nexit 0\n' > "$S2C_DIR/test_alpha.sh"
git -C "$S2C_DIR" add -A
git -C "$S2C_DIR" commit -q -m "touch own file"
S2C_CLOSURES="$S2C_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S2C_CLOSURES"
S2C_STATE="$S2C_DIR/_meta_state.ledger"
{ printf '__MERGES__ 2\n'; printf 'test_alpha.sh %s %s 2\n' "$S2C_GREEN" "$(date +%s)"; } > "$S2C_STATE"

run_skip "$S2C_STATE" "$S2C_CLOSURES" "$S2C_DIR"

assert "S2c: own-file change emits RUN (delta) (own file is an implicit closure member)" \
    out_has "$RUN_OUT" "RUN (delta): test_alpha.sh"
assert "S2c: RUN (delta) names the touched own file" \
    out_has "$RUN_OUT" "touched=test_alpha.sh"
assert "S2c: the own-file-changed member IS executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# ===========================================================================
# Section 3 (step-5): RUN (unmapped) and RUN (no-baseline).
#   Every discovered member gets a per-member decision line when the engine is
#   active (no silent caps): a member with no closure row ⇒ RUN (unmapped); a
#   mapped member with no state baseline ⇒ a RUN line (cannot prove
#   content-clean without a green). Both still execute. RED until step-6.
# ===========================================================================

# -- 3a: unmapped member (no closure row) always runs, even on a clean tree ---
echo ""
echo "--- Section 3a (step-5): RUN (unmapped), member has no closure row ---"

S3A_DIR="$(mktemp -d)"; _TMPDIRS+=("$S3A_DIR")
git_init_fixture "$S3A_DIR"
mk_member "$S3A_DIR" test_gamma.sh 0
git -C "$S3A_DIR" add -A
git -C "$S3A_DIR" commit -q -m "base"
# Closures manifest exists but declares only an unrelated member, so
# test_gamma.sh is UNMAPPED (fail-open ⇒ must run, never skip).
S3A_CLOSURES="$S3A_DIR/_meta_closures.manifest"
printf '# fixture closures\ntest_delta.sh some_dep.txt\n' > "$S3A_CLOSURES"
S3A_STATE="$S3A_DIR/_meta_state.ledger"
printf '__MERGES__ 4\n' > "$S3A_STATE"

run_skip "$S3A_STATE" "$S3A_CLOSURES" "$S3A_DIR"

assert "S3a: emits RUN (unmapped) for the member with no closure row" \
    out_has "$RUN_OUT" "RUN (unmapped): test_gamma.sh"
assert "S3a: the unmapped member IS executed" \
    out_has "$RUN_OUT" "--- Running: test_gamma.sh ---"

# -- 3b: mapped member with no state baseline runs (cannot prove clean) -------
echo ""
echo "--- Section 3b (step-5): RUN (no-baseline), mapped member absent from the ledger ---"

S3B_DIR="$(mktemp -d)"; _TMPDIRS+=("$S3B_DIR")
git_init_fixture "$S3B_DIR"
mk_member "$S3B_DIR" test_alpha.sh 0
printf 'stable\n' > "$S3B_DIR/alpha_dep.txt"
git -C "$S3B_DIR" add -A
git -C "$S3B_DIR" commit -q -m "base"
S3B_CLOSURES="$S3B_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S3B_CLOSURES"
# State has the global counter but NO entry for test_alpha ⇒ no green baseline.
S3B_STATE="$S3B_DIR/_meta_state.ledger"
printf '__MERGES__ 4\n' > "$S3B_STATE"

run_skip "$S3B_STATE" "$S3B_CLOSURES" "$S3B_DIR"

assert "S3b: emits RUN (no-baseline) for the mapped member absent from the ledger" \
    out_has "$RUN_OUT" "RUN (no-baseline): test_alpha.sh"
assert "S3b: the no-baseline member IS executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# ===========================================================================
# Section 4 (step-7): RUN (backstop-due) — an otherwise-clean member is forced
#   to run at least once per MAX_AGE_HOURS / MAX_MERGES. Fixtures are clean
#   (green == HEAD) so ONLY the backstop can trigger the run; premises are
#   deterministic (fixed old epoch / fixed counter delta / tiny knob) — no
#   wall-clock flakiness. RED until step-8 (clean tree currently ⇒ SKIP).
# ===========================================================================

# make_clean_backstop_fixture VARPREFIX — a clean mapped-member fixture
# (green == HEAD). Sets <VARPREFIX>_DIR / _GREEN / _CLOSURES / _STATE; the
# caller writes the state ledger to encode the specific staleness under test.
make_clean_backstop_fixture() {
    local _pfx="$1" _dir
    _dir="$(mktemp -d)"; _TMPDIRS+=("$_dir")
    git_init_fixture "$_dir"
    mk_member "$_dir" test_alpha.sh 0
    printf 'stable\n' > "$_dir/alpha_dep.txt"
    git -C "$_dir" add -A
    git -C "$_dir" commit -q -m "base"
    printf -v "${_pfx}_DIR" '%s' "$_dir"
    printf -v "${_pfx}_GREEN" '%s' "$(git -C "$_dir" rev-parse HEAD)"
    printf -v "${_pfx}_CLOSURES" '%s' "$_dir/_meta_closures.manifest"
    printf -v "${_pfx}_STATE" '%s' "$_dir/_meta_state.ledger"
    printf 'test_alpha.sh alpha_dep.txt\n' > "$_dir/_meta_closures.manifest"
}

# -- 4a: age backstop (last_executed_at far in the past) ----------------------
echo ""
echo "--- Section 4a (step-7): RUN (backstop-due), age exceeds MAX_AGE_HOURS ---"

make_clean_backstop_fixture S4A
# last_executed_at = 1000 (epoch ~1970) ⇒ age >> 24h; merges_since = 0.
{ printf '__MERGES__ 5\n'; printf 'test_alpha.sh %s 1000 5\n' "$S4A_GREEN"; } > "$S4A_STATE"

run_skip "$S4A_STATE" "$S4A_CLOSURES" "$S4A_DIR"

assert "S4a: emits RUN (backstop-due) when last exec is older than MAX_AGE_HOURS" \
    out_has "$RUN_OUT" "RUN (backstop-due): test_alpha.sh"
assert "S4a: the age-backstop member IS executed (not skipped)" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# -- 4b: merges backstop (merge-counter delta >= MAX_MERGES) ------------------
echo ""
echo "--- Section 4b (step-7): RUN (backstop-due), merges since last exec >= MAX_MERGES ---"

make_clean_backstop_fixture S4B
# Fresh timestamp (no age backstop) but merges_since = 30 - 2 = 28 >= 25.
{ printf '__MERGES__ 30\n'; printf 'test_alpha.sh %s %s 2\n' "$S4B_GREEN" "$(date +%s)"; } > "$S4B_STATE"

run_skip "$S4B_STATE" "$S4B_CLOSURES" "$S4B_DIR"

assert "S4b: emits RUN (backstop-due) when merges since last exec >= MAX_MERGES" \
    out_has "$RUN_OUT" "RUN (backstop-due): test_alpha.sh"
assert "S4b: the merges-backstop member IS executed (not skipped)" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

# -- 4c: knob tunability (tiny MAX_AGE_HOURS forces backstop deterministically) --
echo ""
echo "--- Section 4c (step-7): RUN (backstop-due) forced via REIFY_RUN_ALL_SKIP_MAX_AGE_HOURS=0 ---"

make_clean_backstop_fixture S4C
# Fresh state that would SKIP under defaults; MAX_AGE_HOURS=0 ⇒ age(>=0)>=0.
{ printf '__MERGES__ 3\n'; printf 'test_alpha.sh %s %s 3\n' "$S4C_GREEN" "$(date +%s)"; } > "$S4C_STATE"

RUN_SKIP_ENV=(REIFY_RUN_ALL_SKIP_MAX_AGE_HOURS=0)
run_skip "$S4C_STATE" "$S4C_CLOSURES" "$S4C_DIR"
RUN_SKIP_ENV=()

assert "S4c: MAX_AGE_HOURS=0 forces RUN (backstop-due) on an otherwise-clean member" \
    out_has "$RUN_OUT" "RUN (backstop-due): test_alpha.sh"
assert "S4c: the knob-forced backstop member IS executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"

test_summary
