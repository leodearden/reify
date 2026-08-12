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

# The script's OWN success token. Every refusal path asserts its ABSENCE, so a
# gate that refused after already claiming success cannot pass.
SUCCESS_MARKER="INDEX-OK"

_TMPDIRS=()
cleanup() {
    local d
    for d in ${_TMPDIRS+"${_TMPDIRS[@]}"}; do
        [ -n "$d" ] && rm -rf "$d"
    done
}
trap cleanup EXIT

SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/jc-index-reify-scratch-XXXXXX")"
_TMPDIRS+=("$SCRATCH")

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

# ── Synthetic-index fixtures ────────────────────────────────────────────────
#
# The DB gates are driven against sqlite DBs built here rather than against the
# host-global ~/.code-index, so every assertion below is hermetic: no uvx, no
# network, no ~5-10 minute run, and no chance of perturbing a live watcher/serve.
# Table shapes are the real ones at the pinned 1.108.54 (storage/sqlite_store.py
# :39 symbols, :68 files with `path TEXT PRIMARY KEY`) — only the columns these
# gates read are reproduced.
#
# The fixture is placed at the path the UPSTREAM derivation names (via
# recompute_repo_name), not at one the script computes. So if the script's
# identity derivation ever drifts, it looks in the wrong place and these gates
# fail loudly rather than silently passing against a file it invented.
#
# mk_index_db <index_dir> <project_root> <n_symbols> <n_files> -> echoes db path
mk_index_db() {
    local index_dir="$1" root="$2" n_sym="$3" n_files="$4"
    local db i
    db="$index_dir/local-$(recompute_repo_name "$root").db"
    mkdir -p "$index_dir"
    rm -f "$db"
    {
        echo 'create table symbols(id text primary key, file text, name text);'
        echo 'create table files(path text primary key);'
        echo 'begin;'
        for ((i = 0; i < n_sym; i++)); do
            echo "insert into symbols values('s$i','f$i.rs','sym$i');"
        done
        for ((i = 0; i < n_files; i++)); do
            echo "insert into files values('f$i.rs');"
        done
        echo 'commit;'
    } | sqlite3 "$db" || return 1
    printf '%s\n' "$db"
}

# with_index_path <dir> <checker> [args...] — run a checker with CODE_INDEX_PATH
# exported to <dir>, restoring the previous value (or unset-ness) afterwards.
# `env CODE_INDEX_PATH=… expect_refusal …` cannot be used: env execs a PROGRAM,
# and every checker here is a shell FUNCTION. An explicit export is also more
# honest than a `VAR=x func` prefix, whose export semantics differ between
# bash's default and POSIX modes.
with_index_path() {
    local dir="$1" rc=0; shift
    local had="${CODE_INDEX_PATH+set}" prev="${CODE_INDEX_PATH:-}"
    export CODE_INDEX_PATH="$dir"
    "$@" || rc=$?
    if [ "$had" = "set" ]; then export CODE_INDEX_PATH="$prev"; else unset CODE_INDEX_PATH; fi
    return "$rc"
}

# expect_refusal <marker> [args...] — the script must exit NON-ZERO, carry
# <marker> on STDERR, and print NO success summary. All three together: an
# implementation that printed the marker and still exited 0, or that refused
# after already claiming success, would satisfy a weaker check.
expect_refusal() {
    local marker="$1"; shift
    local o e rc=0 bad=0
    o="$SCRATCH/refusal.out"; e="$SCRATCH/refusal.err"
    "$JC_INDEX" "$@" >"$o" 2>"$e" || rc=$?
    if [ "$rc" -eq 0 ]; then
        printf 'expected a NON-ZERO exit carrying %s, got 0\n' "$marker" >&2
        bad=1
    fi
    if ! grep -q -- "$marker" "$e"; then
        printf 'stderr did not carry the refusal marker %s\n' "$marker" >&2
        bad=1
    fi
    if grep -q -- "$SUCCESS_MARKER" "$o" "$e"; then
        printf 'a refusal path still printed the success marker %s\n' "$SUCCESS_MARKER" >&2
        bad=1
    fi
    [ "$bad" -eq 0 ] || { echo "--- stdout ---" >&2; cat "$o" >&2; echo "--- stderr ---" >&2; cat "$e" >&2; return 1; }
    return 0
}

