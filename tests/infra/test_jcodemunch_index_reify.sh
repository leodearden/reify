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

# The ONE run-private root every mk_tmpdir fixture dir is nested under.
# Registered in _TMPDIRS here, in the MAIN shell, exactly once.
_RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/jc-index-reify-root-XXXXXX")"
_TMPDIRS+=("$_RUN_ROOT")

# mk_tmpdir — a fixture dir, echoed on stdout.
#
# SUBSHELL-SAFE BY CONSTRUCTION, and that is the whole reason it nests under
# $_RUN_ROOT rather than appending to _TMPDIRS itself. Every call site reads
# `X="$(mk_tmpdir)"`, so this body runs in a command-substitution SUBSHELL, and
# a `_TMPDIRS+=("$d")` performed here would be silently DISCARDED when that
# subshell exits — leaking every fixture dir this suite ever mints (measured:
# 1476 leaked `jc-index-reify-*` dirs on this host before this fix; see
# test_with_jcodemunch_serve.sh's mk_tmpdir for the sibling fix this mirrors).
# Anchoring cleanup on the one root means the existing `rm -rf` reclaims
# everything in one shot — the same reasoning test_helpers.sh's
# make_isolated_lane documents.
mk_tmpdir() {
    mktemp -d "$_RUN_ROOT/fixture-XXXXXX"
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
# t_sqlite3 — the fixture-side mirror of the script's jc_sqlite3.
#
# The fixtures below BUILD sqlite DBs, so they hit the same trap the script does:
# under reify's native-deps LD_LIBRARY_PATH (/opt/reify-deps/lib, which ships
# libsqlite3.so.3.53.1) a dynamically linked system sqlite3 aborts with "SQLite
# header and source version mismatch" before executing any statement. That is
# what turned this suite red in the merge gate on 2026-08-20 while it passed
# locally, where PATH resolved a statically linked sqlite3.
#
# Deliberately a SEPARATE implementation rather than sourcing the script's copy:
# these fixtures must keep working even if the script's helper is the thing under
# test (or is broken), so the guard cannot depend on the artifact it guards.
T_DEPS_LIB="${REIFY_DEPS_LIB:-/opt/reify-deps/lib}"

t_sqlite3() {
    local filtered="" rest="${LD_LIBRARY_PATH:-}" d
    while [ -n "$rest" ]; do
        d="${rest%%:*}"
        if [ "$d" = "$rest" ]; then rest=""; else rest="${rest#*:}"; fi
        if [ -n "$d" ] && [ "${d%/}" != "${T_DEPS_LIB%/}" ]; then
            filtered="${filtered:+$filtered:}$d"
        fi
    done
    if [ -n "$filtered" ]; then
        LD_LIBRARY_PATH="$filtered" sqlite3 "$@"
    else
        env -u LD_LIBRARY_PATH sqlite3 "$@"
    fi
}

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
    } | t_sqlite3 "$db" || return 1
    printf '%s\n' "$db"
}

