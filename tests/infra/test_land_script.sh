#!/usr/bin/env bash
# Infrastructure test for Fix 3 (main-gate-hardening): scripts/land.sh is the
# sanctioned manual path to land a task branch onto main. It must:
#   - refuse a missing / nonexistent / 'main' branch argument,
#   - refuse running off main,
#   - refuse a dirty working tree (and NOT leave a sentinel behind),
#   - on the happy path, mark the main-gate sentinel BEFORE the merge gate runs
#     and merge via a real --no-ff (so pre-merge-commit runs), with the
#     reference-transaction hook then consuming the sentinel.
#
# Runs entirely in throwaway temp repos; the real repository's main is untouched.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== scripts/land.sh sanctioned manual-landing path (Fix 3) ==="

# make_repo VAR — a throwaway repo on main carrying scripts/land.sh, the hooks,
# and a STUB pre-merge-commit that records whether the sentinel exists when it
# runs (proving land.sh marked it BEFORE the gate), then marks + allows so the
# test stays fast and self-contained (no real verify.sh / cargo). main has a base
# commit; task/foo adds one. scripts/ and hooks/ are committed so the tree is CLEAN.
make_repo() {
    local _var="$1" dir
    dir="$(mktemp -d)"; _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test
    mkdir -p "$dir/scripts" "$dir/hooks"
    cp "$REPO_ROOT/scripts/land.sh" "$dir/scripts/"; chmod +x "$dir/scripts/land.sh"
    cp "$REPO_ROOT/hooks/main-gate-lib.sh" "$dir/hooks/"
    cp "$REPO_ROOT/hooks/reference-transaction" "$dir/hooks/"; chmod +x "$dir/hooks/reference-transaction"
    cat > "$dir/hooks/pre-merge-commit" <<'PMC'
#!/usr/bin/env bash
ROOT="$(git rev-parse --show-toplevel)"
. "$ROOT/hooks/main-gate-lib.sh"
[ -e "$(main_gate_sentinel)" ] && echo yes > "$(git rev-parse --git-common-dir)/gate-saw-sentinel"
main_gate_mark   # stand in for "verify passed"
exit 0
PMC
    chmod +x "$dir/hooks/pre-merge-commit"
    git -C "$dir" config core.hooksPath "$dir/hooks"
    git -C "$dir" add scripts hooks
    git -C "$dir" commit -q -m base
    git -C "$dir" checkout -q -b task/foo
    printf 'work\n' > "$dir/feature.txt"
    git -C "$dir" add feature.txt
    git -C "$dir" commit -q -m "task work"
    git -C "$dir" checkout -q main
    printf -v "$_var" '%s' "$dir"
}

# land <repo> [args...] — run land.sh; sets LAND_RC and LAND_OUT.
land() {
    local dir="$1"; shift
    local rc=0 out
    out="$( ( cd "$dir" && bash scripts/land.sh "$@" ) 2>&1 )" || rc=$?
    LAND_RC=$rc; LAND_OUT="$out"
}

R=""; make_repo R

# -- guard: missing branch arg -> exit 64 -------------------------------------
echo ""
echo "--- guard: missing branch arg ---"
land "$R"
assert "no branch arg -> exit 64 (usage error)" test "$LAND_RC" -eq 64

# -- guard: nonexistent branch ------------------------------------------------
echo ""
echo "--- guard: nonexistent branch ---"
land "$R" no-such-branch
assert "nonexistent branch -> non-zero" test "$LAND_RC" -ne 0
assert "nonexistent branch -> error says 'does not exist'" \
    bash -c "printf '%s\n' \"\$1\" | grep -qi 'does not exist'" _ "$LAND_OUT"

# -- guard: refusing main into main -------------------------------------------
echo ""
echo "--- guard: main into main ---"
land "$R" main
assert "merging main into main -> non-zero" test "$LAND_RC" -ne 0

# -- guard: off-main ----------------------------------------------------------
echo ""
echo "--- guard: off-main ---"
git -C "$R" checkout -q task/foo
land "$R" task/foo
assert "off-main -> non-zero" test "$LAND_RC" -ne 0
assert "off-main -> error says not 'main'" \
    bash -c "printf '%s\n' \"\$1\" | grep -qi \"not 'main'\"" _ "$LAND_OUT"
git -C "$R" checkout -q main

# -- guard: dirty working tree (and no lingering sentinel) --------------------
echo ""
echo "--- guard: dirty working tree ---"
printf 'dirt\n' > "$R/untracked.txt"
rm -f "$R/.git/reify-main-gate-ok"
land "$R" task/foo
assert "dirty tree -> non-zero" test "$LAND_RC" -ne 0
assert "dirty tree -> error says 'dirty'" \
    bash -c "printf '%s\n' \"\$1\" | grep -qi dirty" _ "$LAND_OUT"
assert "dirty-tree refusal leaves no sentinel (mark happens only after guards)" \
    bash -c "! test -e '$R/.git/reify-main-gate-ok'"
rm -f "$R/untracked.txt"

# -- happy path: clean main, real --no-ff merge, sentinel marked BEFORE gate --
echo ""
echo "--- happy path: verified --no-ff merge marks the sentinel before the gate ---"
rm -f "$R/.git/gate-saw-sentinel" "$R/.git/reify-main-gate-ok" "$R/.git/reify-main-gate.log"
before="$(git -C "$R" rev-parse main)"
land "$R" task/foo
after="$(git -C "$R" rev-parse main)"
assert "happy path exits 0" test "$LAND_RC" -eq 0
assert "happy path advances main" bash -c "[ '$before' != '$after' ]"
assert "happy path creates a merge commit (2 parents)" \
    bash -c "[ \"\$(git -C '$R' rev-list --parents -n1 HEAD | wc -w)\" -eq 3 ]"
assert "land.sh marked the sentinel BEFORE the merge gate ran (gate observed it)" \
    bash -c "test -f '$R/.git/gate-saw-sentinel'"
assert "reference-transaction consumed the sentinel (sanctioned, not lingering)" \
    bash -c "! test -e '$R/.git/reify-main-gate-ok'"
assert "main-gate log records the sanctioned move" \
    bash -c "grep -q 'sanctioned main move' '$R/.git/reify-main-gate.log'"
assert "happy path prints the landed SHA on stdout" \
    bash -c "printf '%s\n' \"\$1\" | grep -qE '[0-9a-f]{40}'" _ "$LAND_OUT"

test_summary
