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
    # Route through `env` so RUN_SKIP_ENV tokens (which arrive via array
    # expansion) are applied as assignments — a `KEY=VAL` word produced by
    # expansion is NOT re-recognized as a shell assignment and would otherwise
    # become the command word.
    RUN_OUT="$(
        env \
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

# ===========================================================================
# Section 5 (step-9): fail-open storm-escape + strict inert gating.
#   ACTIVE engine (all three keys set) + a missing OR corrupt state file ⇒
#   exactly ONE loud line + the FULL pool runs (no per-member decision lines,
#   no skips) — the flaky-ledger amnesia lesson: degrade LOUDLY, never silently
#   skip on unknown state. INERT engine (any one key missing) ⇒ strictly SILENT
#   full run (no decision lines, no loud line) even on a fixture that WOULD skip
#   under the active engine, proving each gate key is load-bearing (the two-key
#   + state-path guarantee). RED until step-10: _RA_SKIP_STATE_MISSING /
#   _RA_SKIP_STATE_BAD are computed by _ra_skip_read_state but not yet acted on,
#   so a missing state currently emits per-member RUN lines and a corrupt state
#   still SKIPs its valid entry.
# ===========================================================================

# count_substr HAYSTACK NEEDLE — number of lines of HAYSTACK containing the
# fixed string NEEDLE (fork-once; only used for the "exactly ONE loud line"
# cardinality check, which the bash-native substring predicates cannot count).
count_substr() {
    local _n
    _n="$(printf '%s\n' "$1" | grep -Fc -- "$2")" || _n=0
    printf '%s' "$_n"
}

# run_noflag STATE CLOSURES DIR — invoke run_all.sh with role=merge + a state
# path + a closures manifest but WITHOUT REIFY_RUN_ALL_CONTENT_SKIP (the
# feature flag unset). Proves the flag key is load-bearing: an otherwise
# would-skip fixture must run silently.
run_noflag() {
    local state="$1" closures="$2" dir="$3"
    RUN_RC=0
    RUN_OUT="$(
        env -u REIFY_RUN_ALL_CONTENT_SKIP \
            DF_VERIFY_ROLE=merge \
            REIFY_RUN_ALL_SKIP_STATE="$state" \
            RUN_ALL_SKIP_CLOSURES_MANIFEST="$closures" \
            bash "$RUN_ALL" "$dir" 2>&1
    )" || RUN_RC=$?
}

# -- 5a: engine ACTIVE, state file ABSENT ⇒ one loud line + full pool ----------
echo ""
echo "--- Section 5a (step-9): storm-escape, state file absent ---"

S5A_DIR="$(mktemp -d)"; _TMPDIRS+=("$S5A_DIR")
git_init_fixture "$S5A_DIR"
mk_member "$S5A_DIR" test_alpha.sh 0        # mapped
printf 'stable\n' > "$S5A_DIR/alpha_dep.txt"
mk_member "$S5A_DIR" test_gamma.sh 0        # unmapped
git -C "$S5A_DIR" add -A
git -C "$S5A_DIR" commit -q -m "base"
S5A_CLOSURES="$S5A_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S5A_CLOSURES"
# State path is SET (⇒ engine active) but the file is never written ⇒ absent.
S5A_STATE="$S5A_DIR/_meta_state.ledger"

run_skip "$S5A_STATE" "$S5A_CLOSURES" "$S5A_DIR"

S5A_LOUD="$(count_substr "$RUN_OUT" "content-skip: state")"
assert "S5a: emits EXACTLY ONE loud storm-escape line for the absent state file" \
    test "$S5A_LOUD" -eq 1
assert "S5a: the loud line names the full-pool fallback" \
    out_has "$RUN_OUT" "running full pool"
assert "S5a: the mapped member still executes (full pool)" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"
assert "S5a: the unmapped member still executes (full pool)" \
    out_has "$RUN_OUT" "--- Running: test_gamma.sh ---"
assert "S5a: nothing is skipped under storm-escape (no SKIP line)" \
    out_lacks "$RUN_OUT" "SKIP (content-clean)"
assert "S5a: no per-member RUN (unmapped) decision line under storm-escape" \
    out_lacks "$RUN_OUT" "RUN (unmapped)"
assert "S5a: no per-member RUN (no-baseline) decision line under storm-escape" \
    out_lacks "$RUN_OUT" "RUN (no-baseline)"

# -- 5b: engine ACTIVE, state file CORRUPT ⇒ one loud line + full pool ---------
echo ""
echo "--- Section 5b (step-9): storm-escape, state file corrupt ---"

make_clean_backstop_fixture S5B   # clean mapped test_alpha (would skip)
# A VALID entry that WOULD skip (green==HEAD, fresh, in-window) FOLLOWED by a
# garbage line ⇒ the whole ledger is unparseable ⇒ the storm-escape must
# suppress the otherwise-certain SKIP. Non-vacuity: WITHOUT the escape this run
# skips the valid entry.
{
    printf '__MERGES__ 3\n'
    printf 'test_alpha.sh %s %s 3\n' "$S5B_GREEN" "$(date +%s)"
    printf '!!!this is not a valid ledger line!!!\n'
} > "$S5B_STATE"

