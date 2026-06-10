#!/usr/bin/env bash
# Infrastructure test for task 4502 (β of test-run-concurrency-semaphore PRD).
# Validates semaphore wiring in scripts/verify.sh, hooks/pre-merge-commit, and
# scripts/land.sh — using the hermetic --print-plan oracle and throwaway-repo stub
# patterns established in test_verify_failfast_order.sh / test_land_script.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh + merge-exemption semaphore wiring tests (task 4502) ==="

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# ===========================================================================
# Section 1: Semaphore plan-wiring (--print-plan oracle)
# ===========================================================================
echo ""
echo "--- Section 1: semaphore plan-wiring (--print-plan oracle) ---"

# Capture full plan outputs (including comment lines) and commands-only views.
# DF_VERIFY_ROLE unset (defaults to task), single debug pass.
TASK_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
TASK_CMDS="$(printf '%s\n' "$TASK_FULL" | grep -v '^#')"

# --profile both: debug + release passes in a single plan.
BOTH_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --profile both --print-plan)"
BOTH_CMDS="$(printf '%s\n' "$BOTH_FULL" | grep -v '^#')"

# all --scope all: must have clippy/check OUTSIDE the gated region.
ALL_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan)"
ALL_CMDS="$(printf '%s\n' "$ALL_FULL" | grep -v '^#')"

# (1a) acquire marker present in full output, and is a COMMENT line (starts with #).
assert "task plan: acquire marker present as a comment line" \
    bash -c 'printf "%s\n" "$1" | grep -q "^#.*test-run semaphore.*ACQUIRE"' \
    _ "$TASK_FULL"

# (1b) acquire marker ABSENT from commands-only view (not an executable line).
assert "task plan: acquire marker absent from commands-only view (not executable)" \
    bash -c '! printf "%s\n" "$1" | grep -q "test-run semaphore.*ACQUIRE"' \
    _ "$TASK_CMDS"

# (1c) release marker present in full output as a comment line.
assert "task plan: release marker present as a comment line" \
    bash -c 'printf "%s\n" "$1" | grep -q "^#.*test-run semaphore.*RELEASE"' \
    _ "$TASK_FULL"

# (1d) release marker ABSENT from commands-only view.
assert "task plan: release marker absent from commands-only view (not executable)" \
    bash -c '! printf "%s\n" "$1" | grep -q "test-run semaphore.*RELEASE"' \
    _ "$TASK_CMDS"

# (1e) acquire marker index > psi-gate index (acquire AFTER psi-gate).
assert "task plan: acquire marker ordered AFTER psi-gate" \
    bash -c '
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        ACQ_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        [ -n "$PSI_IDX" ] && [ -n "$ACQ_IDX" ] && [ "$ACQ_IDX" -gt "$PSI_IDX" ]
    ' _ "$TASK_FULL"

# (1f) first nextest pass index > acquire marker index (nextest AFTER acquire).
assert "task plan: first nextest pass ordered AFTER acquire marker" \
    bash -c '
        ACQ_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        NEXT_IDX=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | head -1 | cut -d: -f1)
        [ -n "$ACQ_IDX" ] && [ -n "$NEXT_IDX" ] && [ "$NEXT_IDX" -gt "$ACQ_IDX" ]
    ' _ "$TASK_FULL"

# (1g) release marker index > last nextest pass index (nextest BEFORE release).
assert "task plan: release marker ordered AFTER last nextest pass" \
    bash -c '
        REL_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        LAST_NEXT_IDX=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | tail -1 | cut -d: -f1)
        [ -n "$REL_IDX" ] && [ -n "$LAST_NEXT_IDX" ] && [ "$REL_IDX" -gt "$LAST_NEXT_IDX" ]
    ' _ "$TASK_FULL"

# (1h) for --profile both: debug nextest pass index BETWEEN acquire and release.
assert "both-profile plan: debug nextest pass BETWEEN acquire and release markers" \
    bash -c '
        ACQ_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        # debug pass: nextest run --workspace without --release
        DBG_IDX=$(printf "%s\n" "$1" | grep -n "cargo nextest run --workspace" | grep -v -- "--release" | head -1 | cut -d: -f1)
        [ -n "$ACQ_IDX" ] && [ -n "$REL_IDX" ] && [ -n "$DBG_IDX" ]
        [ "$DBG_IDX" -gt "$ACQ_IDX" ] && [ "$DBG_IDX" -lt "$REL_IDX" ]
    ' _ "$BOTH_FULL"

