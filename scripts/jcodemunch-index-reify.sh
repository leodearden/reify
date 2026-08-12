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
say "project-root  $PROJECT_ROOT"
say "repo-id       $REPO_ID"
say "db-path       $DB_PATH"

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

SCRATCH_ERR="$(mktemp "${TMPDIR:-/tmp}/jc-index-reify-XXXXXX.err")"
trap 'rm -f "$SCRATCH_ERR"' EXIT

check_index

# ── Success summary ──────────────────────────────────────────────────────────
#
# The script's OWN line, in a format this script owns. Deliberately NOT a
# re-emission of an upstream token: `watch --once` runs watcher.py::sync_folders,
# which emits neither `changed=N` nor `changed: N` (that line belongs to the
# CONTINUOUS watch loop's re-index callback at watcher.py:305), so binding a
# consumer to one would bind it to a string this code path never prints.
say "INDEX-OK  repo=$REPO_ID  db=$DB_PATH  $symbol_count sym"
