#!/usr/bin/env bash
# Infrastructure tests for the single `watch --once` index primitive
# scripts/jcodemunch-index-reify.sh (task 6107, β).
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4.4
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/β
#
# Validates the script's CONTRACT executably rather than by grepping its prose
# — PRD §2.4 identifies exactly that vacuous-evidence shape as the disease
# (`L-SMOKE` named a script that did not exist; `jcodemunch_live.rs` was
# PASS-shaped whether or not the chain worked). Three test-only seams keep the
# whole contract hermetic:
#
#   --dry-run     prints the CONSTRUCTED argv, so the invocation contract
#                 (version pin / `watch` / `--once` / `--no-ai-summaries`, and
#                 the `--paths-from` + `index` bans) binds to the real command.
#   --check-only  skips the indexer and runs only identity resolution plus the
#                 DB gates, so the missing/empty/truncated refusals are driven
#                 against synthetic sqlite DBs under a temp CODE_INDEX_PATH.
#   REIFY_JC_INDEXER_CMD  a test-only indexer override, so the two REAL 1.108.54
#                 `watch --once` stderr shapes drive the summary/exit contract.
#
# Deliberately does NOT execute the real `uvx … watch --once` path: it costs
# ~5-10 min, needs PyPI, and mutates the host-global index at ~/.code-index —
# none of which belongs on a merge gate. That one end-to-end run is discharged
# once by the implementer as recorded acceptance evidence (same reasoning the
# capability manifest's `capstone-must-not-become-gate-resident` resolution
# applies to ε), mirroring test_gui_test_script.sh / test_run_gui_scripts.sh,
# which check a launcher's contract without launching it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

JC_INDEX="$REPO_ROOT/scripts/jcodemunch-index-reify.sh"

# The PRODUCTION identity this script exists to maintain, recomputed
# independently in Block 3 below rather than trusted as a constant.
CANONICAL_ROOT="/home/leo/src/reify"
CANONICAL_REPO_ID="local/reify-4ae45bbd"

_TMPDIRS=()
cleanup() {
    local d
    for d in ${_TMPDIRS+"${_TMPDIRS[@]}"}; do
        [ -n "$d" ] && rm -rf "$d"
    done
}
trap cleanup EXIT

mk_tmpdir() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/jc-index-reify-XXXXXX")" || return 1
    _TMPDIRS+=("$d")
    printf '%s\n' "$d"
}

# jc_field <field> [args...] — the value of a `<field>` line from one run of the
# script. The script's exit status is deliberately IGNORED: identity is printed
# on every run, including the refusal paths (Blocks 5/7), so binding these
# assertions to exit 0 would make them re-fail for unrelated reasons the moment
# the DB gates land.
jc_field() {
    local field="$1"; shift
    { "$JC_INDEX" "$@" 2>&1 || true; } \
        | sed -n "s/^jcodemunch-index-reify: ${field}[[:space:]]\{1,\}//p" \
        | head -n1
}

# assert_field <field> <expected> [args...] — jc_field equality with the
# mismatch printed to stderr, which test_helpers.sh's assert() captures and
# dumps on FAIL (so a failure names both sides rather than just "false").
assert_field() {
    local field="$1" expected="$2"; shift 2
    local got
    got="$(jc_field "$field" "$@")"
    if [ "$got" != "$expected" ]; then
        printf 'field %s: expected %s\n                 got %s\n' "$field" "$expected" "${got:-<absent>}" >&2
        return 1
    fi
    return 0
}

# recompute_repo_name <path> — the upstream derivation, expressed independently
# of the script under test. Mirrors jcodemunch_mcp/storage/git_root.py::
# _local_repo_name at the pinned 1.108.54 EXACTLY:
#     f"{folder_path.name}-{sha1(str(folder_path)).hexdigest()[:8]}"
# over a Path(p).expanduser().resolve()'d root. Written in python3 rather than
# sha1sum on purpose: the script uses sha1sum, so a sha1sum-based check here
# would share its method and could agree with it while both were wrong.
recompute_repo_name() {
    python3 -c '
import hashlib, pathlib, sys
p = pathlib.Path(sys.argv[1]).expanduser().resolve()
print(f"{p.name}-{hashlib.sha1(str(p).encode()).hexdigest()[:8]}")
' "$1"
}

# Two-root checks, written as functions rather than inline `bash -c` bodies:
# assert() runs "$@" in THIS shell, so a function sees jc_field/recompute_repo_
# name directly, whereas a `bash -c` subshell would not inherit them (and would
# silently evaluate an empty command substitution instead of failing loudly).
check_distinct_ids() {
    local a b
    a="$(jc_field repo-id --check-only --project-root "$1")"
    b="$(jc_field repo-id --check-only --project-root "$2")"
    if [ -z "$a" ] || [ -z "$b" ]; then
        printf 'one or both repo-ids were absent: %q / %q\n' "$a" "$b" >&2
        return 1
    fi
    if [ "$a" = "$b" ]; then
        printf 'two distinct roots produced the SAME repo-id %s (hardcoded?)\n' "$a" >&2
        return 1
    fi
    return 0
}

