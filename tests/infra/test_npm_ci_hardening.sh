#!/usr/bin/env bash
# Infrastructure tests for npm ci hardening (task 816).
# Validates that check-pm-standardization.sh lives in scripts/ and that
# orchestrator.yaml uses if/then/fi guards instead of || true for npm ci.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# setup_fixture_dir VARNAME
# Creates a temp dir with the standard fixture layout used by Tests 23a/23c/23d:
#   scripts/, tests/infra/, gui/sidecar/, tree-sitter-reify/
# Copies check-pm-standardization.sh and test_helpers.sh into the fixture.
# Initialises a git repo so 'git check-ignore' works inside Check 3.
# Appends the dir to _TMPDIRS so cleanup() removes it at script exit.
# Writes the path back to the caller via printf -v (bash 3.1+; no subshell).
# Requires a non-empty VARNAME argument; returns 1 with a message to stderr otherwise.
setup_fixture_dir() {
    if [ -z "${1:-}" ]; then
        echo "setup_fixture_dir: requires a non-empty varname argument" >&2
        return 1
    fi
    local _varname="$1"
    local dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts" "$dir/tests/infra" "$dir/gui/sidecar" "$dir/tree-sitter-reify"
    cp "$REPO_ROOT/scripts/check-pm-standardization.sh" "$dir/scripts/"
    cp "$SCRIPT_DIR/test_helpers.sh" "$dir/tests/infra/"
    git -C "$dir" init -q
    printf -v "$_varname" '%s' "$dir"
}

echo "=== npm ci hardening tests ==="

# -- Test 1: check-pm-standardization.sh location ----------------------------
echo ""
echo "--- Test 1: script lives in scripts/, not tests/ ---"

assert "scripts/check-pm-standardization.sh exists" \
    test -f "$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "scripts/check-pm-standardization.sh is executable" \
    test -x "$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "tests/check-pm-standardization.sh does NOT exist" \
    bash -c "! test -f '$REPO_ROOT/tests/check-pm-standardization.sh'"

# -- Test 2: script has only checks 1-4 (no 5-9) ----------------------------
echo ""
echo "--- Test 2: script contains only checks 1-4 ---"

SCRIPT="$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "script has no grep calls referencing hooks/project-checks" \
    bash -c "! grep -qE 'grep.*hooks/project-checks|hooks/project-checks.*grep' '$SCRIPT'"

assert "script has no grep calls referencing orchestrator.yaml" \
    bash -c "! grep -qE 'grep.*orchestrator|orchestrator.*grep' '$SCRIPT'"

assert "script has exactly 4 'Check N:' echo statements" \
    bash -c "[ \"\$(grep -cE 'echo \"Check [0-9]' '$SCRIPT')\" = '4' ]"

# -- Test 3: orchestrator.yaml uses if/then/fi guards (not || true) ----------
echo ""
echo "--- Test 3: orchestrator.yaml if/then/fi guards for npm ci ---"

ORCH="$REPO_ROOT/orchestrator.yaml"

assert "test_command has no '|| true' after npm ci" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'npm ci.*|| true\||| true.*npm ci'"

assert "lint_command has no '|| true' after npm ci" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -q 'npm ci.*|| true\||| true.*npm ci'"

assert "test_command uses 'if test' guard pattern for npm ci" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'if test'"

assert "lint_command uses 'if test' guard pattern for npm ci" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test'"

# -- Test 4: script has git preflight check ----------------------------------
echo ""
echo "--- Test 4: script has 'command -v git' preflight ---"

assert "script contains 'command -v git' preflight check" \
    grep -q 'command -v git' "$SCRIPT"

# -- Test 5: Check 1 matches npm@ prefix, not just field presence -------------
echo ""
echo "--- Test 5: Check 1 grep matches npm@ prefix ---"

assert "Check 1 grep pattern includes 'npm@' prefix match" \
    bash -c "grep -qE 'grep.*npm@' '$SCRIPT'"

# -- Test 6: script has cross-file consistency check -------------------------
echo ""
echo "--- Test 6: script has cross-file packageManager consistency check ---"

