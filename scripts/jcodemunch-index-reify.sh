#!/usr/bin/env bash
# scripts/jcodemunch-index-reify.sh
#
# The SINGLE `watch --once` index primitive for the canonical reify checkout.
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4.4
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/β
#
# Runs one bounded, non-daemon jcodemunch index pass over the PRODUCTION reify
# identity and refuses loudly if the resulting index is missing, empty, or
# silently truncated. Everything else in the substrate chain (γ staleness, δ/ε/ζ
# consumers) builds on this one primitive, so its refusals carry stable
# greppable markers rather than prose:
#
#   E_JC_INDEX_MISSING     the index DB file does not exist
#   E_JC_INDEX_EMPTY       DB unreadable / no `symbols` table / 0 symbol rows
#   E_JC_INDEX_TRUNCATED   indexed file count hit the max_folder_files cap
#   E_JC_INDEX_RUN_FAILED  the indexer child exited non-zero
#
# Usage:
#   scripts/jcodemunch-index-reify.sh                    # index + assert
#   scripts/jcodemunch-index-reify.sh --dry-run          # print the argv, exit 0
#   scripts/jcodemunch-index-reify.sh --check-only       # assert an existing index
#   scripts/jcodemunch-index-reify.sh --project-root DIR # override the target
#
# WHY THE DEFAULT PROJECT ROOT IS THE CANONICAL CHECKOUT, NOT THE INVOKING CWD:
# index identity is per-path and derived (`storage/git_root.py::_local_repo_name`)
# as `local/<basename>-<sha1(abspath)[:8]>`. Running this from a warm lane would
# mint a lane-private index (`local/_lane-NN-…`) and leave the production
# identity `local/reify-4ae45bbd` untouched — silently reproducing PRD §2.2's
# "103 per-agent worktree indexes, no reify index". This script's whole purpose
# is the PRODUCTION identity, so it names it explicitly.
#
# BANNED FLAG — the `--paths-from` option is NEVER constructed here (PRD §4.4).
# It short-circuits discovery (`index_folder.py:1505-1511`) and then DELETEs
# every previously-indexed file absent from the list (`sqlite_store.py:1698`,
# `:1480-1483`) with no warning. It is only reachable on the `index` subparser
# (`server.py:6505`), never on `watch`, so this script uses the `watch`
# subcommand exclusively and constructs the bare one-shot form and nothing more.
#
# Prerequisites: uvx (https://docs.astral.sh/uv/), sqlite3, jq.

set -euo pipefail

# The canonical checkout whose index identity this script exists to maintain.
DEFAULT_PROJECT_ROOT="/home/leo/src/reify"

usage() {
    cat <<'USAGE'
Usage: scripts/jcodemunch-index-reify.sh [--project-root DIR] [--dry-run] [--check-only]

Runs one bounded `watch --once` jcodemunch index pass over the canonical reify
checkout, then asserts the resulting index is present, non-empty, and not
silently truncated by the max_folder_files cap.

  --project-root DIR  Index DIR instead of the canonical /home/leo/src/reify.
                      Index identity is per-path, so this changes which index
                      is written (local/<basename>-<sha1(abspath)[:8]>).
  --dry-run           Print the exact indexer argv that would be run, exit 0.
  --check-only        Skip the indexer; run identity resolution and the index
                      assertions against the already-present DB only.
  -h, --help          Show this help and exit.

Refusal markers (stderr, always non-zero exit):
  E_JC_INDEX_MISSING     the index DB file does not exist
  E_JC_INDEX_EMPTY       DB unreadable / no `symbols` table / 0 symbol rows
  E_JC_INDEX_TRUNCATED   indexed file count hit the max_folder_files cap
  E_JC_INDEX_RUN_FAILED  the indexer child exited non-zero
USAGE
}

PROJECT_ROOT="$DEFAULT_PROJECT_ROOT"
DRY_RUN=0
CHECK_ONLY=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --project-root)
            [ "$#" -ge 2 ] || { echo "jcodemunch-index-reify.sh: --project-root requires a DIR argument" >&2; usage >&2; exit 64; }
            PROJECT_ROOT="$2"; shift 2 ;;
        --project-root=*) PROJECT_ROOT="${1#*=}"; shift ;;
        --dry-run)    DRY_RUN=1; shift ;;
        --check-only) CHECK_ONLY=1; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)
            echo "jcodemunch-index-reify.sh: unknown argument '$1'" >&2
            usage >&2
            exit 64
            ;;
    esac