run_skip "$S5B_STATE" "$S5B_CLOSURES" "$S5B_DIR"

S5B_LOUD="$(count_substr "$RUN_OUT" "content-skip: state")"
assert "S5b: emits EXACTLY ONE loud storm-escape line for the corrupt state file" \
    test "$S5B_LOUD" -eq 1
assert "S5b: the loud line names the full-pool fallback" \
    out_has "$RUN_OUT" "running full pool"
assert "S5b: the mapped member still executes (full pool)" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"
assert "S5b: the valid entry is NOT skipped despite the corrupt ledger (non-vacuity)" \
    out_lacks "$RUN_OUT" "SKIP (content-clean)"

# -- 5c: strict inert gating — each missing key ⇒ SILENT full run -------------
# Non-vacuity anchor: build ONE would-skip fixture, prove it SKIPs when fully
# active (role=merge), then prove each single missing key turns it silent.
echo ""
echo "--- Section 5c (step-9): strict inert gating (each gate key is load-bearing) ---"

make_clean_backstop_fixture S5C   # clean mapped test_alpha (would skip)
{ printf '__MERGES__ 3\n'; printf 'test_alpha.sh %s %s 3\n' "$S5C_GREEN" "$(date +%s)"; } > "$S5C_STATE"

# Anchor: fully active ⇒ this fixture SKIPs (proves the inert cases below are
# non-vacuous — the member genuinely would otherwise have been skipped).
run_skip "$S5C_STATE" "$S5C_CLOSURES" "$S5C_DIR"
assert "S5c(anchor): the fixture SKIPs when the engine is fully active" \
    out_has "$RUN_OUT" "SKIP (content-clean): test_alpha.sh"

# c1: flag unset ⇒ silent full run.
run_noflag "$S5C_STATE" "$S5C_CLOSURES" "$S5C_DIR"
assert "S5c1: flag unset ⇒ the would-skip member is executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"
assert "S5c1: flag unset ⇒ no SKIP line" \
    out_lacks "$RUN_OUT" "SKIP (content-clean)"
assert "S5c1: flag unset ⇒ no loud line" \
    out_lacks "$RUN_OUT" "content-skip: state"
assert "S5c1: flag unset ⇒ no per-member decision line" \
    out_lacks "$RUN_OUT" "RUN (no-baseline)"

# c2: role=task ⇒ silent full run (the role key is the merge-vs-task signal;
# _RA_INBOUND_ROLE=task at run_all.sh:230 ⇒ the engine must never skip).
run_skip "$S5C_STATE" "$S5C_CLOSURES" "$S5C_DIR" task
assert "S5c2: role=task ⇒ the would-skip member is executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"
assert "S5c2: role=task ⇒ no SKIP line (role gate is load-bearing)" \
    out_lacks "$RUN_OUT" "SKIP (content-clean)"
assert "S5c2: role=task ⇒ no loud line" \
    out_lacks "$RUN_OUT" "content-skip: state"

# c3: state path empty ⇒ silent full run (feature off, distinct from the
# storm-escape which requires an explicitly-set state path).
run_skip "" "$S5C_CLOSURES" "$S5C_DIR"
assert "S5c3: empty state path ⇒ the would-skip member is executed" \
    out_has "$RUN_OUT" "--- Running: test_alpha.sh ---"
assert "S5c3: empty state path ⇒ no SKIP line" \
    out_lacks "$RUN_OUT" "SKIP (content-clean)"
assert "S5c3: empty state path ⇒ no loud line" \
    out_lacks "$RUN_OUT" "content-skip: state"

# ===========================================================================
# Section 6 (step-11): post-run state-ledger update.
#   After an ACTIVE run where every EXECUTED member passed (failures==0), the
#   ledger advances: each EXECUTED mapped member records green_sha=HEAD + a
#   refreshed timestamp + merges_at_last_exec=the bumped global counter, while
#   SKIPPED members keep their prior entries verbatim. On ANY executed failure
#   the ledger is NOT written (green shas advance only on all-pass). RED until
#   step-12: no post-run write exists yet, so green stays at the prior baseline
#   and the global counter never bumps.
# ===========================================================================

# ledger_field FILE MEMBER N — field N (1=name,2=green,3=at,4=merges) of
# MEMBER's row in the state ledger (empty if the row is absent).
ledger_field() { awk -v m="$2" -v f="$3" '$1==m {print $f; exit}' "$1"; }
# ledger_merges FILE — the global __MERGES__ counter (empty if absent).
ledger_merges() { awk '$1=="__MERGES__" {print $2; exit}' "$1"; }

# -- 6a: all-pass mix (one SKIP + one RUN) advances only the executed member ---
echo ""
echo "--- Section 6a (step-11): all-pass run advances executed members, preserves skipped ---"

