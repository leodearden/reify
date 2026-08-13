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

# mk_config_jsonc <index_dir> <cap|-> — write a jcodemunch config.jsonc, with
# `//` comments present. The live ~/.code-index/config.jsonc is a .jsonc file,
# so the cap parse must tolerate them; a `-` cap omits the max_folder_files key
# entirely, exercising the key-absent fallback.
mk_config_jsonc() {
    local dir="$1" cap="$2"
    mkdir -p "$dir"
    {
        echo '// jcodemunch index config (JSONC — line comments are legal here)'
        echo '{'
        [ "$cap" = "-" ] || echo "  \"max_folder_files\": $cap,   // the walker's per-folder cap"
        echo '  "staleness_days": 3'
        echo '}'
    } > "$dir/config.jsonc"
}

# with_cap <index_dir> <cap> <checker> [args...] — with_index_path plus an
# exported JCODEMUNCH_MAX_FOLDER_FILES (the real env mapping at config.py:26).
with_cap() {
    local dir="$1" cap="$2" rc=0; shift 2
    local had="${JCODEMUNCH_MAX_FOLDER_FILES+set}" prev="${JCODEMUNCH_MAX_FOLDER_FILES:-}"
    export JCODEMUNCH_MAX_FOLDER_FILES="$cap"
    with_index_path "$dir" "$@" || rc=$?
    if [ "$had" = "set" ]; then
        export JCODEMUNCH_MAX_FOLDER_FILES="$prev"
    else
        unset JCODEMUNCH_MAX_FOLDER_FILES
    fi
    return "$rc"
}

# expect_truncation <indexed> <cap> [args...] — an E_JC_INDEX_TRUNCATED refusal
# whose message names BOTH numbers, so an operator reading it can tell how far
# over the cap the repo is without re-deriving anything.
expect_truncation() {
    local indexed="$1" cap="$2"; shift 2
    expect_refusal E_JC_INDEX_TRUNCATED "$@" || return 1
    local e="$SCRATCH/refusal.err" bad=0
    grep -qF -- "indexed=$indexed" "$e" || { printf 'stderr did not name indexed=%s\n' "$indexed" >&2; bad=1; }
    grep -qF -- "cap=$cap"         "$e" || { printf 'stderr did not name cap=%s\n' "$cap" >&2; bad=1; }
    [ "$bad" -eq 0 ] || { cat "$e" >&2; return 1; }
    return 0
}

# ── Invocation-contract helpers (Block 9) ───────────────────────────────────
#
# These bind to the CONSTRUCTED argv, not to grepped prose. A script whose
# header documented `--once` while its command omitted it would pass a prose
# grep and fail every assertion here — which is the whole point (PRD §2.4).
#
# dry_run_argv <root> — the argv from the single --dry-run exec line.
dry_run_argv() {
    "$JC_INDEX" --dry-run --project-root "$1" 2>/dev/null \
        | sed -n 's/^jcodemunch-index-reify: exec[[:space:]]\{1,\}//p'
}

check_dry_run_shape() {
    local root="$1" o="$SCRATCH/dry.out" e="$SCRATCH/dry.err" rc=0 n
    "$JC_INDEX" --dry-run --project-root "$root" >"$o" 2>"$e" || rc=$?
    if [ "$rc" -ne 0 ]; then
        printf -- '--dry-run exited %d (expected 0)\n' "$rc" >&2
        cat "$e" >&2
        return 1
    fi
    n="$(grep -c '^jcodemunch-index-reify: exec ' "$o" || true)"
    if [ "$n" -ne 1 ]; then
        printf -- 'expected exactly ONE exec line, got %s\n' "$n" >&2
        cat "$o" >&2
        return 1
    fi
    # --dry-run must print the command and stop — never reach the index gates.
    if grep -q -- "$SUCCESS_MARKER" "$o"; then
        printf -- '--dry-run printed %s; it must not run the index gates\n' "$SUCCESS_MARKER" >&2
        return 1
    fi
    return 0
}

argv_has() {
    local argv; argv="$(dry_run_argv "$1")"
    case "$argv" in
        *"$2"*) return 0 ;;
        *) printf 'constructed argv lacks %s:\n  %s\n' "$2" "${argv:-<no exec line>}" >&2; return 1 ;;
    esac
}