# mk_origin_repo — a temp project root that is a real git repo carrying an
# `origin` remote, echoed as a path. Used ONLY by Test 14.
#
# WHY THE REMOTE EXISTS, and why this is NOT the shape the ratified description
# bans: the remote is read by the identity-dispatch STUB only, which needs it to
# mimic jcodemunch_mcp/storage/git_root.py:256-265's git branch and produce the
# `leodearden-reify` slug the hijack diagnostic must name. The script under test
# is deliberately GIT-BLIND — its derivation (scripts/jcodemunch-index-reify.sh
# :126-137) is a pure function of the path string — so asserting that the
# SCRIPT's identity moves because a remote is present would pin nothing and is
# correctly forbidden. Both Test 14 assertions bind to the script's OUTPUT, never
# to a claim that the script consulted git. Do not "simplify" this remote away.
#
# `git init` is explicit rather than relying on the ambient tree: without it,
# `git -C <root> config --get remote.origin.url` walks UP and could pick up an
# enclosing repo's origin if TMPDIR ever pointed inside one. No commit and no
# user.name/user.email are needed — `git remote add` and `git config --get` both
# work on a bare `git init`.
mk_origin_repo() {
    local root
    root="$(mk_tmpdir)" || return 1
    [ -n "$root" ] || return 1
    git -C "$root" init -q >/dev/null 2>&1 || return 1
    git -C "$root" remote add origin https://github.com/leodearden/reify.git \
        >/dev/null 2>&1 || return 1
    printf '%s\n' "$root"
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

# with_ld_poison <ld_dir> <deps_dir> <checker> [args...] — run <checker> with
# LD_LIBRARY_PATH pointed at <ld_dir> and REIFY_DEPS_LIB at <deps_dir>, restoring
# both (including unset-ness) afterwards. Same shape as with_index_path above.
#
# Two knobs rather than one because the DIFFERENCE between them is the whole
# test: the script strips exactly $REIFY_DEPS_LIB from LD_LIBRARY_PATH, so
# pointing them at the same dir exercises the fix and pointing them apart
# exercises the unprotected behaviour.
with_ld_poison() {
    local ld="$1" deps="$2" rc=0; shift 2
    local had_ld="${LD_LIBRARY_PATH+set}" prev_ld="${LD_LIBRARY_PATH:-}"
    local had_dp="${REIFY_DEPS_LIB+set}" prev_dp="${REIFY_DEPS_LIB:-}"
    export LD_LIBRARY_PATH="$ld"
    export REIFY_DEPS_LIB="$deps"
    "$@" || rc=$?
    if [ "$had_ld" = "set" ]; then export LD_LIBRARY_PATH="$prev_ld"; else unset LD_LIBRARY_PATH; fi
    if [ "$had_dp" = "set" ]; then export REIFY_DEPS_LIB="$prev_dp"; else unset REIFY_DEPS_LIB; fi
    return "$rc"
}

# require_nonempty <label> <value> — the anti-vacuity guard every substring
# assertion below runs FIRST.
#
# `grep -qF -- ""` matches EVERY line, so an expected substring that arrives
# EMPTY does not fail — it silently degrades its assertion to "the command ran",
# and the test keeps reporting PASS while checking nothing.
#
# Same discipline the argv checkers already apply (argv_lacks / argv_word_absent
# both refuse an empty argv rather than pass vacuously): a dead instrument must
# fail loudly, never read green.
#
# WHAT THIS DOES NOT COVER, measured rather than assumed. It catches a want that
# is empty IN FULL. It cannot catch a want built as a literal prefix PLUS a
# substitution — "repo=local/$(recompute_repo_name …)" stays non-empty as
# "repo=local/" even when the substitution yields nothing, and the script prints
# that prefix unconditionally. Forcing recompute_repo_name to echo nothing was
# measured to leave Test 14(a) PASSING with this guard in place. Any such call
# site must therefore bind and guard its substituted part separately, which is
# why Test 14 hoists SEAM_SLUG into its fixture FAIL arm instead of inlining it.
# It is the only call site of that shape today (checked: it is the only expect_*
# call embedding a command substitution).
require_nonempty() {
    if [ -z "$2" ]; then
        printf 'the expected %s is EMPTY — the assertion would be vacuous (an empty substring matches everything)\n' \
            "$1" >&2
        return 1
    fi
    return 0
}

# expect_refusal <marker> [args...] — the script must exit NON-ZERO, carry
# <marker> on STDERR, and print NO success summary. All three together: an
# implementation that printed the marker and still exited 0, or that refused
# after already claiming success, would satisfy a weaker check.
expect_refusal() {
    local marker="$1"; shift
    require_nonempty 'refusal marker' "$marker" || return 1
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

# mk_real_shaped_config_jsonc <index_dir> <cap|-> — write a config.jsonc that
# reproduces the REAL jcodemunch-mcp 1.108.54 generated template's shape, not
# just the "one strict-JSON object with a couple of trailing `//` comments"
# shape mk_config_jsonc above exercises (task 6486).
#
# Built from jcodemunch_mcp/config.py::generate_template() at the pinned
# 1.108.54 — cited here by module path + version pin rather than by the
# host-local uv wheel cache directory it was originally transcribed from
# (that directory name is a content hash valid only on the machine that
# resolved it, so citing it would not stay resolvable for the next reader).
# Four JSONC shapes the real template carries that mk_config_jsonc above
# carries NONE of:
#   1. A `//`-commented-out "max_folder_files" line (it ships commented out
#      by default — config.py:1826) alongside plenty of OTHER `//` prose.
#   2. A live (uncommented) JSON array — "languages" — whose last element
#      ends with a TRAILING COMMA before the closing `]`, e.g.
#      `"rust",\n  ],`. That comma is legal JSONC but is exactly what made
#      jq fail with "Expected another array element" even AFTER `//`
#      comments were stripped: measured directly against the real template
#      generated from the pinned wheel (tree_sitter_language_pack stubbed
#      out to dodge an unrelated missing native dep; that stub touches only
#      the "languages" array's CONTENTS, not the trailing-comma shape that
#      matters here or the fixed max_folder_files section).
#   3. A NESTED object-of-arrays — "tool_tier_bundles" — mirroring the real
#      template's shape: a trailing comma before each inner array's `]` AND
#      one before the outer object's `}`. Added under task 6486's review
#      pass so nested-depth collapse is ASSERTED rather than assumed — the
#      stripping pipeline is a flat text rewrite with no notion of nesting,
#      but nothing had exercised more than one level of it before this.
#   4. An object whose ENTIRE body is `//` comments — "descriptions" —
#      collapsing to an empty object the same way "meta_fields" above
#      collapses to an empty array, and (placed last) landing its own
#      trailing comma directly before the OUTER closing `}` — a
#      `}`-then-`}` shape the fixture did not previously cover, since
#      "languages" above only ever put a `]` before that final `}`.
#
# `cap` behaves like mk_config_jsonc's: a number makes max_folder_files a
# LIVE (uncommented) key at that value; `-` leaves it commented out, exactly
# like the stock template, so the fallback-to-package-default path is
# exercised against the real shape too.
mk_real_shaped_config_jsonc() {
    local dir="$1" cap="$2"
    mkdir -p "$dir"
    {
        echo '// jcodemunch-mcp configuration'
        echo '// Global: ~/.code-index/config.jsonc'
        echo '// Project: {project_root}/.jcodemunch.jsonc (optional, overrides global)'
        echo '//'
        echo '// All values below show defaults. Uncomment to override.'
        echo '{'
        echo '  // Config version - do not edit. Used for additive migrations.'
        echo '  "version": "1.108.54",'
        echo ''
        echo '  // === Indexing ==='
        if [ "$cap" = "-" ]; then
            echo '  // "max_folder_files": 2000,'
        else
            echo "  \"max_folder_files\": $cap,"
        fi
        echo '  //   Maximum number of files to index when indexing a local folder.'
        echo '  //   Prevents accidental massive indexing jobs.'
        echo ''
        echo '  "meta_fields": ['
        echo '  // "timing_ms",'
        echo '  ],'
        echo ''
        echo '  // === Languages ==='
        echo '  "languages": ['
        echo '     "javascript",'
        echo '  "python",'
        echo '  "rust",'
        echo '  ],'
        echo ''
        echo '  // === Tool tiers (nested object-of-arrays, task 6486) ==='
        echo '  "tool_tier_bundles": {'
        echo '    "fast": ['
        echo '      "ripgrep",'
        echo '      "glob",'
        echo '    ],'
        echo '    "slow": ['
        echo '      "semantic_search",'
        echo '    ],'
        echo '  },'
        echo ''
        echo '  // === Descriptions (all-comment object body, task 6486) ==='
        echo '  "descriptions": {'
        echo '    // "fast": "grep/glob-based literal search",'
        echo '    // "slow": "semantic embedding search",'
        echo '  },'
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
# mk_stub_indexer <path> <no-changes|changed|fail|env-echo|identity-dispatch>
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
        # Reports the identity lever AS THE CHILD ACTUALLY RECEIVED IT, then
        # behaves like `no-changes`. The only mode that can tell "the argv
        # string mentions the var" apart from "the indexer process is really
        # running under it" — the distinction esc-6107-6 turned on.
        env-echo)   echo 'echo "  $root: identity-env=[${JCODEMUNCH_GIT_ROOT_IDENTITY-<unset>}]" >&2; echo "  $root: No changes detected (0.1s)" >&2; exit 0' >> "$path" ;;
        # The ONLY kind that closes the identity loop. Every other kind is
        # identity-blind and lets the SUITE plant the DB at a path it chose;
        # this one mimics jcodemunch_mcp/storage/git_root.py's dispatch at the
        # pinned 1.108.54 — it reads JCODEMUNCH_GIT_ROOT_IDENTITY out of its OWN
        # environment, resolves the identity that lever selects, and writes its
        # index THERE. Test 14 then drives both branches through this one binary.
        #
        # Appended via a quoted heredoc rather than the single-line `echo` the
        # other arms use, simply because the body is multi-line.
        identity-dispatch)
            cat >> "$path" <<'DISPATCH'
# HARD SAFETY PROPERTY: this stub must never be able to write into the real
# host-global ~/.code-index. Refuse to run at all unless redirected.
: "${CODE_INDEX_PATH:?identity-dispatch stub refuses to write to the real host store}"

# Resolve the slug in python3 on purpose — the same reason recompute_repo_name
# exists: the script under test derives with sha1sum, so a sha1sum-based slug
# here could agree with it while both were wrong.
#
# TWO causes reach the same hijack, and the script's own E_JC_INDEX_MISSING NOTE
# names BOTH. This stub models both, so Test 14 can drive each one:
#   (a) the lever never reached the indexer   -> the lever's value decides;
#   (b) a PRE-EXISTING index for this exact source_root short-circuits identity
#       resolution AHEAD of the configured mode — the lever DOES reach the child
#       and is overridden anyway. That is the case the script header flags as
#       "NOT SUFFICIENT ON ITS OWN", and it is the one no fixture drove before:
#       Test 12's mk_foreign_index_db is a DB the suite plants at a slug it
#       chose, with no indexer deciding anything.
slug="$(python3 - "$root" "${JCODEMUNCH_GIT_ROOT_IDENTITY-1}" "$CODE_INDEX_PATH" <<'PY'
import hashlib, pathlib, sqlite3, subprocess, sys

root, lever, store = sys.argv[1], sys.argv[2], sys.argv[3]
p = pathlib.Path(root).expanduser().resolve()

# The LOCAL, per-path branch (git_root.py:55-57) — git is never consulted.
local_slug = f"local-{p.name}-{hashlib.sha1(str(p).encode()).hexdigest()[:8]}"

# The GIT branch (git_root.py:256-265) — <owner>-<name> off origin's URL.
url = subprocess.run(
    ["git", "-C", str(p), "config", "--get", "remote.origin.url"],
    capture_output=True, text=True,
).stdout.strip()
if url.endswith(".git"):
    url = url[:-4]
parts = [seg for seg in url.replace(":", "/").split("/") if seg]
git_slug = "-".join(parts[-2:]) if len(parts) >= 2 else ""


def indexes_this_root(slug):
    """An existing store entry that already claims THIS exact source_root."""
    if not slug:
        return False
    db = pathlib.Path(store) / f"{slug}.db"
    if not db.is_file():
        return False
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        row = conn.execute(
            "select value from meta where key='source_root'"
        ).fetchone()
        conn.close()
    except sqlite3.Error:
        return False
    return bool(row) and row[0] == str(p)


# (1) THE SHORT-CIRCUIT, verified first-hand against the pinned 1.108.54 wheel.
# resolve_index_identity's `requested == "config"` arm returns local_existing,
# then existing_git, and only THEN does `requested = configured` — so an index
# that already claims this source_root decides the identity before the
# configured mode (and hence this lever) can take effect at all. `configured` is
# computed eagerly a few lines earlier, but it is not CONSULTED until that
# assignment. Local outranks git in that same arm. Both present at once raises
# IdentityModeAmbiguous upstream; not modelled, because Test 14 never creates it
# — every assertion uses a fresh index dir carrying at most one planted DB.
for candidate in (local_slug, git_slug):
    if indexes_this_root(candidate):
        print(candidate)
        raise SystemExit(0)

# (2) Only now does the lever matter.
print(local_slug if lever == "0" else git_slug)
PY
)"
if [ -z "$slug" ]; then
    echo "  identity-dispatch stub: could not resolve a slug for $root" >&2
    exit 9
fi

# The real 1.108.54 table shapes (storage/sqlite_store.py:39 symbols, :68 files)
# plus meta.source_root — the key the script's hijack_note matches on, so the
# negative assertion depends on it being written here.
#
# Built with python3's stdlib sqlite3, NOT the sqlite3 CLI: this is a generated
# standalone file that cannot call the suite's t_sqlite3, and routing it through
# the CLI would force a THIRD inline LD_LIBRARY_PATH scrub into the tree. The
# python3 module links its own libsqlite3 and is structurally immune to the
# "SQLite header and source version mismatch" abort that reddened the merge gate
# on 2026-08-20. Exactly 1 file, far under the package-default 2000-file cap, so
# the script's truncation guard cannot fire.
mkdir -p "$CODE_INDEX_PATH"
rm -f "$CODE_INDEX_PATH/$slug.db"
python3 - "$CODE_INDEX_PATH/$slug.db" "$root" <<'PY'
import pathlib, sqlite3, sys

db = sys.argv[1]
root = str(pathlib.Path(sys.argv[2]).expanduser().resolve())
conn = sqlite3.connect(db)
conn.executescript(
    "create table symbols(id text primary key, file text, name text);"
    "create table files(path text primary key);"
    "create table meta(key text primary key, value text);"
)
conn.execute("insert into meta values('source_root',?)", (root,))
conn.execute("insert into symbols values('s0','f0.rs','sym0')")
conn.execute("insert into files values('f0.rs')")
conn.commit()
conn.close()
PY

echo "  $root: 1 symbols (0.1s)" >&2
exit 0
DISPATCH
            ;;
        *) echo "mk_stub_indexer: bad mode $mode" >&2; return 1 ;;
    esac
    chmod +x "$path"
}