assert "Check 2 block uses 'sort -u' for cross-file consistency comparison" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'sort -u'"

assert "Check 2 block references 'packageManager' in consistency logic" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'packageManager'"

# -- Test 7: git check-ignore is NOT called inside a for loop ----------------
echo ""
echo "--- Test 7: git check-ignore is batched (not in a for loop) ---"

# NOTE: the awk range below only matches the one-line `for ...; do` form;
# a multi-line `for ...\ndo` spelling would slip past this check. If a
# future refactor splits the header across lines, broaden the start
# pattern (e.g. `/^for /` + a separate `/^do/` anchor) to keep coverage.
assert "bare git check-ignore (without -v) is not inside for/done loops" \
    bash -c "! awk '{sub(/^[[:space:]]+/,\"\")} /^for [^;]*; *do/,/^done/' '$SCRIPT' | grep 'git check-ignore' | grep -vq -- '-v'"

# -- Test 8: wc -l output is stripped for cross-platform portability ----------
echo ""
echo "--- Test 8: wc -l output has whitespace stripped (cross-platform) ---"

assert "script does not use bare 'wc -l)' without whitespace stripping" \
    bash -c "! grep -qE 'wc -l\)' '$SCRIPT'"

assert "script pipes 'wc -l' into 'tr -d' to strip whitespace" \
    grep -qE 'wc -l[[:space:]]*\|[[:space:]]*tr -d' "$SCRIPT"

# -- Test 9: orchestrator command placement and existence guards ---------------
echo ""
echo "--- Test 9: orchestrator command placement and existence guards ---"

# S1: full-path assertion (the guard-pattern assertion below also provides full-path coverage)
assert "scripts/check-pm-standardization.sh (full path) is in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'scripts/check-pm-standardization.sh'"

assert "check-pm-standardization.sh is NOT in test_command" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'check-pm-standardization.sh'"

# S2: symmetric negative assertion — test-only scripts should not be in lint_command
assert "sync_comments_test.sh is NOT in lint_command" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -q 'sync_comments_test.sh'"

assert "sync_comments_test.sh uses 'if test -f' guard in test_command" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'if test -f tests/sync_comments_test.sh'"

assert "check-pm-standardization.sh uses 'if test -f' guard in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test -f scripts/check-pm-standardization.sh'"

# -- Test 10: WARNING echoes when guards trigger a skip ------------------------
echo ""
echo "--- Test 10: WARNING echoes for guard skips ---"

assert "test_command has WARNING echo for sync_comments_test.sh skip" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'WARNING.*sync_comments_test'"

assert "lint_command has WARNING echo for check-pm-standardization.sh skip" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'WARNING.*check-pm-standardization'"

# -- Test 11: end-to-end execution test ----------------------------------------
echo ""
echo "--- Test 11: check-pm-standardization.sh runs successfully ---"

assert "check-pm-standardization.sh runs successfully in repo context" \
    bash "$REPO_ROOT/scripts/check-pm-standardization.sh"

# -- Test 12: build artifact tracking hygiene ----------------------------------
echo ""
echo "--- Test 12: build artifact tracking hygiene ---"

# tree-sitter-reify/src/.grammar_hash.stamp is listed at .gitignore:34 but was
# previously tracked (pre-dated the gitignore entry). While tracked, every
# tree-sitter build regenerates it and dirties main's working tree, causing
# advance_main to fail with stash_failed (task 1005 blocker). The fix is
# `git rm --cached` to remove the stale index entry so the existing rule applies.
assert "tree-sitter-reify/src/.grammar_hash.stamp is NOT tracked by git" \
    bash -c "cd '$REPO_ROOT' && ! git ls-files --error-unmatch tree-sitter-reify/src/.grammar_hash.stamp >/dev/null 2>&1"

# -- Test 13: script has rev-parse --is-inside-work-tree preflight -----------
echo ""
echo "--- Test 13: script has rev-parse --is-inside-work-tree preflight ---"

assert "script contains 'rev-parse --is-inside-work-tree' preflight" \
    grep -q 'rev-parse --is-inside-work-tree' "$SCRIPT"