done

say() { printf 'jcodemunch-index-reify: %s\n' "$*"; }
die() { printf 'jcodemunch-index-reify: %s\n' "$*" >&2; exit 1; }

# ── Identity resolution ──────────────────────────────────────────────────────
#
# Mirrors jcodemunch_mcp/storage/git_root.py::_local_repo_name at the pinned
# 1.108.54:
#     f"{folder_path.name}-{sha1(str(folder_path)).hexdigest()[:8]}"
# where folder_path is the root AFTER `Path(p).expanduser().resolve()` (which
# watcher.py::sync_folders applies before handing it on). The sha1 is therefore
# over the RESOLVED ABSOLUTE PATH STRING — `readlink -f` is the bash equivalent
# (absolutise + resolve symlinks, non-strict on a missing leaf).
#
# The DB path derives from that via IndexStore._db_path -> _repo_slug(owner,
# name) = f"{owner}-{name}", i.e. `local-<repo-name>.db` under CODE_INDEX_PATH.
#
# Deliberately does NOT stat the project root: identity is a PURE FUNCTION of
# the path string, and keeping it so is what lets the whole contract be tested
# against throwaway paths. Existence is checked later, only where it matters —
# immediately before the indexer actually runs.
sha1_hex() {
    if command -v sha1sum >/dev/null 2>&1; then
        printf '%s' "$1" | sha1sum | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s' "$1" | shasum -a 1 | cut -d' ' -f1
    else
        die "neither sha1sum nor 'shasum -a 1' is available; cannot derive the index identity"
    fi
}

# Expand a leading `~` ourselves: the path may arrive quoted (from a config or
# another script), in which case the caller's shell never expanded it, and
# readlink -f would resolve a literal './~' relative to cwd.
case "$PROJECT_ROOT" in
    "~")   PROJECT_ROOT="$HOME" ;;
    "~/"*) PROJECT_ROOT="$HOME/${PROJECT_ROOT#\~/}" ;;
esac
PROJECT_ROOT="$(readlink -f -- "$PROJECT_ROOT")" \
    || die "could not resolve --project-root to an absolute path"

REPO_NAME="$(basename -- "$PROJECT_ROOT")-$(sha1_hex "$PROJECT_ROOT" | cut -c1-8)"
REPO_ID="local/$REPO_NAME"
CODE_INDEX_DIR="${CODE_INDEX_PATH:-$HOME/.code-index}"
DB_PATH="$CODE_INDEX_DIR/local-$REPO_NAME.db"

# Printed on EVERY run, including the refusal paths below: which index identity
# was touched is the single most load-bearing fact this script reports (PRD §2.2
# was a whole fleet writing to the wrong ones), so it must never be conditional
# on success.
# ── Effective max_folder_files cap ───────────────────────────────────────────
#
# Precedence, matching the package at the pinned 1.108.54:
#   1. $JCODEMUNCH_MAX_FOLDER_FILES   (the real env mapping, config.py:26)
#   2. max_folder_files in $CODE_INDEX_DIR/config.jsonc
#   3. the package default 2000       (config.py:284)
#
# THE DEFAULT IS 2000, NOT 10000. The 10000 that makes reify fit lives in
# ~/.code-index/config.jsonc — host state outside this repo. Resolving the cap
# here rather than assuming a number is what makes the guard below correct if
# that file is ever reset or absent.
PACKAGE_DEFAULT_MAX_FOLDER_FILES=2000
CONFIG_JSONC="$CODE_INDEX_DIR/config.jsonc"