# mk_foreign_index_db <index_dir> <slug> <source_root> — a DB at a DIFFERENT
# identity than the one this script predicts, carrying `meta.source_root` for
# <source_root>. Reproduces the esc-6107-6 shape: the indexer ran, succeeded,
# and wrote under the identity JCODEMUNCH resolved rather than the one this
# script derived.
mk_foreign_index_db() {
    local index_dir="$1" slug="$2" root="$3" db
    db="$index_dir/$slug.db"
    mkdir -p "$index_dir"
    rm -f "$db"
    {
        echo 'create table symbols(id text primary key, file text, name text);'
        echo 'create table files(path text primary key);'
        echo 'create table meta(key text primary key, value text);'
        echo "insert into meta values('source_root','$root');"
        echo "insert into symbols values('s0','f0.rs','sym0');"
        echo "insert into files values('f0.rs');"
    } | t_sqlite3 "$db" || return 1
    printf '%s\n' "$db"
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
    require_nonempty 'stderr substring' "$want" || return 1
    expect_ok "$@" || return 1
    if ! grep -qF -- "$want" "$SCRATCH/ok.err"; then
        printf 'stderr did not contain %s (the indexer output was swallowed)\n' "$want" >&2
        cat "$SCRATCH/ok.err" >&2
        return 1
    fi
    return 0
}