# -- Test 14: script defines PKG_FILES with all three package.json paths ------
echo ""
echo "--- Test 14: script defines PKG_FILES with all three package.json paths ---"

assert "script defines PKG_FILES with all three package.json paths" \
    bash -c "grep -qE '^PKG_FILES=.*gui/package.json.*gui/sidecar/package.json.*tree-sitter-reify/package.json' '$SCRIPT'"

# -- Test 15: Check 1 for-loop iterates $PKG_FILES ----------------------------
echo ""
echo "--- Test 15: Check 1 for-loop iterates \$PKG_FILES ---"

assert "Check 1 for-loop iterates \$PKG_FILES" \
    bash -c "grep -qE 'for pkg in \\\$PKG_FILES' '$SCRIPT'"

# -- Test 16: Check 2 grep arguments expand $PKG_FILES ------------------------
echo ""
echo "--- Test 16: Check 2 grep arguments expand \$PKG_FILES ---"

assert "Check 2 grep arguments expand \$PKG_FILES" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q PKG_FILES"

# -- Test 17: Check 3 has git check-ignore -v diagnostic fallback -------------
echo ""
echo "--- Test 17: Check 3 has 'git check-ignore -v' diagnostic fallback ---"

assert "Check 3 has 'git check-ignore -v' diagnostic fallback" \
    grep -q 'git check-ignore -v' "$SCRIPT"

# -- Test 18: Check 2 subshell has defensive shell flags (task 1326) ---------
echo ""
echo "--- Test 18: Check 2 subshell enables set -euo pipefail ---"

# Without these flags, a missing package.json file silently produces a PASS
# because grep's non-zero exit inside the pipeline is masked by `tr -d` and
# the bash -c subshell does not inherit the outer script's `set -euo pipefail`.
assert "Check 2 subshell enables set -euo pipefail" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'set -euo pipefail'"

# -- Test 19: Check 2 has dual total==PKG_COUNT AND unique==1 assertion (task 1366) --
echo ""
echo "--- Test 19: Check 2 has pre-dedup and post-dedup count assertions ---"

# Without the pre-dedup total==PKG_COUNT assertion, a package.json missing the
# packageManager field would be silently accepted: grep emits fewer matching
# lines, but `sort -u` still collapses them to 1 unique line.
assert "Check 2 block computes a pre-dedup 'total' variable" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'total='"

assert "Check 2 block computes a post-dedup 'unique' variable" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'unique='"

assert "Check 2 total assertion references \$PKG_COUNT not a literal number (task 1366)" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -qF '\$PKG_COUNT'"

assert "Check 2 asserts unique equals 1 (catches version disagreement)" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q \"unique.* = '1'\""

# -- Test 20: Check 2 has explicit file existence preflight (task 1326) ------
echo ""
echo "--- Test 20: Check 2 has explicit file existence preflight ---"

# Belt-and-braces guard: even if the subshell flags were lost, a preflight
# `[ -f "$f" ] || exit 1` loop ensures a missing file aborts the subshell
# before grep is invoked.
assert "Check 2 block has an explicit '[ -f ...' file existence check" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -qE '\\[[[:space:]]*-f'"

# -- Test 21: Check 2 logic behavioral verification with fixtures (task 1326) -
echo ""
echo "--- Test 21: Check 2 logic rejects both bug scenarios ---"

# These tests reproduce the Check 2 subshell logic exactly and run it against
# mktemp fixture directories, independent of the real repo paths. This proves
# the pattern catches both regression modes described in task 1326:
#   bug 1: file missing the packageManager field → total < PKG_COUNT, unique==1 → fail
#   bug 2: file missing entirely → preflight / set -euo pipefail → fail
FIX_DIR="$(mktemp -d)"
_TMPDIRS+=("$FIX_DIR")

CHECK2_HELPER="$FIX_DIR/check2_logic.sh"
cat > "$CHECK2_HELPER" <<'CHECK2EOF'
#!/usr/bin/env bash
# Mirror of scripts/check-pm-standardization.sh Check 2 subshell body.
set -euo pipefail
expected=$#
for f in "$@"; do
    [ -f "$f" ] || exit 1