# (1i) for --profile both: release nextest pass index BETWEEN acquire and release.
assert "both-profile plan: release nextest pass BETWEEN acquire and release markers" \
    bash -c '
        ACQ_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        # release pass: nextest run with --release
        RLS_IDX=$(printf "%s\n" "$1" | grep -n "cargo nextest run.*--release" | head -1 | cut -d: -f1)
        [ -n "$ACQ_IDX" ] && [ -n "$REL_IDX" ] && [ -n "$RLS_IDX" ]
        [ "$RLS_IDX" -gt "$ACQ_IDX" ] && [ "$RLS_IDX" -lt "$REL_IDX" ]
    ' _ "$BOTH_FULL"

# (1j) for all --scope all: cargo clippy index < acquire marker index (lint OUTSIDE gated region).
assert "all plan: cargo clippy ordered BEFORE acquire marker (lint outside gated region)" \
    bash -c '
        ACQ_IDX=$(printf "%s\n" "$1" | grep -n "^#.*test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        CLIP_IDX=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        [ -n "$ACQ_IDX" ] && [ -n "$CLIP_IDX" ] && [ "$CLIP_IDX" -lt "$ACQ_IDX" ]
    ' _ "$ALL_FULL"

# (1k) every cargo nextest run line in commands-only view carries trailing " 9<&-".
assert "task plan: every nextest pass carries trailing ' 9<&-' (FD-close for children)" \
    bash -c '! printf "%s\n" "$1" | grep "cargo nextest run" | grep -vq " 9<&-"' \
    _ "$TASK_CMDS"

assert "both-profile plan: every nextest pass carries trailing ' 9<&-'" \
    bash -c '! printf "%s\n" "$1" | grep "cargo nextest run" | grep -vq " 9<&-"' \
    _ "$BOTH_CMDS"

# (1l) verify.sh sources lib_test_semaphore.sh (structural wiring check).
assert "verify.sh sources scripts/lib_test_semaphore.sh" \
    grep -q "lib_test_semaphore\.sh" "$REPO_ROOT/scripts/verify.sh"

# ===========================================================================
# Section 2: pre-merge-commit merge-exemption
# ===========================================================================
echo ""
echo "--- Section 2: pre-merge-commit merge-exemption ---"

# (2a) Static wiring: pre-merge-commit's verify.sh call carries DF_VERIFY_ROLE=merge.
assert "pre-merge-commit: verify.sh invocation prefixed with DF_VERIFY_ROLE=merge" \
    grep -qE 'DF_VERIFY_ROLE=merge[[:space:]].*scripts/verify\.sh[[:space:]]' \
    "$REPO_ROOT/hooks/pre-merge-commit"

# (2b) Behavioral: throwaway repo confirms role=merge is recorded on `git merge`.
make_pmc_repo() {
    local _var="$1" dir
    dir="$(mktemp -d)"; _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test

    mkdir -p "$dir/scripts" "$dir/hooks"

    # STUB verify.sh: record the role and succeed.
    cat > "$dir/scripts/verify.sh" <<'VSTUB'
#!/usr/bin/env bash
echo "${DF_VERIFY_ROLE:-<unset>}" > "$(git rev-parse --git-common-dir)/recorded-role"
exit 0
VSTUB
    chmod +x "$dir/scripts/verify.sh"

    # REAL pre-merge-commit (sourced from repo).
    cp "$REPO_ROOT/hooks/pre-merge-commit" "$dir/hooks/pre-merge-commit"
    chmod +x "$dir/hooks/pre-merge-commit"
    cp "$REPO_ROOT/hooks/main-gate-lib.sh" "$dir/hooks/"

    # No reference-transaction hook — let the ref update complete unhooked.

    git -C "$dir" config core.hooksPath "$dir/hooks"

    # Base commit on main.
    printf 'base\n' > "$dir/file.txt"
    git -C "$dir" add scripts hooks file.txt
    git -C "$dir" commit -q -m base

    # Branch with a change.
    git -C "$dir" checkout -q -b task/foo
    printf 'work\n' >> "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m "task work"

    git -C "$dir" checkout -q main
    printf -v "$_var" '%s' "$dir"
}

PMC_REPO=""
make_pmc_repo PMC_REPO

git -C "$PMC_REPO" merge --no-ff task/foo
_recorded_role="$(cat "$(git -C "$PMC_REPO" rev-parse --git-common-dir)/recorded-role" 2>/dev/null || echo "<missing>")"
assert "pre-merge-commit behavioral: DF_VERIFY_ROLE=merge recorded during git merge" \
    test "$_recorded_role" = "merge"

# ===========================================================================
# Section 3: land.sh merge-exemption
# ===========================================================================
echo ""
echo "--- Section 3: land.sh merge-exemption ---"

# (3a) Static wiring: land.sh exports DF_VERIFY_ROLE=merge before the merge.
assert "land.sh: exports DF_VERIFY_ROLE=merge (static check)" \
    grep -qE '^[[:space:]]*export DF_VERIFY_ROLE=merge' "$REPO_ROOT/scripts/land.sh"

# (3b) Behavioral: throwaway repo confirms inherited role=merge is recorded.
make_land_repo() {
    local _var="$1" dir
    dir="$(mktemp -d)"; _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test

    mkdir -p "$dir/scripts" "$dir/hooks"

    # REAL land.sh.
    cp "$REPO_ROOT/scripts/land.sh" "$dir/scripts/"
    chmod +x "$dir/scripts/land.sh"

    # REAL main-gate-lib.sh and reference-transaction (needed by land.sh's sentinel path).
    cp "$REPO_ROOT/hooks/main-gate-lib.sh" "$dir/hooks/"
    cp "$REPO_ROOT/hooks/reference-transaction" "$dir/hooks/"
    chmod +x "$dir/hooks/reference-transaction"

    # STUB pre-merge-commit: record the INHERITED role, mark sentinel, succeed.
    cat > "$dir/hooks/pre-merge-commit" <<'PMC'
#!/usr/bin/env bash
ROOT="$(git rev-parse --show-toplevel)"
. "$ROOT/hooks/main-gate-lib.sh"
echo "${DF_VERIFY_ROLE:-<unset>}" > "$(git rev-parse --git-common-dir)/recorded-role"
main_gate_mark
exit 0
PMC
    chmod +x "$dir/hooks/pre-merge-commit"

    git -C "$dir" config core.hooksPath "$dir/hooks"

    # Base commit on main.
    printf 'base\n' > "$dir/file.txt"
    git -C "$dir" add scripts hooks file.txt
    git -C "$dir" commit -q -m base

    # Branch with a change.
    git -C "$dir" checkout -q -b task/foo
    printf 'work\n' >> "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m "task work"

    git -C "$dir" checkout -q main
    printf -v "$_var" '%s' "$dir"
}

LAND_REPO=""
make_land_repo LAND_REPO

_land_rc=0
( cd "$LAND_REPO" && bash scripts/land.sh task/foo ) >/dev/null 2>&1 || _land_rc=$?
assert "land.sh behavioral: exits 0 (merge succeeded)" test "$_land_rc" -eq 0

_land_role="$(cat "$(git -C "$LAND_REPO" rev-parse --git-common-dir)/recorded-role" 2>/dev/null || echo "<missing>")"
assert "land.sh behavioral: DF_VERIFY_ROLE=merge inherited by pre-merge-commit" \
    test "$_land_role" = "merge"

# Land advances main.
_main_sha="$(git -C "$LAND_REPO" rev-parse main)"
_foo_sha="$(git -C "$LAND_REPO" rev-parse task/foo)"
assert "land.sh behavioral: main advanced beyond task/foo (merge commit exists)" \
    bash -c 'git -C "$1" merge-base --is-ancestor "$2" "$3"' \
    _ "$LAND_REPO" "$_foo_sha" "$_main_sha"

test_summary