check_db_path_honours_code_index_path() {
    local root="$1" index_path="$2" got want
    got="$(CODE_INDEX_PATH="$index_path" jc_field db-path --check-only --project-root "$root")"
    want="$index_path/local-$(recompute_repo_name "$root").db"
    if [ "$got" != "$want" ]; then
        printf 'db-path: expected %s\n              got %s\n' "$want" "${got:-<absent>}" >&2
        return 1
    fi
    return 0
}

echo "=== jcodemunch-index-reify.sh index-primitive tests ==="

# -- Test 1: file exists + is executable -------------------------------------
echo ""
echo "--- Test 1: scripts/jcodemunch-index-reify.sh exists and is executable ---"

assert "scripts/jcodemunch-index-reify.sh exists" \
    test -f "$JC_INDEX"

assert "scripts/jcodemunch-index-reify.sh is executable" \
    test -x "$JC_INDEX"

# -- Test 2: shebang and strict-mode flags -----------------------------------
echo ""
echo "--- Test 2: shebang and 'set -euo pipefail' ---"

assert "scripts/jcodemunch-index-reify.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$JC_INDEX' | grep -qE '^#!/usr/bin/env bash$'"

assert "scripts/jcodemunch-index-reify.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$JC_INDEX"

assert "scripts/jcodemunch-index-reify.sh passes 'bash -n' syntax check" \
    bash -n "$JC_INDEX"

# -- Test 3: identity resolution is a FUNCTION of the project root ------------
# Index identity is per-path and derived (storage/git_root.py::_local_repo_name,
# byte-identical at the pinned 1.108.54):
#     local/<basename>-<sha1(resolved abspath)[:8]>
# and the DB slug is f"{owner}-{name}" (IndexStore._repo_slug), i.e.
#     ${CODE_INDEX_PATH:-$HOME/.code-index}/local-<basename>-<sha1[:8]>.db
#
# Pure-function assertions: hermetic, no network, no indexer, no DB needed.
echo ""
echo "--- Test 3: repo-id / db-path identity resolution ---"

assert "resolves the canonical checkout to the PRODUCTION identity $CANONICAL_REPO_ID" \
    assert_field repo-id "$CANONICAL_REPO_ID" --check-only --project-root "$CANONICAL_ROOT"

assert "canonical db-path is \$HOME/.code-index/local-reify-4ae45bbd.db by default" \
    assert_field db-path "$HOME/.code-index/local-reify-4ae45bbd.db" \
        --check-only --project-root "$CANONICAL_ROOT"

# THE DEFAULT MUST BE THE CANONICAL CHECKOUT, NOT THE INVOKING WORKTREE.
# This suite runs from a warm-lane worktree, so a script that defaulted to its
# own cwd/repo-root would mint a lane-private index (local/_lane-NN-…) and leave
# the production identity untouched — silently reproducing PRD §2.2's "103
# per-agent worktree indexes, no reify index". Asserted from THIS lane's cwd.
assert "with NO --project-root, defaults to the canonical checkout (not this worktree)" \
    assert_field repo-id "$CANONICAL_REPO_ID" --check-only

TMP_ROOT_A="$(mk_tmpdir)"
TMP_ROOT_B="$(mk_tmpdir)"

if [ -z "$TMP_ROOT_A" ] || [ -z "$TMP_ROOT_B" ]; then
    echo "  FAIL: could not mktemp -d two project roots for the identity checks"
    FAIL=$((FAIL + 1))
else
    # A hardcoded local/reify-4ae45bbd would pass all three asserts above. These
    # prove the id is genuinely DERIVED: recomputed independently in python3
    # from the upstream expression, for a path that did not exist until now.
    assert "derives a temp root's repo-id from its path (independently recomputed)" \
        assert_field repo-id "local/$(recompute_repo_name "$TMP_ROOT_A")" \
            --check-only --project-root "$TMP_ROOT_A"

    assert "two different temp roots derive two DIFFERENT repo-ids" \
        check_distinct_ids "$TMP_ROOT_A" "$TMP_ROOT_B"

    # The DB lives under CODE_INDEX_PATH (default $HOME/.code-index). Honouring
    # the override is what lets Blocks 5/7 below drive the DB gates against
    # synthetic fixtures instead of the host-global index.
    assert "db-path honours a temp CODE_INDEX_PATH" \
        check_db_path_honours_code_index_path "$TMP_ROOT_A" "$TMP_ROOT_B"
fi

test_summary