# expect_refusal_names <substring> [args...] — an E_JC_INDEX_MISSING refusal
# whose message also carries <substring>. Kept separate from the marker
# assertion so a refusal that is correct but UNDIAGNOSTIC fails loudly rather
# than passing on the marker alone.
expect_refusal_names() {
    local want="$1"; shift
    require_nonempty 'substring the refusal must name' "$want" || return 1
    expect_refusal E_JC_INDEX_MISSING "$@" || return 1
    if ! grep -qF -- "$want" "$SCRATCH/refusal.err"; then
        printf 'the refusal did not name %s\n' "$want" >&2
        cat "$SCRATCH/refusal.err" >&2
        return 1
    fi
    return 0
}

# expect_refusal_lacks <substring> [args...] — the negative control for the
# above: an E_JC_INDEX_MISSING refusal that must NOT carry <substring>.
expect_refusal_lacks() {
    local unwanted="$1"; shift
    require_nonempty 'substring the refusal must NOT carry' "$unwanted" || return 1
    expect_refusal E_JC_INDEX_MISSING "$@" || return 1
    if grep -qF -- "$unwanted" "$SCRATCH/refusal.err"; then
        printf 'the refusal carried %s when it should not have\n' "$unwanted" >&2
        cat "$SCRATCH/refusal.err" >&2
        return 1
    fi
    return 0
}