done
total=$(grep -ohE '"packageManager"\s*:\s*"[^"]+"' "$@" | wc -l | tr -d ' ')
unique=$(grep -ohE '"packageManager"\s*:\s*"[^"]+"' "$@" | sort -u | wc -l | tr -d ' ')
[ "$total" = "$expected" ] && [ "$unique" = '1' ]
CHECK2EOF
chmod +x "$CHECK2_HELPER"

# Case A: three files present, all agree → PASS
mkdir -p "$FIX_DIR/case_a"
for i in 1 2 3; do
    printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_a/p${i}.json"
done
assert "Check 2 logic accepts three files all agreeing on packageManager" \
    "$CHECK2_HELPER" "$FIX_DIR/case_a/p1.json" "$FIX_DIR/case_a/p2.json" "$FIX_DIR/case_a/p3.json"

# Case B: one file missing the packageManager field → FAIL (bug 1)
mkdir -p "$FIX_DIR/case_b"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_b/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_b/p2.json"
printf '{\n  "name": "no-pm-field"\n}\n' > "$FIX_DIR/case_b/p3.json"
assert "Check 2 logic rejects a file missing the packageManager field (bug 1)" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_b/p1.json' '$FIX_DIR/case_b/p2.json' '$FIX_DIR/case_b/p3.json'"

# Case C: one file does not exist on disk → FAIL (bug 2)
mkdir -p "$FIX_DIR/case_c"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_c/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_c/p2.json"
# case_c/p3.json intentionally not created
assert "Check 2 logic rejects a missing package.json file (bug 2)" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_c/p1.json' '$FIX_DIR/case_c/p2.json' '$FIX_DIR/case_c/p3.json'"

# Case D: three files with differing packageManager versions → FAIL
mkdir -p "$FIX_DIR/case_d"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_d/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_d/p2.json"
printf '{\n  "packageManager": "npm@10.1.0"\n}\n' > "$FIX_DIR/case_d/p3.json"
assert "Check 2 logic rejects differing packageManager versions" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_d/p1.json' '$FIX_DIR/case_d/p2.json' '$FIX_DIR/case_d/p3.json'"

# Case E: four files all agreeing → PASS (proves dynamic count works for N≠3)
# This fails until CHECK2_HELPER uses expected=$# instead of hardcoded '3'.
mkdir -p "$FIX_DIR/case_e"
for i in 1 2 3 4; do
    printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_e/p${i}.json"
done
assert "Check 2 logic accepts four files all agreeing on packageManager (N≠3)" \
    "$CHECK2_HELPER" \
    "$FIX_DIR/case_e/p1.json" "$FIX_DIR/case_e/p2.json" \
    "$FIX_DIR/case_e/p3.json" "$FIX_DIR/case_e/p4.json"

# -- Test 22: script derives PKG_COUNT dynamically from PKG_FILES (task 1366) -
echo ""
echo "--- Test 22: script derives PKG_COUNT dynamically from PKG_FILES ---"

# The magic number '3' in Check 2's total assertion must be replaced by a
# PKG_COUNT variable that is computed from PKG_FILES so that adding a new
# package.json path to PKG_FILES automatically adjusts the assertion.
assert "script uses 'set -- \$PKG_FILES' to load positional parameters" \
    bash -c "grep -qE 'set -- \\\$PKG_FILES' '$SCRIPT'"

assert "script derives PKG_COUNT from positional parameter count (PKG_COUNT=\$#)" \
    bash -c "grep -qE 'PKG_COUNT=\\\$#' '$SCRIPT'"

assert "Check 2 block references PKG_COUNT in the total assertion" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'PKG_COUNT'"

# -- Test 23: behavioral integration tests (task 1328) ------------------------
echo ""
echo "--- Test 23: behavioral integration tests ---"

setup_fixture_dir FIXTURE_DIR