S6A_DIR="$(mktemp -d)"; _TMPDIRS+=("$S6A_DIR")
git_init_fixture "$S6A_DIR"
mk_member "$S6A_DIR" test_skip.sh 0
printf 'skip stable\n' > "$S6A_DIR/skip_dep.txt"
mk_member "$S6A_DIR" test_run.sh 0
printf 'run v1\n' > "$S6A_DIR/run_dep.txt"
git -C "$S6A_DIR" add -A
git -C "$S6A_DIR" commit -q -m "base"
S6A_RUN_GREEN="$(git -C "$S6A_DIR" rev-parse HEAD)"   # older baseline for test_run
# Second commit touches ONLY run_dep.txt ⇒ test_run has a delta, test_skip clean.
printf 'run v2 (changed)\n' > "$S6A_DIR/run_dep.txt"
git -C "$S6A_DIR" add -A
git -C "$S6A_DIR" commit -q -m "touch run closure"
S6A_HEAD="$(git -C "$S6A_DIR" rev-parse HEAD)"
S6A_CLOSURES="$S6A_DIR/_meta_closures.manifest"
{ printf 'test_skip.sh skip_dep.txt\n'; printf 'test_run.sh run_dep.txt\n'; } > "$S6A_CLOSURES"
S6A_STATE="$S6A_DIR/_meta_state.ledger"
# test_skip: green==HEAD, FRESH ts ⇒ SKIP. test_run: green==older baseline ⇒ RUN (delta).
{
    printf '__MERGES__ 5\n'
    printf 'test_skip.sh %s %s 5\n' "$S6A_HEAD" "$(date +%s)"
    printf 'test_run.sh %s 1000 4\n' "$S6A_RUN_GREEN"
} > "$S6A_STATE"

run_skip "$S6A_STATE" "$S6A_CLOSURES" "$S6A_DIR"

# Sanity: the fixture really is a SKIP+RUN mix, all-pass (stable across step-12).
assert "S6a: test_skip.sh is skipped (content-clean)" \
    out_has "$RUN_OUT" "SKIP (content-clean): test_skip.sh"
assert "S6a: test_run.sh runs (delta)" \
    out_has "$RUN_OUT" "RUN (delta): test_run.sh"
assert "S6a: the all-pass run exits 0" \
    test "$RUN_RC" -eq 0

# The executed mapped member advances to HEAD, refreshed ts, bumped counter.
assert "S6a: executed member green advances to HEAD" \
    test "$(ledger_field "$S6A_STATE" test_run.sh 2)" = "$S6A_HEAD"
assert "S6a: executed member timestamp is refreshed (not the old 1000)" \
    test "$(ledger_field "$S6A_STATE" test_run.sh 3)" -gt 1000000000
assert "S6a: executed member merges_at is the bumped global counter (6)" \
    test "$(ledger_field "$S6A_STATE" test_run.sh 4)" = "6"
assert "S6a: the global merge counter is bumped 5 -> 6" \
    test "$(ledger_merges "$S6A_STATE")" = "6"
# The skipped member keeps its prior entry verbatim.
assert "S6a: skipped member green is preserved (still the HEAD baseline)" \
    test "$(ledger_field "$S6A_STATE" test_skip.sh 2)" = "$S6A_HEAD"
assert "S6a: skipped member merges_at is preserved (still 5, not bumped)" \
    test "$(ledger_field "$S6A_STATE" test_skip.sh 4)" = "5"

# -- 6b: a failing executed member ⇒ ledger NOT written (no green advance) -----
echo ""
echo "--- Section 6b (step-11): an executed failure leaves the ledger untouched ---"

S6B_DIR="$(mktemp -d)"; _TMPDIRS+=("$S6B_DIR")
git_init_fixture "$S6B_DIR"
mk_member "$S6B_DIR" test_fail.sh 1      # mapped, delta ⇒ runs, then FAILS
printf 'fail v1\n' > "$S6B_DIR/fail_dep.txt"
git -C "$S6B_DIR" add -A
git -C "$S6B_DIR" commit -q -m "base"
S6B_GREEN="$(git -C "$S6B_DIR" rev-parse HEAD)"
printf 'fail v2 (changed)\n' > "$S6B_DIR/fail_dep.txt"
git -C "$S6B_DIR" add -A
git -C "$S6B_DIR" commit -q -m "touch fail closure"
S6B_CLOSURES="$S6B_DIR/_meta_closures.manifest"
printf 'test_fail.sh fail_dep.txt\n' > "$S6B_CLOSURES"
S6B_STATE="$S6B_DIR/_meta_state.ledger"
{ printf '__MERGES__ 7\n'; printf 'test_fail.sh %s 1000 6\n' "$S6B_GREEN"; } > "$S6B_STATE"

run_skip "$S6B_STATE" "$S6B_CLOSURES" "$S6B_DIR"

assert "S6b: the failing member executed (suite exits non-zero) — non-vacuity" \
    test "$RUN_RC" -ne 0
assert "S6b: green is NOT advanced on a failed run (stays the prior baseline)" \
    test "$(ledger_field "$S6B_STATE" test_fail.sh 2)" = "$S6B_GREEN"
assert "S6b: the global merge counter is NOT bumped on a failed run (stays 7)" \
    test "$(ledger_merges "$S6B_STATE")" = "7"

test_summary