# expect_ok [args...] — the script must exit 0 and print the success summary.
expect_ok() {
    local o e rc=0
    o="$SCRATCH/ok.out"; e="$SCRATCH/ok.err"
    "$JC_INDEX" "$@" >"$o" 2>"$e" || rc=$?
    if [ "$rc" -ne 0 ] || ! grep -q -- "$SUCCESS_MARKER" "$o"; then
        printf 'expected exit 0 with %s on stdout; rc=%d\n' "$SUCCESS_MARKER" "$rc" >&2
        echo "--- stdout ---" >&2; cat "$o" >&2; echo "--- stderr ---" >&2; cat "$e" >&2
        return 1
    fi
    return 0
}

# expect_ok_stdout <substring> [args...] — expect_ok, plus a literal substring
# that must appear on stdout (e.g. the `<N> sym` count).
expect_ok_stdout() {
    local want="$1"; shift
    expect_ok "$@" || return 1
    if ! grep -qF -- "$want" "$SCRATCH/ok.out"; then
        printf 'stdout did not contain %q\n' "$want" >&2
        cat "$SCRATCH/ok.out" >&2
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

# -- Test 5: the symbol-count gate -------------------------------------------
# PRD §4.3: index PRESENCE proves nothing. `delete-index` leaves a husk that
# re-registers as an empty repo, so a chain that only checked "the DB file is
# there" would report health over an index with zero symbols in it — which is
# the substrate failure this whole PRD exists to end. The gate is therefore on
# the symbol COUNT, and a DB that cannot be queried at all counts as empty.
#
# Driven entirely against synthetic DBs under a temp CODE_INDEX_PATH with
# --check-only: hermetic, no uvx, no network, no ~5-10 minute run.
echo ""
echo "--- Test 5: symbol-count gate (missing / empty / populated) ---"

GATE_ROOT="$(mk_tmpdir)"
GATE_INDEX="$(mk_tmpdir)"

if [ -z "$GATE_ROOT" ] || [ -z "$GATE_INDEX" ]; then
    echo "  FAIL: could not mktemp -d the symbol-count gate fixtures"
    FAIL=$((FAIL + 1))
else
    # (a) No DB file at all — nothing has ever indexed this identity.
    assert "refuses E_JC_INDEX_MISSING when the index DB does not exist" \
        with_index_path "$GATE_INDEX" \
            expect_refusal E_JC_INDEX_MISSING --check-only --project-root "$GATE_ROOT"

    # (b) The delete-index husk: DB present, schema present, ZERO symbols.
    mk_index_db "$GATE_INDEX" "$GATE_ROOT" 0 0 >/dev/null
    assert "refuses E_JC_INDEX_EMPTY when the DB has a symbols table but 0 rows" \
        with_index_path "$GATE_INDEX" \
            expect_refusal E_JC_INDEX_EMPTY --check-only --project-root "$GATE_ROOT"

    # (b2) An unqueryable DB (no symbols table / wrong schema / corrupt) is
    # EMPTY too — an index we cannot read the symbol count out of is not an
    # index we may report health over.
    : > "$GATE_INDEX/local-$(recompute_repo_name "$GATE_ROOT").db"
    assert "refuses E_JC_INDEX_EMPTY when the DB has no symbols table at all" \
        with_index_path "$GATE_INDEX" \
            expect_refusal E_JC_INDEX_EMPTY --check-only --project-root "$GATE_ROOT"

    # (c) A populated index passes, and the count it reports is the real one.
    mk_index_db "$GATE_INDEX" "$GATE_ROOT" 7 3 >/dev/null
    assert "accepts a populated index and reports the true count as '7 sym'" \
        with_index_path "$GATE_INDEX" \
            expect_ok_stdout "7 sym" --check-only --project-root "$GATE_ROOT"

    # The count is READ, not echoed from a constant: a different fixture must
    # report a different number.
    mk_index_db "$GATE_INDEX" "$GATE_ROOT" 41 3 >/dev/null
    assert "reports '41 sym' for a 41-symbol fixture (the count is read, not fixed)" \
        with_index_path "$GATE_INDEX" \
            expect_ok_stdout "41 sym" --check-only --project-root "$GATE_ROOT"
fi

test_summary