# .gitignore: pnpm-lock.yaml gitignored (Check 4); package-lock.json files NOT listed (Check 3)
echo "gui/pnpm-lock.yaml" > "$FIXTURE_DIR/.gitignore"

# Write consistent packageManager versions for Test 23a
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    printf '{"packageManager":"npm@10.9.0"}\n' > "$FIXTURE_DIR/$pkg"
done

# Test 23a: all files agree on the same version -> script exits 0
# Capture combined stdout+stderr so we can pin both the exit status and the
# absence of DIAGNOSTIC: emissions with two separate named assertions.
out23a=$(cd "$FIXTURE_DIR" && bash scripts/check-pm-standardization.sh 2>&1); status23a=$?

assert "23a: consistent packageManager versions -> exit 0" \
    bash -c '[ "$1" = "0" ]' _ "$status23a"

assert "23a: no DIAGNOSTIC: emitted when no npm lockfiles are gitignored" \
    bash -c '! printf "%s\n" "$1" | grep -q DIAGNOSTIC:' _ "$out23a"

# Test 23b: introduce a version mismatch -> script exits non-zero
printf '{"packageManager":"npm@9.0.0"}\n' > "$FIXTURE_DIR/tree-sitter-reify/package.json"

assert "23b: mismatched packageManager versions -> exit non-zero" \
    bash -c "! (cd '$FIXTURE_DIR' && bash scripts/check-pm-standardization.sh)"

# Test 23c: all files use the SAME non-npm@ packageManager (yarn@1.22.0)
# Check 1 fails (no npm@ prefix); Check 2 still passes (total=3, unique=1 — same value everywhere)
# Checks 3 and 4 pass because lockfile/.gitignore state is correct
setup_fixture_dir FIXTURE_23C

# .gitignore: pnpm-lock.yaml gitignored (Check 4 pass); npm lockfiles NOT listed (Check 3 pass)
echo "gui/pnpm-lock.yaml" > "$FIXTURE_23C/.gitignore"

# All three files use the SAME non-npm@ value — Check 2 still passes (total=3, unique=1)
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    printf '{"packageManager":"yarn@1.22.0"}\n' > "$FIXTURE_23C/$pkg"
done

assert "23c: non-npm@ packageManager (yarn@1.22.0 in all files) -> exit non-zero (Check 1 fails)" \
    bash -c "! (cd '$FIXTURE_23C' && bash scripts/check-pm-standardization.sh)"

# Test 23d: consistent npm@ versions but gui/package-lock.json gitignored
# Checks 1 and 2 pass (npm@10.9.0 in all files, total=3, unique=1)
# Check 3 fails (.gitignore lists gui/package-lock.json so git check-ignore returns 0)
# Check 4 passes (gui/pnpm-lock.yaml still in .gitignore)
setup_fixture_dir FIXTURE_23D

# .gitignore: BOTH pnpm-lock.yaml (Check 4 pass) AND gui/package-lock.json (Check 3 fail)
printf 'gui/pnpm-lock.yaml\ngui/package-lock.json\n' > "$FIXTURE_23D/.gitignore"

# All three files use consistent npm@10.9.0 — Checks 1 and 2 pass
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    printf '{"packageManager":"npm@10.9.0"}\n' > "$FIXTURE_23D/$pkg"
done

assert "23d: gui/package-lock.json gitignored -> exit non-zero (Check 3 fails)" \
    bash -c "! (cd '$FIXTURE_23D' && bash scripts/check-pm-standardization.sh)"

# -- Test 24: LOCK_FILES is hoisted (defined before 'Check 1:' echo) ----------
echo ""
echo "--- Test 24: LOCK_FILES is hoisted (defined before 'Check 1:' echo) ---"