# The two BAN checkers below must refuse an EMPTY argv rather than pass on it.
# A negative assertion over nothing is satisfied by nothing: without this, a
# --dry-run that stopped emitting an exec line at all would leave both bans
# reporting green forever — the dead-instrument shape this suite exists to
# avoid. (check_dry_run_shape also catches that, but a guard that depends on a
# sibling assert to not be vacuous is one refactor away from being vacuous.)
argv_lacks() {
    local argv; argv="$(dry_run_argv "$1")"
    if [ -z "$argv" ]; then
        printf 'no exec line to check the %s ban against (the assertion would be vacuous)\n' "$2" >&2
        return 1
    fi
    case "$argv" in
        *"$2"*) printf 'constructed argv CONTAINS the banned %s:\n  %s\n' "$2" "$argv" >&2; return 1 ;;
        *) return 0 ;;
    esac
}

# argv_word_absent <root> <word> — no argv WORD equals <word>. Word-wise, not
# substring: the fixture root is a /tmp/jc-index-reify-XXXXXX path that
# CONTAINS "index" as a substring, so a substring test for the banned `index`
# subcommand would false-fire on the very path being indexed.
argv_word_absent() {
    local root="$1" word="$2" w argv
    argv="$(dry_run_argv "$root")"
    if [ -z "$argv" ]; then
        printf 'no exec line to check the %s ban against (the assertion would be vacuous)\n' "$word" >&2
        return 1
    fi
    for w in $argv; do
        if [ "$w" = "$word" ]; then
            printf 'constructed argv uses the banned word %s:\n  %s\n' "$word" "$(dry_run_argv "$root")" >&2
            return 1
        fi
    done
    return 0
}

# argv_subcommand_is <root> <expected> — the token immediately after the
# `jcodemunch-mcp` entry point is the subcommand.
argv_subcommand_is() {
    local root="$1" expected="$2" prev="" w got=""
    for w in $(dry_run_argv "$root"); do
        if [ "$prev" = "jcodemunch-mcp" ]; then got="$w"; break; fi
        prev="$w"
    done
    if [ "$got" != "$expected" ]; then
        printf 'subcommand after jcodemunch-mcp: expected %s, got %s\n  %s\n' \
            "$expected" "${got:-<none>}" "$(dry_run_argv "$root")" >&2
        return 1
    fi
    return 0
}

# The ban must be DOCUMENTED and never INVOKED: every occurrence of the literal
# in the script is a comment line, and there is at least one (an absence is not
# a ban — it is a ban nobody recorded, which the next editor re-introduces).
check_paths_from_only_in_comments() {
    local hits bad
    hits="$(grep -n -- '--paths-from' "$JC_INDEX" || true)"
    if [ -z "$hits" ]; then
        printf 'the --paths-from ban is not documented anywhere in %s\n' "$JC_INDEX" >&2
        return 1
    fi
    bad="$(printf '%s\n' "$hits" | grep -vE '^[0-9]+:[[:space:]]*#' || true)"
    if [ -n "$bad" ]; then
        printf 'these --paths-from occurrences are NOT comment lines:\n%s\n' "$bad" >&2
        return 1
    fi
    return 0
}

# ── Stub indexer (Block 11) ─────────────────────────────────────────────────
#
# Reproduces the REAL `watch --once` output shapes at the pinned 1.108.54. The
# one-shot path is watcher.py::sync_folders (:878-935), which prints to STDERR:
#
#     Syncing <folder>...
#       <folder>: <msg> (<duration>s)
#
# where msg = result.get("message", f"{result.get('symbol_count','?')} symbols").
# index_folder sets "message": "No changes detected" on a no-op (:1303, :1372,
# :1755) and OMITS "message" entirely on both changed branches (:1456-1466,
# :1859-1870), so those two lines are the only two real shapes.
#
# mk_stub_indexer <path> <no-changes|changed|fail>
mk_stub_indexer() {
    local path="$1" mode="$2"
    cat > "$path" <<'STUB'
#!/usr/bin/env bash
# Test stub: argv is `watch <root> --once --no-ai-summaries`.
root="$2"
echo "Syncing $root..." >&2
STUB
    case "$mode" in
        no-changes) echo 'echo "  $root: No changes detected (0.4s)" >&2; exit 0' >> "$path" ;;
        changed)    echo 'echo "  $root: 54233 symbols (77.8s)" >&2; exit 0' >> "$path" ;;
        fail)       echo 'echo "  $root: boom" >&2; exit 3' >> "$path" ;;
        *) echo "mk_stub_indexer: bad mode $mode" >&2; return 1 ;;
    esac
    chmod +x "$path"
}