resolve_file_cap() {
    if [ -n "${JCODEMUNCH_MAX_FOLDER_FILES:-}" ]; then
        FILE_CAP="$JCODEMUNCH_MAX_FOLDER_FILES"
        FILE_CAP_SOURCE="env JCODEMUNCH_MAX_FOLDER_FILES"
        return 0
    fi

    if [ -f "$CONFIG_JSONC" ]; then
        # Strip `//` line comments before jq — the file is JSONC, and jq is a
        # strict JSON parser. Only comments that START a line (after optional
        # whitespace) or follow whitespace are stripped, so a `//` inside a
        # value (a URL, an ignore pattern) is left alone.
        local stripped from_config
        stripped="$(sed -e 's|^[[:space:]]*//.*$||' -e 's|[[:space:]]//[^"]*$||' "$CONFIG_JSONC")"

        if ! from_config="$(printf '%s\n' "$stripped" | jq -r '.max_folder_files // empty' 2>/dev/null)"; then
            # A config.jsonc that exists but will not parse is an operator
            # error we must NOT paper over: falling back to 2000 while the real
            # cap is 10000 would make the truncation guard below fire spuriously
            # on every full-size repo. Refuse and name the fix.
            die "cannot parse $CONFIG_JSONC — fix it, or set JCODEMUNCH_MAX_FOLDER_FILES to override the cap explicitly"
        fi

        if [ -n "$from_config" ]; then
            FILE_CAP="$from_config"
            FILE_CAP_SOURCE="config.jsonc"
            return 0
        fi
    fi

    FILE_CAP="$PACKAGE_DEFAULT_MAX_FOLDER_FILES"
    FILE_CAP_SOURCE="package default"
}

resolve_file_cap
case "$FILE_CAP" in
    ''|*[!0-9]*) die "resolved a non-numeric max_folder_files cap '$FILE_CAP' from $FILE_CAP_SOURCE" ;;
esac

say "project-root  $PROJECT_ROOT"
say "repo-id       $REPO_ID"
say "db-path       $DB_PATH"
say "file-cap      $FILE_CAP ($FILE_CAP_SOURCE)"

# ── Refusals ─────────────────────────────────────────────────────────────────
#
# Every refusal carries a stable, greppable marker rather than prose, so the
# consumers downstream of this primitive (γ/δ/ε/ζ/θ/λ) and this script's own
# guard bind to CODE IDENTITY that a later reword cannot break — the INV-SF-6
# house pattern already applied to γ's E_JC_INDEX_STALE/E_JC_INDEX_EMPTY.
#
# Markers go to STDERR and always exit non-zero. Nothing prints the success
# summary on a refusal path: a caller scraping for INDEX-OK must never see it
# for an index this script just declined.
refuse() {
    local marker="$1"; shift
    printf 'jcodemunch-index-reify: %s: %s\n' "$marker" "$*" >&2
    exit 1
}

# ── The symbol-count gate ────────────────────────────────────────────────────
#
# PRD §4.3: index PRESENCE proves nothing. `delete-index` leaves a husk that
# re-registers as an EMPTY repo, so a check that only asked "is the DB file
# there?" would report health over an index with zero symbols in it. The gate
# is on the symbol COUNT, and an index whose count cannot be read at all is
# treated as empty — we may not report health over a DB we cannot query.
#
# Read STRICTLY READ-ONLY via the `file:…?mode=ro` URI form, so a concurrent
# watcher or serve holding the same DB can never be perturbed by this probe.
# (The URI form is required for ?mode=ro; sqlite3 3.45.1 on this host supports
# it. mode=ro also means we can never create the file by probing it — a
# would-be false GREEN on a missing index.)
db_query() {
    sqlite3 "file:${DB_PATH}?mode=ro" "$1"
}

symbol_count=""
check_index() {
    [ -f "$DB_PATH" ] || refuse E_JC_INDEX_MISSING \
        "no index DB at $DB_PATH for $REPO_ID — nothing has indexed this identity. Run this script without --check-only."

    local err
    if ! symbol_count="$(db_query 'select count(*) from symbols' 2>"$SCRATCH_ERR")"; then
        err="$(cat "$SCRATCH_ERR" 2>/dev/null || true)"
        refuse E_JC_INDEX_EMPTY \
            "cannot read the symbol count from $DB_PATH (${err:-unreadable / no symbols table}) — an index whose count cannot be queried is not one to report health over."
    fi

    case "$symbol_count" in
        ''|*[!0-9]*) refuse E_JC_INDEX_EMPTY \
            "non-numeric symbol count ${symbol_count:-<empty>} from $DB_PATH" ;;
    esac

    [ "$symbol_count" -gt 0 ] || refuse E_JC_INDEX_EMPTY \
        "$REPO_ID has 0 symbols at $DB_PATH — the index is a husk (PRD §4.3: a delete-index husk re-registers as an empty repo, so presence proves nothing)."
}