# expect_refusal_absent <marker> <unwanted> [args...] — a refusal that must NOT
# also carry <unwanted>, used to prove a later gate was never reached.
expect_refusal_absent() {
    local marker="$1" unwanted="$2"; shift 2
    require_nonempty 'substring the refusal must NOT carry' "$unwanted" || return 1
    expect_refusal "$marker" "$@" || return 1
    if grep -qF -- "$unwanted" "$SCRATCH/refusal.out" "$SCRATCH/refusal.err"; then
        printf 'the run-failure path still reached %s\n' "$unwanted" >&2
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
    require_nonempty 'stdout substring' "$want" || return 1
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

    # (i-b) task 6486: the REAL jcodemunch-generated config.jsonc, not just a
    # synthetic strict-JSON-plus-comments fixture. `//` comment stripping
    # alone is not sufficient — the real template's live "languages" array
    # ends in a trailing comma before `]`, which jq rejects even after
    # comments are gone ("Expected another array element"). Both arms below
    # must not hit the "cannot parse" refusal.
    mk_real_shaped_config_jsonc "$CAP_INDEX" -
    assert "parses the real-template shape (trailing-comma array); falls through to default when max_folder_files is commented out, as the stock template ships it" \
        with_index_path "$CAP_INDEX" \
            assert_field file-cap "2000 (package default)" --check-only --project-root "$CAP_ROOT"

    mk_real_shaped_config_jsonc "$CAP_INDEX" 9999
    assert "parses the real-template shape (trailing-comma array) and reads a LIVE max_folder_files out of it" \
        with_index_path "$CAP_INDEX" \
            assert_field file-cap "9999 (config.jsonc)" --check-only --project-root "$CAP_ROOT"

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

    # THE IDENTITY LEVER (esc-6107-6/-7). At 1.108.54 `git_root_identity`
    # DEFAULTS TO TRUE (config.py:384), so without this var jcodemunch resolves
    # the canonical checkout to the git identity `leodearden/reify` and every
    # gate below interrogates a per-path DB nothing will ever write. Asserted on
    # the CONSTRUCTED argv so --dry-run stays a faithful reproduction; Test 12
    # asserts the child process actually receives it.
    assert "argv carries JCODEMUNCH_GIT_ROOT_IDENTITY=0 (forces the per-path identity)" \
        argv_has "$ARGV_ROOT" "JCODEMUNCH_GIT_ROOT_IDENTITY=0"

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
fi

# -- Test 12: identity lever and the hijack diagnostic -----------------------
#
# THE HOLE THIS CLOSES. Every other DB gate in this suite drives a SYNTHETIC db
# the suite itself creates at the predicted path, so the suite was green while
# the identity premise was false — it could not, by construction, observe which
# identity jcodemunch actually resolves. Both assertions here are about that
# seam: one that the lever reaches the real child process, one that a refusal
# caused by the identity landing elsewhere says so instead of reading as
# "nothing has ever indexed this repo".
echo ""
echo "--- Test 12: identity lever reaches the child; hijack refusal is diagnostic ---"

ID_ROOT="$(mk_tmpdir)"
ID_INDEX="$(mk_tmpdir)"

if [ -z "$ID_ROOT" ] || [ -z "$ID_INDEX" ]; then
    echo "  FAIL: could not mktemp -d the identity fixtures"
    FAIL=$((FAIL + 1))
else
    mk_stub_indexer "$STUB_DIR/env-echo" env-echo
    mk_index_db "$ID_INDEX" "$ID_ROOT" 7 2 >/dev/null

    # The behavioural half: the value is read out of the CHILD's environment,
    # not out of the argv string the parent printed.
    assert "the indexer child actually runs with JCODEMUNCH_GIT_ROOT_IDENTITY=0" \
        with_stub "$ID_INDEX" "$STUB_DIR/env-echo" \
            expect_ok_stderr "identity-env=[0]" --project-root "$ID_ROOT"

    # The diagnostic half. No DB at the predicted path, but a foreign-identity
    # DB claiming this exact source_root — precisely the state the canonical
    # checkout was left in on 2026-08-13. --check-only so no indexer runs.
    HIJACK_INDEX="$(mk_tmpdir)"
    if [ -z "$HIJACK_INDEX" ]; then
        echo "  FAIL: could not mktemp -d the hijack fixture"
        FAIL=$((FAIL + 1))
    else
        mk_foreign_index_db "$HIJACK_INDEX" "leodearden-reify" "$ID_ROOT" >/dev/null

        assert "a hijacked identity still refuses E_JC_INDEX_MISSING (never retargets)" \
            with_index_path "$HIJACK_INDEX" \
                expect_refusal E_JC_INDEX_MISSING --check-only --project-root "$ID_ROOT"

        # …and the refusal NAMES the index that stole the identity. Without
        # this, the operator sees "nothing has indexed this identity" moments
        # after the indexer reported success — the exact ambiguity that cost a
        # task cycle.
        assert "the hijack refusal names the foreign index holding this source_root" \
            with_index_path "$HIJACK_INDEX" \
                expect_refusal_names "leodearden-reify.db" \
                    --check-only --project-root "$ID_ROOT"

        # The negative control: with NO foreign index, the note must NOT appear.
        # A diagnostic that fires unconditionally is not a diagnostic.
        CLEAN_INDEX="$(mk_tmpdir)"
        if [ -z "$CLEAN_INDEX" ]; then
            echo "  FAIL: could not mktemp -d the clean-store control"
            FAIL=$((FAIL + 1))
        else
            assert "with no foreign index, the refusal carries NO hijack note" \
                with_index_path "$CLEAN_INDEX" \
                    expect_refusal_lacks "NOTE:" \
                        --check-only --project-root "$ID_ROOT"
        fi
    fi
fi

echo "--- Test 13: sqlite3 is insulated from a shadowing LD_LIBRARY_PATH ---"

# REGRESSION for the 2026-08-20 merge-gate failure.
#
# reify's own tooling PREPENDS /opt/reify-deps/lib to LD_LIBRARY_PATH
# (.cargo/run-with-occt.sh; the merge-verify env mirrors it) and that directory
# ships libsqlite3.so.3.53.1 next to OCCT. A dynamically linked system sqlite3
# then loads the wrong library and aborts BEFORE running any statement:
#
#     SQLite header and source version mismatch
#
# exiting 1 with EMPTY stdout. check_index cannot tell that apart from an
# unreadable index, so it refuses E_JC_INDEX_EMPTY over a PERFECTLY HEALTHY one —
# a false "this index is a husk" on every host whose sqlite3 is dynamically
# linked. This suite went green twice locally while that was true, because the
# local PATH resolved a STATICALLY linked sqlite3 that ignores LD_LIBRARY_PATH.
#
# The fixture is hermetic: it poisons a temp dir with a bogus libsqlite3.so.0 and
# never touches the real /opt/reify-deps.

LD_ROOT="$(mk_tmpdir)"
LD_INDEX="$(mk_tmpdir)"
LD_POISON="$(mk_tmpdir)"

if [ -z "$LD_ROOT" ] || [ -z "$LD_INDEX" ] || [ -z "$LD_POISON" ]; then
    echo "  FAIL: could not mktemp -d the LD_LIBRARY_PATH fixtures"
    FAIL=$((FAIL + 1))
else
    printf 'this is not an ELF shared object' > "$LD_POISON/libsqlite3.so.0"
    # Built BEFORE the poison is applied — t_sqlite3 strips the real deps dir,
    # not this temp one, so fixture creation must happen outside with_ld_poison.
    mk_index_db "$LD_INDEX" "$LD_ROOT" 9 3 >/dev/null

    # POSITIVE CONTROL on the FIXTURE, and the anti-vacuity guard for both
    # assertions below: is the poison actually potent against the sqlite3 THIS
    # host resolves? A statically linked sqlite3 ignores LD_LIBRARY_PATH
    # entirely, and against one both assertions would pass no matter what the
    # script did. Skip loudly rather than bank a vacuous pass.
    if LD_LIBRARY_PATH="$LD_POISON" sqlite3 :memory: 'select 1;' >/dev/null 2>&1; then
        echo "  SKIP: this host's sqlite3 ignores a poisoned LD_LIBRARY_PATH (statically linked), so the shadowing defect cannot be reproduced here and Test 13 would be vacuous"
    else
        # NEGATIVE CONTROL FIRST: with the poisoned dir NOT declared as the deps
        # dir, nothing strips it and the healthy 9-symbol index is misreported as
        # a husk. This is the defect verbatim, and it is what proves the positive
        # assertion below is not passing merely because the poison was inert.
        assert "an unstripped shadowing LD_LIBRARY_PATH misreports a healthy index as E_JC_INDEX_EMPTY" \
            with_ld_poison "$LD_POISON" "$LD_POISON/not-the-deps-dir" \
                with_index_path "$LD_INDEX" \
                    expect_refusal E_JC_INDEX_EMPTY --check-only --project-root "$LD_ROOT"

        # THE FIX: naming that dir as the deps dir makes jc_sqlite3 drop it from
        # the child's LD_LIBRARY_PATH, so sqlite3 resolves its own library and the
        # true count is read.
        assert "stripping the deps dir insulates sqlite3 and the true count is read (9 sym)" \
            with_ld_poison "$LD_POISON" "$LD_POISON" \
                with_index_path "$LD_INDEX" \
                    expect_ok_stdout "9 sym" --check-only --project-root "$LD_ROOT"
    fi
fi

# -- Test 14: the identity SEAM ----------------------------------------------
#
# THE HOLE THIS CLOSES (esc-6107-7). Test 9 pins that the lever is CONSTRUCTED
# into the argv; Test 12 pins that it REACHES THE CHILD's environment. Neither
# closes the LOOP: nothing yet drives an indexer that actually RESOLVES ITS OWN
# IDENTITY from that lever, writes where that resolution lands, and is then
# checked by the script against the identity the script independently predicted.
# Test 12's hijack half is driven by a STATIC mk_foreign_index_db fixture the
# suite plants at a slug it chose — an indexer never decided anything. So the
# seam this whole script exists to hold — "force the per-path identity, then
# verify the one you actually depend on" — was untested.
#
# ONE stub binary takes BOTH branches, decided solely by the lever's value, so
# the two assertions are mutually non-vacuous by construction: neither can pass
# for a reason unrelated to identity dispatch.
#
# HONEST LIMIT, recorded so a later reader does not overclaim this test: a stub
# encodes the TESTER'S MODEL of upstream, so it pins the SEAM, not upstream
# truth. If jcodemunch's own dispatch changes at a pin bump, this test keeps
# passing against the old model. That hazard is deliberately prose-only — the
# PIN-BUMP CHECKLIST comment in the script — with no gate.
echo ""
echo "--- Test 14: the identity SEAM (the script forces the lever, then verifies the identity it depends on) ---"

SEAM_ROOT="$(mk_origin_repo)"
SEAM_INDEX="$(mk_tmpdir)"
# A SECOND, fresh index dir for (b): reusing (a)'s would leave the local-*.db
# that (a) just wrote sitting at the predicted path, and the DB gate would be
# satisfied by it no matter where the negative run's indexer landed.
SEAM_INDEX_2="$(mk_tmpdir)"
# A THIRD for (c), for the same reason, and pre-planted rather than empty.
SEAM_INDEX_3="$(mk_tmpdir)"
# The husk's meta.source_root must be the RESOLVED root: the script resolves
# --project-root with `readlink -f` before comparing (jcodemunch-index-reify.sh
# :130-131), and the stub resolves with pathlib. Planting the raw mktemp path
# would silently miss on any host whose TMPDIR traverses a symlink.
SEAM_ROOT_REAL="$(readlink -f -- "$SEAM_ROOT")"
# Bound to a variable, NOT inlined into (a)'s expected substring, so it can be
# guarded below. require_nonempty cannot catch this one on its own: (a)'s want
# is a literal prefix PLUS this substitution, so an empty slug still yields the
# non-empty "repo=local/" — which the script prints unconditionally, silently
# reducing (a) to a bare expect_ok. Measured: with recompute_repo_name forced to
# echo nothing, (a) PASSED vacuously until this guard was added.
SEAM_SLUG="$(recompute_repo_name "$SEAM_ROOT")"

if [ -z "$SEAM_ROOT" ] || [ -z "$SEAM_INDEX" ] || [ -z "$SEAM_INDEX_2" ] \
   || [ -z "$SEAM_INDEX_3" ] || [ -z "$SEAM_ROOT_REAL" ] || [ -z "$SEAM_SLUG" ]; then
    echo "  FAIL: could not create the identity-seam fixtures"
    FAIL=$((FAIL + 1))
else
    mk_stub_indexer "$STUB_DIR/identity-dispatch" identity-dispatch

    # (a) POSITIVE — the loop closes: the script's own lever reaches the
    # indexer, the indexer resolves the PER-PATH identity from it, writes there,
    # and the script finds and reports exactly that identity. The expected slug
    # comes from recompute_repo_name (the independent python3 mirror), never
    # from the script's own output, so a drift in the script's derivation still
    # fails this loudly.
    assert "under the script's own lever the indexer resolves and writes the PER-PATH identity, and the run reaches INDEX-OK" \
        with_stub "$SEAM_INDEX" "$STUB_DIR/identity-dispatch" \
            expect_ok_stdout "repo=local/$SEAM_SLUG" \
                --project-root "$SEAM_ROOT"

    # (b) NEGATIVE — the SAME stub binary, with the lever overridden downstream
    # of the script's own `env` prefix via its documented word-split seam
    # (scripts/jcodemunch-index-reify.sh:496-503): INDEXER_ARGV becomes
    # `env …=0 env …=1 <stub> watch …` and the second env wins. The stub then
    # takes git_root.py's GIT branch and writes leodearden-reify.db, which is
    # the real failure mode the script's own hijack_note names —
    # "JCODEMUNCH_GIT_ROOT_IDENTITY=0 did not reach the indexer".
    #
    # This is also the MUTATION GUARD for (a): if the script ever drops its
    # JC_IDENTITY_ENV prefix, (a) reproduces exactly this refusal and fails.
    #
    # expect_refusal_names already asserts non-zero exit, the
    # E_JC_INDEX_MISSING marker, and the ABSENCE of INDEX-OK.
    assert "when the lever does not take effect the indexer lands on the git identity and the script refuses E_JC_INDEX_MISSING naming it" \
        with_stub "$SEAM_INDEX_2" "env JCODEMUNCH_GIT_ROOT_IDENTITY=1 $STUB_DIR/identity-dispatch" \
            expect_refusal_names "leodearden-reify.db" \
                --project-root "$SEAM_ROOT"

    # (c) THE SECOND DOCUMENTED CAUSE — the one the script header calls out as
    # "NOT SUFFICIENT ON ITS OWN", and the half (b) cannot reach.
    #
    # (b) drives cause (a) of the script's own hijack NOTE: "JCODEMUNCH_GIT_
    # ROOT_IDENTITY=0 did not reach the indexer". Cause (b) of that same NOTE is
    # different in kind — the lever DOES reach the child, unmodified, and is
    # overridden anyway, because resolve_index_identity returns an
    # ALREADY-EXISTING index for this source_root (git_root.py:174-178) before
    # the configured mode is ever consulted. Verified first-hand against the
    # pinned 1.108.54 wheel: the `requested == "config"` arm returns
    # local_existing, then existing_git, and only then assigns
    # `requested = configured`.
    #
    # So this runs with the script's OWN, UNMODIFIED lever — no `env …=1`
    # override — and the identity still lands on git, decided by the store's
    # contents rather than by the lever. Test 12's static mk_foreign_index_db
    # cannot pin this: there, no indexer resolves anything, so it cannot show a
    # lever being honoured-and-overridden. Here the same one stub binary that
    # took the local branch in (a) takes the git branch instead, purely because
    # a non-inert husk is already sitting in the store.
    #
    # The husk must carry meta.source_root — an EMPTY one (no source_root) is
    # inert and correctly does NOT short-circuit, which is exactly what the
    # script's "EXPECTED AND BENIGN" paragraph records.
    if [ -z "$(mk_foreign_index_db "$SEAM_INDEX_3" leodearden-reify "$SEAM_ROOT_REAL")" ]; then
        echo "  FAIL: could not plant the pre-existing git-identity husk"
        FAIL=$((FAIL + 1))
    else
        assert "a pre-existing git-identity index overrides the lever even when it DOES reach the indexer, and the script still refuses E_JC_INDEX_MISSING naming it" \
            with_stub "$SEAM_INDEX_3" "$STUB_DIR/identity-dispatch" \
                expect_refusal_names "leodearden-reify.db" \
                    --project-root "$SEAM_ROOT"
    fi
fi

test_summary