# with_stub <index_dir> <stub> <checker> [args...]
with_stub() {
    local dir="$1" stub="$2" rc=0; shift 2
    local had="${REIFY_JC_INDEXER_CMD+set}" prev="${REIFY_JC_INDEXER_CMD:-}"
    export REIFY_JC_INDEXER_CMD="$stub"
    with_index_path "$dir" "$@" || rc=$?
    if [ "$had" = "set" ]; then
        export REIFY_JC_INDEXER_CMD="$prev"
    else
        unset REIFY_JC_INDEXER_CMD
    fi
    return "$rc"
}

# expect_ok_stderr <substring> [args...] — expect_ok, plus a substring that must
# appear on STDERR (where all of upstream's --once output goes).
expect_ok_stderr() {
    local want="$1"; shift
    expect_ok "$@" || return 1
    if ! grep -qF -- "$want" "$SCRATCH/ok.err"; then
        printf 'stderr did not contain %s (the indexer output was swallowed)\n' "$want" >&2
        cat "$SCRATCH/ok.err" >&2
        return 1
    fi
    return 0
}

# expect_refusal_absent <marker> <unwanted> [args...] — a refusal that must NOT
# also carry <unwanted>, used to prove a later gate was never reached.
expect_refusal_absent() {
    local marker="$1" unwanted="$2"; shift 2
    expect_refusal "$marker" "$@" || return 1
    if grep -qF -- "$unwanted" "$SCRATCH/refusal.out" "$SCRATCH/refusal.err"; then
        printf 'the run-failure path still reached %s\n' "$unwanted" >&2
        return 1
    fi
    return 0
}