# ── The truncation guard (G7 INV-SF-3) ───────────────────────────────────────
#
# WHY THIS IS A POST-RUN DB CHECK AND NOT AN OUTPUT SCRAPE. At the pinned
# 1.108.54 the walker truncates when `len(files) > max_files`
# (index_folder.py:856-857), priority-sorts, and keeps EXACTLY max_files. The
# cap warning it raises is written into index_folder's RESULT DICT
# (:2220-2230, result["warnings"] + files_skipped_cap) — and
# watcher.py::sync_folders, the `--once` path, prints only `msg` and
# `duration`, never result["warnings"]. So the truncation is COMPLETELY SILENT
# on this code path: there is no line on stdout or stderr for a scrape to find.
# The only observable is the DB itself.
#
# `indexed >= cap` is the exact signature and cannot false-fire below the cap:
# the walker keeps exactly max_files, so landing ON the cap means files were
# dropped. `>` rather than `>=` would miss every real truncation; `==` would
# miss a cap lowered after an earlier larger run.
check_truncation() {
    local indexed err
    if ! indexed="$(db_query 'select count(*) from files' 2>"$SCRATCH_ERR")"; then
        err="$(cat "$SCRATCH_ERR" 2>/dev/null || true)"
        refuse E_JC_INDEX_EMPTY \
            "cannot read the indexed-file count from $DB_PATH (${err:-unreadable / no files table})"
    fi

    case "$indexed" in
        ''|*[!0-9]*) refuse E_JC_INDEX_EMPTY \
            "non-numeric indexed-file count ${indexed:-<empty>} from $DB_PATH" ;;
    esac

    indexed_files="$indexed"

    if [ "$indexed_files" -ge "$FILE_CAP" ]; then
        refuse E_JC_INDEX_TRUNCATED \
"$REPO_ID indexed=$indexed_files cap=$FILE_CAP (from $FILE_CAP_SOURCE) — the walker keeps exactly max_folder_files when it truncates, so landing on or above the cap means files were SILENTLY dropped (the cap warning never reaches stdout/stderr on the watch --once path). Raise the cap, e.g. JCODEMUNCH_MAX_FOLDER_FILES=$((indexed_files * 2))."
    fi
}

SCRATCH_ERR="$(mktemp "${TMPDIR:-/tmp}/jc-index-reify-XXXXXX.err")"
trap 'rm -f "$SCRATCH_ERR"' EXIT

# ── Pre-run headroom note (DIAGNOSTIC ONLY — never a gate) ───────────────────
#
# A cheap early hint that the cap is about to bite, printed before the ~5-10
# minute run rather than after it. It is NOT a gate, because the tracked-file
# count bounds nothing: the walker also sees untracked non-ignored files (which
# pushes the real number UP) and filters by extension and by
# extra_ignore_patterns (which pushes it DOWN). Only the post-run DB assertion
# in check_truncation is load-bearing.
headroom_note() {
    local tracked
    git -C "$PROJECT_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 0
    tracked="$(git -C "$PROJECT_ROOT" ls-files 2>/dev/null | wc -l)" || return 0
    say "headroom      $tracked tracked files vs cap $FILE_CAP (diagnostic only; the DB check after the run is the real gate)"
    return 0
}

# ── The indexer command ──────────────────────────────────────────────────────
#
# The BARE one-shot form and nothing more. All three flags re-verified on the
# `watch` subparser at the pinned 1.108.54 (server.py:6326-6369):
#
#     uvx --from jcodemunch-mcp==1.108.54 jcodemunch-mcp watch <root> --once --no-ai-summaries
#
# The pin is 1.108.54 because PRD §8 records that 1.108.27 is no longer on
# PyPI. An unpinned invocation would silently follow upstream into a version
# whose flags and schema this script has not been verified against.
#
# NOTHING is ever appended to this array — see the `--paths-from` ban in the
# header (PRD §4.4). In particular the `index` subcommand is never used: it is
# the only subparser that accepts --paths-from at all (server.py:6505).
JC_PIN="jcodemunch-mcp==1.108.54"