assert "LOCK_FILES is defined before the first 'Check 1:' echo" \
    bash -c "
        lock_line=\$(grep -n '^LOCK_FILES=' '$SCRIPT' | head -1 | cut -d: -f1)
        check1_line=\$(grep -n 'echo \"Check 1:' '$SCRIPT' | head -1 | cut -d: -f1)
        [ -n \"\$lock_line\" ] && [ -n \"\$check1_line\" ] && [ \"\$lock_line\" -lt \"\$check1_line\" ]
    "

# -- Test 25: Check 3 emits DIAGNOSTIC: when a lockfile is gitignored ----------
echo ""
echo "--- Test 25: Check 3 emits DIAGNOSTIC: when a lockfile is gitignored ---"

FIXTURE24="$(mktemp -d)"
_TMPDIRS+=("$FIXTURE24")
mkdir -p "$FIXTURE24/scripts" "$FIXTURE24/tests/infra"
cp "$SCRIPT" "$FIXTURE24/scripts/check-pm-standardization.sh"
cp "$SCRIPT_DIR/test_helpers.sh" "$FIXTURE24/tests/infra/test_helpers.sh"
git -C "$FIXTURE24" init -q
git -C "$FIXTURE24" config user.email "test@test.com"
git -C "$FIXTURE24" config user.name "Test"
printf 'gui/package-lock.json\n' > "$FIXTURE24/.gitignore"

# The || true makes the capture tolerant of the script's non-zero exit: Check 3's
# assert fails when a lockfile is gitignored, and because check-pm-standardization.sh
# has set -e, it short-circuits before test_summary. The explicit printf | grep avoids
# depending on the outer bash -c to not enable pipefail, or on assert()'s internal
# >/dev/null 2>&1 redirection swallowing the output before grep can see it.
out24=$(bash "$FIXTURE24/scripts/check-pm-standardization.sh" 2>&1 || true)
assert "Check 3 emits DIAGNOSTIC: when gui/package-lock.json is gitignored" \
    bash -c 'printf "%s\n" "$1" | grep -q DIAGNOSTIC:' _ "$out24"

# -- Test 26: setup_fixture_dir helper function --------------------------------
echo ""
echo "--- Test 26: setup_fixture_dir helper function ---"

assert "setup_fixture_dir function is defined" declare -f setup_fixture_dir

tmpdirs_before=${#_TMPDIRS[@]}
setup_fixture_dir FIXTURE_T26

assert "setup_fixture_dir: scripts/ subdir exists" \
    test -d "$FIXTURE_T26/scripts"
assert "setup_fixture_dir: tests/infra/ subdir exists" \
    test -d "$FIXTURE_T26/tests/infra"
assert "setup_fixture_dir: gui/sidecar/ subdir exists" \
    test -d "$FIXTURE_T26/gui/sidecar"
assert "setup_fixture_dir: tree-sitter-reify/ subdir exists" \
    test -d "$FIXTURE_T26/tree-sitter-reify"
assert "setup_fixture_dir: check-pm-standardization.sh copied" \
    test -f "$FIXTURE_T26/scripts/check-pm-standardization.sh"
assert "setup_fixture_dir: test_helpers.sh copied" \
    test -f "$FIXTURE_T26/tests/infra/test_helpers.sh"
assert "setup_fixture_dir: fixture is a git work tree" \
    bash -c "cd '$FIXTURE_T26' && git rev-parse --is-inside-work-tree"
assert "setup_fixture_dir: appended to _TMPDIRS cleanup array" \
    test "${#_TMPDIRS[@]}" -gt "$tmpdirs_before"

# -- Test 27: setup_fixture_dir rejects empty/missing varname argument ----------
echo ""
echo "--- Test 27: setup_fixture_dir argument validation guard ---"

tmpdirs_baseline_t27=${#_TMPDIRS[@]}
guard_err_t27=$(setup_fixture_dir "" 2>&1 || true)
guard_rc_t27=0
setup_fixture_dir "" 2>/dev/null || guard_rc_t27=$?

assert "setup_fixture_dir: rejects empty varname with non-zero return" \
    test "$guard_rc_t27" -ne 0
assert "setup_fixture_dir: guard fires before _TMPDIRS mutation" \
    test "${#_TMPDIRS[@]}" -eq "$tmpdirs_baseline_t27"
assert "setup_fixture_dir: error message mentions function name" \
    bash -c 'printf "%s\n" "$1" | grep -qi setup_fixture_dir' _ "$guard_err_t27"

test_summary