# check_no_upstream_changed_token <file> — THE G6 NEGATIVE ASSERTION.
#
# Re-verified against the PINNED 1.108.54: `watch --once` runs sync_folders,
# which emits NEITHER `changed=N` NOR `changed: N`. That summary line
# (watcher.py:305) belongs to the CONTINUOUS watch loop's re-index callback, so
# a consumer bound to it would be bound to a string this code path never prints
# — the PRD's `changed: 0` and the decompose-time pin's `changed=0` are BOTH
# wrong here. The only upstream token either file may match on this path is the
# literal `No changes detected`.
#
# The two forbidden patterns are assembled from fragments so this file does not
# itself contain the literal it forbids — otherwise the check would flag its own
# source and could only be satisfied by exempting itself.
UPSTREAM_TOKEN_EQ="chang""ed="
UPSTREAM_TOKEN_COLON="chang""ed:"
check_no_upstream_changed_token() {
    local f="$1" hits
    hits="$(grep -n -e "$UPSTREAM_TOKEN_EQ" -e "$UPSTREAM_TOKEN_COLON" "$f" || true)"
    if [ -n "$hits" ]; then
        printf '%s binds to an upstream token the watch --once path never emits:\n%s\n' "$f" "$hits" >&2
        return 1
    fi
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

# -- Test 7: max_folder_files truncation guard -------------------------------
# G7 INV-SF-3: declared intent (index the repo) silently going unconsumed (most
# of it dropped). At the pinned 1.108.54 the walker truncates when
# len(files) > max_files (index_folder.py:856-857) and then keeps EXACTLY
# max_files of them — and the cap warning it raises is written into
# index_folder's RESULT DICT (:2220-2230), which watcher.py::sync_folders never
# prints. So on the `watch --once` path the truncation is COMPLETELY SILENT: no
# stdout/stderr scrape can detect it, and the guard must be a post-run DB
# assertion. `indexed >= cap` is the exact signature and cannot false-fire below
# the cap.
#
# The stakes are not hypothetical: the package DEFAULT is 2000 (config.py:284).
# The 10000 that makes reify fit is host state in ~/.code-index/config.jsonc,
# outside this repo — if that file were ever reset, reify's ~3,870 tracked files
# (plus untracked non-ignored ones) would be silently truncated, which is
# precisely the failure this guard exists to catch.
echo ""
echo "--- Test 7: effective-cap resolution and the truncation guard ---"

CAP_ROOT="$(mk_tmpdir)"
CAP_INDEX="$(mk_tmpdir)"

if [ -z "$CAP_ROOT" ] || [ -z "$CAP_INDEX" ]; then
    echo "  FAIL: could not mktemp -d the truncation-guard fixtures"
    FAIL=$((FAIL + 1))
else
    # (i) Effective-cap precedence: env > config.jsonc > package default.
    rm -f "$CAP_INDEX/config.jsonc"
    assert "cap falls back to the package default 2000 when nothing sets it" \
        with_index_path "$CAP_INDEX" \
            assert_field file-cap "2000 (package default)" --check-only --project-root "$CAP_ROOT"

    mk_config_jsonc "$CAP_INDEX" 4321
    assert "cap reads max_folder_files from config.jsonc, tolerating // comments" \
        with_index_path "$CAP_INDEX" \
            assert_field file-cap "4321 (config.jsonc)" --check-only --project-root "$CAP_ROOT"

    assert "env JCODEMUNCH_MAX_FOLDER_FILES OVERRIDES config.jsonc" \
        with_cap "$CAP_INDEX" 10 \
            assert_field file-cap "10 (env JCODEMUNCH_MAX_FOLDER_FILES)" \
                --check-only --project-root "$CAP_ROOT"

    # A config.jsonc that exists but omits the key is not a cap of 0 — it falls
    # through to the package default, per jq's `// empty`.
    mk_config_jsonc "$CAP_INDEX" -
    assert "a config.jsonc WITHOUT max_folder_files falls through to the default" \
        with_index_path "$CAP_INDEX" \
            assert_field file-cap "2000 (package default)" --check-only --project-root "$CAP_ROOT"

    rm -f "$CAP_INDEX/config.jsonc"

    # (ii) The post-run DB assertion itself, at the exact >= boundary.
    mk_index_db "$CAP_INDEX" "$CAP_ROOT" 5 9 >/dev/null
    assert "9 indexed files under a cap of 10 is accepted (no false fire below the cap)" \
        with_cap "$CAP_INDEX" 10 \
            expect_ok --check-only --project-root "$CAP_ROOT"

    # indexed == cap is the precise truncation signature: the walker keeps
    # exactly max_files, so landing ON the cap means files were dropped.
    mk_index_db "$CAP_INDEX" "$CAP_ROOT" 5 10 >/dev/null
    assert "10 indexed files under a cap of 10 refuses E_JC_INDEX_TRUNCATED" \
        with_cap "$CAP_INDEX" 10 \
            expect_truncation 10 10 --check-only --project-root "$CAP_ROOT"

    # Over the cap: the two numbers now DIFFER, so this is what proves the
    # message names the real indexed count and the real cap rather than one
    # value twice — and that the comparison is >=, not ==.
    mk_index_db "$CAP_INDEX" "$CAP_ROOT" 5 12 >/dev/null
    assert "12 indexed files under a cap of 10 refuses, naming indexed=12 and cap=10" \
        with_cap "$CAP_INDEX" 10 \
            expect_truncation 12 10 --check-only --project-root "$CAP_ROOT"
fi

# -- Test 9: the invocation contract, bound to the CONSTRUCTED argv -----------
# Every assertion here reads the command the script would actually exec, not
# its prose. A header that documented `--once` while the command omitted it
# would sail through a grep-the-script guard and fail every check below.
echo ""
echo "--- Test 9: constructed indexer argv (pin / watch --once, and the bans) ---"

ARGV_ROOT="$(mk_tmpdir)"

if [ -z "$ARGV_ROOT" ]; then
    echo "  FAIL: could not mktemp -d the invocation-contract fixture"
    FAIL=$((FAIL + 1))
else
    assert "--dry-run exits 0 and prints exactly ONE exec line, running no gates" \
        check_dry_run_shape "$ARGV_ROOT"

    # PRD §8: 1.108.27 is no longer on PyPI, so the pin is 1.108.54. An
    # unpinned invocation would silently follow upstream into a version whose
    # flags and schema this script has not been verified against.
    assert "argv pins jcodemunch-mcp==1.108.54" \
        argv_has "$ARGV_ROOT" "jcodemunch-mcp==1.108.54"

    # All three flags re-verified on the `watch` subparser at 1.108.54
    # (server.py:6326-6369).
    assert "argv uses the 'watch' subcommand" \
        argv_subcommand_is "$ARGV_ROOT" watch

    assert "argv carries --once (one bounded pass, not a daemon)" \
        argv_has "$ARGV_ROOT" --once

    assert "argv carries --no-ai-summaries" \
        argv_has "$ARGV_ROOT" --no-ai-summaries

    assert "argv names the resolved project root" \
        argv_has "$ARGV_ROOT" "$ARGV_ROOT"

    # THE NEGATIVE HALF. PRD §4.4: --paths-from short-circuits discovery
    # (index_folder.py:1505-1511) and then DELETEs every previously-indexed file
    # absent from the list (sqlite_store.py:1698, :1480-1483) — with no warning.
    # It is only reachable on the `index` subparser (server.py:6505), never on
    # `watch`, so banning the subcommand and the flag are two halves of one
    # guarantee and both are asserted.
    assert "argv NEVER contains --paths-from (PRD §4.4: it silently deletes rows)" \
        argv_lacks "$ARGV_ROOT" --paths-from

    assert "argv NEVER uses the 'index' subcommand (the only path to --paths-from)" \
        argv_word_absent "$ARGV_ROOT" index

    # Belt and braces: the ban is recorded in the script, and every occurrence
    # of the literal there is a comment — documented, never invoked.
    assert "the --paths-from ban is documented in the script and only in comments" \
        check_paths_from_only_in_comments
fi

# -- Test 11: run summary and exit propagation -------------------------------
# Driven by a stub indexer through the REIFY_JC_INDEXER_CMD seam, so the two
# REAL 1.108.54 `watch --once` stderr shapes are exercised offline.
echo ""
echo "--- Test 11: run summary, changed-file report, exit propagation ---"

RUN_ROOT="$(mk_tmpdir)"
RUN_INDEX="$(mk_tmpdir)"
STUB_DIR="$(mk_tmpdir)"

if [ -z "$RUN_ROOT" ] || [ -z "$RUN_INDEX" ] || [ -z "$STUB_DIR" ]; then
    echo "  FAIL: could not mktemp -d the run-summary fixtures"
    FAIL=$((FAIL + 1))
else
    mk_stub_indexer "$STUB_DIR/no-changes" no-changes
    mk_stub_indexer "$STUB_DIR/changed"    changed
    mk_stub_indexer "$STUB_DIR/fail"       fail

    # (a) The no-op shape. This is the task's user-observable signal: an
    # immediate SECOND run reports zero changed files.
    mk_index_db "$RUN_INDEX" "$RUN_ROOT" 12 3 >/dev/null
    assert "a 'No changes detected' run exits 0 and reports jc-changed-files=0" \
        with_stub "$RUN_INDEX" "$STUB_DIR/no-changes" \
            expect_ok_stdout "jc-changed-files=0" --project-root "$RUN_ROOT"

    # The child's stderr is the ONLY place upstream says anything on this path,
    # so it must reach the operator rather than being swallowed by the capture.
    assert "the indexer's stderr is passed through to the operator" \
        with_stub "$RUN_INDEX" "$STUB_DIR/no-changes" \
            expect_ok_stderr "No changes detected" --project-root "$RUN_ROOT"

    # (b) The changed shape. `message` is omitted on both changed branches, so
    # msg falls back to "<symbol_count> symbols" — no "No changes detected".
    assert "a '54233 symbols' run exits 0 and reports jc-changed-files=some" \
        with_stub "$RUN_INDEX" "$STUB_DIR/changed" \
            expect_ok_stdout "jc-changed-files=some" --project-root "$RUN_ROOT"

    # A successful run does NOT excuse the DB gate: the count still decides.
    assert "the changed run still reports the DB's true symbol count (12 sym)" \
        with_stub "$RUN_INDEX" "$STUB_DIR/changed" \
            expect_ok_stdout "12 sym" --project-root "$RUN_ROOT"

    mk_index_db "$RUN_INDEX" "$RUN_ROOT" 0 3 >/dev/null
    assert "a successful run over an EMPTY index still refuses E_JC_INDEX_EMPTY" \
        with_stub "$RUN_INDEX" "$STUB_DIR/changed" \
            expect_refusal E_JC_INDEX_EMPTY --project-root "$RUN_ROOT"

    # (c) Exit propagation. The DB here is deliberately EMPTY, so if the script
    # fell through to the symbol gate it would emit E_JC_INDEX_EMPTY — its
    # absence is what proves the run failure returned BEFORE that gate.
    assert "a failing indexer refuses E_JC_INDEX_RUN_FAILED without reaching the symbol gate" \
        with_stub "$RUN_INDEX" "$STUB_DIR/fail" \
            expect_refusal_absent E_JC_INDEX_RUN_FAILED E_JC_INDEX_EMPTY \
                --project-root "$RUN_ROOT"

    # THE G6 NEGATIVE ASSERTION — see check_no_upstream_changed_token.
    assert "the script binds to no upstream token the watch --once path never emits" \
        check_no_upstream_changed_token "$JC_INDEX"

    assert "this test binds to no upstream token the watch --once path never emits" \
        check_no_upstream_changed_token "$SCRIPT_DIR/test_jcodemunch_index_reify.sh"

    # The changed-file report is the script's OWN token. `No changes detected`
    # is the one upstream marker this path may legitimately match.
    assert "the script's changed-file report uses its own jc-changed-files token" \
        grep -q 'jc-changed-files=' "$JC_INDEX"

    assert "the only upstream marker the script matches is 'No changes detected'" \
        grep -q 'No changes detected' "$JC_INDEX"
fi

test_summary