INDEXER_CMD=(uvx --from "$JC_PIN" jcodemunch-mcp)
if [ -n "${REIFY_JC_INDEXER_CMD:-}" ]; then
    # TEST-ONLY SEAM. Replaces the `uvx …` prefix so the guard can drive the
    # summary/exit contract against the two real 1.108.54 stderr shapes offline.
    # Never set in production use; word-split deliberately, so a caller can pass
    # a multi-word command.
    # shellcheck disable=SC2206
    INDEXER_CMD=(${REIFY_JC_INDEXER_CMD})
fi
INDEXER_ARGV=("${INDEXER_CMD[@]}" watch "$PROJECT_ROOT" --once --no-ai-summaries)

if [ "$DRY_RUN" -eq 1 ]; then
    say "exec  $(printf '%q ' "${INDEXER_ARGV[@]}")"
    exit 0
fi

# Resolve the indexer entry point before the ~5-10 minute run rather than
# failing partway into it. Only on the real path: --dry-run prints a command it
# does not run, and --check-only never invokes one, so neither needs uvx present
# (which is what keeps this script's guard hermetic in a task worktree, where
# jcodemunch is legitimately absent — PRD §9).
require_indexer() {
    command -v "${INDEXER_CMD[0]}" >/dev/null 2>&1 && return 0
    die "'${INDEXER_CMD[0]}' is not on PATH. Install uv (https://docs.astral.sh/uv/) or add its bin dir — on this host uvx lives at /home/leo/.local/bin/uvx. Use --dry-run to see the exact command, or --check-only to assert an existing index without running one."
}

# ── Running the indexer ──────────────────────────────────────────────────────
#
# ALL of upstream's `--once` output goes to STDERR (watcher.py::sync_folders
# prints `Syncing <folder>...` then `  <folder>: <msg> (<duration>s)` there and
# nowhere else), so the child's stderr is both the operator's only view of the
# run AND the only place the no-op marker can be read. It is therefore tee'd:
# passed through live, and kept for the marker test below.
#
# THE ONLY UPSTREAM MARKER THIS PATH MAY BE READ FOR is the literal
# "No changes detected" (index_folder.py:1303/:1372/:1755). The
# `<n> new / <n> deleted` style summary line at watcher.py:305 belongs to the
# CONTINUOUS watch loop's re-index callback and is NEVER printed by the one-shot
# path, so nothing here may key on it — see the changed-file token below.
JC_CHANGED="n/a"
run_indexer() {
    local out="$SCRATCH_ERR.run" rc=0
    if ! "${INDEXER_ARGV[@]}" 2> >(tee "$out" >&2); then
        rc=1
    fi
    wait 2>/dev/null || true

    [ "$rc" -eq 0 ] || refuse E_JC_INDEX_RUN_FAILED \
        "the indexer exited non-zero for $REPO_ID (see its output above). Command: $(printf '%q ' "${INDEXER_ARGV[@]}")"

    # The script's OWN report, derived from that one sanctioned marker. Its
    # token is deliberately distinct from any upstream one, so a consumer
    # grepping for it can never be silently satisfied by upstream prose.
    if grep -q 'No changes detected' "$out" 2>/dev/null; then
        JC_CHANGED=0
    else
        JC_CHANGED=some
    fi
    rm -f "$out"
}

indexed_files=""
if [ "$CHECK_ONLY" -eq 0 ]; then
    require_indexer
    headroom_note
    run_indexer
fi
check_index
check_truncation

# ── Success summary ──────────────────────────────────────────────────────────
#
# The script's OWN line, in a format this script owns. Deliberately NOT a
# re-emission of an upstream token: `watch --once` runs watcher.py::sync_folders,
# which emits NEITHER spelling of the `<n> new / <n> deleted` summary line (it
# belongs to the CONTINUOUS watch loop's re-index callback, watcher.py:305), so
# binding a consumer to it would bind it to a string this path never prints.
say "INDEX-OK  repo=$REPO_ID  db=$DB_PATH  $symbol_count sym  indexed=$indexed_files cap=$FILE_CAP  jc-changed-files=$JC_CHANGED"
